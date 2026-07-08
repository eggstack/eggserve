//! Confined path resolution ensuring requests stay within the serving root.

/// A path that has been validated to stay within the configured root directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinedPath {
    /// The original request path (percent-decoded).
    pub request_path: String,
    /// The resolved absolute filesystem path.
    pub resolved: std::path::PathBuf,
}
