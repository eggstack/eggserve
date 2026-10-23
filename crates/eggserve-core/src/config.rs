//! Configuration types for static file serving.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::limits::Limits;
use crate::policy::{
    DirectoryListingPolicy, DotfilePolicy, ErrorRepresentationPolicy, StaticPolicy, SymlinkPolicy,
};
use crate::primitives::canonical::is_hop_by_hop_header;
use crate::primitives::header_block::{HeaderName, HeaderValue};
// Plan 219: the pinned-root/filesystem implementation lives once in
// `eggserve-static`; `ServeState` retains the static `SecureRoot` capability
// through the primitives facade instead of a second core resolver.
use crate::primitives::SecureRoot;

/// Static-serving configuration for one server instance.
///
/// `ServeConfig` carries the listen address, the served root, the resource
/// [`Limits`], the [`StaticPolicy`], and the operator-supplied response
/// metadata that the runtime validates before a listener is activated.
///
/// The [`Default`] values are the safe defaults: loopback bind, the current
/// directory as root, no directory listing, no symlink following, and no
/// dotfile serving. [`ServeConfig::startup_summary`] reports the effective
/// values.
#[derive(Debug, Clone)]
#[must_use]
pub struct ServeConfig {
    /// Listen address of the server socket.
    ///
    /// Defaults to `127.0.0.1:8000`. A wildcard address (`0.0.0.0`) is an
    /// explicit opt-out from the loopback default and is reported by
    /// [`StartupSummary::bind_is_unspecified`].
    pub bind: SocketAddr,
    /// Directory tree served by this instance.
    ///
    /// The path is resolved and pinned once, when the server starts, so a
    /// later filesystem change cannot move the served tree. Defaults to the
    /// current working directory.
    pub root: PathBuf,
    /// Resource ceilings applied to connections, streams, and request sizes.
    pub limits: Limits,
    /// Static path policy: directory listing, symlink, and dotfile handling.
    ///
    /// Defaults to [`StaticPolicy::safe_default()`], which denies every
    /// optional behavior.
    pub static_policy: StaticPolicy,
    /// Content type substituted for the MIME authority's generic
    /// `application/octet-stream` result.
    ///
    /// The MIME authority reports `application/octet-stream` for a file whose
    /// extension it does not recognize; this value replaces that result in
    /// the response. Defaults to `application/octet-stream`, so the
    /// substitution is a no-op unless an operator sets it.
    pub default_content_type: String,
    /// Extra `(name, value)` response headers added to every response.
    ///
    /// A name the response plan already set is not overwritten. The list is
    /// checked by [`validate_static_metadata`] before the server is
    /// activated: runtime-owned and representation-owned names (hop-by-hop
    /// fields, `Content-Length`, `Date`, `Server`, `Content-Type`,
    /// `Content-Range`, `Accept-Ranges`, `ETag`, `Last-Modified`, and
    /// `X-Content-Type-Options`) are rejected. Defaults to empty.
    pub extra_response_headers: Vec<(String, String)>,
    /// Canonical runtime-error representation for static errors (403/404/405…).
    /// Default: `Minimal`. `Empty` emits no body bytes. Application `Ok`
    /// bodies are never rewritten. Transferred to `RuntimeConfig` by
    /// `try_from_serve_config` so `serve_config()` static errors share the
    /// runtime error profile.
    pub error_policy: ErrorRepresentationPolicy,
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
            error_policy: ErrorRepresentationPolicy::Minimal,
        }
    }
}

/// Validate static representation metadata before a server is activated.
pub fn validate_static_metadata(
    default_content_type: &str,
    extra_response_headers: &[(String, String)],
) -> Result<(), String> {
    let limits = Limits::default();
    validate_static_metadata_with_limits(
        default_content_type,
        extra_response_headers,
        limits.max_extra_headers,
        limits.max_extra_header_bytes,
    )
}

