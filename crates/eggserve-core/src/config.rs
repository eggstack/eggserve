//! Configuration types for static file serving.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::fs::PinnedRoot;
use crate::limits::Limits;
use crate::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy, SymlinkPolicy};

#[derive(Debug, Clone)]
#[must_use]
pub struct ServeConfig {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub limits: Limits,
    pub static_policy: StaticPolicy,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            root: PathBuf::from("."),
            limits: Limits::default(),
            static_policy: StaticPolicy::safe_default(),
        }
    }
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

pub struct ServeState {
    pub(crate) config: Arc<ServeConfig>,
    pub(crate) pinned_root: Arc<PinnedRoot>,
    pub(crate) file_stream_semaphore: Arc<Semaphore>,
}

impl ServeState {
    pub fn new(config: Arc<ServeConfig>) -> Result<Self, std::io::Error> {
        let pinned_root = Arc::new(PinnedRoot::new(&config.root)?);
        let file_stream_semaphore = Arc::new(Semaphore::new(config.limits.max_file_streams));
        Ok(Self {
            config,
            pinned_root,
            file_stream_semaphore,
        })
    }

    pub fn config(&self) -> &Arc<ServeConfig> {
        &self.config
    }

    pub(crate) fn pinned_root(&self) -> &Arc<PinnedRoot> {
        &self.pinned_root
    }

    pub fn file_stream_semaphore(&self) -> &Arc<Semaphore> {
        &self.file_stream_semaphore
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
}
