//! Bounded interim (1xx) response capability (Plan 198 Tracks E–F).
//!
//! [`InterimSender`] is the request-scoped, bounded capability for emitting
//! informational responses before the final response. `Service::call` keeps
//! returning the final [`Response`](crate::primitives::canonical::Response);
//! interim responses never change that return type (Plan 197 Track C
//! decision).
//!
//! # Contract
//!
//! - Only `1xx` statuses are accepted; `101 Switching Protocols` is rejected
//!   because upgrades remain deferred (Plan 176/199) and `101` cannot survive
//!   normalization.
//! - Interim responses carry headers only — no body, no trailers. The API
//!   takes no body parameter, so content cannot be smuggled by type.
//! - The final response cannot be sent through this capability (`200+`
//!   rejected as [`InterimError::NotInterim`]).
//! - No interim after final commitment: the runtime marks the sender
//!   committed when the final head commits; later sends fail with
//!   [`InterimError::AfterCommit`].
//! - Bounded count and aggregate header bytes per request
//!   ([`InterimLimits`], defaults `4` messages / `8 KiB`). Flooding fails
//!   with [`InterimError::TooMany`] / [`InterimError::TooLarge`].
//! - Runtime-owned/forbidden response fields pass through interim
//!   normalization: hop-by-hop headers are stripped, framing headers
//!   (`content-length`, `transfer-encoding`) are rejected, and the privacy
//!   denylist/`Server`/`Date` rules apply as for final responses where they
//!   have interim meaning. `Date` is still emitted by the runtime when the
//!   interim is actually placed on the wire; suppressed interims emit nothing.
//! - HTTP/1.0 never emits interim wire bytes: application sends are validated
//!   and counted, then suppressed with [`InterimDisposition::SuppressedHttp10`]
//!   rather than producing invalid wire behavior.
//! - `100 Continue` deduplication: at most one `100` per request through this
//!   capability. A second application `100` fails with
//!   [`InterimError::DuplicateContinue`]; the runtime-generated `100`
//!   (Hyper auto-emission when a `Buffer`/`Stream` body is polled) is the
//!   wire authority and application `100`s never duplicate it on the wire in
//!   the current Hyper pipeline (interims are validated/recorded, wire
//!   emission is owned by Hyper where its server APIs permit it — see
//!   `docs/http-primitives.md` for the limitation record).
//!
//! # Wire reality (Track G)
//!
//! Hyper's server APIs do not expose controlled interim emission that stays
//! inside the canonical pipeline, so the H1/H2 adapters validate, bound, and
//! record interim responses without manufacturing raw-socket fallback bytes.
//! The limitation is documented, not worked around. H2/H3 stream-local
//! semantics reuse this same core: failures are stream-scoped and never widen
//! to siblings.

use std::sync::{Arc, Mutex};

use crate::primitives::canonical::StatusCode;
use crate::primitives::header_block::{HeaderBlock, HeaderError};
use crate::primitives::version::HttpVersion;

/// Default maximum interim messages per request.
pub const DEFAULT_MAX_INTERIM_COUNT: usize = 4;
/// Default maximum aggregate interim header bytes per request.
pub const DEFAULT_MAX_INTERIM_BYTES: usize = 8 * 1024;
/// Minimum interim count accepted as a configured limit.
pub const MIN_MAX_INTERIM_COUNT: usize = 1;
/// Maximum interim count accepted as a configured limit.
pub const MAX_MAX_INTERIM_COUNT: usize = 16;
/// Minimum aggregate interim bytes accepted as a configured limit.
pub const MIN_MAX_INTERIM_BYTES: usize = 512;
/// Maximum aggregate interim bytes accepted as a configured limit.
pub const MAX_MAX_INTERIM_BYTES: usize = 64 * 1024;

/// Bounds for interim count and aggregate header bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterimLimits {
    /// Maximum interim messages per request.
    pub max_count: usize,
    /// Maximum aggregate interim header bytes per request.
    pub max_bytes: usize,
}

impl Default for InterimLimits {
    fn default() -> Self {
        Self {
            max_count: DEFAULT_MAX_INTERIM_COUNT,
            max_bytes: DEFAULT_MAX_INTERIM_BYTES,
        }
    }
}

