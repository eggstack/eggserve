//! Compatibility static-service facade.
//!
//! Static request planning, confinement, listing rendering, metadata, and
//! capability continuity are owned by \`eggserve-static\`. This module keeps
//! the historical builder and \`ServeConfig\` projections while delegating
//! every request to that direct authority.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{ServeConfig, ServeState};
use crate::policy::StaticPolicy;
use crate::server::service::{Service, ServiceError};

#[derive(Debug)]
#[must_use]
pub struct StaticServiceBuilder {
    root: PathBuf,
    policy: StaticPolicy,
    default_content_type: String,
    extra_response_headers: Vec<(String, String)>,
    error_policy: crate::policy::ErrorRepresentationPolicy,
    ops: Option<crate::ops::OpsContext>,
}

impl StaticServiceBuilder {
    pub fn policy(mut self, policy: StaticPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn default_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.default_content_type = content_type.into();
        self
    }

    pub fn extra_response_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_response_headers = headers;
        self
    }

    pub fn error_policy(mut self, policy: crate::policy::ErrorRepresentationPolicy) -> Self {
        self.error_policy = policy;
        self
    }

    pub fn ops_context(mut self, ops: crate::ops::OpsContext) -> Self {
        self.ops = Some(ops);
        self
    }

    pub fn build(self) -> Result<StaticService, ServiceError> {
        let config = Arc::new(ServeConfig {
            root: self.root,
            static_policy: self.policy,
            default_content_type: self.default_content_type,
            extra_response_headers: self.extra_response_headers,
            error_policy: self.error_policy,
            ..ServeConfig::default()
        });
        let ops = self
            .ops
            .unwrap_or_else(|| crate::ops::OpsContext::global().clone());
        StaticService::from_serve_config_with_ops(config, ops).map_err(|error| {
            ServiceError::internal(format!("failed to initialize static root: {error}"))
        })
    }
}

#[derive(Clone)]
pub struct StaticService {
    inner: eggserve_static::StaticService,
    ops: crate::ops::OpsContext,
}

impl StaticService {
    pub fn builder(root: impl AsRef<Path>) -> StaticServiceBuilder {
        StaticServiceBuilder {
            root: root.as_ref().to_path_buf(),
            policy: StaticPolicy::safe_default(),
            default_content_type: "application/octet-stream".to_owned(),
            extra_response_headers: Vec::new(),
            error_policy: crate::policy::ErrorRepresentationPolicy::Minimal,
            ops: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_serve_config(config: Arc<ServeConfig>) -> Result<Self, std::io::Error> {
        Self::from_serve_config_with_ops(config, crate::ops::OpsContext::global().clone())
    }

    pub(crate) fn from_serve_config_with_ops(
        config: Arc<ServeConfig>,
        ops: crate::ops::OpsContext,
    ) -> Result<Self, std::io::Error> {
        let state = Arc::new(ServeState::new(config)?);
        Self::from_state_with_ops(state, ops)
    }

    #[allow(dead_code)]
    pub(crate) fn from_state(state: Arc<ServeState>) -> Self {
        Self::from_state_with_ops(state, crate::ops::OpsContext::global().clone())
            .expect("validated static state must project into direct service")
    }

    fn from_state_with_ops(
        state: Arc<ServeState>,
        ops: crate::ops::OpsContext,
    ) -> Result<Self, std::io::Error> {
        let config = state.config();
        let inner = eggserve_static::StaticService::from_root(
            Arc::new(state.secure_root.clone()),
            config.default_content_type.clone(),
            config.extra_response_headers.clone(),
            config.error_policy,
            config.limits.max_listing_entries,
            config.limits.max_listing_response_bytes,
        );
        ops.emit(crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::RootInitialized,
            "root initialized",
        ));
        Ok(Self { inner, ops })
    }

    #[allow(dead_code)]
    pub(crate) fn ops(&self) -> &crate::ops::OpsContext {
        &self.ops
    }
}

impl Service for StaticService {
    fn request_body_policy(
        &self,
        head: &crate::primitives::RequestHead,
    ) -> crate::primitives::RequestBodyPolicy {
        self.inner.request_body_policy(head)
    }

    fn call(
        &self,
        request: crate::primitives::Request,
    ) -> crate::server::service::ServiceFuture<'_> {
        self.inner.call(request)
    }
}
