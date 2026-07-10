use std::fmt;
use std::path::{Path, PathBuf};

use crate::fs::RootGuard;
use crate::path::{ConfinedPath, PathPolicy, PathRejection};
use crate::policy::StaticPolicy;
use crate::primitives::body::{BodySource, BodySourceError};
use crate::primitives::http::ReadOnlyMethod;
use crate::primitives::response::{BodyPlan, FileRange, StaticResponsePlan};

#[derive(Debug)]
pub enum ResourceDeniedReason {
    SymlinkDenied,
    DotfileDenied,
    RootEscapeDenied,
    PolicyDenied(PathRejection),
}

impl fmt::Display for ResourceDeniedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymlinkDenied => write!(f, "symlink denied"),
            Self::DotfileDenied => write!(f, "dotfile denied"),
            Self::RootEscapeDenied => write!(f, "root escape denied"),
            Self::PolicyDenied(inner) => write!(f, "{}", inner),
        }
    }
}

impl std::error::Error for ResourceDeniedReason {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PolicyDenied(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<PathRejection> for ResourceDeniedReason {
    fn from(rejection: PathRejection) -> Self {
        match rejection {
            PathRejection::SymlinkDenied => ResourceDeniedReason::SymlinkDenied,
            PathRejection::DotfileDenied => ResourceDeniedReason::DotfileDenied,
            PathRejection::RootEscapeDenied => ResourceDeniedReason::RootEscapeDenied,
            other => ResourceDeniedReason::PolicyDenied(other),
        }
    }
}

/// A resolved file capability created by the filesystem resolver.
///
/// This type cannot be constructed directly by external callers; it is only
/// created through the resolver's path confinement pipeline.
#[derive(Debug)]
pub struct ResolvedFile {
    inner: crate::fs::ResolvedFile,
}

impl ResolvedFile {
    #[allow(dead_code)]
    pub fn len(&self) -> u64 {
        self.inner.metadata.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[allow(dead_code)]
    pub fn modified(&self) -> Option<std::time::SystemTime> {
        self.inner.metadata.modified().ok()
    }

    #[allow(dead_code)]
    pub fn content_type(&self) -> &'static str {
        let path: PathBuf = self.inner.safe_relative_components.iter().collect();
        crate::mime::mime_for_path(&path)
    }

    #[allow(dead_code)]
    pub fn safe_relative_components(&self) -> &[String] {
        &self.inner.safe_relative_components
    }

    /// Extract the underlying `std::fs::File`.
    ///
    /// # Security note
    ///
    /// After extraction, the confinement guarantee no longer applies to the
    /// raw file handle. This is intended for internal bridging to the Python
    /// bindings layer.
    #[allow(dead_code)]
    pub fn into_std_file(self) -> std::fs::File {
        self.inner.file
    }

    /// Extract the underlying file and metadata.
    ///
    /// # Security note
    ///
    /// After extraction, the confinement guarantee no longer applies to the
    /// raw file handle. This is intended for internal bridging to the Python
    /// bindings layer.
    #[allow(dead_code)]
    pub fn into_parts(self) -> (std::fs::File, std::fs::Metadata) {
        (self.inner.file, self.inner.metadata)
    }

    /// Reconstruct a `ResolvedFile` from raw components.
    ///
    /// # Security note
    ///
    /// This constructor does not verify that the file was opened through the
    /// path confinement pipeline. It is intended for internal use by the
    /// Python bindings where the file was already resolved through a secure
    /// path. External Rust consumers should use [`SecureRoot::resolve`] or
    /// [`resolve_and_plan`] instead.
    #[allow(dead_code)]
    pub fn from_parts(
        file: std::fs::File,
        metadata: std::fs::Metadata,
        safe_relative_components: Vec<String>,
    ) -> Self {
        Self {
            inner: crate::fs::ResolvedFile {
                file,
                metadata,
                safe_relative_components,
            },
        }
    }

    #[allow(dead_code)]
    pub fn into_body(self, plan: &StaticResponsePlan) -> Result<BodySource, BodySourceError> {
        match &plan.body {
            BodyPlan::Empty => Ok(BodySource::Empty),
            BodyPlan::FullBytes(b) => Ok(BodySource::Bytes(b.clone())),
            BodyPlan::FileFull => {
                let len = self.inner.metadata.len();
                let path: PathBuf = self.inner.safe_relative_components.iter().collect();
                let mime = crate::mime::mime_for_path(&path);
                Ok(BodySource::FileFull {
                    file: self.inner.file,
                    len,
                    mime,
                })
            }
            BodyPlan::FileRange {
                start,
                end_inclusive,
            } => {
                let total_len = self.inner.metadata.len();
                if *end_inclusive >= total_len {
                    return Err(BodySourceError::InvalidRange);
                }
                let range = FileRange::new(*start, *end_inclusive);
                let path: PathBuf = self.inner.safe_relative_components.iter().collect();
                let mime = crate::mime::mime_for_path(&path);
                Ok(BodySource::FileRange {
                    file: self.inner.file,
                    range,
                    total_len,
                    mime,
                })
            }
        }
    }

    #[allow(dead_code)]
    pub fn into_range_body(
        self,
        start: u64,
        end_inclusive: u64,
    ) -> Result<BodySource, BodySourceError> {
        let total_len = self.inner.metadata.len();
        if end_inclusive >= total_len {
            return Err(BodySourceError::InvalidRange);
        }
        let range = FileRange::new(start, end_inclusive);
        let path: PathBuf = self.inner.safe_relative_components.iter().collect();
        let mime = crate::mime::mime_for_path(&path);
        Ok(BodySource::FileRange {
            file: self.inner.file,
            range,
            total_len,
            mime,
        })
    }
}

#[derive(Debug)]
pub struct ResolvedDirectory {
    inner: crate::fs::ResolvedDirectory,
}

impl ResolvedDirectory {
    #[allow(dead_code)]
    pub fn components(&self) -> &[String] {
        &self.inner.components
    }

    #[allow(dead_code)]
    pub fn resolve_child(&self, child: &str, root: &SecureRoot) -> ResolvedResource {
        let guard = match RootGuard::new(&root.root) {
            Ok(g) => g,
            Err(_) => return ResolvedResource::NotFound,
        };
        guard.resolve_child(&self.inner, child, &root.policy).into()
    }

    #[allow(dead_code)]
    pub fn list(&self, root: &SecureRoot) -> Result<Vec<(String, bool)>, std::io::Error> {
        let guard = RootGuard::new(&root.root)?;
        guard.list_directory(&self.inner, &root.policy)
    }
}

#[derive(Debug)]
pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(ResourceDeniedReason),
}

impl ResolvedResource {
    #[allow(dead_code)]
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    #[allow(dead_code)]
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }

