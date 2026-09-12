//! Generic tunnel / upgrade capability (Plan 199).
//!
//! EggServe provides a safe, generic duplex capability after a validated HTTP
//! transition. It does **not** implement WebSocket framing, ping/pong,
//! fragmentation, close codes, permessage-deflate, ASGI `websocket.*` events,
//! SOCKS, CONNECT proxy policy, or arbitrary application tunneling policy.
//!
//! # What this module owns
//!
//! - [`TunnelKind`]: `Http1Upgrade`, `Connect`, `ExtendedConnect`.
//! - [`ProtocolName`]: validated, bounded `token` (RFC 9110 `token`, 1..=64).
//! - [`TunnelRequest`]: validated tunnel intent (kind + protocol + authority).
//!   Pseudo-headers never appear as ordinary headers; protocol bytes are
//!   validated/bounded before allocation/service dispatch.
//! - [`TunnelCapability`]: non-cloneable, one-shot, transport-backed
//!   acceptance capability attached to [`RequestContext`](super::request_context::RequestContext).
//!   Ordinary clones never duplicate ownership; double-accept is impossible
//!   (second `take` returns `None`) or deterministic [`TunnelError`];
//!   dropping/ignoring uses the normal HTTP denial path; the capability
//!   becomes unusable after final response commitment.
//! - [`TunnelIo`]: EggServe-owned duplex abstraction (`AsyncRead` +
//!   `AsyncWrite`, `Unpin + Send`). Single-owner by default; splitting via
//!   `tokio::io::split` is the explicit supported operation. Bounded
//!   backpressure (32 KiB duplex buffer), H1 read-ahead preserved via Hyper's
//!   `Upgraded` buffering, H2/H3 flow control respected via transport bridges,
//!   lifecycle cancellation wakes idle tasks, no implicit buffering
//!   proportional to attacker input.
//!
//! # What this module does NOT own
//!
//! - No WebSocket codec, no `WebSocket` enum variant as the only protocol.
//!   The capability stays generic.
//! - No raw socket/QUIC/Hyper/h2/h3/Quinn types in public signatures.
//!   `hyper::upgrade::OnUpgrade` is held privately (crate-internal) for
//!   H1/H2; H3 streams are adapted by the runtime without naming h3 types.
//! - No `ServiceOutcome`: `Service::call` keeps returning [`Response`](super::canonical::Response).
//!   [`TunnelCapability::accept`] consumes the capability and returns a
//!   handshake [`Response`] carrying a crate-private acceptance token. Only
//!   `accept` can create that token, so ordinary responses cannot forge a
//!   tunnel handshake.
//!
//! # Phase-zero audit (Track A, current deps)
//!
//! - H1 (`hyper` 1.11.1): `OnUpgrade` + `.with_upgrades()` + `Upgraded`
//!   (with `read_buf` for buffered post-handshake bytes). TLS and
//!   caller-owned IO work because the driver wraps any
//!   `AsyncRead + AsyncWrite` stream. Driver completion after handoff is
//!   `Ok` (upgrade), and tunnel tasks hold the tunnel budget + lifecycle.
//! - H2 (`hyper` 1.11.1 + `h2` via `hyper::server::conn::http2`):
//!   `Builder::enable_connect_protocol()` advertises
//!   `SETTINGS_ENABLE_CONNECT_PROTOCOL`; `CONNECT` requests carry
//!   `OnUpgrade` in extensions + generic `:protocol` via
//!   `hyper::ext::Protocol` (generic, e.g. `websocket`). Success is any
//!   `2xx` (we emit `200`), followed by duplex stream data. Stream reset
//!   stays stream-local; siblings survive.
//! - H3 (`h3` 0.0.8 / `h3-quinn` 0.0.10 / `quinn` 0.11.11):
//!   `server::builder().enable_extended_connect(true)` advertises support,
//!   but `h3::ext::Protocol::from_str` only accepts `webtransport` and
//!   `connect-udp`. Generic `:protocol` values (e.g. `websocket`) are
//!   rejected as malformed before EggServe sees them. Documented as blocked:
//!   H3 supports `CONNECT` (no `:protocol`, kind `Connect`) and the two
//!   h3-crate protocols (kind `ExtendedConnect` with those names) as
//!   stream-scoped tunnels; generic H3 websocket-style `:protocol` is
//!   blocked by the dependency, not bypassed with raw wire code.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::primitives::authority::Authority;
use crate::primitives::canonical::{Response, ResponseBody, StatusCode};
use crate::primitives::header_block::{HeaderBlock, HeaderError};
use crate::primitives::request_lifecycle::RequestLifecycle;

