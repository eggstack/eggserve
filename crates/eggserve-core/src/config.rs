//! Configuration types for static file serving.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::fs::PinnedRoot;
use crate::limits::Limits;
use crate::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy, SymlinkPolicy};
use crate::primitives::canonical::is_hop_by_hop_header;
use crate::primitives::header_block::{HeaderName, HeaderValue};

#[derive(Debug, Clone)]
#[must_use]
pub struct ServeConfig {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub limits: Limits,
    pub static_policy: StaticPolicy,
    pub default_content_type: String,
    pub extra_response_headers: Vec<(String, String)>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            root: PathBuf::from("."),
            limits: Limits::default(),
            static_policy: StaticPolicy::safe_default(),
            default_content_type: "application/octet-stream".to_string(),
            extra_response_headers: Vec::new(),
        }
    }
}

/// Validate static representation metadata before a server is activated.
pub fn validate_static_metadata(
    default_content_type: &str,
    extra_response_headers: &[(String, String)],
) -> Result<(), String> {
    let content_type = HeaderValue::new(default_content_type.trim())
        .map_err(|e| format!("invalid default content type: {e}"))?;
    if content_type.as_str().is_empty() {
        return Err("default content type must be a non-empty value without CR/LF/NUL".into());
    }
    for (name, value) in extra_response_headers {
        HeaderName::new(name.clone()).map_err(|e| format!("invalid extra response header: {e}"))?;
        if value.trim().is_empty() {
            return Err(format!(
                "invalid extra response header: value for {name} must contain non-whitespace"
            ));
        }
        let value = HeaderValue::new(value.clone())
            .map_err(|e| format!("invalid extra response header: {e}"))?;
        if value.as_str().is_empty() {
            return Err(format!(
                "invalid extra response header: value for {name} must not be empty"
            ));
        }
        let lower = name.to_ascii_lowercase();
        if is_hop_by_hop_header(&lower)
            || matches!(
                lower.as_str(),
                "content-length"
                    | "date"
                    | "server"
                    | "content-type"
                    | "content-range"
                    | "accept-ranges"
                    | "etag"
                    | "last-modified"
                    | "x-content-type-options"
            )
        {
            return Err(format!(
                "extra response header is runtime- or representation-owned: {name}"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct StartupSummary {
    pub bind_is_unspecified: bool,
    pub directory_listing_enabled: bool,
    pub symlinks_followed: bool,
    pub dotfiles_served: bool,
    pub max_connections: usize,
    pub max_file_streams: usize,
}

impl ServeConfig {
    /// Build a logging-friendly summary of this configuration.
    ///
    /// The binary crate uses this to print a startup banner. Callers that
    /// embed `eggserve-core` directly can use it for their own logging.
    pub fn startup_summary(&self) -> StartupSummary {
        StartupSummary {
            bind_is_unspecified: self.bind.ip().is_unspecified(),
            directory_listing_enabled: matches!(
                self.static_policy.directory_listing,
                DirectoryListingPolicy::Enabled
            ),
            symlinks_followed: matches!(self.static_policy.symlinks, SymlinkPolicy::Follow),
            dotfiles_served: matches!(self.static_policy.dotfiles, DotfilePolicy::Serve),
            max_connections: self.limits.max_connections,
            max_file_streams: self.limits.max_file_streams,
        }
    }
}

#[derive(Clone)]
pub struct ServeState {
    pub(crate) config: Arc<ServeConfig>,
    pub(crate) pinned_root: Arc<PinnedRoot>,
}

impl ServeState {
    pub fn new(config: Arc<ServeConfig>) -> Result<Self, std::io::Error> {
        validate_static_metadata(&config.default_content_type, &config.extra_response_headers)
            .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
        let pinned_root = Arc::new(PinnedRoot::new(&config.root)?);
        Ok(Self {
            config,
            pinned_root,
        })
    }

    pub fn config(&self) -> &Arc<ServeConfig> {
        &self.config
    }

    pub(crate) fn pinned_root(&self) -> &Arc<PinnedRoot> {
        &self.pinned_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_binds_loopback() {
        let config = ServeConfig::default();
        assert!(config.bind.ip().is_loopback());
    }

    #[test]
    fn default_config_binds_port_8000() {
        let config = ServeConfig::default();
        assert_eq!(config.bind.port(), 8000);
    }

    #[test]
    fn default_startup_summary_is_safe() {
        let summary = ServeConfig::default().startup_summary();
        assert!(!summary.bind_is_unspecified);
        assert!(!summary.directory_listing_enabled);
        assert!(!summary.symlinks_followed);
        assert!(!summary.dotfiles_served);
        assert_eq!(summary.max_connections, 64);
        assert_eq!(summary.max_file_streams, 32);
    }

    #[test]
    fn extra_response_header_rejects_whitespace_only_value() {
        let error = validate_static_metadata(
            "application/octet-stream",
            &[("X-Test".to_owned(), " \t ".to_owned())],
        )
        .unwrap_err();
        assert!(error.contains("value for X-Test must contain non-whitespace"));
    }

    #[test]
    fn extra_response_header_accepts_nonempty_value_with_ows() {
        assert!(validate_static_metadata(
            "application/octet-stream",
            &[("X-Test".to_owned(), " value ".to_owned())],
        )
        .is_ok());
    }

    #[test]
    fn default_content_type_rejects_whitespace_only_value() {
        let error = validate_static_metadata(" \t ", &[]).unwrap_err();
        assert!(error.contains("default content type must be a non-empty value"));
    }
}
