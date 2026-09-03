//! Final-boundary response privacy policy (Plan 165).
//!
//! [`ResponsePolicy`] makes origin-response metadata explicit so a caller can
//! minimize gratuitous server/host fingerprint signals without bypassing
//! canonical HTTP framing.
//!
//! This does **not** make a server un-fingerprintable. Parser behavior,
//! timing, application content, cache validators, transport behavior, and
//! protocol choices can all remain distinguishing signals. The goal is
//! narrower: remove unnecessary implementation/version/host metadata and make
//! the remaining behavior deliberate.
//!
//! The policy is applied at the final EggServe-owned response boundary after
//! service/static response construction and canonical metadata normalization
//! but before bytes are emitted. No service/frontend may bypass it by
//! constructing raw Hyper responses: the TCP/TLS `Server` and the
//! transport-neutral `serve_http1_connection` driver share one finalization
//! path.
//!
//! Normal defaults remain RFC-compatible and preserve the current one-`Date`
//! behavior. The anonymity-sensitive profile composes stricter settings from
//! ordinary config fields; see [`ResponsePolicy::minimal_fingerprint`] and
//! `docs/deployment.md`.

use std::sync::Arc;
use std::time::SystemTime;

pub use crate::policy::ErrorRepresentationPolicy;

/// Origin time source for `Date` generation.
///
/// The provider must return a valid time value, not an arbitrary header
/// string, so EggServe continues to own formatting/validation. Invalid
/// values (pre-epoch or beyond year 9999, which `httpdate` cannot format)
/// omit `Date` rather than emitting a malformed header.
pub type DateProvider = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// `Date` generation policy (RFC 9110 §6.6.1).
///
/// An origin server with a clock must generate `Date` on 2xx/3xx/4xx
/// responses; an origin without a clock must not generate it. Suppressing
/// `Date` on such responses from an origin that has a clock is therefore
/// explicitly not RFC-conformant and must be an deliberate
/// privacy/interoperability tradeoff.
///
/// Do not use a fixed/stale `Date` or randomized timestamps; those are worse
/// protocol behavior and can themselves fingerprint the server.
#[derive(Clone, Default)]
pub enum DatePolicy {
    /// Generate the current HTTP-date from the system clock (default).
    /// Preserves current compatibility.
    #[default]
    SystemClock,
    /// Suppress `Date` emission. Explicitly not RFC 9110-conformant for
    /// 2xx/3xx/4xx from an origin with a clock.
    Suppress,
    /// Generate `Date` from a caller-supplied trusted time source (for
    /// example a network-adjusted/router clock). Preferred
    /// anonymity-sensitive mode when such a clock exists.
    Custom(DateProvider),
}

impl std::fmt::Debug for DatePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SystemClock => write!(f, "SystemClock"),
            Self::Suppress => write!(f, "Suppress"),
            Self::Custom(_) => write!(f, "Custom(..)"),
        }
    }
}

impl DatePolicy {
    /// Resolve the policy to a concrete time, or `None` when suppressed or
    /// when the provider returned an unformattable value.
    pub fn now(&self) -> Option<SystemTime> {
        match self {
            Self::SystemClock => Some(SystemTime::now()),
            Self::Suppress => None,
            Self::Custom(provider) => {
                let t = provider();
                if is_formattable_date(t) {
                    Some(t)
                } else {
                    None
                }
            }
        }
    }
}

/// Returns `true` when `httpdate::fmt_http_date` can format `t` without
/// panicking (epoch through year 9999 exclusive).
fn is_formattable_date(t: SystemTime) -> bool {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() < 253_402_300_800,
        Err(_) => false,
    }
}

/// Validate a denylisted outbound response header name.
///
/// Names are normalized/validated by canonical header types. Framing and
/// hop-by-hop headers remain governed by canonical normalization regardless
/// of denylist configuration, so they are rejected here rather than allowed
/// to break framing invariants. `date` is rejected: use [`DatePolicy`].
/// `server` is allowed (redundant with suppression, but removes
/// service-supplied values before fixed insertion).
pub fn validate_stripped_header_name(name: &str) -> Result<String, String> {
    let canonical = crate::primitives::header_block::HeaderName::new(name.to_string())
        .map_err(|e| format!("invalid stripped response header '{name}': {e}"))?;
    let lower = canonical.as_str().to_ascii_lowercase();
    // Runtime-owned framing/hop-by-hop fields. `content-range` is included
    // because stripping it from 206/416 would make the response ambiguous;
    // `date` is owned by DatePolicy.
    const BLOCKED: &[&str] = &[
        "date",
        "content-length",
        "transfer-encoding",
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "upgrade",
        "content-range",
    ];
    if BLOCKED.contains(&lower.as_str()) {
        return Err(format!(
            "stripped response header is runtime-owned and cannot be denylisted: {name}"
        ));
    }
    Ok(canonical.as_str().to_owned())
}

/// Conservative built-in anonymity-sensitive denylist.
///
/// Removes obvious implementation-identification fields. Does not blindly
/// remove all `X-*`, cache headers, CSP/security headers, CORS headers,
/// `Content-Type`, or application metadata; the caller extends the explicit
/// denylist for project-specific fields.
pub fn minimal_fingerprint_stripped_headers() -> Vec<String> {
    vec!["x-powered-by".to_owned()]
}