/// Maximum `ProtocolName` bytes (validated token, bounded before allocation).
pub const MAX_TUNNEL_PROTOCOL_BYTES: usize = 64;
/// Maximum handshake header fields accepted via [`TunnelCapability::accept`].
pub const MAX_TUNNEL_HEADER_COUNT: usize = 32;
/// Maximum aggregate handshake header bytes (name+value) via `accept`.
pub const MAX_TUNNEL_HEADER_BYTES: usize = 8 * 1024;
/// Duplex bridge buffer bytes for [`TunnelIo::pair`] (bounded backpressure).
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
///   limited to h3-crate values (see module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelRequest {
    kind: TunnelKind,
    protocol: Option<ProtocolName>,
    authority: Option<Authority>,
}

impl TunnelRequest {
    /// Create validated tunnel intent (crate-internal: only the runtime
    /// creates this after header/pseudo-header validation + transport
    /// capability presence; services cannot fabricate by constructing headers).
    pub(crate) fn new(
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

/// Tunnel capability/acceptance failures (sanitized, no payload bytes logged).
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

/// Shared one-shot state (commitment + acceptance), cloned between the
/// runtime's pre-service snapshot and the service-owned capability.
#[derive(Debug)]
pub(crate) struct TunnelShared {
    committed: AtomicBool,
    accepted: AtomicBool,
}

impl TunnelShared {
    pub(crate) fn new() -> Self {
        Self {
            committed: AtomicBool::new(false),
            accepted: AtomicBool::new(false),
        }
    }

    pub(crate) fn mark_committed(&self) {
        self.committed.store(true, Ordering::Release);
    }

    pub(crate) fn is_committed(&self) -> bool {
        self.committed.load(Ordering::Acquire)
    }

    /// Claim acceptance exactly once; `false` when already accepted.
    pub(crate) fn try_accept(&self) -> bool {
        self.accepted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

/// Boxed tunnel handler: receives duplex IO + lifecycle, owns protocol codec.
///
/// `Send + 'static` so downstream tasks can own it; single-owner duplex.
/// The runtime spawns it after the validated handshake; it must observe
/// `lifecycle.cancelled()` for peer/reset/shutdown/timeout/close.
type TunnelHandlerBox = Box<
    dyn FnOnce(TunnelIo, RequestLifecycle) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + 'static,
>;

/// Crate-private acceptance token carried by handshake [`Response`].
///
/// Only [`TunnelCapability::accept`] constructs this (via
/// `Response::with_tunnel_acceptance`), so ordinary responses cannot forge a
/// tunnel handshake. Holds the downstream handler + transport upgrade future
/// (H1/H2) or `None` (H3/test, where streams are owned by the adapter).
pub(crate) struct TunnelAcceptance {
    pub(crate) handler: TunnelHandlerBox,
    pub(crate) upgrade: Option<hyper::upgrade::OnUpgrade>,
    pub(crate) kind: TunnelKind,
}

impl fmt::Debug for TunnelAcceptance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelAcceptance")
            .field("kind", &self.kind)
            .field("has_upgrade", &self.upgrade.is_some())
            .finish()
    }
}

/// One-shot, non-cloneable, transport-backed tunnel capability.
///
/// Obtained via `RequestContext::take_tunnel()` (or inspected via
/// `tunnel_request()`). Consumed by [`accept`](Self::accept) to produce a
/// handshake [`Response`]; dropping/ignoring uses the normal HTTP denial path.
pub struct TunnelCapability {
    request: TunnelRequest,
    shared: Arc<TunnelShared>,
    upgrade: Option<hyper::upgrade::OnUpgrade>,
}

impl fmt::Debug for TunnelCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelCapability")
            .field("request", &self.request)
            .field("has_upgrade", &self.upgrade.is_some())
            .finish()
    }
}

