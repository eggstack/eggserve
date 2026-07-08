use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::limits::Limits;
use crate::policy::StaticPolicy;

#[derive(Debug, Clone)]
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

pub struct ServeState {
    pub config: Arc<ServeConfig>,
    pub file_stream_semaphore: Arc<Semaphore>,
}

impl ServeState {
    pub fn new(config: Arc<ServeConfig>) -> Self {
        let file_stream_semaphore = Arc::new(Semaphore::new(config.limits.max_file_streams));
        Self {
            config,
            file_stream_semaphore,
        }
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
}
