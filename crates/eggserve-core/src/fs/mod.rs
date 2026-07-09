//! Filesystem confinement: root guard and resolved resource types.
//!
//! Under safe defaults (symlinks denied), resolution uses descriptor-relative
//! traversal via `openat` with `O_NOFOLLOW` on Unix. Each component is
//! pre-checked with `statat(AT_SYMLINK_NOFOLLOW)` and then opened
//! no-follow; if a symlink is swapped in between the two, the open fails
//! rather than following it. Under follow-symlinks mode, a canonicalize-based
//! fallback is used.
//!
//! The `RootGuard` is constructed per request. The configured root is
//! canonicalized and opened as a directory descriptor during request
//! resolution.

use std::fs;
use std::path::{Path, PathBuf};

use crate::path::{ConfinedPath, PathRejection};
use crate::policy::{DotfilePolicy, StaticPolicy, SymlinkPolicy};
use crate::primitives::body::{BodySource, BodySourceError};
use crate::primitives::response::{BodyPlan, FileRange, StaticResponsePlan};

#[cfg(unix)]
pub(crate) mod unix;

/// A resolved file with a pre-opened handle.
///
/// The file is opened during resolution via `openat` with `O_NOFOLLOW` on
/// Unix safe defaults, so the service layer does not reopen by absolute path.
/// MIME detection uses `safe_relative_components`, never the absolute path.
#[derive(Debug)]
pub(crate) struct ResolvedFile {
    pub(crate) file: fs::File,
    pub(crate) metadata: fs::Metadata,
    /// Safe relative path components for MIME detection only.
    /// Never used for file access.
    pub(crate) safe_relative_components: Vec<String>,
}

impl ResolvedFile {
    pub(crate) fn into_body(
        self,
        plan: &StaticResponsePlan,
    ) -> Result<BodySource, BodySourceError> {
        match &plan.body {
            BodyPlan::Empty => Ok(BodySource::Empty),
            BodyPlan::FullBytes(b) => Ok(BodySource::Bytes(b.clone())),
            BodyPlan::FileFull => {
                let len = self.metadata.len();
                let path: std::path::PathBuf = self.safe_relative_components.iter().collect();
                let mime = crate::mime::mime_for_path(&path);
                Ok(BodySource::FileFull {
                    file: self.file,
                    len,
                    mime,
                })
            }
            BodyPlan::FileRange {
                start,
                end_inclusive,
            } => {
                let total_len = self.metadata.len();
                if *end_inclusive >= total_len {
                    return Err(BodySourceError::InvalidRange);
                }
                let range = FileRange::new(*start, *end_inclusive);
                let path: std::path::PathBuf = self.safe_relative_components.iter().collect();
                let mime = crate::mime::mime_for_path(&path);
                Ok(BodySource::FileRange {
                    file: self.file,
                    range,
                    total_len,
                    mime,
                })
            }
        }
    }
}

/// A resolved directory with an optional pre-opened handle.
///
/// On Unix safe defaults, `dir_fd` is an open directory file descriptor used
/// for fd-relative child resolution and listing. On the fallback path
/// (follow-symlinks or non-Unix), `canonical_path` is used instead.
#[derive(Debug)]
pub(crate) struct ResolvedDirectory {
    #[cfg(unix)]
    pub(crate) dir_fd: fs::File,
    pub(crate) canonical_path: PathBuf,
    pub(crate) components: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(#[allow(dead_code)] PathRejection),
}

pub(crate) struct RootGuard {
    canonical_root: PathBuf,
    #[cfg(unix)]
    root_fd: fs::File,
}

impl RootGuard {
    pub(crate) fn new(root: &Path) -> Result<Self, std::io::Error> {
        let canonical_root = fs::canonicalize(root)?;
        #[cfg(unix)]
        let root_fd = fs::File::open(&canonical_root)?;
        Ok(Self {
            canonical_root,
            #[cfg(unix)]
            root_fd,
        })
    }

    pub(crate) fn resolve(
        &self,
        confined: &ConfinedPath,
        policy: &StaticPolicy,
    ) -> ResolvedResource {
        #[cfg(unix)]
        if policy.symlinks == SymlinkPolicy::Denied {
            return unix::resolve_fd_relative(
                &self.root_fd,
                &self.canonical_root,
                confined.components(),
                policy,
            );
        }
        self.resolve_fallback(confined.components(), policy)
    }

    pub(crate) fn resolve_child(
        &self,
        dir: &ResolvedDirectory,
        child: &str,
        policy: &StaticPolicy,
    ) -> ResolvedResource {
        #[cfg(unix)]
        if policy.symlinks == SymlinkPolicy::Denied {
            return unix::resolve_child_fd(&dir.dir_fd, &dir.components, child, policy);
        }
        let mut components = dir.components.clone();
        components.push(child.to_string());
        self.resolve_fallback(&components, policy)
    }