pub(crate) fn validate_static_metadata_with_limits(
    default_content_type: &str,
    extra_response_headers: &[(String, String)],
    max_extra_headers: usize,
    max_extra_header_bytes: usize,
) -> Result<(), String> {
    // `HeaderValue::new` trims OWS (SP/HTAB) and rejects transport-refused
    // bytes; no outer `trim()` is needed — passing the raw value preserves
    // the precise `InvalidValue` signal for leading/trailing control bytes.
    // Byte limits below count bytes, not Unicode scalars.
    let content_type = HeaderValue::new(default_content_type)
        .map_err(|e| format!("invalid default content type: {e}"))?;
    if content_type.is_empty() {
        return Err("default content type must be a non-empty value without CR/LF/NUL".into());
    }
    if extra_response_headers.len() > max_extra_headers {
        return Err(format!(
            "too many extra response headers: maximum is {max_extra_headers}"
        ));
    }
    let total_header_bytes =
        extra_response_headers
            .iter()
            .try_fold(0usize, |total, (name, value)| {
                total
                    .checked_add(name.len())
                    .and_then(|total| total.checked_add(value.len()))
            });
    if total_header_bytes.is_none_or(|total| total > max_extra_header_bytes) {
        return Err(format!(
            "extra response headers exceed maximum size of {max_extra_header_bytes} bytes"
        ));
    }
    for (name, value) in extra_response_headers {
        HeaderName::new(name.clone()).map_err(|e| format!("invalid extra response header: {e}"))?;
        // `HeaderValue::new` canonicalizes by trimming SP/HTAB OWS; a value
        // that is empty after that trimming is whitespace-only and must be
        // rejected. No separate `value.trim()` check is needed — the stored
        // header is later canonicalized again at emission time
        // (`static_service::append_extra_headers` and Python
        // `apply_static_metadata` trim via `HeaderValue`), so validation and
        // wire value agree.
        let canonical = HeaderValue::new(value.clone())
            .map_err(|e| format!("invalid extra response header: {e}"))?;
        if canonical.is_empty() {
            return Err(format!(
                "invalid extra response header: value for {name} must contain non-whitespace"
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

/// Logging-friendly summary of the effective [`ServeConfig`].
///
/// The summary reports the security-relevant switches an operator should
/// surface at startup — an unspecified bind address, directory listing,
/// symlink following, dotfile serving — plus the concurrency ceilings.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct StartupSummary {
    /// `true` when the bind address uses a wildcard IP, so the server is
    /// reachable from outside the host.
    pub bind_is_unspecified: bool,
    /// `true` when directory listings are rendered.
    pub directory_listing_enabled: bool,
    /// `true` when symbolic links inside the root are followed.
    pub symlinks_followed: bool,
    /// `true` when dotfiles are served.
    pub dotfiles_served: bool,
    /// Maximum concurrent connections.
    pub max_connections: usize,
    /// Maximum concurrent file-stream responses.
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

/// Validated serving state shared by the server's components.
///
/// `ServeState` pairs the configuration with the [`SecureRoot`] capability
/// resolved from it. The root is resolved and pinned once, at construction,
/// so later filesystem changes cannot move the served tree.
#[derive(Clone)]
pub struct ServeState {
    pub(crate) config: Arc<ServeConfig>,
    pub(crate) secure_root: SecureRoot,
}

impl ServeState {
    /// Validate the configuration and resolve the serving root it names.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::InvalidInput`] when the default content
    /// type or the extra response headers are rejected by the static
    /// metadata validation, and the underlying I/O error when the root
    /// cannot be canonicalized, is not a directory, or cannot be opened.
    pub fn new(config: Arc<ServeConfig>) -> Result<Self, std::io::Error> {
        validate_static_metadata_with_limits(
            &config.default_content_type,
            &config.extra_response_headers,
            config.limits.max_extra_headers,
            config.limits.max_extra_header_bytes,
        )
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
        let secure_root = SecureRoot::new(&config.root, config.static_policy.clone())?;
        Ok(Self {
            config,
            secure_root,
        })
    }

    /// The configuration this state was built from.
    pub fn config(&self) -> &Arc<ServeConfig> {
        &self.config
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

    #[test]
    fn extra_response_headers_have_default_count_limit() {
        let headers = (0..=Limits::default().max_extra_headers)
            .map(|index| (format!("X-Test-{index}"), "ok".to_owned()))
            .collect::<Vec<_>>();
        let error = validate_static_metadata("application/octet-stream", &headers).unwrap_err();
        assert!(error.contains("too many extra response headers"));
    }

    #[test]
    fn extra_response_headers_have_default_size_limit() {
        let headers = vec![("X-Test".to_owned(), "a".repeat(8 * 1024))];
        let error = validate_static_metadata("application/octet-stream", &headers).unwrap_err();
        assert!(error.contains("maximum size"));
    }

    #[test]
    fn extra_response_header_limits_can_be_configured() {
        assert!(validate_static_metadata_with_limits(
            "application/octet-stream",
            &[("X-Test".to_owned(), "ok".to_owned())],
            1,
            9,
        )
        .is_ok());
    }
}
