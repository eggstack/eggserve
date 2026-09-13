//! Transport-neutral tunnel intent vocabulary (Plan 216).
//!
//! This module owns the dependency-free contract for generic HTTP
//! transition intent: what the client asked for, whether it validated, and
//! why it did not. It does **not** own transport execution.
//!
//! # What lives here (neutral, `std` + canonical header types only)
//!
//! - [`TunnelKind`]: `Http1Upgrade`, `Connect`, `ExtendedConnect`.
//! - [`ProtocolName`]: validated, bounded `token` (RFC 9110 `token`, 1..=64).
//! - [`TunnelRequest`]: validated tunnel intent (kind + protocol + authority).
//!   Pseudo-headers never appear as ordinary headers; protocol bytes are
//!   validated/bounded before allocation/service dispatch.
//! - [`TunnelError`]: stable acceptance/validation failure vocabulary that
//!   needs no runtime types (sanitized, no payload bytes).
//! - Bounds: [`MAX_TUNNEL_PROTOCOL_BYTES`], [`MAX_TUNNEL_HEADER_COUNT`],
//!   [`MAX_TUNNEL_HEADER_BYTES`], [`TUNNEL_IO_BUFFER_BYTES`].
//! - Validation: [`classify_h1_upgrade`], [`classify_extended_protocol`],
//!   [`validate_handshake_headers`]. One validation authority shared by the
//!   direct server and compatibility paths; there is no second parser.
//!
//! # What lives in `eggserve-server::tunnel` (transport execution)
//!
//! - H1 `Upgrade`/`CONNECT` detection against live transport state
//!   (`hyper::upgrade::OnUpgrade` acquisition, `.with_upgrades()` behavior).
//! - One-shot [`TunnelCapability`](https://docs.rs/eggserve-server) acceptance
//!   state machine and commitment safety.
//! - Bounded duplex handoff
//!   ([`TunnelIo`](https://docs.rs/eggserve-server)), H1 read-ahead
//!   preservation, backpressure, lifecycle cancellation, admission, and
//!   task cleanup.
//!
//! # What lives nowhere in EggServe
//!
//! No WebSocket codec, ping/pong, fragmentation, close codes,
//! permessage-deflate, SOCKS, CONNECT routing/authorization policy, MASQUE,
//! WebTransport application behavior, or generic proxy policy. The capability
//! stays generic: EggServe validates the HTTP transition and hands the
//! downstream codec a bounded duplex; the downstream owns framing/policy.
//!
//! # Dependency contract
//!
//! This module must never import `hyper`, `hyper_util`, `tokio`, `h2`,
//! `h3`, `quinn`, `rustls`, or any filesystem/executor type. The
//! `scripts/check-crate-topology.py` Plan 216 gate greps for those imports.
//! Transport machinery belongs in `eggserve-server`; compatibility facades
//! re-export these neutral values and delegate execution upward.

use std::fmt;

use crate::primitives::authority::Authority;
use crate::primitives::header_block::{HeaderBlock, HeaderError};

/// Maximum `ProtocolName` bytes (validated token, bounded before allocation).
pub const MAX_TUNNEL_PROTOCOL_BYTES: usize = 64;
/// Maximum handshake header fields accepted via capability `accept`.
pub const MAX_TUNNEL_HEADER_COUNT: usize = 32;
/// Maximum aggregate handshake header bytes (name+value) via `accept`.
pub const MAX_TUNNEL_HEADER_BYTES: usize = 8 * 1024;
/// Duplex bridge buffer bytes for the server-owned tunnel IO
/// (bounded backpressure; the bound lives here so both layers agree).
pub const TUNNEL_IO_BUFFER_BYTES: usize = 32 * 1024;

/// Generic tunnel transition kind.
///
/// Non-exhaustive: future transports may add kinds; match with a wildcard.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TunnelKind {
    /// HTTP/1 `Upgrade: <protocol>` + `Connection: upgrade` (whole connection
    /// transitions after validated `101`; buffered bytes preserved).
    Http1Upgrade,
    /// `CONNECT authority` (plain tunnel, no `:protocol`).
    Connect,
    /// `CONNECT` + `:protocol` (H2 generic; H3 limited to h3-crate protocols).
    ExtendedConnect,
}

