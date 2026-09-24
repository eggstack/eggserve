//! Generic tunnel / upgrade execution (Plan 216: direct H1 authority).
//!
//! This module owns the server-side transport handoff for validated HTTP
//! transitions. It does **not** implement WebSocket framing, ping/pong,
//! fragmentation, close codes, permessage-deflate, SOCKS, CONNECT proxy
//! policy, or any application codec. The downstream owns its protocol codec;
//! EggServe validates the HTTP transition, emits the handshake, and hands
//! over a bounded duplex.
//!
//! # Split with `eggserve-primitives::tunnel`
//!
//! - Intent vocabulary (`TunnelKind`, `ProtocolName`, `TunnelRequest`,
//!   `TunnelError`, bounds, `classify_h1_upgrade`,
//!   `classify_extended_protocol`, `validate_handshake_headers`) lives in
//!   `eggserve-primitives` and is re-exported here for convenience. There is
//!   one validation authority; this crate never parses a second one.
//! - Execution lives here: [`TunnelCapability`] (one-shot, transport-backed),
//!   [`TunnelIo`] (bounded duplex), acceptance state, H1 detection against
//!   live `OnUpgrade`, admission, bridging, and cleanup.
//!
//! # How services use it
//!
//! Ordinary `Service::call` implementations never see a capability: the
//! runtime calls the additive `Service::call_with_tunnel` entry point, whose
//! default implementation drops the capability and runs `call` (denial stays
//! ordinary HTTP). Tunnel-aware services implement `call_with_tunnel`,
//! inspect `capability.request()` for routing, and either ignore/drop the
//! capability (ordinary HTTP denial) or consume it via
//! [`TunnelCapability::accept`] to produce the handshake [`Response`].
//!
//! ```no_run
//! use eggserve_primitives::header_block::HeaderBlock;
//! use eggserve_server::service_fn_with_tunnel;
//! use eggserve_server::tunnel::TunnelIo;
//!
//! let service = service_fn_with_tunnel(|req, tunnel| async move {
//!     let Some(capability) = tunnel else {
//!         return Ok(eggserve_primitives::canonical::Response::builder()
//!             .status(eggserve_primitives::canonical::StatusCode::OK)
//!             .body(eggserve_primitives::canonical::ResponseBody::Bytes(
//!                 b"no-tunnel".to_vec(),
//!             ))
//!             .unwrap());
//!     };
//!     // Capture cancellation before accepting; the handler owns only IO.
//!     let lifecycle = req.lifecycle_clone();
//!     let handler = |mut io: TunnelIo| async move {
//!         use tokio::io::{AsyncReadExt, AsyncWriteExt};
//!         let mut buf = vec![0u8; 8192];
//!         loop {
//!             tokio::select! {
//!                 read = io.read(&mut buf) => {
//!                     match read {
//!                         Ok(0) => break,
//!                         Ok(n) => {
//!                             if io.write_all(&buf[..n]).await.is_err() {
//!                                 break;
//!                             }
//!                         }
//!                         Err(_) => break,
//!                     }
//!                 }
//!                 _ = lifecycle.cancelled() => break,
//!             }
//!         }
//!     };
//!     capability
//!         .accept(HeaderBlock::new(), handler)
//!         .map_err(|e| eggserve_server::service::ServiceError::internal(e.to_string()))
//! });
//! ```
//!
//! # Acceptance contract (Plan 199 semantics, direct ownership)
//!
//! - Exactly one successful take/accept: `accept(self)` consumes the
//!   capability; the shared state rejects a second accept and rejects use
//!   after final response commitment (`AfterCommit`).
//! - Ordinary HTTP denial when the service declines (drops) the capability.
//! - `101 Switching Protocols` for validated H1 `Upgrade` (runtime owns
//!   `Connection: upgrade` + `Upgrade` values); `200 OK` for
//!   `CONNECT`/`ExtendedConnect`. No `101` is synthesized for CONNECT.
//! - Runtime-owned framing: application handshake headers are bounded
//!   (32 fields / 8 KiB), framing (`content-length`, `transfer-encoding`)
//!   is rejected, hop-by-hop is stripped. Only `accept` can produce the
//!   handshake; ordinary responses cannot forge one (the pipeline only
//!   performs the transport handoff when the sidecar acceptance is present).
//! - Bounded single-owner duplex ([`TunnelIo`], 32 KiB), H1 read-ahead
//!   preserved exactly once via Hyper's `Upgraded` buffering, lifecycle
//!   cancellation wakes idle work, permits/tasks release on every terminal
//!   path, errors sanitized.
//!
//! # H2 readiness (Plan 217 input)
//!
//! [`TunnelKind::ExtendedConnect`] and validated `:protocol` metadata are
//! first-class here so Plan 217 can attach H2 Extended CONNECT transport
//! without a new service API. This module classifies H1 only; H2 attachment
//! stays compatibility-owned until Plan 217.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eggserve_primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_primitives::header_block::HeaderBlock;
use eggserve_primitives::request_lifecycle::RequestLifecycle;
pub use eggserve_primitives::tunnel::{
    classify_extended_protocol, classify_h1_upgrade, validate_handshake_headers, ProtocolName,
    TunnelError, TunnelKind, TunnelRequest, MAX_TUNNEL_HEADER_BYTES, MAX_TUNNEL_HEADER_COUNT,
    MAX_TUNNEL_PROTOCOL_BYTES, TUNNEL_IO_BUFFER_BYTES,
};

