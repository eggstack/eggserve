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

    /// Unwrap for runtime bridging (runtime-internal).
    ///
    /// Hidden: pipelines and compatibility H3 glue unwrap the bridge end
    /// for transport copying; services never unwrap (use `split` or
    /// async IO directly).
    #[doc(hidden)]
    pub fn into_duplex(self) -> tokio::io::DuplexStream {
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
    tunnel_semaphore: &Arc<tokio::sync::Semaphore>,
    ops: &crate::ops::OpsContext,
    conn_id: u64,
    lifecycle: RequestLifecycle,
    acceptance: TunnelAcceptance,
) -> bool {
    let permit = match tunnel_semaphore.clone().try_acquire_owned() {
        Ok(p) => p,
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
    _permit: tokio::sync::OwnedSemaphorePermit,
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

    // Bounded duplex: one end for the downstream codec, one for the bridge.
    // H1 read-ahead bytes are already inside `Upgraded` (`read_buf`);
    // wrapping via `TokioIo` preserves them without loss.
    let (io_for_handler, io_for_bridge) = TunnelIo::pair();
    let handler_join = tokio::spawn(async move {
        handler(io_for_handler).await;
    });

    let mut transport = hyper_util::rt::TokioIo::new(upgraded);
    let mut bridge_end = io_for_bridge.into_duplex();
    let bridge_result = tokio::select! {
        result = tokio::io::copy_bidirectional(&mut bridge_end, &mut transport) => {
            Some(result)
        }
        _ = cancel => {
            None
        }
    };
    // Bridge ended (peer close / transport failure / cancellation): ensure
    // the handler cannot linger without transport. Abort is safe: the
    // handler owns only TunnelIo, no raw transport.
    handler_join.abort();
    let _ = handler_join.await;
    if let Some(Err(_)) = bridge_result {
        // Transport copy failure is already terminal; no payload logged.
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
