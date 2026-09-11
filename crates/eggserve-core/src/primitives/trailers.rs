//! Canonical trailer representation (Plan 198 Track A).
//!
//! [`Trailers`] is the transport-neutral terminal metadata section for HTTP
//! requests and responses. It reuses the byte-preserving, duplicate/
//! order-preserving [`HeaderBlock`] vocabulary for field storage, but keeps
//! trailers structurally distinct from initial headers so convenience APIs
//! cannot accidentally merge the two sections.
//!
//! # Validation model
//!
//! Trailer field names/values reuse canonical header-name/value rules
//! ([`HeaderName`]/[`HeaderValue`]). On top of that, EggServe enforces a
//! conservative denylist of fields that must never appear as application
//! trailers because the runtime owns their semantics:
//!
//! - framing: `content-length`, `transfer-encoding`, `trailer`, `te`
//! - connection management: `connection`, `keep-alive`, `proxy-connection`
//! - routing: `host`, `upgrade`
//! - proxy authentication: `proxy-authenticate`, `proxy-authorization`
//! - expectation negotiation: `expect`
//!
//! This is a denylist plus protocol validation (not a full field-semantics
//! allowlist): pseudo-header syntax (`:` prefix) is already rejected by
//! [`HeaderName`] token validation, and per-protocol adapters enforce their
//! own field-section ceilings on top. No protocol adapter maintains a second
//! trailer validation policy — all adapters call [`validate_trailers`].
//!
//! # Limits
//!
//! [`TrailerLimits`] bounds field count and decoded aggregate bytes. Defaults
//! are conservative (`32` fields, `8 KiB` aggregate) and are enforced before
//! unbounded allocation and before exposing data to services. Protocol
//! adapters may impose lower effective bounds via their own transport ceilings
//! (H2 `max_header_list_size`, H3 field-section limits); the canonical limits
//! are the application-visible ceiling, transport ceilings only narrow it.
//!
//! # H1 policy
//!
//! Application code never controls transfer coding to obtain trailers. The
//! runtime owns `Transfer-Encoding` and `Trailer` mechanics (see
//! `docs/http-primitives.md`): services declare trailers via
//! [`crate::primitives::ResponseStream::with_trailers`], never by setting
//! framing headers.

use crate::primitives::header_block::{HeaderBlock, HeaderError};

/// Default maximum trailer fields per message.
pub const DEFAULT_MAX_TRAILER_FIELDS: usize = 32;
/// Default maximum decoded aggregate trailer bytes (name + value) per message.
pub const DEFAULT_MAX_TRAILER_BYTES: usize = 8 * 1024;
/// Minimum trailer field count EggServe will accept as a configured limit.
pub const MIN_MAX_TRAILER_FIELDS: usize = 1;
/// Maximum trailer field count EggServe will accept as a configured limit.
pub const MAX_MAX_TRAILER_FIELDS: usize = 1024;
/// Minimum aggregate trailer bytes EggServe will accept as a configured limit.
pub const MIN_MAX_TRAILER_BYTES: usize = 256;
/// Maximum aggregate trailer bytes EggServe will accept as a configured limit.
pub const MAX_MAX_TRAILER_BYTES: usize = 1024 * 1024;

/// Bounds for trailer field count and decoded aggregate bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrailerLimits {
    /// Maximum number of trailer fields.
    pub max_fields: usize,
    /// Maximum decoded aggregate name + value bytes.
    pub max_bytes: usize,
}

impl Default for TrailerLimits {
    fn default() -> Self {
        Self {
            max_fields: DEFAULT_MAX_TRAILER_FIELDS,
            max_bytes: DEFAULT_MAX_TRAILER_BYTES,
        }
    }
}

impl TrailerLimits {
    /// Create explicit limits with validation.
    ///
    /// # Errors
    ///
    /// Returns [`TrailerValidationError::InvalidLimits`] when either bound is
    /// outside its accepted range.
    pub fn new(max_fields: usize, max_bytes: usize) -> Result<Self, TrailerValidationError> {
        if !(MIN_MAX_TRAILER_FIELDS..=MAX_MAX_TRAILER_FIELDS).contains(&max_fields) {
            return Err(TrailerValidationError::InvalidLimits(format!(
                "max_fields {max_fields} outside {MIN_MAX_TRAILER_FIELDS}..={MAX_MAX_TRAILER_FIELDS}"
            )));
        }
        if !(MIN_MAX_TRAILER_BYTES..=MAX_MAX_TRAILER_BYTES).contains(&max_bytes) {
            return Err(TrailerValidationError::InvalidLimits(format!(
                "max_bytes {max_bytes} outside {MIN_MAX_TRAILER_BYTES}..={MAX_MAX_TRAILER_BYTES}"
            )));
        }
        Ok(Self {
            max_fields,
            max_bytes,
        })
    }
}