/// Final-boundary response privacy policy.
///
/// Applied after service/static construction and canonical normalization but
/// before bytes are emitted. Application attempts to set `Server`/`Date`
/// remain subordinate to this policy.
#[derive(Clone, Debug, Default)]
pub struct ResponsePolicy {
    /// Server identification. `None` suppresses `Server` (secure default).
    /// `Some(fixed)` emits exactly that fixed value. Never emits crate,
    /// Rust, Hyper, OS, TLS, or Python versions automatically.
    pub server_identification: Option<String>,
    /// `Date` generation policy. Default: [`DatePolicy::SystemClock`].
    pub date_policy: DatePolicy,
    /// Validated denylist of outbound response header names, applied after
    /// service construction so applications cannot re-add stripped
    /// identifiers. Duplicates are all removed. Framing/hop-by-hop headers
    /// cannot be denylisted (see [`validate_stripped_header_name`]).
    pub stripped_response_headers: Vec<String>,
    /// Canonical runtime-error representation. Default:
    /// [`ErrorRepresentationPolicy::Minimal`].
    pub error_policy: ErrorRepresentationPolicy,
}

impl ResponsePolicy {
    /// Standards-compliant defaults: `Server` suppressed, system-clock
    /// `Date`, no denylist, minimal generic errors. Preserves current
    /// one-`Date` wire behavior.
    pub fn standard() -> Self {
        Self::default()
    }

    /// Generic minimal-fingerprint profile composed from ordinary fields.
    ///
    /// The name does not imply cryptographic anonymity: it minimizes
    /// gratuitous fingerprint signals. Combine with
    /// [`crate::policy::StaticMetadataPolicy::minimal_fingerprint`] (suppress
    /// `ETag`/`Last-Modified`) and stricter Plan 164 resource limits for the
    /// full anonymity-sensitive origin profile. The router/WAF owns peer
    /// identity, rate limiting, and network abuse prevention; this profile
    /// is origin resource/privacy hardening only.
    ///
    /// Defaults: `Server` suppressed, system-clock `Date` (override with a
    /// caller-supplied [`DatePolicy::Custom`] provider when a
    /// network-adjusted clock exists, or explicit [`DatePolicy::Suppress`]
    /// as a documented RFC tradeoff), obvious identifiers stripped, minimal
    /// errors.
    pub fn minimal_fingerprint() -> Self {
        Self {
            server_identification: None,
            date_policy: DatePolicy::SystemClock,
            stripped_response_headers: minimal_fingerprint_stripped_headers(),
            error_policy: ErrorRepresentationPolicy::Minimal,
        }
    }

    /// Validate all fields. Returns the first error as a string.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(server) = &self.server_identification {
            crate::primitives::header_block::HeaderValue::new(server.clone())
                .map_err(|e| format!("invalid server_identification: {e}"))?;
            if server.trim_matches([' ', '\t']).is_empty() {
                return Err("server_identification must not be empty or whitespace-only".into());
            }
        }
        for name in &self.stripped_response_headers {
            validate_stripped_header_name(name)?;
        }
        // Duplicate denylist entries are harmless (all occurrences are
        // removed); no wildcard is supported, so no further checks.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_standard() {
        let policy = ResponsePolicy::default();
        assert_eq!(policy.server_identification, None);
        assert!(matches!(policy.date_policy, DatePolicy::SystemClock));
        assert!(policy.stripped_response_headers.is_empty());
        assert_eq!(policy.error_policy, ErrorRepresentationPolicy::Minimal);
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn minimal_fingerprint_strips_powered_by() {
        let policy = ResponsePolicy::minimal_fingerprint();
        assert_eq!(policy.server_identification, None);
        assert!(policy
            .stripped_response_headers
            .contains(&"x-powered-by".to_owned()));
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn date_suppress_resolves_to_none() {
        assert!(DatePolicy::Suppress.now().is_none());
    }

    #[test]
    fn date_system_clock_resolves() {
        assert!(DatePolicy::SystemClock.now().is_some());
    }

    #[test]
    fn date_custom_provider_is_used() {
        let fixed = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let provider: DateProvider = Arc::new(move || fixed);
        let policy = DatePolicy::Custom(provider);
        assert_eq!(policy.now(), Some(fixed));
    }

    #[test]
    fn date_custom_pre_epoch_omits() {
        let pre = std::time::UNIX_EPOCH - std::time::Duration::from_secs(1);
        let provider: DateProvider = Arc::new(move || pre);
        assert!(DatePolicy::Custom(provider).now().is_none());
    }

    #[test]
    fn stripped_rejects_framing_and_date() {
        for name in [
            "date",
            "content-length",
            "transfer-encoding",
            "connection",
            "content-range",
            "te",
            "upgrade",
        ] {
            assert!(
                validate_stripped_header_name(name).is_err(),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn stripped_accepts_identifiers() {
        for name in ["x-powered-by", "x-generator", "server", "x-aspnet-version"] {
            assert!(
                validate_stripped_header_name(name).is_ok(),
                "{name} should be accepted"
            );
        }
    }

    #[test]
    fn server_identification_rejects_empty() {
        let policy = ResponsePolicy {
            server_identification: Some("   ".into()),
            ..Default::default()
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn server_identification_rejects_crlf() {
        let policy = ResponsePolicy {
            server_identification: Some("bad\r\nvalue".into()),
            ..Default::default()
        };
        assert!(policy.validate().is_err());
    }
}
