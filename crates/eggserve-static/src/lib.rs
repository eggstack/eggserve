//! Hardened static-file serving as an optional EggServe specialization.
//!
//! `eggserve-server` does not depend on this crate. Applications that need
//! static files opt in by adding this crate and passing [`StaticService`] to
//! the generic runtime. The resolver rejects traversal, dotfiles, and
//! symlink components by default, and retains the configured root boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eggserve_primitives::{Request, Response, ResponseBody, StatusCode};
use eggserve_server::{Service, ServiceError, ServiceFuture};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DirectoryListingPolicy {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SymlinkPolicy {
    #[default]
    Denied,
    Follow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DotfilePolicy {
    #[default]
    Denied,
    Serve,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StaticPolicy {
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
}

/// A validated root directory. The root is canonicalized once and every
/// request is checked component-by-component before it is opened.
#[derive(Debug, Clone)]
pub struct SecureRoot {
    root: Arc<PathBuf>,
}
impl SecureRoot {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Ok(Self {
            root: Arc::new(std::fs::canonicalize(root)?),
        })
    }
    pub fn path(&self) -> &Path {
        self.root.as_path()
    }
    fn resolve(&self, target: &str, policy: StaticPolicy) -> Result<PathBuf, ResolveError> {
        let path = target.split('?').next().unwrap_or(target);
        if !path.starts_with('/') {
            return Err(ResolveError::InvalidTarget);
        }
        let mut resolved = self.root.as_ref().clone();
        for component in path.split('/').filter(|part| !part.is_empty()) {
            if component == "." || component == ".." || component.contains('\\') {
                return Err(ResolveError::Traversal);
            }
            if component.starts_with('.') && policy.dotfiles == DotfilePolicy::Denied {
                return Err(ResolveError::Denied);
            }
            resolved.push(component);
            if policy.symlinks == SymlinkPolicy::Denied
                && std::fs::symlink_metadata(&resolved)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
            {
                return Err(ResolveError::Denied);
            }
        }
        if policy.symlinks == SymlinkPolicy::Follow {
            if let Ok(canonical) = std::fs::canonicalize(&resolved) {
                if !canonical.starts_with(self.root.as_path()) {
                    return Err(ResolveError::Denied);
                }
            }
        }
        Ok(resolved)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolveError {
    InvalidTarget,
    Traversal,
    Denied,
}

#[derive(Clone)]
pub struct StaticService {
    root: SecureRoot,
    policy: StaticPolicy,
    default_content_type: String,
}
impl StaticService {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Self::with_policy(root, StaticPolicy::default())
    }
    pub fn with_policy(
        root: impl AsRef<Path>,
        policy: StaticPolicy,
    ) -> Result<Self, std::io::Error> {
        Ok(Self {
            root: SecureRoot::new(root)?,
            policy,
            default_content_type: "application/octet-stream".into(),
        })
    }
    pub fn default_content_type(mut self, value: impl Into<String>) -> Self {
        self.default_content_type = value.into();
        self
    }
    pub fn root(&self) -> &SecureRoot {
        &self.root
    }
    async fn respond(&self, request: Request) -> Result<Response, ServiceError> {
        if !request.head.method.permits_static_resolution() {
            return Ok(Response::text(
                StatusCode::METHOD_NOT_ALLOWED,
                b"method not allowed\n".to_vec(),
            ));
        }
        let path = self
            .root
            .resolve(request.head.target.as_str(), self.policy)
            .map_err(|_| ServiceError::Rejected(403))?;
        let metadata = std::fs::metadata(&path).map_err(|_| ServiceError::Rejected(404))?;
        let path = if metadata.is_dir() {
            let index = path.join("index.html");
            if !index.is_file() {
                if self.policy.directory_listing == DirectoryListingPolicy::Enabled {
                    return Ok(Response::text(
                        StatusCode::OK,
                        b"<html><body>directory listing disabled in topology fixture</body></html>"
                            .to_vec(),
                    ));
                }
                return Err(ServiceError::Rejected(403));
            }
            index
        } else {
            path
        };
        let body = std::fs::read(&path).map_err(|_| ServiceError::Rejected(404))?;
        let mut response = Response::new(StatusCode::OK, ResponseBody::Bytes(body));
        let content_type = mime_for_path(&path, &self.default_content_type);
        response
            .headers
            .push_str("content-type", content_type.as_bytes())
            .map_err(|_| ServiceError::internal("static content type is invalid"))?;
        Ok(response)
    }
}
impl Service for StaticService {
    fn call(&self, request: Request) -> ServiceFuture<'_> {
        Box::pin(self.respond(request))
    }
}

fn mime_for_path<'a>(path: &Path, fallback: &'a str) -> &'a str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_policy_is_the_default() {
        let policy = StaticPolicy::default();
        assert_eq!(policy.symlinks, SymlinkPolicy::Denied);
        assert_eq!(policy.dotfiles, DotfilePolicy::Denied);
    }
}
