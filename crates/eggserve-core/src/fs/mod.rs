use std::fs;
use std::path::{Path, PathBuf};

use crate::path::{ConfinedPath, PathRejection};
use crate::policy::{DotfilePolicy, StaticPolicy, SymlinkPolicy};

#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub path: PathBuf,
    pub metadata: fs::Metadata,
}

#[derive(Debug, Clone)]
pub struct ResolvedDirectory {
    pub path: PathBuf,
    pub components: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(PathRejection),
}

pub struct RootGuard {
    canonical_root: PathBuf,
}

impl RootGuard {
    pub fn new(root: &Path) -> Result<Self, std::io::Error> {
        let canonical_root = fs::canonicalize(root)?;
        Ok(Self { canonical_root })
    }

    pub fn resolve(&self, confined: &ConfinedPath, policy: &StaticPolicy) -> ResolvedResource {
        self.resolve_components(confined.components(), policy)
    }

    pub fn resolve_index(
        &self,
        dir_confined: &ConfinedPath,
        policy: &StaticPolicy,
    ) -> ResolvedResource {
        let mut components = dir_confined.components().to_vec();
        components.push("index.html".to_string());
        self.resolve_components(&components, policy)
    }

    pub fn resolve_child(
        &self,
        dir: &ResolvedDirectory,
        child: &str,
        policy: &StaticPolicy,
    ) -> ResolvedResource {
        let mut components = dir.components.clone();
        components.push(child.to_string());
        self.resolve_components(&components, policy)
    }

    fn resolve_components(&self, components: &[String], policy: &StaticPolicy) -> ResolvedResource {
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
                            return ResolvedResource::Denied(PathRejection::ParentComponent);
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
            return ResolvedResource::Denied(PathRejection::ParentComponent);
        }

        match fs::metadata(&canonical) {
            Ok(meta) => {
                if meta.is_dir() {
                    ResolvedResource::Directory(ResolvedDirectory {
                        path: canonical,
                        components: components.to_vec(),
                    })
                } else {
                    ResolvedResource::File(ResolvedFile {
                        path: canonical,
                        metadata: meta,
                    })
                }
            }
            Err(_) => ResolvedResource::NotFound,
        }
    }
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
        assert!(matches!(result, ResolvedResource::Denied(_)));
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
        assert!(matches!(result, ResolvedResource::Denied(_)));
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
        assert!(matches!(result, ResolvedResource::Denied(_)));
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
        assert!(matches!(result, ResolvedResource::Denied(_)));
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
        assert!(matches!(result, ResolvedResource::Denied(_)));
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
}