impl TunnelCapability {
    /// Create a capability (crate-internal: runtime only, after validation).
    pub(crate) fn new(request: TunnelRequest, upgrade: Option<hyper::upgrade::OnUpgrade>) -> Self {
        Self {
            request,
            shared: Arc::new(TunnelShared::new()),
            upgrade,
        }
    }

    /// Returns validated tunnel intent.
    pub fn request(&self) -> &TunnelRequest {
        &self.request
    }

    /// Shared commitment/acceptance state (runtime pre-service snapshot).
    pub(crate) fn shared(&self) -> Arc<TunnelShared> {
        self.shared.clone()
    }

    /// Accept the tunnel: validate handshake headers, claim one-shot
    /// ownership, and return a handshake [`Response`] carrying the handler.
    ///
    /// - H1 (`Http1Upgrade`): `101 Switching Protocols`; runtime adds
    ///   `Upgrade: <protocol>` + `Connection: upgrade` (service must not
    ///   supply framing; `Upgrade`/`Connection` in `headers` are stripped and
    ///   replaced with validated values).
    /// - `Connect` / `ExtendedConnect`: `200 OK`; hop-by-hop stripped, no
    ///   `101` synthesized.
    /// - `headers`: application handshake fields (e.g. `Sec-WebSocket-Accept`);
    ///   framing (`content-length`, `transfer-encoding`) rejected; hop-by-hop
    ///   stripped; bounded (32 fields / 8 KiB).
    /// - `handler`: downstream codec (`FnOnce(TunnelIo, RequestLifecycle)`).
    ///   The runtime, not the application, writes transition/framing bytes;
    ///   the handler never sees the raw socket/QUIC connection.
    pub fn accept<F, Fut>(self, headers: HeaderBlock, handler: F) -> Result<Response, TunnelError>
    where
        F: FnOnce(TunnelIo, RequestLifecycle) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.shared.is_committed() {
            return Err(TunnelError::AfterCommit);
        }
        if !self.shared.try_accept() {
            return Err(TunnelError::AlreadyAccepted);
        }
        let mut headers = headers;
        validate_handshake_headers(&mut headers)?;
        let status = match self.request.kind {
            TunnelKind::Http1Upgrade => StatusCode::SWITCHING_PROTOCOLS,
            TunnelKind::Connect | TunnelKind::ExtendedConnect => StatusCode::OK,
        };
        // H1: runtime is the sole `Upgrade`/`Connection` authority. Strip any
        // service-supplied values, then add validated ones. H2/H3: no 101,
        // no hop-by-hop; they were already stripped.
        if self.request.kind == TunnelKind::Http1Upgrade {
            headers.retain(|f| {
                !f.name.as_str().eq_ignore_ascii_case("upgrade")
                    && !f.name.as_str().eq_ignore_ascii_case("connection")
            });
            if let Some(protocol) = self.request.protocol.as_ref() {
                headers.push(
                    crate::primitives::header_block::HeaderName::new("upgrade").map_err(|_| {
                        TunnelError::InvalidHeader(
                            crate::primitives::header_block::HeaderError::InvalidName,
                        )
                    })?,
                    crate::primitives::header_block::HeaderValue::from_bytes(protocol.as_bytes())
                        .map_err(|_| {
                        TunnelError::InvalidHeader(
                            crate::primitives::header_block::HeaderError::InvalidValue,
                        )
                    })?,
                );
                headers.push(
                    crate::primitives::header_block::HeaderName::new("connection").map_err(
                        |_| {
                            TunnelError::InvalidHeader(
                                crate::primitives::header_block::HeaderError::InvalidName,
                            )
                        },
                    )?,
                    crate::primitives::header_block::HeaderValue::from_bytes(b"upgrade").map_err(
                        |_| {
                            TunnelError::InvalidHeader(
                                crate::primitives::header_block::HeaderError::InvalidValue,
                            )
                        },
                    )?,
                );
            }
        }
        let boxed: TunnelHandlerBox =
            Box::new(move |io, lifecycle| Box::pin(handler(io, lifecycle)));
        let acceptance = TunnelAcceptance {
            handler: boxed,
            upgrade: self.upgrade,
            kind: self.request.kind,
        };
        let mut response = Response::builder()
            .status(status)
            .body(ResponseBody::Empty)
            .map_err(|_| TunnelError::ForbiddenHeader("invalid tunnel status".to_string()))?;
        for field in headers.iter() {
            response
                .head_mut()
                .headers_mut()
                .push(field.name.clone(), field.value.clone());
        }
        response.with_tunnel_acceptance(acceptance);
        Ok(response)
    }
}