    pub(crate) fn list_directory(
        &self,
        dir: &ResolvedDirectory,
        policy: &StaticPolicy,
    ) -> Result<Vec<(String, bool)>, std::io::Error> {
        #[cfg(unix)]
        if policy.symlinks == SymlinkPolicy::Denied {
            return unix::list_directory_fd(&dir.dir_fd, policy);
        }
        build_listing_entries_fallback(&dir.canonical_path, policy)
    }

    fn resolve_fallback(&self, components: &[String], policy: &StaticPolicy) -> ResolvedResource {
        let mut candidate = self.canonical_root.clone();

        for component in components {
            if policy.dotfiles == DotfilePolicy::Denied && component.starts_with('.') {
                return ResolvedResource::Denied(PathRejection::DotfileDenied);
            }

            candidate.push(component);

            if policy.symlinks == SymlinkPolicy::Denied {
                match fs::symlink_metadata(&candidate) {
                    Ok(meta) => {
                        if meta.file_type().is_symlink() {
                            return ResolvedResource::Denied(PathRejection::SymlinkDenied);
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            return ResolvedResource::NotFound;
                        }
                        return ResolvedResource::NotFound;
                    }
                }
            }
        }

        let canonical = match fs::canonicalize(&candidate) {
            Ok(p) => p,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return ResolvedResource::NotFound;
                }
                return ResolvedResource::NotFound;
            }
        };

        if !canonical.starts_with(&self.canonical_root) {
            return ResolvedResource::Denied(PathRejection::RootEscapeDenied);
        }

        match fs::metadata(&canonical) {
            Ok(meta) => {
                if meta.is_dir() {
                    #[cfg(unix)]
                    {
                        match fs::File::open(&canonical) {
                            Ok(dir_fd) => ResolvedResource::Directory(ResolvedDirectory {
                                dir_fd,
                                canonical_path: canonical,
                                components: components.to_vec(),
                            }),
                            Err(_) => ResolvedResource::NotFound,
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        ResolvedResource::Directory(ResolvedDirectory {
                            canonical_path: canonical,
                            components: components.to_vec(),
                        })
                    }
                } else {
                    match fs::File::open(&canonical) {
                        Ok(file) => ResolvedResource::File(ResolvedFile {
                            file,
                            metadata: meta,
                            safe_relative_components: components.to_vec(),
                        }),
                        Err(_) => ResolvedResource::NotFound,
                    }
                }
            }
            Err(_) => ResolvedResource::NotFound,
        }
    }
}