    #[allow(dead_code)]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
    }

    #[allow(dead_code)]
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied(_))
    }

    #[allow(dead_code)]
    pub fn as_file(&self) -> Option<&ResolvedFile> {
        match self {
            Self::File(f) => Some(f),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_directory(&self) -> Option<&ResolvedDirectory> {
        match self {
            Self::Directory(d) => Some(d),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn into_file(self) -> Option<ResolvedFile> {
        match self {
            Self::File(f) => Some(f),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn into_directory(self) -> Option<ResolvedDirectory> {
        match self {
            Self::Directory(d) => Some(d),
            _ => None,
        }
    }
}

impl From<crate::fs::ResolvedResource> for ResolvedResource {
    fn from(inner: crate::fs::ResolvedResource) -> Self {
        match inner {
            crate::fs::ResolvedResource::File(f) => {
                ResolvedResource::File(ResolvedFile { inner: f })
            }
            crate::fs::ResolvedResource::Directory(d) => {
                ResolvedResource::Directory(ResolvedDirectory { inner: d })
            }
            crate::fs::ResolvedResource::NotFound => ResolvedResource::NotFound,
            crate::fs::ResolvedResource::Denied(r) => ResolvedResource::Denied(r.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecureRoot {
    root: PathBuf,
    policy: StaticPolicy,
}

impl SecureRoot {
    pub fn new(root: impl AsRef<Path>, policy: StaticPolicy) -> Result<Self, std::io::Error> {
        let canonical = std::fs::canonicalize(root.as_ref())?;
        if !canonical.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "root path is not a directory",
            ));
        }
        Ok(Self {
            root: canonical,
            policy,
        })
    }

    #[allow(dead_code)]
    pub fn policy(&self) -> &StaticPolicy {
        &self.policy
    }

    #[allow(dead_code)]
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    #[allow(dead_code)]
    pub fn resolve(&self, path: &ConfinedPath) -> ResolvedResource {
        let guard = match RootGuard::new(&self.root) {
            Ok(g) => g,
            Err(_) => return ResolvedResource::NotFound,
        };
        guard.resolve(path, &self.policy).into()
    }

    #[allow(dead_code)]
    pub fn resolve_uri(&self, uri: &str) -> Result<ResolvedResource, PathRejection> {
        let path_policy = PathPolicy {
            dotfiles: match self.policy.dotfiles {
                crate::policy::DotfilePolicy::Denied => PathPolicy::default().dotfiles,
                crate::policy::DotfilePolicy::Serve => crate::path::DotfilePolicy::Allow,
            },
            reject_backslash: true,
        };
        let confined = ConfinedPath::parse(uri, &path_policy)?;
        Ok(self.resolve(&confined))
    }
}

/// Resolve a confined path against a [`SecureRoot`] and, if the result is a
/// file, plan the response and produce a [`BodySource`] in one call.
///
/// This exists because [`plan_file_response`] requires `&Metadata` (which is
/// only available inside [`ResolvedFile`]) and [`ResolvedFile::into_body`]
/// consumes the file.  Combining both steps here avoids exposing the internal
/// `Metadata` field across crate boundaries.
///
/// Returns `(StaticResponsePlan, BodySource)` on success, or a
/// [`ResourceDeniedReason`] / [`PathRejection`] on failure.
#[allow(dead_code)]
pub fn resolve_and_plan(
    root: &SecureRoot,
    path: &ConfinedPath,
    method: ReadOnlyMethod,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    range_header: Option<&str>,
    if_range: Option<&str>,
) -> Result<(StaticResponsePlan, BodySource), ResolveAndPlanError> {
    let resource = root.resolve(path);
    match resource {
        ResolvedResource::File(file) => {
            let content_type = file.content_type();
            let plan = crate::primitives::planner::plan_file_response(
                method,
                &file.inner.metadata,
                content_type,
                if_none_match,
                if_modified_since,
                range_header,
                if_range,
            );
            let body = file.into_body(&plan)?;
            Ok((plan, body))
        }
        ResolvedResource::Directory(_) => Err(ResolveAndPlanError::IsDirectory),
        ResolvedResource::NotFound => Err(ResolveAndPlanError::NotFound),
        ResolvedResource::Denied(reason) => Err(ResolveAndPlanError::Denied(reason)),
    }
}

/// Errors returned by [`resolve_and_plan`].
#[derive(Debug)]
#[allow(dead_code)]
pub enum ResolveAndPlanError {
    NotFound,
    IsDirectory,
    Denied(ResourceDeniedReason),
    Body(BodySourceError),
}

impl fmt::Display for ResolveAndPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::IsDirectory => write!(f, "is a directory"),
            Self::Denied(r) => write!(f, "{}", r),
            Self::Body(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ResolveAndPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Denied(r) => Some(r),
            Self::Body(e) => Some(e),
            _ => None,
        }
    }
}

impl From<BodySourceError> for ResolveAndPlanError {
    fn from(e: BodySourceError) -> Self {
        Self::Body(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::PathPolicy as PP;
    use crate::policy::{DirectoryListingPolicy, DotfilePolicy, SymlinkPolicy};
    use crate::primitives::body::BodyKind;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, SecureRoot) {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        fs::write(tmp.path().join(".env"), "secret").unwrap();
        fs::write(tmp.path().join("empty.txt"), "").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("file.txt"), "file").unwrap();
        fs::write(
            tmp.path().join("subdir").join("index.html"),
            "<html>hi</html>",
        )
        .unwrap();
        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        (tmp, root)
    }

    fn parse(raw: &str) -> ConfinedPath {
        ConfinedPath::parse(raw, &PP::default()).unwrap()
    }

    // ── Basic construction ──────────────────────────────────────────

    #[test]
    fn secure_root_new_accepts_existing_directory() {
        let tmp = TempDir::new().unwrap();
        let result = SecureRoot::new(tmp.path(), StaticPolicy::safe_default());
        assert!(result.is_ok());
    }

    #[test]
    fn secure_root_new_rejects_missing_path() {
        let result = SecureRoot::new(
            "/nonexistent/path/that/does/not/exist",
            StaticPolicy::safe_default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn secure_root_new_rejects_file_not_directory() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("file.txt");
        fs::write(&path, "content").unwrap();
        let err = SecureRoot::new(&path, StaticPolicy::safe_default()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[test]
    fn secure_root_policy_returns_configured_policy() {
        let (_tmp, root) = setup();
        let policy = root.policy();
        assert_eq!(policy.symlinks, SymlinkPolicy::Denied);
        assert_eq!(policy.dotfiles, DotfilePolicy::Denied);
        assert_eq!(policy.directory_listing, DirectoryListingPolicy::Disabled);
    }

    #[test]
    fn secure_root_root_path_is_canonical() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("a").join("b");
        fs::create_dir_all(&subdir).unwrap();
        let root = SecureRoot::new(&subdir, StaticPolicy::safe_default()).unwrap();
        let rp = root.root_path();
        assert!(rp.is_absolute());
        assert!(rp.exists());
        assert_eq!(rp, fs::canonicalize(&subdir).unwrap());
    }

    // ── File resolution ─────────────────────────────────────────────

    #[test]
    fn resolve_normal_file() {
        let (_tmp, root) = setup();
        let result = root.resolve(&parse("/hello.txt"));
        assert!(result.is_file());
    }

    #[test]
    fn resolve_file_is_empty() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/empty.txt")).into_file().unwrap();
        assert!(file.is_empty());
        assert_eq!(file.len(), 0);
    }

    #[test]
    fn resolve_file_non_empty() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        assert!(!file.is_empty());
        assert_eq!(file.len(), 5);
    }

    #[test]
    fn resolve_file_modified_returns_some() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        assert!(file.modified().is_some());
    }

    #[test]
    fn resolve_file_content_type_matches_extension() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        assert_eq!(file.content_type(), "text/plain; charset=utf-8");
    }

    #[test]
    fn resolve_file_safe_relative_components() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        assert_eq!(file.safe_relative_components(), &["hello.txt"]);
    }

    // ── Directory resolution ────────────────────────────────────────

    #[test]
    fn resolve_normal_directory() {
        let (_tmp, root) = setup();
        let result = root.resolve(&parse("/subdir"));
        assert!(result.is_directory());
    }

    #[test]
    fn resolve_root_path_returns_directory() {
        let (_tmp, root) = setup();
        let result = root.resolve(&parse("/"));
        assert!(result.is_directory());
    }

    #[test]
    fn resolve_directory_components() {
        let (_tmp, root) = setup();
        let dir = root.resolve(&parse("/subdir")).into_directory().unwrap();
        assert_eq!(dir.components(), &["subdir"]);
    }

    // ── NotFound ────────────────────────────────────────────────────

    #[test]
    fn resolve_missing_path() {
        let (_tmp, root) = setup();
        let result = root.resolve(&parse("/nonexistent.txt"));
        assert!(result.is_not_found());
    }

    // ── Dotfile policy ──────────────────────────────────────────────

    #[test]
    fn resolve_dotfile_denied_under_defaults() {
        let (_tmp, root) = setup();
        let pp = PP {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PP::default()
        };
        let path = ConfinedPath::parse("/.env", &pp).unwrap();
        let result = root.resolve(&path);
        match result {
            ResolvedResource::Denied(reason) => {
                assert!(matches!(reason, ResourceDeniedReason::DotfileDenied));
            }
            other => panic!("expected Denied(DotfileDenied), got {:?}", other),
        }
    }

    #[test]
    fn resolve_dotfile_allowed_when_policy_permits() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".env"), "secret").unwrap();
        let mut policy = StaticPolicy::safe_default();
        policy.dotfiles = DotfilePolicy::Serve;
        let root = SecureRoot::new(tmp.path(), policy).unwrap();

        let pp = PP {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PP::default()
        };
        let path = ConfinedPath::parse("/.env", &pp).unwrap();
        let result = root.resolve(&path);
        assert!(result.is_file());
    }

    // ── Symlink policy (cfg(unix)) ──────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn resolve_symlink_denied_under_defaults() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let result = root.resolve(&parse("/link.txt"));
        match result {
            ResolvedResource::Denied(reason) => {
                assert!(matches!(reason, ResourceDeniedReason::SymlinkDenied));
            }
            other => panic!("expected Denied(SymlinkDenied), got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_symlink_allowed_when_follow_enabled() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let mut policy = StaticPolicy::safe_default();
        policy.symlinks = SymlinkPolicy::Follow;
        let root = SecureRoot::new(tmp.path(), policy).unwrap();

        let pp = PP {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PP::default()
        };
        let path = ConfinedPath::parse("/link.txt", &pp).unwrap();
        let result = root.resolve(&path);
        assert!(result.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_intermediate_symlink_denied() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("real_dir")).unwrap();
        fs::write(tmp.path().join("real_dir").join("file.txt"), "content").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir"))
            .unwrap();

        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let result = root.resolve(&parse("/link_dir/file.txt"));
        match result {
            ResolvedResource::Denied(reason) => {
                assert!(matches!(reason, ResourceDeniedReason::SymlinkDenied));
            }
            other => panic!("expected Denied(SymlinkDenied), got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_outside_root_symlink_denied_when_follow_enabled() {
        let tmp_root = TempDir::new().unwrap();
        let tmp_outside = TempDir::new().unwrap();
        fs::write(tmp_outside.path().join("secret.txt"), "leaked").unwrap();
        std::os::unix::fs::symlink(
            tmp_outside.path().join("secret.txt"),
            tmp_root.path().join("escape.txt"),
        )
        .unwrap();

        let mut policy = StaticPolicy::safe_default();
        policy.symlinks = SymlinkPolicy::Follow;
        let root = SecureRoot::new(tmp_root.path(), policy).unwrap();

        let pp = PP {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PP::default()
        };
        let path = ConfinedPath::parse("/escape.txt", &pp).unwrap();
        let result = root.resolve(&path);
        match result {
            ResolvedResource::Denied(reason) => {
                assert!(matches!(reason, ResourceDeniedReason::RootEscapeDenied));
            }
            other => panic!("expected Denied(RootEscapeDenied), got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn directory_listing_hides_symlinks_under_defaults() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let dir = root.resolve(&parse("/")).into_directory().unwrap();
        let entries = dir.list(&root).unwrap();
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"real.txt"));
        assert!(!names.contains(&"link.txt"));
    }

    // ── Directory child resolution ──────────────────────────────────

    #[test]
    fn directory_resolve_child_index() {
        let (_tmp, root) = setup();
        let dir = root.resolve(&parse("/subdir")).into_directory().unwrap();
        let result = dir.resolve_child("index.html", &root);
        assert!(result.is_file());
    }

    #[test]
    fn directory_resolve_child_missing() {
        let (_tmp, root) = setup();
        let dir = root.resolve(&parse("/subdir")).into_directory().unwrap();
        let result = dir.resolve_child("missing.txt", &root);
        assert!(result.is_not_found());
    }

    #[test]
    fn directory_resolve_child_dotfile_denied() {
        let (_tmp, root) = setup();
        fs::write(root.root_path().join("subdir").join(".env"), "secret").unwrap();
        let dir = root.resolve(&parse("/subdir")).into_directory().unwrap();
        let result = dir.resolve_child(".env", &root);
        match result {
            ResolvedResource::Denied(reason) => {
                assert!(matches!(reason, ResourceDeniedReason::DotfileDenied));
            }
            other => panic!("expected Denied(DotfileDenied), got {:?}", other),
        }
    }

    // ── Directory listing ───────────────────────────────────────────

    #[test]
    fn directory_list_returns_entries() {
        let (_tmp, root) = setup();
        let dir = root.resolve(&parse("/subdir")).into_directory().unwrap();
        let entries = dir.list(&root).unwrap();
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"file.txt"));
        assert!(names.contains(&"index.html"));
    }

    #[test]
    fn directory_list_hides_dotfiles_under_defaults() {
        let (_tmp, root) = setup();
        fs::write(root.root_path().join("subdir").join(".env"), "secret").unwrap();
        let dir = root.resolve(&parse("/subdir")).into_directory().unwrap();
        let entries = dir.list(&root).unwrap();
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert!(!names.contains(&".env"));
    }

    // ── URI resolution ──────────────────────────────────────────────

    #[test]
    fn resolve_uri_simple_file() {
        let (_tmp, root) = setup();
        let result = root.resolve_uri("/hello.txt").unwrap();
        assert!(result.is_file());
    }

    #[test]
    fn resolve_uri_rejects_dotfile_path() {
        let (_tmp, root) = setup();
        let result = root.resolve_uri("/.env");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), PathRejection::DotfileDenied);
    }

    #[test]
    fn resolve_uri_rejects_traversal() {
        let (_tmp, root) = setup();
        let result = root.resolve_uri("/../etc/passwd");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), PathRejection::ParentComponent);
    }

    // ── Resource accessors ──────────────────────────────────────────

    #[test]
    fn resolved_resource_is_file() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/hello.txt"));
        assert!(r.is_file());
        assert!(!r.is_directory());
        assert!(!r.is_not_found());
        assert!(!r.is_denied());
    }

    #[test]
    fn resolved_resource_is_directory() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/subdir"));
        assert!(r.is_directory());
        assert!(!r.is_file());
        assert!(!r.is_not_found());
        assert!(!r.is_denied());
    }

    #[test]
    fn resolved_resource_is_not_found() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/missing.txt"));
        assert!(r.is_not_found());
        assert!(!r.is_file());
        assert!(!r.is_directory());
        assert!(!r.is_denied());
    }

    #[test]
    fn resolved_resource_is_denied() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".env"), "secret").unwrap();
        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let pp = PP {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PP::default()
        };
        let path = ConfinedPath::parse("/.env", &pp).unwrap();
        let r = root.resolve(&path);
        assert!(r.is_denied());
        assert!(!r.is_file());
        assert!(!r.is_directory());
        assert!(!r.is_not_found());
    }

    #[test]
    fn resolved_resource_as_file() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/hello.txt"));
        assert!(r.as_file().is_some());
        assert!(r.as_directory().is_none());
    }

    #[test]
    fn resolved_resource_as_directory() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/subdir"));
        assert!(r.as_directory().is_some());
        assert!(r.as_file().is_none());
    }

    #[test]
    fn resolved_resource_into_file() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/hello.txt"));
        assert!(r.into_file().is_some());
    }

    #[test]
    fn resolved_resource_into_file_returns_none_for_directory() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/subdir"));
        assert!(r.into_file().is_none());
    }

    #[test]
    fn resolved_resource_into_directory() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/subdir"));
        assert!(r.into_directory().is_some());
    }

    #[test]
    fn resolved_resource_into_directory_returns_none_for_file() {
        let (_tmp, root) = setup();
        let r = root.resolve(&parse("/hello.txt"));
        assert!(r.into_directory().is_none());
    }

    // ── ResourceDeniedReason ────────────────────────────────────────

    #[test]
    fn resource_denied_reason_display() {
        let cases: &[(ResourceDeniedReason, &str)] = &[
            (ResourceDeniedReason::SymlinkDenied, "symlink denied"),
            (ResourceDeniedReason::DotfileDenied, "dotfile denied"),
            (ResourceDeniedReason::RootEscapeDenied, "root escape denied"),
            (
                ResourceDeniedReason::PolicyDenied(PathRejection::Empty),
                "empty path",
            ),
        ];
        for (reason, expected) in cases {
            assert_eq!(reason.to_string(), *expected);
        }
    }

    #[test]
    fn resource_denied_reason_from_path_rejection() {
        let cases: &[(PathRejection, ResourceDeniedReason)] = &[
            (
                PathRejection::SymlinkDenied,
                ResourceDeniedReason::SymlinkDenied,
            ),
            (
                PathRejection::DotfileDenied,
                ResourceDeniedReason::DotfileDenied,
            ),
            (
                PathRejection::RootEscapeDenied,
                ResourceDeniedReason::RootEscapeDenied,
            ),
        ];
        for (rejection, expected) in cases {
            let reason: ResourceDeniedReason = (*rejection).into();
            assert!(std::mem::discriminant(&reason) == std::mem::discriminant(expected));
        }

        // Non-special rejections become PolicyDenied
        let reason: ResourceDeniedReason = PathRejection::ParentComponent.into();
        assert!(matches!(
            reason,
            ResourceDeniedReason::PolicyDenied(PathRejection::ParentComponent)
        ));
    }

    // ── File parts ──────────────────────────────────────────────────

    #[test]
    fn resolved_file_into_std_file() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let std_file = file.into_std_file();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "hello");
    }

    #[test]
    fn resolved_file_into_parts() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let (std_file, metadata) = file.into_parts();
        assert!(metadata.is_file());
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "hello");
    }

    // ── Body source conversion ─────────────────────────────────────

    #[test]
    fn into_body_file_full() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let plan = StaticResponsePlan {
            status: crate::primitives::response::ResponseStatus::OK,
            headers: crate::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::FileFull,
        };
        let mut body = file.into_body(&plan).unwrap();
        assert_eq!(body.kind(), BodyKind::FileFull);
        assert_eq!(body.len(), 5);
        assert_eq!(body.read_all().unwrap(), b"hello");
    }

    #[test]
    fn into_body_empty() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let plan = StaticResponsePlan {
            status: crate::primitives::response::ResponseStatus::OK,
            headers: crate::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::Empty,
        };
        let body = file.into_body(&plan).unwrap();
        assert_eq!(body.kind(), BodyKind::Empty);
        assert!(body.is_empty());
    }

    #[test]
    fn into_body_file_range() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let plan = StaticResponsePlan {
            status: crate::primitives::response::ResponseStatus::PARTIAL_CONTENT,
            headers: crate::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::FileRange {
                start: 0,
                end_inclusive: 2,
            },
        };
        let mut body = file.into_body(&plan).unwrap();
        assert_eq!(body.kind(), BodyKind::FileRange);
        assert_eq!(body.len(), 3);
        assert_eq!(body.read_all().unwrap(), b"hel");
    }

    #[test]
    fn into_body_file_range_invalid() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let plan = StaticResponsePlan {
            status: crate::primitives::response::ResponseStatus::PARTIAL_CONTENT,
            headers: crate::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::FileRange {
                start: 0,
                end_inclusive: 999,
            },
        };
        let err = file.into_body(&plan).unwrap_err();
        assert!(matches!(err, BodySourceError::InvalidRange));
    }

    #[test]
    fn into_range_body_valid() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let mut body = file.into_range_body(1, 3).unwrap();
        assert_eq!(body.kind(), BodyKind::FileRange);
        assert_eq!(body.len(), 3);
        assert_eq!(body.read_all().unwrap(), b"ell");
    }

    #[test]
    fn into_range_body_invalid() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let err = file.into_range_body(0, 999).unwrap_err();
        assert!(matches!(err, BodySourceError::InvalidRange));
    }

    #[test]
    fn into_body_prevents_double_use() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let plan = StaticResponsePlan {
            status: crate::primitives::response::ResponseStatus::OK,
            headers: crate::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::FileFull,
        };
        let _body = file.into_body(&plan).unwrap();
        // file is consumed — cannot use again
    }

    #[test]
    fn into_body_bytes_variant() {
        let (_tmp, root) = setup();
        let file = root.resolve(&parse("/hello.txt")).into_file().unwrap();
        let plan = StaticResponsePlan {
            status: crate::primitives::response::ResponseStatus::OK,
            headers: crate::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::FullBytes(b"custom".to_vec()),
        };
        let mut body = file.into_body(&plan).unwrap();
        assert_eq!(body.kind(), BodyKind::Bytes);
        assert_eq!(body.read_all().unwrap(), b"custom");
    }
}