/// Shared one-shot state (commitment + acceptance), cloned between the
/// pipeline's pre-service snapshot and the service-owned capability.
///
/// Runtime-internal: pipelines construct and commit this; services never
/// touch it (they consume the capability via `accept`).
#[derive(Debug)]
pub struct TunnelShared {
    committed: AtomicBool,
    accepted: AtomicBool,
}

impl TunnelShared {
    /// Create uncommitted, unaccepted shared state (runtime only).
    pub fn new() -> Self {
        Self {
            committed: AtomicBool::new(false),
            accepted: AtomicBool::new(false),
        }
    }
    /// Mark final response commitment (runtime only).
    pub fn mark_committed(&self) {
        self.committed.store(true, Ordering::Release);
    }

    /// Returns `true` after final response commitment.
    pub fn is_committed(&self) -> bool {
        self.committed.load(Ordering::Acquire)
    }
    /// Claim acceptance exactly once; `false` when already accepted.
    /// Runtime-internal (called by `accept`).
    #[doc(hidden)]
    pub fn try_accept(&self) -> bool {
        self.accepted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

impl Default for TunnelShared {
    fn default() -> Self {
        Self::new()
    }
}

/// Boxed tunnel handler: receives duplex IO, owns the protocol codec.
///
/// `Send + 'static` so downstream tasks can own it; single-owner duplex.
/// The runtime spawns it after the validated handshake. Cancellation
/// (peer/reset/shutdown/timeout/close) is observed by capturing the
/// request's [`RequestLifecycle`](eggserve_primitives::RequestLifecycle)
/// in the handler closure before accepting; the bridge additionally ends
/// on transport close even if the handler never polls cancellation.
///
/// Runtime-internal alias (named by the hidden acceptance type); services
/// write handlers as plain `FnOnce(TunnelIo)` closures.
#[doc(hidden)]
pub type TunnelHandlerBox =
    Box<dyn FnOnce(TunnelIo) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static>;

/// Transport acceptance carried out-of-band from the handshake response.
///
/// The pipeline holds the sidecar (`Arc<Mutex<Option<Self>>>`) while the
/// service owns the capability. `accept` stores exactly one acceptance;
/// the pipeline takes it after `Service::call_with_tunnel` returns and
/// performs admission + transport handoff. Ordinary responses never carry
/// one, so ordinary responses cannot forge a handshake.
///
/// Runtime-internal: named in public signatures only through the hidden
/// sidecar type below; services never name it.
#[doc(hidden)]
pub struct TunnelAcceptance {
    #[doc(hidden)]
    pub handler: TunnelHandlerBox,
    #[doc(hidden)]
    pub upgrade: Option<hyper::upgrade::OnUpgrade>,
    #[doc(hidden)]
    pub kind: TunnelKind,
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
/// Obtained via `Service::call_with_tunnel` (second parameter). Inspect via
/// [`request`](Self::request); consume via [`accept`](Self::accept) to
/// produce the handshake [`Response`]. Dropping/ignoring uses the normal
/// HTTP denial path.
pub struct TunnelCapability {
    request: TunnelRequest,
    shared: Arc<TunnelShared>,
    upgrade: Option<hyper::upgrade::OnUpgrade>,
    sidecar: Arc<std::sync::Mutex<Option<TunnelAcceptance>>>,
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
    /// Create a capability (runtime only, after validation).
    ///
    /// The pipeline classifies intent, acquires `OnUpgrade`, and shares
    /// `shared`/`sidecar` with its post-service commitment check.
    /// Hidden: services receive capabilities, never construct them.
    #[doc(hidden)]
    pub fn new(
        request: TunnelRequest,
        upgrade: Option<hyper::upgrade::OnUpgrade>,
        shared: Arc<TunnelShared>,
        sidecar: Arc<std::sync::Mutex<Option<TunnelAcceptance>>>,
    ) -> Self {
        Self {
            request,
            shared,
            upgrade,
            sidecar,
        }
    }

    /// Returns validated tunnel intent.
    pub fn request(&self) -> &TunnelRequest {
        &self.request
    }

    /// Shared commitment/acceptance state (runtime-internal snapshot).
    ///
    /// Hidden: pipelines snapshot this before `Service::call_with_tunnel`
    /// and mark it committed after the final outcome.
    #[doc(hidden)]
    pub fn shared(&self) -> Arc<TunnelShared> {
        self.shared.clone()
    }

    /// Staged-acceptance sidecar (runtime-internal snapshot).
    ///
    /// Hidden: pipelines take the staged [`TunnelAcceptance`] from here
    /// after the service returns; ordinary responses never stage one.
    #[doc(hidden)]
    pub fn sidecar(&self) -> Arc<std::sync::Mutex<Option<TunnelAcceptance>>> {
        self.sidecar.clone()
    }

    /// Accept the tunnel: validate handshake headers, claim one-shot
    /// ownership, stage the transport acceptance, and return the handshake
    /// [`Response`].
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
    /// - `handler`: downstream codec (`FnOnce(TunnelIo)`). Capture the
    ///   request lifecycle in the closure when cancellation is needed.
    ///   The runtime, not the application, writes transition/framing bytes;
    ///   the handler never sees the raw socket/QUIC connection.
    ///
    /// The returned response is already a valid handshake (status +
    ///   headers + empty body). The pipeline sends it without re-running
    ///   the ordinary response normalizer (which would strip the
    ///   runtime-owned `Upgrade`/`Connection` handshake); privacy
    ///   finalization still applies.
    ///
    ///   Mutating the response via `head_mut`/`take_body` after `accept`
    ///   is a service bug (the staged acceptance stays while the handshake
    ///   bytes change): treat accepted responses as final.
    pub fn accept<F, Fut>(self, headers: HeaderBlock, handler: F) -> Result<Response, TunnelError>
    where
        F: FnOnce(TunnelIo) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.shared.is_committed() {
            return Err(TunnelError::AfterCommit);
        }
        if !self.shared.try_accept() {
            return Err(TunnelError::AlreadyAccepted);
        }
        let response = build_handshake_response(&self.request, headers)?;
        let boxed: TunnelHandlerBox = Box::new(move |io| Box::pin(handler(io)));
        let acceptance = TunnelAcceptance {
            handler: boxed,
            upgrade: self.upgrade,
            kind: self.request.kind(),
        };
        // Stage exactly once; a poisoned mutex means the pipeline cannot
        // observe acceptance — fail safe without sending a handshake.
        match self.sidecar.lock() {
            Ok(mut slot) => {
                *slot = Some(acceptance);
            }
            Err(_) => return Err(TunnelError::AlreadyAccepted),
        }
        Ok(response)
    }
}

/// Build a validated handshake response without staging acceptance.
///
/// Shared by [`TunnelCapability::accept`] and compatibility glue: validates
/// handshake headers, selects the transition status, and injects
/// runtime-owned H1 framing. Returns the handshake [`Response`] (status +
/// headers + empty body) or the deterministic [`TunnelError`].
/// Runtime-internal: services accept through capabilities, never this.
#[doc(hidden)]
pub fn build_handshake_response(
    request: &TunnelRequest,
    headers: HeaderBlock,
) -> Result<Response, TunnelError> {
    let mut headers = headers;
    validate_handshake_headers(&mut headers)?;
    let status = match request.kind() {
        TunnelKind::Http1Upgrade => StatusCode::SWITCHING_PROTOCOLS,
        TunnelKind::Connect | TunnelKind::ExtendedConnect => StatusCode::OK,
        // Non-exhaustive future kinds fail closed (no handshake forged).
        _ => return Err(TunnelError::NoCapability),
    };
    // H1: runtime is the sole `Upgrade`/`Connection` authority. Strip any
    // service-supplied values, then add validated ones. H2/H3: no 101,
    // no hop-by-hop; they were already stripped.
    if request.kind() == TunnelKind::Http1Upgrade {
        headers.retain(|f| {
            !f.name.as_str().eq_ignore_ascii_case("upgrade")
                && !f.name.as_str().eq_ignore_ascii_case("connection")
        });
        if let Some(protocol) = request.protocol() {
            headers.push(
                eggserve_primitives::header_block::HeaderName::new("upgrade").map_err(|_| {
                    TunnelError::InvalidHeader(
                        eggserve_primitives::header_block::HeaderError::InvalidName,
                    )
                })?,
                eggserve_primitives::header_block::HeaderValue::from_bytes(protocol.as_bytes())
                    .map_err(|_| {
                        TunnelError::InvalidHeader(
                            eggserve_primitives::header_block::HeaderError::InvalidValue,
                        )
                    })?,
            );
            headers.push(
                eggserve_primitives::header_block::HeaderName::new("connection").map_err(|_| {
                    TunnelError::InvalidHeader(
                        eggserve_primitives::header_block::HeaderError::InvalidName,
                    )
                })?,
                eggserve_primitives::header_block::HeaderValue::from_bytes(b"upgrade").map_err(
                    |_| {
                        TunnelError::InvalidHeader(
                            eggserve_primitives::header_block::HeaderError::InvalidValue,
                        )
                    },
                )?,
            );
        }
    }
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
    Ok(response)
}

/// Classify a validated H1 tunnel candidate.
///
/// Returns `(TunnelRequest, OnUpgrade)` only when:
/// - no body is present (`has_body == false`; smuggled CL/TE/body never
///   crosses the transition),
/// - transport provides `OnUpgrade` (`None` => ordinary path),
/// - H1 `Upgrade`/`Connection` tokens strictly validate (single `upgrade`
///   token, single protocol token, HTTP/1.1 only), or the method is H1
///   `CONNECT` with a validated authority (already in `head.authority()`).
///
/// Ordinary requests yield `None` (no capability, ordinary HTTP path).
/// H2 `ExtendedConnect` classification stays compatibility-owned until
/// Plan 217; this helper represents the kind in the neutral vocabulary but
/// never produces it on the direct H1 path.
///
/// Runtime-internal helper (named in no service signature); compatibility
/// H2 glue classifies Extended CONNECT itself via the neutral
/// `classify_extended_protocol` and constructs capabilities via the hidden
/// [`TunnelCapability::new`].
pub(crate) fn classify_tunnel(
    head: &eggserve_primitives::request_head::RequestHead,
    has_body: bool,
    on_upgrade: Option<hyper::upgrade::OnUpgrade>,
) -> Option<(TunnelRequest, hyper::upgrade::OnUpgrade)> {
    use eggserve_primitives::version::HttpVersion;

    if has_body {
        return None;
    }
    let upgrade = on_upgrade?;
    if head.version() != HttpVersion::Http11 {
        return None;
    }
    if head.method().as_str() == "CONNECT" {
        let authority = head.authority().cloned()?;
        let req = TunnelRequest::new(TunnelKind::Connect, None, Some(authority));
        return Some((req, upgrade));
    }
    let protocol = classify_h1_upgrade(head.headers(), head.version())?;
    let req = TunnelRequest::new(
        TunnelKind::Http1Upgrade,
        Some(protocol),
        head.authority().cloned(),
    );
    Some((req, upgrade))
}

/// EggServe-owned duplex abstraction for downstream protocol codecs.
///
/// Opaque wrapper around a bounded duplex pipe (32 KiB). Production instances
/// always come from the runtime after a validated handshake (H1 read-ahead
/// preserved via `Upgraded::read_buf` bridging, lifecycle cancellation wakes
/// idle tasks). `AsyncRead + AsyncWrite + Unpin + Send`; single-owner by
/// default, explicit split via `tokio::io::split`. Bounded backpressure; no
/// payload bytes logged; no Hyper/h2/h3/Quinn types named.
pub struct TunnelIo {
    inner: TunnelIoInner,
}

trait TunnelTransport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> TunnelTransport for T {}

enum TunnelIoInner {
    Pair(tokio::io::DuplexStream),
    Direct(Pin<Box<dyn TunnelTransport>>),
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
        (
            Self {
                inner: TunnelIoInner::Pair(a),
            },
            Self {
                inner: TunnelIoInner::Pair(b),
            },
        )
    }

    fn from_transport<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static>(
        transport: T,
    ) -> Self {
        Self {
            inner: TunnelIoInner::Direct(Box::pin(transport)),
        }
    }

    /// Unwrap for runtime bridging (runtime-internal).
    ///
    /// Hidden: pipelines and compatibility H3 glue unwrap the bridge end
    /// for transport copying; services never unwrap (use `split` or
    /// async IO directly).
    #[doc(hidden)]
    pub fn into_duplex(self) -> tokio::io::DuplexStream {
        match self.inner {
            TunnelIoInner::Pair(stream) => stream,
            TunnelIoInner::Direct(_) => {
                panic!("direct tunnel transport cannot be unwrapped as a duplex")
            }
        }
    }
}

impl tokio::io::AsyncRead for TunnelIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.inner {
            TunnelIoInner::Pair(stream) => Pin::new(stream).poll_read(cx, buf),
            TunnelIoInner::Direct(stream) => stream.as_mut().poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for TunnelIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.inner {
            TunnelIoInner::Pair(stream) => Pin::new(stream).poll_write(cx, buf),
            TunnelIoInner::Direct(stream) => stream.as_mut().poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.inner {
            TunnelIoInner::Pair(stream) => Pin::new(stream).poll_flush(cx),
            TunnelIoInner::Direct(stream) => stream.as_mut().poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.inner {
            TunnelIoInner::Pair(stream) => Pin::new(stream).poll_shutdown(cx),
            TunnelIoInner::Direct(stream) => stream.as_mut().poll_shutdown(cx),
        }
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.inner {
            TunnelIoInner::Pair(stream) => Pin::new(stream).poll_write_vectored(cx, bufs),
            TunnelIoInner::Direct(stream) => stream.as_mut().poll_write_vectored(cx, bufs),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match &self.inner {
            TunnelIoInner::Pair(stream) => stream.is_write_vectored(),
            TunnelIoInner::Direct(stream) => stream.is_write_vectored(),
        }
    }
}

/// Admit one H1 tunnel and spawn its tracked task.
///
/// - Tries the server-wide tunnel budget (`try_acquire_owned`, no queueing);
///   exhaustion returns `false` (caller renders deterministic 503, drops the
///   acceptance so the handler never runs and `OnUpgrade` fails safe).
/// - On admission, spawns via `activity.spawn_tunnel` (driver drains before
///   reporting completion, so H1 keeps the owning connection alive and no
///   detached task survives `wait()`).
/// - The task awaits `OnUpgrade`, bridges with bounded backpressure, runs the
///   downstream handler, and releases the permit + gauge exactly once.
///
/// Returns `true` when admitted (handshake should be sent), `false` on
/// exhaustion (caller must send 503 instead).
pub(crate) async fn admit_and_spawn(
    activity: &Arc<crate::connection::activity::ConnectionActivity>,
    tunnel_semaphore: Option<&Arc<tokio::sync::Semaphore>>,
    ops: &crate::ops::OpsContext,
    conn_id: u64,
    lifecycle: RequestLifecycle,
    acceptance: TunnelAcceptance,
) -> bool {
    let permit = match tunnel_semaphore {
        None => None,
        Some(semaphore) => match semaphore.clone().try_acquire_owned() {
            Ok(p) => Some(p),
            Err(_) => {
                ops.counters()
                    .tunnels_rejected
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Warn,
                        crate::ops::EventKind::TunnelRejected,
                        "tunnel saturated: active tunnel limit",
                    )
                    .connection_id(conn_id),
                );
                return false;
            }
        },
    };
    let kind = acceptance.kind;
    let cancel = async move { lifecycle.cancelled().await };
    activity
        .spawn_tunnel(run_tunnel(
            permit,
            ops.clone(),
            conn_id,
            cancel,
            acceptance,
            kind,
        ))
        .await;
    true
}

/// Run one admitted H1/H2 upgrade-backed tunnel to completion.
///
/// Awaits `OnUpgrade` (buffered H1 read-ahead preserved via Hyper's
/// `Upgraded`), bridges with bounded backpressure to an EggServe-owned
/// [`TunnelIo`], runs the downstream handler, and releases the permit +
/// gauge exactly once. `cancel` (usually the request lifecycle) wakes idle
/// work; without it the bridge still ends on peer close/reset, and the
/// owning driver aborts remainders past its drain budget. Errors are
/// sanitized (no payload bytes).
///
/// Hidden runtime-internal entry point: the direct pipeline reaches it via
/// the crate-internal [`admit_and_spawn`]; compatibility glue spawns it on
/// its own tracked task set after its own admission (H2 until Plan 217).
/// Services never call it. Holding `_permit` keeps the tunnel budget until
/// return.
#[doc(hidden)]
pub async fn run_tunnel(
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ops: crate::ops::OpsContext,
    conn_id: u64,
    cancel: impl Future<Output = ()> + Send + 'static,
    acceptance: TunnelAcceptance,
    kind: TunnelKind,
) {
    let TunnelAcceptance {
        handler,
        upgrade,
        kind: _,
    } = acceptance;
    tokio::pin!(cancel);
    let upgrade = match upgrade {
        Some(u) => u,
        None => {
            // No transport upgrade (e.g. hand-constructed test capability):
            // fail safe without running the handler.
            ops.counters()
                .tunnel_upgrade_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::TunnelUpgradeFailed,
                    "tunnel upgrade unavailable",
                )
                .connection_id(conn_id),
            );
            return;
        }
    };
    let upgraded = match upgrade.await {
        Ok(u) => u,
        Err(_) => {
            ops.counters()
                .tunnel_upgrade_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::TunnelUpgradeFailed,
                    "tunnel upgrade failed",
                )
                .connection_id(conn_id),
            );
            return;
        }
    };
    ops.counters()
        .tunnels_accepted
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.counters()
        .active_tunnels
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::TunnelAccepted,
            format!("tunnel accepted: {kind}"),
        )
        .connection_id(conn_id),
    );
    let _active_guard = ActiveTunnelGuard { ops: ops.clone() };

    // The opaque TunnelIo owns the upgraded transport directly. Hyper's
    // Upgraded retains any bytes read beyond the HTTP boundary. The handler
    // remains in this tracked task, so shutdown can abort it and dropping its
    // TunnelIo closes the underlying transport.
    let transport = hyper_util::rt::TokioIo::new(upgraded);
    let mut handler_join =
        tokio::spawn(async move { handler(TunnelIo::from_transport(transport)).await });
    tokio::select! {
        _ = &mut handler_join => {}
        _ = cancel => {
            handler_join.abort();
            let _ = handler_join.await;
        }
    }
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::TunnelClosed,
            format!("tunnel closed: {kind}"),
        )
        .connection_id(conn_id),
    );
    // `_permit` + `_active_guard` release exactly once on drop.
}