fn build_listing_entries_fallback(
    dir: &Path,
    policy: &crate::policy::StaticPolicy,
) -> Result<Vec<(String, bool)>, std::io::Error> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if policy.dotfiles == DotfilePolicy::Denied && name.starts_with('.') {
            continue;
        }

        let meta = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if policy.symlinks == SymlinkPolicy::Denied && meta.file_type().is_symlink() {
            continue;
        }

        let is_dir = meta.is_dir();
        entries.push((name, is_dir));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::PathPolicy;
    use std::fs;
    use tempfile::TempDir;

    fn setup_root() -> (TempDir, RootGuard) {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("file.txt"), "file").unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        (tmp, guard)
    }

    fn parse_path(raw: &str) -> ConfinedPath {
        ConfinedPath::parse(raw, &PathPolicy::default()).unwrap()
    }

    fn parse_path_with_policy(raw: &str, policy: &PathPolicy) -> ConfinedPath {
        ConfinedPath::parse(raw, policy).unwrap()
    }

    #[test]
    fn resolve_normal_file() {
        let (_tmp, guard) = setup_root();
        let path = parse_path("/hello.txt");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::File(_)));
    }

    #[test]
    fn resolve_normal_directory() {
        let (_tmp, guard) = setup_root();
        let path = parse_path("/subdir");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::Directory(_)));
    }

    #[test]
    fn resolve_missing_path() {
        let (_tmp, guard) = setup_root();
        let path = parse_path("/nonexistent.txt");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_symlink_denied() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path = parse_path("/link.txt");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(
            result,
            ResolvedResource::Denied(PathRejection::SymlinkDenied)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_symlink_allowed() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path = parse_path("/link.txt");
        let mut policy = StaticPolicy::safe_default();
        policy.symlinks = SymlinkPolicy::Follow;
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::File(_)));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_intermediate_symlink_denied_when_symlinks_denied() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("real_dir")).unwrap();
        fs::write(tmp.path().join("real_dir").join("file.txt"), "content").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir"))
            .unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path = parse_path("/link_dir/file.txt");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(
            result,
            ResolvedResource::Denied(PathRejection::SymlinkDenied)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_intermediate_symlink_inside_root_allowed_when_follow_enabled() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("real_dir")).unwrap();
        fs::write(tmp.path().join("real_dir").join("file.txt"), "content").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir"))
            .unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path = parse_path("/link_dir/file.txt");
        let mut policy = StaticPolicy::safe_default();
        policy.symlinks = SymlinkPolicy::Follow;
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::File(_)));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_intermediate_symlink_escape_denied_when_follow_enabled() {
        let tmp_root = TempDir::new().unwrap();
        let tmp_outside = TempDir::new().unwrap();
        fs::create_dir(tmp_outside.path().join("secret_dir")).unwrap();
        fs::write(
            tmp_outside.path().join("secret_dir").join("file.txt"),
            "leaked",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            tmp_outside.path().join("secret_dir"),
            tmp_root.path().join("link_dir"),
        )
        .unwrap();

        let guard = RootGuard::new(tmp_root.path()).unwrap();
        let path = parse_path("/link_dir/file.txt");
        let mut policy = StaticPolicy::safe_default();
        policy.symlinks = SymlinkPolicy::Follow;
        let result = guard.resolve(&path, &policy);
        assert!(matches!(
            result,
            ResolvedResource::Denied(PathRejection::RootEscapeDenied)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_final_symlink_outside_root_denied_when_follow_enabled() {
        let tmp_root = TempDir::new().unwrap();
        let tmp_outside = TempDir::new().unwrap();
        fs::write(tmp_outside.path().join("secret.txt"), "leaked").unwrap();
        std::os::unix::fs::symlink(
            tmp_outside.path().join("secret.txt"),
            tmp_root.path().join("escape.txt"),
        )
        .unwrap();

        let guard = RootGuard::new(tmp_root.path()).unwrap();
        let path = parse_path("/escape.txt");
        let mut policy = StaticPolicy::safe_default();
        policy.symlinks = SymlinkPolicy::Follow;
        let result = guard.resolve(&path, &policy);
        assert!(matches!(
            result,
            ResolvedResource::Denied(PathRejection::RootEscapeDenied)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_nested_intermediate_symlink_denied() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("a")).unwrap();
        fs::create_dir(tmp.path().join("b")).unwrap();
        fs::write(tmp.path().join("b").join("file.txt"), "content").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("b"), tmp.path().join("a").join("link_b"))
            .unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path = parse_path("/a/link_b/file.txt");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(
            result,
            ResolvedResource::Denied(PathRejection::SymlinkDenied)
        ));
    }

    #[test]
    fn resolve_path_escape_denied() {
        let tmp = TempDir::new().unwrap();
        let _guard = RootGuard::new(tmp.path()).unwrap();

        let path_policy = PathPolicy {
            reject_backslash: true,
            ..PathPolicy::default()
        };
        let path = ConfinedPath::parse("/../etc/passwd", &path_policy);
        assert!(path.is_err());
    }

    #[test]
    fn resolve_dotfile_denied() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".env"), "secret").unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path_policy = PathPolicy {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        let path = parse_path_with_policy("/.env", &path_policy);
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(
            result,
            ResolvedResource::Denied(PathRejection::DotfileDenied)
        ));
    }

    #[test]
    fn resolve_dotfile_allowed() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".env"), "secret").unwrap();

        let guard = RootGuard::new(tmp.path()).unwrap();
        let path_policy = PathPolicy {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        let path = parse_path_with_policy("/.env", &path_policy);
        let mut policy = StaticPolicy::safe_default();
        policy.dotfiles = DotfilePolicy::Serve;
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::File(_)));
    }

    #[test]
    fn resolve_root_path() {
        let (_tmp, guard) = setup_root();
        let path = parse_path("/");
        let policy = StaticPolicy::safe_default();
        let result = guard.resolve(&path, &policy);
        assert!(matches!(result, ResolvedResource::Directory(_)));
    }

    #[cfg(unix)]
    #[test]
    fn openat_nofollow_kernel_rejects_symlink() {
        use rustix::fs::{openat, Mode, OFlags};

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let root_fd = fs::File::open(tmp.path()).unwrap();

        let result = openat(
            &root_fd,
            "link.txt",
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        );

        match result {
            Err(rustix::io::Errno::LOOP) | Err(rustix::io::Errno::MLINK) => {}
            Err(other) => {
                panic!("expected ELOOP/EMLINK from openat(NOFOLLOW) on symlink, got {other:?}")
            }
            Ok(_) => panic!("openat(NOFOLLOW) on a symlink unexpectedly succeeded"),
        }
    }
}