/// Validate handshake headers for `accept`.
///
/// Framing (`content-length`, `transfer-encoding`) is forbidden (not stripped):
/// attempting transfer coding via a handshake is an application bug. Hop-by-hop
/// is stripped (runtime-owned); `Upgrade`/`Connection` are stripped here and
/// re-added validated for H1 by `accept`. Bounded before service dispatch.
fn validate_handshake_headers(headers: &mut HeaderBlock) -> Result<(), TunnelError> {
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

/// EggServe-owned duplex abstraction for downstream protocol codecs.
///
/// Opaque wrapper around a bounded duplex pipe (32 KiB). Production instances
/// always come from the runtime after a validated handshake (H1 read-ahead
/// preserved via `Upgraded::read_buf`, H2/H3 flow control via transport
/// bridges). `AsyncRead + AsyncWrite + Unpin + Send`; single-owner by default,
/// explicit split via `tokio::io::split`. Bounded backpressure; lifecycle
/// cancellation wakes idle tasks via the handler's `RequestLifecycle`; no
/// payload bytes logged by default; no Hyper/h2/h3/Quinn types named.
pub struct TunnelIo {
    inner: tokio::io::DuplexStream,
}

impl fmt::Debug for TunnelIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelIo").finish()
    }
}

impl TunnelIo {
    /// Create an in-memory duplex pair (tests/fixtures only; production
    /// instances come from the runtime). Bounded (`TUNNEL_IO_BUFFER_BYTES`).
    pub fn pair() -> (Self, Self) {
        let (a, b) = tokio::io::duplex(TUNNEL_IO_BUFFER_BYTES);
        (Self { inner: a }, Self { inner: b })
    }

    /// Unwrap for runtime bridging (crate-internal).
    pub(crate) fn into_duplex(self) -> tokio::io::DuplexStream {
        self.inner
    }
}

impl tokio::io::AsyncRead for TunnelIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TunnelIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

// ---------------------------------------------------------------------------
// Classification helpers (crate-internal, single validation core)
// ---------------------------------------------------------------------------

/// Classify an H1 upgrade request from canonical headers.
///
/// Strict: `Connection` must contain exactly one `upgrade` token (case-insensitive,
/// all tokens valid `token`, no empty tokens); `Upgrade` must contain exactly
/// one valid bounded protocol token across all fields. Duplicates/malformed
/// yield `None` (no capability, ordinary HTTP path). HTTP/1.0 never yields a
/// capability. Bodies must be absent (caller checks `has_body` first).
pub(crate) fn classify_h1_upgrade(
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
/// Returns `None` for absent (plain `CONNECT`) vs `Some(Err)` for present-but-invalid?
/// For simplicity: `None` input yields `None` (plain CONNECT); `Some` input
/// validated strictly, invalid yields `None` (no ExtendedConnect capability;
/// caller falls back to ordinary path, never fabricates).
pub(crate) fn classify_extended_protocol(protocol: Option<&str>) -> Option<ProtocolName> {
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

    #[tokio::test]
    async fn tunnel_io_pair_echoes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut a, mut b) = TunnelIo::pair();
        a.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn accept_after_commit_fails() {
        use crate::primitives::request_lifecycle::RequestShared;
        let req = TunnelRequest::new(TunnelKind::Http1Upgrade, None, None);
        let cap = TunnelCapability::new(req, None);
        cap.shared.mark_committed();
        let err = cap
            .accept(HeaderBlock::new(), |_io, _lc| async move {})
            .unwrap_err();
        assert_eq!(err, TunnelError::AfterCommit);
        let _ = RequestShared::new_active();
    }

    #[test]
    fn handshake_rejects_framing() {
        let mut h = headers(&[("content-length", "5")]);
        assert!(matches!(
            validate_handshake_headers(&mut h),
            Err(TunnelError::ForbiddenHeader(_))
        ));
    }
}