struct ActiveTunnelGuard {
    ops: crate::ops::OpsContext,
}

impl Drop for ActiveTunnelGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_tunnels
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual same-host A/B qualification harness. It models the old
    /// `duplex + copy_bidirectional` bridge and the direct opaque transport
    /// using identical deterministic Tokio duplex transports. Not a CI gate.
    #[tokio::test]
    #[ignore = "manual tunnel bridge A/B qualification"]
    async fn tunnel_transport_ab_qualification() {
        use std::time::Instant;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn trial(direct: bool, payload_bytes: usize, iterations: usize) -> Vec<u128> {
            let (mut client, transport) = tokio::io::duplex(4 * 1024 * 1024);
            let (handler_io, bridge_join) = if direct {
                (TunnelIo::from_transport(transport), None)
            } else {
                let (handler, bridge_end) = TunnelIo::pair();
                let mut bridge_end = bridge_end.into_duplex();
                let mut transport = transport;
                let bridge = tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut bridge_end, &mut transport).await;
                });
                (handler, Some(bridge))
            };
            let mut handler_io = handler_io;
            let handler = tokio::spawn(async move {
                let mut buffer = vec![0; 64 * 1024];
                loop {
                    let n = match handler_io.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if handler_io.write_all(&buffer[..n]).await.is_err() {
                        break;
                    }
                }
            });
            let payload = vec![0x5a; payload_bytes];
            let mut times = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let start = Instant::now();
                client.write_all(&payload).await.unwrap();
                let mut echoed = vec![0; payload_bytes];
                client.read_exact(&mut echoed).await.unwrap();
                assert_eq!(echoed, payload);
                times.push(start.elapsed().as_nanos());
            }
            drop(client);
            handler.abort();
            let _ = handler.await;
            if let Some(bridge) = bridge_join {
                let _ = bridge.await;
            }
            times
        }

        fn percentile(samples: &[u128], p: usize) -> u128 {
            let mut sorted = samples.to_vec();
            sorted.sort_unstable();
            sorted[(sorted.len() - 1) * p / 100]
        }
        for repeat in 1..=3 {
            for bytes in [1024, 64 * 1024, 1024 * 1024] {
                for direct in [false, true] {
                    let samples = trial(direct, bytes, 100).await;
                    let elapsed: u128 = samples.iter().sum();
                    let payload = bytes as u128 * samples.len() as u128;
                    let mib_s = payload as f64 / (elapsed as f64 / 1e9) / (1024.0 * 1024.0);
                    eprintln!("repeat={} mode={} payload_bytes={} iterations={} p50_ns={} p95_ns={} p99_ns={} aggregate_mib_s={:.3} harness_spawned_tasks={} bridge_buffer_bytes={}", repeat, if direct {"direct"} else {"bridge"}, bytes, samples.len(), percentile(&samples,50), percentile(&samples,95), percentile(&samples,99), mib_s, if direct {1} else {2}, if direct {0} else {TUNNEL_IO_BUFFER_BYTES});
                }
            }
        }
    }
    use eggserve_primitives::header_block::HeaderBlock;
    use eggserve_primitives::version::HttpVersion;

    fn headers(pairs: &[(&str, &str)]) -> HeaderBlock {
        let mut b = HeaderBlock::new();
        for (n, v) in pairs {
            b.push_str(*n, *v).unwrap();
        }
        b
    }

    #[test]
    fn handshake_rejects_framing_at_server_boundary() {
        let mut h = headers(&[("content-length", "5")]);
        assert!(matches!(
            validate_handshake_headers(&mut h),
            Err(TunnelError::ForbiddenHeader(_))
        ));
    }

    #[test]
    fn h1_classification_strict_at_server_boundary() {
        let good = headers(&[("connection", "upgrade"), ("upgrade", "eggserve-test")]);
        assert!(classify_h1_upgrade(&good, HttpVersion::Http11).is_some());
        assert!(classify_h1_upgrade(&good, HttpVersion::Http10).is_none());
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
        let req = TunnelRequest::new(TunnelKind::Http1Upgrade, None, None);
        let shared = Arc::new(TunnelShared::new());
        let sidecar = Arc::new(std::sync::Mutex::new(None));
        shared.mark_committed();
        let cap = TunnelCapability::new(req, None, shared, sidecar);
        let err = cap
            .accept(HeaderBlock::new(), |_io| async move {})
            .unwrap_err();
        assert_eq!(err, TunnelError::AfterCommit);
    }

    #[tokio::test]
    async fn double_accept_is_deterministic() {
        let req = TunnelRequest::new(TunnelKind::Http1Upgrade, None, None);
        let shared = Arc::new(TunnelShared::new());
        // First capability claims acceptance.
        let sidecar = Arc::new(std::sync::Mutex::new(None));
        let first = TunnelCapability::new(req.clone(), None, shared.clone(), sidecar.clone());
        first
            .accept(HeaderBlock::new(), |_io| async move {})
            .unwrap();
        // A second capability sharing the same commitment state cannot
        // accept again (deterministic AlreadyAccepted, not a second task).
        let second =
            TunnelCapability::new(req, None, shared, Arc::new(std::sync::Mutex::new(None)));
        let err = second
            .accept(HeaderBlock::new(), |_io| async move {})
            .unwrap_err();
        assert_eq!(err, TunnelError::AlreadyAccepted);
    }
}
