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

/// Builder for the compatibility [`StaticService`].
///
/// The builder collects the static-service settings and projects them into a
/// [`ServeConfig`]. Every setting starts at its safe default: the safe
/// [`StaticPolicy`], `application/octet-stream` as the default content type,
/// no extra response headers, and the
/// [`Minimal`](crate::policy::ErrorRepresentationPolicy::Minimal) error
/// representation.
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
    /// Set the static path policy.
    ///
    /// Replaces the default [`StaticPolicy::safe_default()`]. Every optional
    /// behavior the policy enables — directory listing, symlink following,
    /// dotfile serving — is opted into explicitly here.
    pub fn policy(mut self, policy: StaticPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set the content type substituted for the MIME authority's generic
    /// `application/octet-stream` result.
    ///
    /// The MIME authority reports `application/octet-stream` for a file whose
    /// extension it does not recognize; this value replaces that result.
    pub fn default_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.default_content_type = content_type.into();
        self
    }

    /// Set the extra `(name, value)` response headers.
    ///
    /// A name the response plan already set is not overwritten. The list is
    /// validated when the service is built, so runtime-owned and
    /// representation-owned names are rejected before the server starts.
    pub fn extra_response_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_response_headers = headers;
        self
    }

    /// Set the representation policy for runtime-generated static errors.
    ///
    /// The policy governs the bodies of runtime-generated errors (403, 404,
    /// 405, …) only; successful application bodies are never rewritten.
    pub fn error_policy(mut self, policy: crate::policy::ErrorRepresentationPolicy) -> Self {
        self.error_policy = policy;
        self
    }

    /// Set the ops context that receives this service's runtime events.
    ///
    /// Without this call the service uses the global ops context.
    pub fn ops_context(mut self, ops: crate::ops::OpsContext) -> Self {
        self.ops = Some(ops);
        self
    }

    /// Build the static service.
    ///
    /// # Errors
    ///
    /// Returns an internal [`ServiceError`] (mapping to 500) when the
    /// configuration is rejected: the static metadata is invalid, or the
    /// root cannot be resolved and pinned.
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

/// Compatibility static service.
///
/// The service delegates request planning, path confinement, listing
/// rendering, and response metadata to the `eggserve-static` authority; this
/// type keeps only the historical construction paths and the ops context the
/// runtime events are emitted through.
#[derive(Clone)]
pub struct StaticService {
    inner: eggserve_static::StaticService,
    ops: crate::ops::OpsContext,
}

impl StaticService {
    /// Start a builder for a service serving `root`.
    ///
    /// The builder starts from the safe defaults; see
    /// [`StaticServiceBuilder`].
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
    pub(crate) fn from_state(state: Arc<ServeState>) -> Result<Self, std::io::Error> {
        Self::from_state_with_ops(state, crate::ops::OpsContext::global().clone())
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