impl InterimLimits {
    /// Create explicit limits with validation.
    pub fn new(max_count: usize, max_bytes: usize) -> Result<Self, InterimError> {
        if !(MIN_MAX_INTERIM_COUNT..=MAX_MAX_INTERIM_COUNT).contains(&max_count) {
            return Err(InterimError::InvalidLimits(format!(
                "max_count {max_count} outside {MIN_MAX_INTERIM_COUNT}..={MAX_MAX_INTERIM_COUNT}"
            )));
        }
        if !(MIN_MAX_INTERIM_BYTES..=MAX_MAX_INTERIM_BYTES).contains(&max_bytes) {
            return Err(InterimError::InvalidLimits(format!(
                "max_bytes {max_bytes} outside {MIN_MAX_INTERIM_BYTES}..={MAX_MAX_INTERIM_BYTES}"
            )));
        }
        Ok(Self {
            max_count,
            max_bytes,
        })
    }
}

/// Interim send failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterimError {
    /// Status is not an informational 1xx, or is the deferred 101.
    NotInterim(u16),
    /// A header name/value failed canonical validation.
    InvalidHeader(HeaderError),
    /// A runtime-owned framing field was supplied.
    ForbiddenHeader(String),
    /// Too many interim messages for this request.
    TooMany { count: usize, limit: usize },
    /// Aggregate interim header bytes exceed the limit.
    TooLarge { bytes: usize, limit: usize },
    /// Interim attempted after final commitment.
    AfterCommit,
    /// A second `100 Continue` was attempted through this capability.
    DuplicateContinue,
    /// Configured limits are outside the accepted range.
    InvalidLimits(String),
}

impl std::fmt::Display for InterimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInterim(code) => write!(f, "not an interim status: {code}"),
            Self::InvalidHeader(e) => write!(f, "invalid interim header: {e}"),
            Self::ForbiddenHeader(name) => write!(f, "forbidden interim header: {name}"),
            Self::TooMany { count, limit } => {
                write!(f, "too many interim responses: {count} exceeds {limit}")
            }
            Self::TooLarge { bytes, limit } => {
                write!(
                    f,
                    "interim headers too large: {bytes} exceeds {limit} bytes"
                )
            }
            Self::AfterCommit => write!(f, "interim response after final commitment"),
            Self::DuplicateContinue => write!(f, "duplicate 100 Continue suppressed"),
            Self::InvalidLimits(msg) => write!(f, "invalid interim limits: {msg}"),
        }
    }
}

impl std::error::Error for InterimError {}

impl From<HeaderError> for InterimError {
    fn from(e: HeaderError) -> Self {
        Self::InvalidHeader(e)
    }
}

/// Outcome of an interim send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterimDisposition {
    /// Validated, bounded, and recorded (wire emission owned by the adapter
    /// where its server APIs permit it).
    Sent,
    /// Validated and counted, but suppressed because the request version is
    /// HTTP/1.0 (never emits invalid wire bytes).
    SuppressedHttp10,
}

impl InterimDisposition {
    /// Returns `true` when the interim was recorded for potential emission.
    pub fn is_sent(&self) -> bool {
        matches!(self, Self::Sent)
    }

    /// Returns `true` when the interim was suppressed for HTTP/1.0.
    pub fn is_suppressed(&self) -> bool {
        matches!(self, Self::SuppressedHttp10)
    }
}

#[derive(Debug)]
struct InterimInner {
    limits: InterimLimits,
    version: HttpVersion,
    count: usize,
    bytes: usize,
    committed: bool,
    sent_continue: bool,
    /// Recorded interims (status + headers) for observability/tests.
    recorded: Vec<(u16, HeaderBlock)>,
}

/// Request-scoped bounded interim-response sender.
///
/// Cloneable handle sharing one per-request allocation. All validation,
/// bounding, and commitment checks happen here so every protocol adapter
/// shares one semantic core.
#[derive(Debug, Clone)]
pub struct InterimSender {
    inner: Arc<Mutex<InterimInner>>,
}

impl InterimSender {
    /// Create a sender for a request version with default limits.
    pub fn new(version: HttpVersion) -> Self {
        Self::with_limits(version, InterimLimits::default())
    }