/// Trailer validation failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrailerValidationError {
    /// A header name/value failed canonical validation.
    InvalidHeader(HeaderError),
    /// A runtime-owned framing/routing field was supplied as a trailer.
    ForbiddenField(String),
    /// Too many trailer fields.
    TooManyFields { count: usize, limit: usize },
    /// Decoded aggregate trailer bytes exceed the limit.
    TooLarge { bytes: usize, limit: usize },
    /// Configured limits are outside the accepted range.
    InvalidLimits(String),
}

impl std::fmt::Display for TrailerValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHeader(e) => write!(f, "invalid trailer field: {e}"),
            Self::ForbiddenField(name) => {
                write!(f, "forbidden trailer field: {name}")
            }
            Self::TooManyFields { count, limit } => {
                write!(f, "too many trailer fields: {count} exceeds {limit}")
            }
            Self::TooLarge { bytes, limit } => {
                write!(f, "trailer block too large: {bytes} exceeds {limit} bytes")
            }
            Self::InvalidLimits(msg) => write!(f, "invalid trailer limits: {msg}"),
        }
    }
}

impl std::error::Error for TrailerValidationError {}

impl From<HeaderError> for TrailerValidationError {
    fn from(e: HeaderError) -> Self {
        Self::InvalidHeader(e)
    }
}

/// Returns `true` when `name` is forbidden as an application trailer.
///
/// The denylist covers runtime-owned framing (`content-length`,
/// `transfer-encoding`, `trailer`, `te`), connection management
/// (`connection`, `keep-alive`, `proxy-connection`), routing (`host`,
/// `upgrade`), proxy authentication (`proxy-authenticate`,
/// `proxy-authorization`), and expectation negotiation (`expect`).
/// Matching is ASCII case-insensitive. Pseudo-header syntax is rejected
/// separately by [`HeaderName`](crate::primitives::header_block::HeaderName)
/// token validation.
pub fn is_forbidden_trailer_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "content-length"
            | "transfer-encoding"
            | "trailer"
            | "te"
            | "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "upgrade"
            | "host"
            | "expect"
    )
}

/// Decoded aggregate trailer size: sum of name + value bytes.
pub fn trailer_block_bytes(block: &HeaderBlock) -> usize {
    block
        .iter()
        .map(|f| f.name.as_str().len().saturating_add(f.value.len()))
        .fold(0usize, |a, b| a.saturating_add(b))
}

/// Validate a trailer block against the denylist and limits.
///
/// Checks run in this order: forbidden fields first (so hostile framing is
/// rejected even when the block is also oversized), then field count, then
/// aggregate bytes. All checks run before the block is exposed to a service.
pub fn validate_trailers(
    block: &HeaderBlock,
    limits: &TrailerLimits,
) -> Result<(), TrailerValidationError> {
    for field in block.iter() {
        if is_forbidden_trailer_field(field.name.as_str()) {
            return Err(TrailerValidationError::ForbiddenField(
                field.name.as_str().to_ascii_lowercase(),
            ));
        }
    }
    if block.len() > limits.max_fields {
        return Err(TrailerValidationError::TooManyFields {
            count: block.len(),
            limit: limits.max_fields,
        });
    }
    let bytes = trailer_block_bytes(block);
    if bytes > limits.max_bytes {
        return Err(TrailerValidationError::TooLarge {
            bytes,
            limit: limits.max_bytes,
        });
    }
    Ok(())
}

/// Canonical terminal trailer section.
///
/// Wraps a validated [`HeaderBlock`] so initial headers and trailers cannot
/// be accidentally merged by convenience APIs. Construction validates the
/// denylist and limits before the value exists; there is no unchecked
/// constructor.
///
/// Duplicate legal fields preserve order (inherited from [`HeaderBlock`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailers {
    block: HeaderBlock,
}

impl Trailers {
    /// Validate and wrap a header block as trailers.
    ///
    /// # Errors
    ///
    /// Returns [`TrailerValidationError`] for forbidden fields, excess count,
    /// or excess aggregate bytes.
    pub fn new(block: HeaderBlock) -> Result<Self, TrailerValidationError> {
        Self::with_limits(block, &TrailerLimits::default())
    }