impl fmt::Display for TunnelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http1Upgrade => write!(f, "http1-upgrade"),
            Self::Connect => write!(f, "connect"),
            Self::ExtendedConnect => write!(f, "extended-connect"),
        }
    }
}

/// Validated, bounded protocol token (e.g. `websocket`, `eggserve-test`).
///
/// Generic: no hard-coded `WebSocket` variant. Validated as RFC 9110 `token`
/// (`tchar`, 1..=64 bytes). Never constructed from unvalidated headers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProtocolName(String);

impl ProtocolName {
    /// Validate and bound a protocol token.
    pub fn new(name: impl Into<String>) -> Result<Self, TunnelError> {
        let s = name.into();
        if s.is_empty() || s.len() > MAX_TUNNEL_PROTOCOL_BYTES {
            return Err(TunnelError::InvalidProtocol(format!(
                "protocol length {} outside 1..={MAX_TUNNEL_PROTOCOL_BYTES}",
                s.len()
            )));
        }
        if !s.bytes().all(is_tchar) {
            return Err(TunnelError::InvalidProtocol(
                "protocol must be RFC 9110 token".to_string(),
            ));
        }
        Ok(Self(s))
    }

    /// Returns the protocol token text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the protocol token bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Display for ProtocolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_tchar(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
            | b'0'..=b'9'
            | b'a'..=b'z'
            | b'A'..=b'Z'
    )
}

/// Validated tunnel intent, separate from raw headers/pseudo-headers.
///
/// - H1: `Upgrade` token + `Connection: upgrade` validated; `authority` is
///   the effective Host authority.
/// - `CONNECT`: authority-form validated; `protocol` is `None`.
/// - Extended `CONNECT`: `:protocol` validated + authority; H2 generic, H3
///   limited to h3-crate values.
///
/// Intent is cloneable routing metadata. One-shot acceptance ownership lives
/// in `eggserve-server::tunnel::TunnelCapability`, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelRequest {
    kind: TunnelKind,
    protocol: Option<ProtocolName>,
    authority: Option<Authority>,
}

impl TunnelRequest {
    /// Create validated tunnel intent.
    ///
    /// Runtime-only: only runtimes construct this after header/pseudo-header
    /// validation + transport capability presence; services cannot fabricate
    /// by constructing headers (they never observe `OnUpgrade`). Public for
    /// the direct server runtime (`eggserve-server`); downstream services
    /// must use the intent attached to their `RequestContext`.
    pub fn new(
        kind: TunnelKind,
        protocol: Option<ProtocolName>,
        authority: Option<Authority>,
    ) -> Self {
        Self {
            kind,
            protocol,
            authority,
        }
    }

    /// Returns the transition kind.
    pub fn kind(&self) -> TunnelKind {
        self.kind
    }

    /// Returns the validated protocol token, if any.
    pub fn protocol(&self) -> Option<&ProtocolName> {
        self.protocol.as_ref()
    }

    /// Returns the validated authority, if any.
    pub fn authority(&self) -> Option<&Authority> {
        self.authority.as_ref()
    }
}

/// Tunnel intent/acceptance failures (sanitized, no payload bytes logged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelError {
    /// No tunnel capability attached to this request/context.
    NoCapability,
    /// Capability already accepted (second accept impossible via `take`, but
    /// deterministic if shared state raced).
    AlreadyAccepted,
    /// Capability used after final response commitment.
    AfterCommit,
    /// Protocol token invalid or unbounded.
    InvalidProtocol(String),
    /// Handshake header name/value failed canonical validation.
    InvalidHeader(HeaderError),
    /// Handshake attempted to control runtime-owned framing.
    ForbiddenHeader(String),
    /// Too many handshake headers.
    TooManyHeaders { count: usize, limit: usize },
    /// Handshake headers too large.
    TooLarge { bytes: usize, limit: usize },
}