    /// Create a sender with explicit limits.
    pub fn with_limits(version: HttpVersion, limits: InterimLimits) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InterimInner {
                limits,
                version,
                count: 0,
                bytes: 0,
                committed: false,
                sent_continue: false,
                recorded: Vec::new(),
            })),
        }
    }

    /// Send one interim response.
    ///
    /// Validates status (`100..=199` except `101`), strips hop-by-hop
    /// headers, rejects framing headers, enforces count/byte bounds, rejects
    /// sends after final commitment, and deduplicates `100 Continue`.
    /// HTTP/1.0 sends are validated/counted then suppressed.
    pub fn send(
        &self,
        status: StatusCode,
        mut headers: HeaderBlock,
    ) -> Result<InterimDisposition, InterimError> {
        let code = status.as_u16();
        if !(100..200).contains(&code) || code == 101 {
            return Err(InterimError::NotInterim(code));
        }
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| InterimError::InvalidLimits("interim state poisoned".to_string()))?;
        if guard.committed {
            return Err(InterimError::AfterCommit);
        }
        if code == 100 && guard.sent_continue {
            return Err(InterimError::DuplicateContinue);
        }
        // Interim normalization: strip hop-by-hop (runtime-owned), reject
        // framing. This mirrors final-response privacy for interim meaning:
        // applications never control transfer coding via interims.
        normalize_interim_headers(&mut headers)?;
        let msg_bytes: usize = headers
            .iter()
            .map(|f| f.name.as_str().len().saturating_add(f.value.len()))
            .fold(0usize, |a, b| a.saturating_add(b));
        if guard.count + 1 > guard.limits.max_count {
            return Err(InterimError::TooMany {
                count: guard.count + 1,
                limit: guard.limits.max_count,
            });
        }
        if guard.bytes.saturating_add(msg_bytes) > guard.limits.max_bytes {
            return Err(InterimError::TooLarge {
                bytes: guard.bytes.saturating_add(msg_bytes),
                limit: guard.limits.max_bytes,
            });
        }
        guard.count += 1;
        guard.bytes = guard.bytes.saturating_add(msg_bytes);
        if code == 100 {
            guard.sent_continue = true;
        }
        guard.recorded.push((code, headers));
        if guard.version == HttpVersion::Http10 {
            Ok(InterimDisposition::SuppressedHttp10)
        } else {
            Ok(InterimDisposition::Sent)
        }
    }

    /// Mark final commitment: later sends fail with [`InterimError::AfterCommit`].
    pub fn mark_committed(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.committed = true;
        }
    }

    /// Returns `true` once final commitment was marked.
    pub fn is_committed(&self) -> bool {
        self.inner.lock().map(|g| g.committed).unwrap_or(false)
    }

    /// Number of recorded interims.
    pub fn count(&self) -> usize {
        self.inner.lock().map(|g| g.count).unwrap_or(0)
    }

    /// Aggregate recorded header bytes.
    pub fn bytes(&self) -> usize {
        self.inner.lock().map(|g| g.bytes).unwrap_or(0)
    }

    /// Recorded interims for observability/tests.
    pub fn recorded(&self) -> Vec<(u16, HeaderBlock)> {
        self.inner
            .lock()
            .map(|g| g.recorded.clone())
            .unwrap_or_default()
    }
}

/// Strip runtime-owned hop-by-hop headers and reject framing for interims.
///
/// Interim responses have no content or trailers, so `content-length` and
/// `transfer-encoding` are always forbidden here (not merely stripped).
fn normalize_interim_headers(headers: &mut HeaderBlock) -> Result<(), InterimError> {
    // Framing is forbidden, not stripped: attempting transfer coding via an
    // interim is an application bug.
    for name in ["content-length", "transfer-encoding"] {
        if headers.contains(name) {
            return Err(InterimError::ForbiddenHeader(name.to_string()));
        }
    }
    headers.retain(|f| !crate::primitives::canonical::is_hop_by_hop_header(f.name.as_str()));
    Ok(())
}

/// Validate an `Expect` header value against the body policy.
///
/// - `100-continue` is accepted for `Buffer`/`Stream` (wire emission owned by
///   Hyper when the body is polled) and rejected for `Reject` (handled by the
///   pipeline as `413` without inviting the body).
/// - Any other non-empty `Expect` value is an unknown expectation and maps to
///   `417 Expectation Failed` (returned as `Err(())` with the offending
///   value for the pipeline to render).
/// - Absent `Expect` is always accepted.
pub fn check_expect_header(
    expect: Option<&str>,
    policy_rejects: bool,
) -> Result<ExpectDecision, String> {
    let Some(value) = expect else {
        return Ok(ExpectDecision::NoExpect);
    };
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("100-continue") {
        if policy_rejects {
            return Ok(ExpectDecision::ContinueRejected);
        }
        return Ok(ExpectDecision::ContinueAccepted);
    }
    if trimmed.is_empty() {
        return Ok(ExpectDecision::NoExpect);
    }
    Err(trimmed.to_string())
}