    /// Validate with explicit limits.
    pub fn with_limits(
        block: HeaderBlock,
        limits: &TrailerLimits,
    ) -> Result<Self, TrailerValidationError> {
        validate_trailers(&block, limits)?;
        Ok(Self { block })
    }

    /// Wrap a pre-validated block without re-checking.
    ///
    /// Crate-private: adapters validate at the wire boundary before calling
    /// this; services must go through [`Trailers::new`].
    #[allow(dead_code)]
    pub(crate) fn from_validated(block: HeaderBlock) -> Self {
        Self { block }
    }

    /// Returns the underlying header block.
    pub fn as_block(&self) -> &HeaderBlock {
        &self.block
    }

    /// Consume into the underlying header block.
    pub fn into_block(self) -> HeaderBlock {
        self.block
    }

    /// Number of trailer fields.
    pub fn len(&self) -> usize {
        self.block.len()
    }

    /// Returns `true` when there are no trailer fields.
    pub fn is_empty(&self) -> bool {
        self.block.is_empty()
    }

    /// Iterate over trailer fields in order.
    pub fn iter(&self) -> impl Iterator<Item = &crate::primitives::header_block::HeaderField> {
        self.block.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::header_block::HeaderBlock;

    fn block_with(pairs: &[(&str, &str)]) -> HeaderBlock {
        let mut b = HeaderBlock::new();
        for (n, v) in pairs {
            b.push_str(*n, *v).unwrap();
        }
        b
    }

    #[test]
    fn legal_trailers_preserve_duplicates_and_order() {
        let b = block_with(&[("x-check", "a"), ("x-check", "b"), ("x-other", "c")]);
        let t = Trailers::new(b).unwrap();
        assert_eq!(t.len(), 3);
        let vals: Vec<String> = t
            .as_block()
            .get_all("x-check")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert_eq!(vals, vec!["a".to_string(), "b".to_string()]);
        let names: Vec<&str> = t.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["x-check", "x-check", "x-other"]);
    }

    #[test]
    fn forbidden_framing_rejected() {
        for name in [
            "content-length",
            "transfer-encoding",
            "trailer",
            "te",
            "connection",
            "keep-alive",
            "proxy-connection",
            "proxy-authenticate",
            "proxy-authorization",
            "upgrade",
            "host",
            "expect",
            "Content-Length",
            "TRANSFER-ENCODING",
        ] {
            let b = block_with(&[(name, "x")]);
            let err = Trailers::new(b).unwrap_err();
            assert!(
                matches!(err, TrailerValidationError::ForbiddenField(_)),
                "field {name} must be forbidden, got {err:?}"
            );
        }
    }

    #[test]
    fn count_limit_enforced_before_exposure() {
        let mut b = HeaderBlock::new();
        for i in 0..33 {
            b.push_str(format!("x-t-{i}"), "v").unwrap();
        }
        let err = Trailers::new(b).unwrap_err();
        assert!(matches!(err, TrailerValidationError::TooManyFields { .. }));
    }

    #[test]
    fn byte_limit_enforced() {
        let mut b = HeaderBlock::new();
        b.push_str("x-big", "a".repeat(9000)).unwrap();
        let err = Trailers::new(b).unwrap_err();
        assert!(matches!(err, TrailerValidationError::TooLarge { .. }));
    }

    #[test]
    fn forbidden_wins_over_size() {
        let mut b = HeaderBlock::new();
        b.push_str("content-length", "5").unwrap();
        b.push_str("x-big", "a".repeat(9000)).unwrap();
        let err = Trailers::new(b).unwrap_err();
        assert!(matches!(err, TrailerValidationError::ForbiddenField(_)));
    }

    #[test]
    fn custom_limits() {
        let b = block_with(&[("x-a", "1"), ("x-b", "2")]);
        let limits = TrailerLimits::new(2, 1024).unwrap();
        assert!(Trailers::with_limits(b.clone(), &limits).is_ok());
        let tight = TrailerLimits::new(1, 1024).unwrap();
        assert!(Trailers::with_limits(b, &tight).is_err());
        assert!(TrailerLimits::new(0, 1024).is_err());
        assert!(TrailerLimits::new(32, 10).is_err());
    }

    #[test]
    fn byte_count_is_name_plus_value() {
        let b = block_with(&[("ab", "cde")]);
        assert_eq!(trailer_block_bytes(&b), 5);
    }
}