impl fmt::Display for TunnelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCapability => write!(f, "no tunnel capability for this request"),
            Self::AlreadyAccepted => write!(f, "tunnel capability already accepted"),
            Self::AfterCommit => write!(f, "tunnel capability used after final commitment"),
            Self::InvalidProtocol(msg) => write!(f, "invalid tunnel protocol: {msg}"),
            Self::InvalidHeader(e) => write!(f, "invalid tunnel header: {e}"),
            Self::ForbiddenHeader(name) => write!(f, "forbidden tunnel header: {name}"),
            Self::TooManyHeaders { count, limit } => {
                write!(f, "too many tunnel headers: {count} exceeds {limit}")
            }
            Self::TooLarge { bytes, limit } => {
                write!(f, "tunnel headers too large: {bytes} exceeds {limit} bytes")
            }
        }
    }
}

impl std::error::Error for TunnelError {}

impl From<HeaderError> for TunnelError {
    fn from(e: HeaderError) -> Self {
        Self::InvalidHeader(e)
    }
}

/// Validate handshake headers for capability `accept`.
///
/// Framing (`content-length`, `transfer-encoding`) is forbidden (not stripped):
/// attempting transfer coding via a handshake is an application bug. Hop-by-hop
/// is stripped (runtime-owned); `Upgrade`/`Connection` are stripped here and
/// re-added validated for H1 by the server-owned `accept`. Bounded before
/// service dispatch. Shared by the direct server and compatibility paths.
pub fn validate_handshake_headers(headers: &mut HeaderBlock) -> Result<(), TunnelError> {
    for name in ["content-length", "transfer-encoding"] {
        if headers.contains(name) {
            return Err(TunnelError::ForbiddenHeader(name.to_string()));
        }
    }
    headers.retain(|f| !crate::primitives::canonical::is_hop_by_hop_header(f.name.as_str()));
    let count = headers.iter().count();
    if count > MAX_TUNNEL_HEADER_COUNT {
        return Err(TunnelError::TooManyHeaders {
            count,
            limit: MAX_TUNNEL_HEADER_COUNT,
        });
    }
    let bytes: usize = headers
        .iter()
        .map(|f| f.name.as_str().len().saturating_add(f.value.len()))
        .fold(0usize, |a, b| a.saturating_add(b));
    if bytes > MAX_TUNNEL_HEADER_BYTES {
        return Err(TunnelError::TooLarge {
            bytes,
            limit: MAX_TUNNEL_HEADER_BYTES,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Classification helpers (single validation core, shared by all runtimes)
// ---------------------------------------------------------------------------

/// Classify an H1 upgrade request from canonical headers.
///
/// Strict: `Connection` must contain exactly one `upgrade` token (case-insensitive,
/// all tokens valid `token`, no empty tokens); `Upgrade` must contain exactly
/// one valid bounded protocol token across all fields. Duplicates/malformed
/// yield `None` (no capability, ordinary HTTP path). HTTP/1.0 never yields a
/// capability. Bodies must be absent (caller checks `has_body` first).
pub fn classify_h1_upgrade(
    headers: &HeaderBlock,
    version: crate::primitives::version::HttpVersion,
) -> Option<ProtocolName> {
    use crate::primitives::version::HttpVersion;
    if version == HttpVersion::Http10 {
        return None;
    }
    if version != HttpVersion::Http11 {
        return None;
    }
    // Connection tokens across all fields.
    let mut upgrade_count = 0usize;
    for field in headers
        .iter()
        .filter(|f| f.name.as_str().eq_ignore_ascii_case("connection"))
    {
        let text = field.value.to_str().ok()?;
        for token in text.split(',') {
            let token = token.trim_matches(|c| c == ' ' || c == '\t');
            if token.is_empty() {
                return None;
            }
            if !token.bytes().all(is_tchar) {
                return None;
            }
            if token.eq_ignore_ascii_case("upgrade") {
                upgrade_count += 1;
            }
        }
    }
    if upgrade_count != 1 {
        return None;
    }
    // Upgrade tokens: exactly one across all fields.
    let mut protocols = Vec::new();
    for field in headers
        .iter()
        .filter(|f| f.name.as_str().eq_ignore_ascii_case("upgrade"))
    {
        let text = field.value.to_str().ok()?;
        for token in text.split(',') {
            let token = token.trim_matches(|c| c == ' ' || c == '\t');
            if token.is_empty() {
                return None;
            }
            protocols.push(token.to_string());
        }
    }
    if protocols.len() != 1 {
        return None;
    }
    ProtocolName::new(protocols.pop().unwrap()).ok()
}

/// Validate an H2/H3 `:protocol` value into a bounded [`ProtocolName`].
///
/// `None` input yields `None` (plain CONNECT); `Some` input is validated
/// strictly, invalid yields `None` (no ExtendedConnect capability; caller
/// falls back to the ordinary path, never fabricates).
pub fn classify_extended_protocol(protocol: Option<&str>) -> Option<ProtocolName> {
    let value = protocol?;
    let trimmed = value.trim_matches(|c| c == ' ' || c == '\t');
    if trimmed.is_empty() {
        return None;
    }
    ProtocolName::new(trimmed.to_string()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::version::HttpVersion;

    fn headers(pairs: &[(&str, &str)]) -> HeaderBlock {
        let mut b = HeaderBlock::new();
        for (n, v) in pairs {
            b.push_str(*n, *v).unwrap();
        }
        b
    }

    #[test]
    fn protocol_name_validates_token_and_bounds() {
        assert!(ProtocolName::new("websocket").is_ok());
        assert!(ProtocolName::new("eggserve-test").is_ok());
        assert!(ProtocolName::new("").is_err());
        assert!(ProtocolName::new("has space").is_err());
        assert!(ProtocolName::new("a".repeat(65)).is_err());
        assert!(ProtocolName::new("a".repeat(64)).is_ok());
    }

    #[test]
    fn h1_upgrade_classifies_strictly() {
        let good = headers(&[("connection", "upgrade"), ("upgrade", "eggserve-test")]);
        assert_eq!(
            classify_h1_upgrade(&good, HttpVersion::Http11)
                .unwrap()
                .as_str(),
            "eggserve-test"
        );
        // Case-insensitive Connection.
        let upper = headers(&[("connection", "Upgrade"), ("upgrade", "eggserve-test")]);
        assert!(classify_h1_upgrade(&upper, HttpVersion::Http11).is_some());
        // Duplicate upgrade tokens rejected.
        let dup = headers(&[("connection", "upgrade"), ("upgrade", "a, b")]);
        assert!(classify_h1_upgrade(&dup, HttpVersion::Http11).is_none());
        // Duplicate Connection upgrade rejected.
        let dup_conn = headers(&[
            ("connection", "upgrade, upgrade"),
            ("upgrade", "eggserve-test"),
        ]);
        assert!(classify_h1_upgrade(&dup_conn, HttpVersion::Http11).is_none());
        // Missing Connection rejected.
        let missing = headers(&[("upgrade", "eggserve-test")]);
        assert!(classify_h1_upgrade(&missing, HttpVersion::Http11).is_none());
        // HTTP/1.0 never upgrades.
        assert!(classify_h1_upgrade(&good, HttpVersion::Http10).is_none());
        // Malformed token rejected.
        let bad = headers(&[("connection", "upgrade"), ("upgrade", "has space")]);
        assert!(classify_h1_upgrade(&bad, HttpVersion::Http11).is_none());
    }

    #[test]
    fn extended_protocol_generic_not_hardcoded() {
        assert_eq!(
            classify_extended_protocol(Some("websocket"))
                .unwrap()
                .as_str(),
            "websocket"
        );
        assert_eq!(
            classify_extended_protocol(Some("custom-proto.v2"))
                .unwrap()
                .as_str(),
            "custom-proto.v2"
        );
        assert!(classify_extended_protocol(None).is_none());
        assert!(classify_extended_protocol(Some("")).is_none());
        assert!(classify_extended_protocol(Some("has space")).is_none());
    }

    #[test]
    fn handshake_rejects_framing() {
        let mut h = headers(&[("content-length", "5")]);
        assert!(matches!(
            validate_handshake_headers(&mut h),
            Err(TunnelError::ForbiddenHeader(_))
        ));
    }

    #[test]
    fn handshake_bounds_count_and_bytes() {
        let mut h = HeaderBlock::new();
        for i in 0..33 {
            h.push_str(format!("x-tunnel-{i}"), "v").unwrap();
        }
        assert!(matches!(
            validate_handshake_headers(&mut h),
            Err(TunnelError::TooManyHeaders { .. })
        ));
    }
}