/// Outcome of [`check_expect_header`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectDecision {
    /// No `Expect` header present.
    NoExpect,
    /// `100-continue` accepted (body will be polled; Hyper owns wire `100`).
    ContinueAccepted,
    /// `100-continue` must be rejected without inviting the body.
    ContinueRejected,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderBlock {
        let mut b = HeaderBlock::new();
        for (n, v) in pairs {
            b.push_str(*n, *v).unwrap();
        }
        b
    }

    #[test]
    fn accepts_103_early_hints() {
        let s = InterimSender::new(HttpVersion::Http11);
        let disp = s
            .send(
                StatusCode::new(103).unwrap(),
                headers(&[("link", "</a>; rel=preload")]),
            )
            .unwrap();
        assert_eq!(disp, InterimDisposition::Sent);
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn rejects_non_1xx_and_101() {
        let s = InterimSender::new(HttpVersion::Http11);
        assert!(matches!(
            s.send(StatusCode::new(200).unwrap(), headers(&[])),
            Err(InterimError::NotInterim(200))
        ));
        assert!(matches!(
            s.send(StatusCode::new(101).unwrap(), headers(&[])),
            Err(InterimError::NotInterim(101))
        ));
    }

    #[test]
    fn rejects_framing_in_interim() {
        let s = InterimSender::new(HttpVersion::Http11);
        assert!(matches!(
            s.send(
                StatusCode::new(103).unwrap(),
                headers(&[("content-length", "5")])
            ),
            Err(InterimError::ForbiddenHeader(_))
        ));
    }

    #[test]
    fn strips_hop_by_hop() {
        let s = InterimSender::new(HttpVersion::Http11);
        s.send(
            StatusCode::new(103).unwrap(),
            headers(&[("connection", "close"), ("x-ok", "1")]),
        )
        .unwrap();
        let rec = s.recorded();
        assert_eq!(rec.len(), 1);
        assert!(!rec[0].1.contains("connection"));
        assert!(rec[0].1.contains("x-ok"));
    }

    #[test]
    fn http10_suppresses() {
        let s = InterimSender::new(HttpVersion::Http10);
        let disp = s.send(StatusCode::new(103).unwrap(), headers(&[])).unwrap();
        assert_eq!(disp, InterimDisposition::SuppressedHttp10);
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn after_commit_rejected() {
        let s = InterimSender::new(HttpVersion::Http11);
        s.mark_committed();
        assert!(matches!(
            s.send(StatusCode::new(103).unwrap(), headers(&[])),
            Err(InterimError::AfterCommit)
        ));
    }

    #[test]
    fn duplicate_continue_rejected() {
        let s = InterimSender::new(HttpVersion::Http11);
        s.send(StatusCode::CONTINUE, headers(&[])).unwrap();
        assert!(matches!(
            s.send(StatusCode::CONTINUE, headers(&[])),
            Err(InterimError::DuplicateContinue)
        ));
        // Other 1xx still allowed after a 100.
        assert!(s.send(StatusCode::new(103).unwrap(), headers(&[])).is_ok());
    }

    #[test]
    fn flooding_bounded() {
        let s =
            InterimSender::with_limits(HttpVersion::Http11, InterimLimits::new(2, 10_000).unwrap());
        s.send(StatusCode::new(103).unwrap(), headers(&[])).unwrap();
        s.send(StatusCode::new(103).unwrap(), headers(&[])).unwrap();
        assert!(matches!(
            s.send(StatusCode::new(103).unwrap(), headers(&[])),
            Err(InterimError::TooMany { .. })
        ));
    }

    #[test]
    fn expect_decisions() {
        assert_eq!(
            check_expect_header(None, false).unwrap(),
            ExpectDecision::NoExpect
        );
        assert_eq!(
            check_expect_header(Some("100-continue"), false).unwrap(),
            ExpectDecision::ContinueAccepted
        );
        assert_eq!(
            check_expect_header(Some("100-continue"), true).unwrap(),
            ExpectDecision::ContinueRejected
        );
        assert!(check_expect_header(Some("other"), false).is_err());
    }
}
