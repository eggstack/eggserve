# Runtime Architecture

Custom startup is filesystem-agnostic, static planner metadata is attached to
canonical responses, and the only production file-stream admission pool is
created by `RuntimeState` once per running server. Fully consumed streamed
request bodies remain keep-alive eligible; an `Active` body delegated to a
downstream task keeps reuse pending until completion, while an abandoned or
failed body causes the response and connection to close.

## Overview

> **Status: Experimental.** The `server` module API is subject to change without notice.

`eggserve-server` is the direct generic dependency layer for new
application-server consumers. It depends on `eggserve-primitives`, preserves
one-shot request-body and response-stream semantics, and has no static-file
edge. Since Plan 215 it is the implementation home of the mature H1
connection runtime (ops, error taxonomy, response policy, shared limit
authority, service contract shape, connection vocabulary, H1 config/state,
 H1 driver, Hyper conversion boundary; Plan 249: the only production H1 Hyper
builder/connection driver, with compatibility `Auto` classified before any
Hyper service exists and core executing H2 only), proven wire-for-wire against
the compatibility pipeline by `crates/eggserve-core/tests/direct_h1_parity.rs`
(16 scenarios; tunnel excluded, Plan 216 input) plus the Plan 217 direct-service convergence fixture and the Plan 249 `auto_h1_delegation` accept-path suite. `eggserve-static`
composes hardened static behavior on top. The
historical `eggserve-core::server` remains the compatibility runtime for
advanced H2/H3, tunnel, listener, proxy, and TLS paths during extraction.

The runnable public-API demonstrations are [`static_server.rs`](../crates/eggserve-core/examples/static_server.rs),
[`custom_service.rs`](../crates/eggserve-core/examples/custom_service.rs),
and [`application_service.rs`](../crates/eggserve-core/examples/application_service.rs)
(Plan 197: buffered echo, bounded streamed pipe, lifecycle long-poll, no
static filesystem).
Both wait for `ready()`, use loopback defaults, and call synchronous
`shutdown()` followed by consuming `wait()` after Ctrl+C. The examples are
intentionally not routers or application frameworks.

## Components

### Server (direct `eggserve-server`: TCP-only H1)

The main entry point for the **direct** crate. Created via
`eggserve-server::Server::builder()`, configured with a `RuntimeConfig`,
then started with a custom service via `.start_with_service(service)` —
the only start entry on the direct server, which binds (or adopts) a TCP
listener and drives every accepted connection through the direct H1
driver. The direct server has NO `start()` / `serve_config` /
`static_service()`, NO `from_unix_listener` / `from_systemd_*` /
`http3_socket`, and NO `LifecycleState` / `ready()` / `force_shutdown()` /
`endpoints()` / `tcp_local_addr()` — those are `eggserve-core`
compatibility orchestration (Unix/TLS/H3/listener composition) documented
as core below.

### ServerBuilder (direct scope + core extensions)

Direct `eggserve-server::ServerBuilder` (TCP-only H1) exposes:

- `runtime(config)` — set the `RuntimeConfig`
- `bind(addr)` — override the bind address; the server will bind to this address on start
- `ops_context(ops)` — attach an explicit per-runtime `OpsContext` (sink, counters, correlation IDs); unset clones the process-global default so CLI/compatibility construction needs no new configuration (experimental)
- `from_listener(listener)` — use a pre-bound `TcpListener` instead of binding on start; ownership transfers to the runtime after start, and nonblocking mode is normalized automatically.
- `from_std_listener(std_listener)` (Plan 201) — same as above from a standard-library TCP listener; nonblocking normalized, other socket options preserved, no duplicate bind. Returns `Result` (failed conversion closes the passed socket)
- `build()` — validate configuration and construct the server.

The following are **`eggserve-core` compatibility `ServerBuilder`
extensions only** (same accept/admission/pipeline underneath, plus
listener/TLS/H3 composition) — not present on the direct builder:

- `serve_config(config)` — compatibility convenience for `start()`; it is
  consumed into one `StaticService` during `build()` and is ignored by
  `start_with_service()` custom-service startup
- `from_listener` on core additionally fronts the H1/H2 selector when
  `http2` is enabled (strict HTTP/1 by default). With `http3`, core also
  binds a same-port UDP/QUIC endpoint after resolving the TCP address;
  `http3_identity` supplies its separate TLS 1.3/`h3` identity.
  Caller-owned streams use the corresponding connection entry points
- `from_unix_listener(listener)` / `from_std_unix_listener(std_listener)` (Plan 201, Unix only) — serve HTTP over a pre-bound Unix-domain listener through the same accept/admission/pipeline (`ConnectionContext::for_unix()`, truthful `None` IP endpoints). Filesystem path creation/removal stays with the caller (never unlinked); abstract-namespace sockets need no cleanup. Unix is plaintext (TCP TLS is not implicitly enabled) and H3 is unavailable over Unix streams
- `from_systemd_index(i)` / `from_systemd_name(name)` (Plan 201, Unix only) — adopt an explicit socket-activation descriptor (`LISTEN_PID`/`LISTEN_FDS`/`LISTEN_FDNAMES`, never silent fd 3; `SOCK_STREAM` + `SO_ACCEPTCONN` + `AF_INET`/`AF_INET6`→TCP / `AF_UNIX`→Unix via `rustix::net`; datagram/connected-socket rejection; failure never closes). Returns `Result`; no supervision/notification in core (`clear_systemd_activation_env` is explicit)
- `http3_socket(std_socket)` (Plan 201, `http3` only) — prebound UDP for the QUIC endpoint, wrapped in Quinn (`TokioRuntime`) at startup with no Quinn types in the public contract; ports must match the resolved TCP port (same-port TCP+UDP), and a supplied socket with H3 disabled fails closed
- `build()` on core additionally constructs the built-in `StaticService`
  once when `serve_config()` was supplied; invalid static roots fail here
- `static_service(root)` — compatibility convenience: create a `StaticService` rooted at the given path

### RuntimeConfig

Transport-level configuration separate from service-level concerns:
- Bind address
- Connection limits (`max_connections`)
- In-flight service limit (`max_in_flight_requests`, independent of idle keep-alive connections)
- File-stream limits
- Parser ceilings (`max_buf_size`, `max_headers` set explicitly on Hyper; `max_header_bytes`, `max_request_target_bytes` enforced pre-service)
- Timeouts (header read, connection total, handler, body read, graceful shutdown, keep-alive idle, response write no-progress)
- Maximum requests per connection (`max_requests_per_connection`, `None` = unlimited)
- Final-boundary response privacy (`response_policy`: `Server` suppressed by
  default, `Date` system-clock by default with EggServe ownership and Hyper
  automatic `Date` disabled, validated denylist, minimal errors; direct H1
  service metadata ownership is a separate explicit projection override)
- TLS configuration (feature-gated)
- Maximum request body size (hard ceiling)

Safe defaults match or strengthen CLI defaults and are owned once by the
Plan 179 canonical kernel (`crate::runtime_limits`). `Limits::validate()`
delegates shared checks to that kernel; `RuntimeConfigBuilder::build()`
validates the candidate shared group plus `ResponsePolicy`;
`RuntimeConfig::validate()` guards hand-constructed configs (fields are
public); `ServerBuilder::build()`, `RuntimeState::try_new()`, and the
caller-owned `serve_http1_connection` boundary all enforce before
semaphore/Hyper use. `connection_total_timeout` defaults to 60 seconds;
`Duration::ZERO` explicitly disables only that total lifetime ceiling.
Independent idle, handler, body, header, write, admission, request-count, and
shutdown bounds remain active. Migration from `server_header`: `None` is
`response_policy.server_identification = None`; use
`RuntimeConfigBuilder::server_header(..)` or
`RuntimeConfig::server_header_value()`; see `docs/migration-guide.md`.
Services may lower request-body ceilings but cannot raise the runtime hard
ceiling.

### Service Trait

```rust
pub trait Service: Send + Sync + 'static {
    fn request_body_policy(
        &self,
        _head: &RequestHead,
    ) -> RequestBodyPolicy {
        RequestBodyPolicy::Reject
    }

    fn call(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>>;

    // Additive tunnel-aware entry (default drops the capability and runs
    // `call`, so ordinary services deny with ordinary HTTP unchanged).
    fn call_with_tunnel(
        &self,
        request: Request,
        tunnel: Option<TunnelCapability>,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>> {
        let _ = tunnel;
        self.call(request)
    }
}
```

Tunnel-aware services implement `Service::call_with_tunnel` directly or
via `service_fn_with_tunnel` / `TunnelServiceFn` (see
`crates/eggserve-server/src/service.rs`).

- `request_body_policy()` declares the service's body policy per request head; default is `Reject` (safe static default). The runtime enforces the hard `max_request_body_bytes` ceiling — services may lower it, never raise it
- Receives canonical `Request` envelope (RequestHead + RequestBody + `RequestContext`)
- Returns canonical `Response` or `ServiceError` — Plan 197 Track C keeps this shape (no `ServiceOutcome`); Plan 198 implements trailers in the message-body abstraction (`ResponseStream::with_trailers`) and interim via the request-scoped `InterimSender`. Tunnels landed in Plans 199/216/217 via the additive `Service::call_with_tunnel` entry (see Tunnel handoff below); Plan 284 keeps the direct `TunnelIo` transport (`TunnelIoInner::Direct` in `crates/eggserve-server/src/tunnel.rs`)
- Must be `Send + Sync` for sharing across connections; no `poll_ready` — Tower readiness belongs in `eggserve-server::tower` (`TowerToEggserve` per-request clones, `EggserveToTower` adapter-local ready; Plans 200/276), native admission stays runtime-owned and deterministic (Plan 197 Track E)
- Panics caught at tokio task boundary

### RequestContext (Plans 197–198)

`eggserve_core::primitives::RequestContext` is the single deliberate
attachment point for transport-authenticated metadata and opaque
capabilities. It owns `ConnectionInfo` + `RequestLifecycle` +
bounded `InterimSender` (`interim()`); tunnel capabilities (Plans
199/216/217, landed) arrive via `Service::call_with_tunnel` with cloneable
validated intent at `RequestContext::tunnel_request()` (the 0.1
compatibility `take_tunnel()` slot was removed by Plan 217). Cloning is cheap (`ConnectionInfo`
value + `Arc`-backed lifecycle/interim) and never clones the one-shot
`RequestBody`. There is no generic type map: downstream state belongs in
the service wrapper, Tower/framework maps belong in the `http-interop`/`tower`
adapters (Plan 200 implemented; see `docs/http-interop.md`). No
raw socket, Hyper, H2/H3, rustls-session, or executor handle is exposed.
`ConnectionInfo`/`TlsInfo` come from the observed transport only;
`Forwarded`/`X-Forwarded-*` stay ordinary untrusted headers by default and
populate provenance-tagged effective fields only under an explicit Plan 202
trusted-proxy policy (raw peer/local endpoints are always preserved).

Direct H1 keeps origin-form request targets by default. A caller may opt into
`Http1RequestTargetMode::OriginOrAbsolute` for generic proxy-shaped service
dispatch; the canonical target preserves its semantic URI scheme/authority
and exposes path/query separately. This does not add outbound forwarding,
does not change CONNECT tunnel handling, and does not widen static path
resolution (`RequestTarget::parse` and `ConfinedPath` remain origin-only).

The fixed-cost campaign (Plans 234–240) preserves this boundary while keeping
common request work compact: `RequestTarget` stores indices into its validated
raw target, wire-trailer state is materialized only for consumers that need it,
and already-terminal lifecycles avoid a redundant transition notification.
The H1 adapter keeps its callable generic internally, and connections skip the
tunnel-set mutex until a tunnel exists; the JoinSet remains the sole tunnel
owner and drainer. These are internal representations with no public API,
framing, lifecycle, or cancellation change.

`Request` exposes `context()` / `new_with_context()` /
`into_parts_with_context()` for the forward-compatible path;
`connection()` / `lifecycle()` / `into_parts()` /
`into_parts_with_lifecycle()` forward to the context and preserve the
Plan 175 common path.

### Commitment and cancellation contract (Plans 197–198, normative)

Seven ordered stages (not started → interim emitted → final head
committed → body streaming → terminal trailers emitted → complete /
cancelled / failed; tunnel handoff is a staged transport acceptance
observed out-of-band by the pipeline, never a revisited stage); later stages never
revisit earlier ones. Interim 1xx are bounded via `InterimSender` (only 1xx,
no 101/body/trailers, no post-commit, HTTP/1.0 suppressed, single 100).
The final head commits once the service returns
`Ok(Response)` and the runtime normalizes it (marking interim committed).
Response trailers stream as one terminal block after body completion (no data
after, `HEAD`/body-forbidden never poll). There is never a second
HTTP error after commitment: producer/body/trailer failures after commitment
close (H1) or reset the stream (H2/H3, siblings survive) with sanitized diagnostics only.
Peer/shutdown/timeout/transport failure cancels the `RequestLifecycle`
(first reason wins; `#[non_exhaustive]` — match with a wildcard);
normal return, body EOF, or normal keep-alive completion never cancel by
themselves. Delegated `Active` bodies defer reuse until `Complete`;
`Abandoned`/`Failed` forces safe close; in-memory bodies never force
close. Full stage/race table lives in
`docs/downstream-app-server.md`.

### Error taxonomy at the boundary (Plan 197 Track F)

Client-facing errors stay sanitized (fixed `<status> <reason>` or empty).
Callers distinguish rejection (`200..=599` preserved, `1xx`/out-of-range
→ 500), internal, panic (`is_panic`), timeout (`is_timeout`, 504),
cancellation (lifecycle), and committed-stream failure (close/reset, no
second error). `ServiceError` is a struct with a private kind so future
categories do not break construction. `RequestBodyError`, `ServerError`,
`RequestCancellationReason`, and `ConnectionOutcome` are
`#[non_exhaustive]` — match with a wildcard arm. H2/H3 reset codes are not
exposed to ordinary applications.

### StaticService

Hardened static file service implementing `Service`:
- Descriptor-relative path confinement (Unix)
- Dotfile, symlink, and directory-listing policy enforcement
- GET/HEAD-only semantics
- Conditional and range request handling
- ETag and Last-Modified generation
- Canonical file/range bodies retained as opened capabilities until transport
- Static-service body policy is `Reject` for every request; bodyless unsupported
  methods reach the service and return 405, while body-bearing requests are
  rejected before invocation
- Normalizes eagerly via `normalize_response`; the connection pipeline
  normalizes again idempotently (`Response::is_normalized`) so custom
  streaming responses share the single framing policy

### Gateway adapters (CGI/FastCGI live downstream)

Plan 167 closed as no-go: no in-tree CGI executor or FastCGI gateway. A
downstream gateway is a plain `Service` — no core exceptions, no parser or
framing ownership. It maps backend output into a canonical `Response`
(`ResponseBody::Stream` for gateway bodies), lets the connection pipeline own
framing/normalization and the Plan 165 privacy boundary, and enforces its own
bounds (subprocess concurrency, env/PARAMS caps, stdout header scan, STDERR
cap, deadlines, kill/abort with reaping on timeout/disconnect/shutdown/drop).

### Tunnel handoff (Plans 199 + 216 — generic upgrade/Extended CONNECT)

Plan 199 implements generic tunnels, superseding deferred Plan 176;
Plan 216 moves authority to the direct crates without a new service model.
Neutral intent vocabulary (`TunnelKind`/`ProtocolName`/`TunnelRequest`/
`TunnelError`, classifiers, handshake validator) lives once in
`eggserve-primitives::tunnel` (Hyper/Tokio-free); transport execution
(`TunnelCapability`/`TunnelIo`/acceptance state/H1 detection/bounded
bridging/shared `run_tunnel`) lives once in `eggserve-server::tunnel`.
Plan 217 removes the 0.1 compatibility `RequestContext::take_tunnel()` slot; services use `Service::call_with_tunnel`. The removed wrapper yielded the thin compatibility
capability (same method names; `accept` delegates and converts only the
handshake response shape). Direct services receive the capability via the
additive `Service::call_with_tunnel` parameter (default drops it, so
ordinary services deny with ordinary HTTP unchanged); validated intent is
visible cloneably via `RequestContext::tunnel_request()` on both stacks.
`Service` still returns `Response` only (Plan 197 Track C kept; no
`ServiceOutcome`): `accept(headers, handler)` consumes the capability and
returns a handshake `Response` (`101` H1 / `200` otherwise, runtime owns
framing, no raw socket); the staged transport acceptance is observed
out-of-band by the pipeline (sidecar), never carried inside the value
(`is_tunnel()` is always `false`). Handlers own only IO (`FnOnce(TunnelIo)`;
capture the lifecycle for cancellation). Bounded single-owner `TunnelIo`
(`AsyncRead + AsyncWrite`, 32 KiB, lifecycle-aware). Ordinary `101` via
`Response` cannot forge a handoff. H1 runs with `.with_upgrades()`
(read-ahead preserved), H2 with `enable_connect_protocol()`, H3 with
`enable_extended_connect(true)`; unused capabilities drop (denial stays
ordinary HTTP). Downstream WebSocket framing lives over `TunnelIo` (see
`tunnel_upgrade.rs` echo + `tokio-tungstenite` fixture); raw
Hyper/h2/h3/Quinn bypass remains unsupported. This authority is
inbound-only (Plan 223): eggserve accepts server-side tunnels and never
dials outbound CONNECT proxies — the shared outbound H1 wire primitive
(authority/request-head encode, bounded response-head parse, read-ahead
preservation; dialing/DNS/TLS/timeout/retry/routing/lifecycle
caller-owned) lives outside eggserve for eggfetch/eggress with no
eggserve dependency, and eggress inbound `handle_connect`/auth/
forwarding/relay stays locally owned there.

### ServerHandle

For direct `eggserve-server` H1 embedding, `ServerHandle::into_parts()` yields
a cloneable `ServerControl` and single-owner `ServerCompletion`. Keep the
control value in a supervisor while selecting on
`completion.wait() -> Result<ShutdownResult, ServerError>`; this future
borrows the value and can be cancelled and awaited again. The typed path
reports top-level and escaping runtime connection-task panics as
`ServerError::Terminal`. The legacy direct `ServerHandle::wait(self) -> ()`
remains source-compatible and discards that terminal detail. Dropping an
unfinished `ServerCompletion` requests graceful shutdown. See the
[leaf-only fixture](../crates/eggserve-server/tests/downstream_embedding.rs).

Control handle returned by `Server::start()`. Not `Clone` — there is exactly one handle per server instance.

The following `LifecycleState` / `ready()` / `force_shutdown()` /
`endpoints()` / `tcp_local_addr()` surface is the **core compatibility
handle** (multi-listener orchestration), not the direct
`start_with_service` handle (direct supervisors use
`ServerHandle::into_parts()` + `ServerControl` + typed
`ServerCompletion::wait()`, which surfaces `ServerError::Terminal` per
Plan 270; legacy `wait()` discards that terminal detail):

- `local_addr()` — bound address (useful for port-zero discovery)
- `tcp_local_addr()` (Plan 201) — `Some` TCP address, or `None` for Unix-only servers (never fabricated); `local_addr()` panics there with a pointer to `endpoints()`
- `endpoints()` (Plan 201) — all adopted listeners as `BoundEndpoint` with stable IDs (`tcp-0`, `unix-0`), not positions; readiness means every entry was adopted and protocol config validated
- `state()` — current `LifecycleState`
- `ready().await` — wait for Running state; returns `Startup` if the server failed during startup or if a shutdown raced startup (`Starting` → `Stopped` via `Lifecycle::drain`); other non-ready states return `Config`
- `shutdown()` — trigger graceful shutdown (idempotent; multiple calls are safe)
- `force_shutdown(deadline).await` — graceful shutdown followed by deadline; if the server doesn't stop within `deadline`, remaining tasks are abandoned and `ShutdownResult::Forced` is returned
- `wait().await` — consume handle, trigger graceful shutdown if still running, wait for completion
- `ops_context()` — this server's `OpsContext` (cheap cloneable handle)
- `ops_snapshot()` — bounded, non-blocking snapshot of this server's counters (never reset-on-read; no exporter/endpoint)
- Drop behavior: triggers graceful shutdown — the server stops accepting new connections and drains in-flight requests

### Error Types

- `ServerError` — startup and lifecycle errors:
  - `Bind(io::Error)` — TCP bind failure
  - `Config(String)` — invalid configuration
  - `AlreadyStarted` — double-start attempt
  - `NotStarted` — operation on unstarted server
  - `Accept(io::Error)` — accept-loop error
  - `TlsSetup(String)` — TLS certificate/config error
  - `Transport(String)` — response normalization or body conversion failure
  - `ShutdownTimeout` — graceful shutdown timed out
  - `Startup(String)` — fatal startup error (bind failure, TLS error, etc.)
  - `Terminal(String)` — terminal runtime error
- `ServiceError` — per-request errors (Internal, Rejected, Panic, Timeout)
- `ShutdownResult` — outcome of a shutdown operation: `Clean`, `Timeout`, or `Forced`

## Lifecycle State Machine (core compatibility handle)

The Created → Starting → Running → Draining → Stopped/Failed machine and
the `AlreadyStarted` / `NotStarted` double-start guards below describe the
core `ServerHandle` orchestration, not direct `start_with_service` (whose
typed completion reports `ServerError::Terminal` via
`ServerCompletion::wait()`, Plan 270).

```text
Created → Starting → Running → Draining → Stopped
            ↓                    ↓
         Failed               Failed
```

States:
- **Created** — initial state after `ServerBuilder::build()`
- **Starting** — `Server::start()` called; binding and accept-loop init in progress
- **Running** — listener bound, accept loop polled, ready to accept connections
- **Draining** — shutdown requested; draining in-flight connections
- **Stopped** — all connections drained; terminal state
- **Failed** — fatal error during startup or drain; terminal state

Allowed operations per state (`ok` = succeeds, `err` = returns error, `yes` = accepted, `noop` = no-op, `idempot` = idempotent):

| State     | build | start | ready | shutdown | force_shutdown | wait |
|-----------|-------|-------|-------|----------|----------------|------|
| Created   | yes   | yes   | err   | yes      | yes            | yes  |
| Starting  | —     | err   | yes   | yes      | yes            | yes  |
| Running   | —     | err   | ok    | ok       | ok             | yes  |
| Draining  | —     | err   | err   | idempot  | ok             | yes  |
| Stopped   | —     | err   | err   | noop     | noop           | ok   |
| Failed    | —     | err   | err   | noop     | noop           | err  |

`ready()` before `start()` returns a not-started/config error rather than waiting — callers must invoke `start()` first.
`Lifecycle::drain()` from `Created` or `Starting` moves directly to `Stopped` (and wakes `ready()` waiters with a `Startup("shutdown raced with startup")` error) so that shutdown-before-ready does not hang.

Race safety: state is stored in an `AtomicU8` with `compare_exchange` for all transitions. Channel notifications (`watch` for readiness, `broadcast` for terminal state) ensure waiters are awakened without polling.

## Listener Error Classification

Listener errors are classified by `io::ErrorKind` into transient, resource-exhaustion, and persistent categories. All errors emit structured log events.

- **Direct** `eggserve-server` accept loop: fixed 10 ms sleep on accept
  errors (`Duration::from_millis(10)` in `crates/eggserve-server/src/lib.rs`).
- **Core** compatibility `accept_loop_multi`: bounded exponential backoff
  starting at 1 ms (`BACKOFF_MS` ramp in
  `crates/eggserve-core/src/server/accept.rs`, via
  `classify_accept_error()`), shared across families.

Plan 201 runs one `accept_loop_multi` over every adopted listener (TCP plus,
on Unix, UDS) through the same admission/backoff/pipeline — not a second
loop. Admission uses `try_acquire` so accepted sockets never queue
unboundedly (saturation drops/closes); the backoff state is shared across
families; every accept event carries a stable `listener` field (`tcp-0`,
`unix-0`); shutdown breaks the select promptly and the drain below closes
undispatched transports.

## Connection/Task Tracking

- Each accepted connection spawns a tokio task, tracked in a `JoinSet` with bounded concurrency
- Graceful drain waits for each task up to the configured deadline; remaining tasks are dropped (aborted)
- Forced shutdown abandons remaining tasks immediately
- RAII permits ensure connection, file-stream, and in-flight-service permits are released on drop, even under cancellation. Canonical file-backed responses acquire the shared file-stream permit at transport conversion, so custom Rust services and the Python static façade share the same ceiling. Service permits are held across `Service::call()` and recovered on timeout, panic, cancellation, disconnect, and shutdown.
- The per-connection driver enforces four independent deadlines (hard total lifetime, keep-alive idle, response write no-progress, shutdown) from shared `ConnectionActivity` state: the Hyper service closure observes requests/responses while `ProgressIo` observes socket progress and `TrackedBody` observes response completion, so a stalled response is distinguishable from an idle keep-alive connection.
- Normal peer resets do not terminate the server; only fatal runtime errors transition to Failed
- Python callback failures are converted to generic service errors with fixed diagnostic categories; handler exception text and response data are not logged.

### Runtime ownership corrective contract

`RuntimeState` now lives in `server/runtime.rs` and `accept_loop_multi` plus
TLS handlers live in `server/accept.rs` (Plan 206 Track A). The facade
`server/mod.rs` keeps `Server`/`ServerBuilder` and re-exports
`RuntimeState`/`accept::*` (`pub(super)` where facade needs). Public import
paths (`server::RuntimeState`) resolve unchanged.

Each running server creates exactly one `RuntimeState`, including one
`max_file_streams` semaphore, one `max_in_flight_requests` semaphore, and one
observability context (`OpsContext`: sink, counters, correlation-ID source).
`RuntimeState::new` / `try_new` clone the process-global default;
`RuntimeState::with_ops` attaches an explicit context for isolated embedding,
and `ops_snapshot()` reads that runtime's counters without a monitoring
endpoint. The accept loop clones that state into every connection;
`StaticService` owns only its pinned root, policy, listing limits, validated
static representation metadata, and its service-owned event context (wired to
the server context by `ServerBuilder`; direct embedders pass the same context
via `StaticServiceBuilder::ops_context` for coherent ownership). All
canonical file and range responses acquire the same file-stream permit at the
single Hyper conversion boundary, and every `Service::call()` execution holds
an in-flight permit (acquired with `try_acquire`, so exhaustion answers 503
immediately with no hidden queue). Custom Rust and Python services have no
implicit root or static state. Caller-owned drivers (`serve_http1_connection`
and the feature-gated `serve_http_connection`) number connections from the
shared state's context (the separate static ID source is gone); explicit IDs
via the corresponding `*_with_id` functions still win.

The runtime asks the service for the body policy for the actual request. GET,
HEAD, DELETE, OPTIONS, and extension methods are not globally body-forbidden;
TRACE content remains rejected. An unconsumed streamed body marks the response
the protocol-neutral `close_and_cancel_body` disposition, which the HTTP/1
adapter maps to `Connection: close`, drops the body, and prevents connection
reuse.

## Shutdown Semantics

**Graceful shutdown** (`shutdown()` / `wait()`):
1. Stop accepting new connections (broadcast signal breaks accept loop)
2. Signal active connections to stop accepting new requests
3. Allow in-flight requests and response streams to complete
4. Wait until the configured `graceful_shutdown_timeout` deadline
5. Abort remaining tasks and close connections
6. Release all permits and resources
7. Return `ShutdownResult::Clean`

**Forced shutdown** (`force_shutdown(deadline)`):
Same as graceful, but with a caller-specified deadline. If the server doesn't stop within the deadline, remaining tasks are abandoned and `ShutdownResult::Forced` is returned.

**ShutdownResult variants:**
- `Clean` — all in-flight connections completed within the grace period
- `Timeout` — the grace period expired; some connections were forcibly cancelled
- `Forced` — the server was forcefully terminated by the caller

## Tokio Integration

- Requires an existing Tokio runtime; the server does not create nested runtimes
- Supports both multi-threaded and current-thread runtimes
- All `Server` and `ServerHandle` methods that return futures are `Send` and can be awaited from any runtime thread
- `Service` trait requires `Send + Sync + 'static` for sharing across connection tasks
- No blocking operations on core async threads beyond known filesystem constraints

## Connection Pipeline

Direct `eggserve-server` is H1-only: it exposes
`serve_http1_connection` (+ `_with_id` for explicit correlation IDs, `+_with_policy`
for a pre-projected `H1ConnectionPolicy`) and nothing else — there is no
direct `serve_http_connection`. The feature-gated `serve_http_connection`
entry, the H2 prior-knowledge/TLS-ALPN selector, and H3 lifecycle are
`eggserve-core` / `eggserve-h3` compatibility paths that delegate H1 work
to the direct driver (Plan 249: compatibility `Auto` classifies before any
Hyper service exists; core executes H2 only). Three entry paths converge
on the same canonical service pipeline:

1. TCP accept with connection permit → optional TLS handshake (feature-gated)
2. TLS accept with connection permit → TLS handshake completed by caller
3. Unix-domain accept with connection permit (Plan 201, Unix only; plaintext, `for_unix()`)
4. Caller-owned stream (no socket, scheme asserted by caller)

All paths then share the same steps:

4. Protocol selection and connection setup via Hyper: direct H1 uses the
    Plan 282 `H1ConnectionPolicy` projection of the runtime config
    (`RuntimeConfig::h1_connection_policy()`, projected once at the
    accept loop; see `crates/eggserve-server/src/config.rs`). The narrow
    projection excludes bind/TLS-handshake/listener/file-stream settings.
    HTTP/2 on the core compatibility path uses the validated `Http2Config`
    projection. Cleartext
    H2 uses bounded prior-knowledge detection; TLS uses ALPN (`h2` before
    `http/1.1`). There is no HTTP/1 `Upgrade: h2c` path.
5. Request conversion to canonical types (EggServe `max_request_target_bytes` → 414, `max_header_bytes` → 431, pre-service)
6. Body ingestion (policy selection, Content-Length preflight, transfer decoding; Stream creates a shared lifecycle + `RequestLifecycle` and registers for cancellation)
7. Shared service admission (`max_in_flight_requests`; 503 on exhaustion) and
   invocation with timeout (`min(body_read_timeout, handler_timeout)` during
   Stream calls for compatibility, disambiguated via lifecycle state;
   `handler_timeout` bounds response-start and the remaining
   `body_read_timeout` continues after response-start via watchdog)
8. Canonical response normalization (`normalize_then_convert`: idempotent
   `normalize_response` then Hyper conversion; runtime owns `Content-Length`,
   `Transfer-Encoding`, reuse) with `ErrorRepresentationPolicy` for conversion
   failures (generic 500/503, no leaks)
9. Transport-body conversion with completion tracking (buffered/file/streaming; known-length mismatch
   and producer failure close post-commitment; `HEAD`/body-forbidden never
   poll; every body releases the outstanding-response slot on end/error/drop)
10. Final-boundary privacy (`finalize_runtime_response`: denylist after service
    construction, `Server` subordinate to policy by default, `Date` runtime-
    owned by default with explicit direct-service override and
    Hyper auto-`Date` disabled, `Last-Modified <= Date` enforcement, no peer
    metadata copied, no log/error text reflected)
11. Protocol-neutral lifecycle disposition and protocol adapter mapping, then
    permit release and connection termination under the driver deadline loop
    (keep-alive idle, write no-progress, hard lifetime, shutdown). H2 tracks
    per-response application-body poll progress, which prevents sibling
    traffic from refreshing a stalled producer, but Hyper 1.11.1 exposes no
    safe public stream-local wire-progress/reset hook. Bytes already handed
    to Hyper are bounded by its configured send buffer and the hard
    connection lifetime; the timeout therefore uses a conservative
    connection shutdown and must not be described as a guaranteed per-stream
    wire no-progress timer.

### Connection module ownership (Plan 180)

The pipeline above lives in `server/connection/` (facade `mod.rs` plus nine
invariant-owned submodules); the split is mechanical and behavior-preserving.
External code imports only the facade (`ConnectionContext`,
`ConnectionShutdown`, `ConnectionOutcome`, `serve_http1_connection`,
`serve_http1_connection_with_id`, and, with `http2`,
`serve_http_connection`/`serve_http_connection_with_id`).

| Module | Owns |
|--------|------|
| `context.rs` | Public facade types: transport context, shutdown token, outcome |
| `lifecycle.rs` | Live-request registry + abnormal-termination cancellation (contextual) |
| `activity.rs` | Connection deadlines, request/response activity identities, in-flight admission guard, tracked response bodies; carries the connection's `OpsContext` |
| `transport.rs` | `ProgressIo` read/write progress observation |
| `driver.rs` | HTTP/1 and feature-gated HTTP/2 builders, protocol selection, graceful close/GOAWAY, outcome classification, deadline/select loop (observability via activity's context) |
| `pipeline.rs` | `CanonicalHyperService`, body preparation, and the single shared service-invocation kernel (explicit `OpsContext` parameter) |
| `request.rs` | Target/header ceilings, framing checks, body-policy selection, body bridge (contextual rejections) |
| `response.rs` | Normalization, panic containment, body-error mapping, neutral dispositions, final privacy, and the HTTP/1 disposition adapter; contextual streaming/file conversion |
| `deferred_body.rs` | Deferred-body watchdog + terminal-state tracker (contextual) |

Dependency direction is acyclic: `pipeline`/`driver` depend on the rest,
`activity` depends on `response` (final privacy only), and nothing depends
back on `pipeline`/`driver` except the facade. Hyper types stay out of public
signatures; HTTP/1 upgrade machinery and H2 extended CONNECT/upgrade paths are
not enabled.

### Transport-neutral connection driver (Plan 163)

`serve_http1_connection(io, service, config, context, runtime_state, shutdown)`
is the strict HTTP/1 connection driver. With the `http2` feature,
`serve_http_connection(...)` is the H1/H2 selector for caller-owned streams.
The caller supplies an already-established
bidirectional async byte stream (`AsyncRead + AsyncWrite`), a canonical
`Service`, and the following per-connection state:

- **`ConnectionContext`** — transport description: `for_tcp(local, remote, tls)`
  for TCP/TLS socket connections, `for_quic(local, remote, tls)` for native
  HTTP/3 QUIC sockets, `for_unix()` for Unix-domain streams (Unix only;
  plaintext, no IP endpoints), and `for_non_socket(scheme, tls)` for caller-owned
  streams. No I2P types, no `Any` map, no fabricated addresses.
  `Forwarded`/`X-Forwarded-*` headers are ordinary untrusted headers by
  default, not part of this type; a Plan 202 trusted-proxy policy may attach
  a PROXY preamble result here via `with_proxy_endpoints` (raw endpoints
  preserved). Scheme and TLS are asserted by the caller.
- **`ConnectionShutdown`** — per-connection graceful-shutdown token, independent
  of `ServerHandle`. Shutdown is level-triggered and idempotent: once
  `shutdown()` is called, every current and future `cancelled()` waiter
  completes (check/register/recheck, no polling), including a token
  pre-signaled before `serve_http1_connection()` starts over duplex.
  Permits and producer tasks are released on driver exit regardless of
  outcome.
- **`Arc<RuntimeState>`** — shared admission (file-stream and in-flight
  service semaphores). Callers
  must share one `Arc` across all streams; `RuntimeState` owns only transport
  budgets, never static filesystem state or routing.

`serve_http1_connection_with_id` is the same pipeline with an explicit
`conn_id` for structured log correlation (used by the TCP/TLS accept loop).

**`ConnectionOutcome`** is returned for observability. Variants: `Normal` (clean
EOF/keep-alive close), `ClientError` (protocol or client error),
`HeaderTimeout`, `IdleTimeout` (keep-alive idle expiry, clean),
`WriteTimeout` (response no-progress expiry), `TotalTimeout`,
`Shutdown` (graceful shutdown requested),
`Internal`. `is_clean()` returns `true` for `Normal`, `Shutdown`, and
`IdleTimeout`.

**TCP/TLS Server** drives H1 through the single direct authority:
`eggserve-server` owns every Hyper H1 connection; the compatibility layer
owns H2 selection/execution plus PROXY/TLS/listener composition. The
feature-gated protocol selector resolves `Auto` via bounded cleartext
H2-prior-knowledge detection before any Hyper service exists — H1 delegates
the replayable stream to the direct driver, H2 enters core H2 execution.
TLS selects from ALPN (`h2` then `http/1.1`), with ALPN H1 delegating to the
same direct authority. The accept loop bridges its `broadcast` shutdown
signal to a per-connection `ConnectionShutdown` token inline
(`run_with_connection_shutdown`, structured under the JoinSet-owned
connection task; no detached forwarder, receiver drops on normal completion).
The historical `serve_connection_with_runtime_state` compatibility entry
likewise adapts its broadcast receiver to the token in-task and delegates to
the direct H1 driver. Raw Hyper helpers are `pub(crate)` — external callers
use the public connection facade.

The runnable caller-owned-stream demonstration is
[`caller_owned_stream.rs`](../crates/eggserve-core/examples/caller_owned_stream.rs):
it drives one request through a `tokio::io::duplex` pair with a non-socket
`ConnectionContext` and one shared `RuntimeState`, then exits without
binding a socket.

Advanced direct H1 hosts may project `RuntimeConfig` once into
`H1ConnectionPolicy` and pass that effective policy to the caller-owned
driver. `PolicyOwner` makes six selected application deadlines/semantic
ceilings independently EggServe-owned or externally owned; parser, framing,
header bounds, and optional hard total connection lifetime remain runtime
authority. `AdmissionOwnership` independently represents service and tunnel
gates; externally owned gates are absent from `RuntimeState` rather than
approximated with a large semaphore. Both ownership objects default to
EggServe, preserving the standalone server's bounded behavior.

Plans 288–289 add two direct-H1-only seams to the projected policy. Parser
buffer and header-count validation no longer treats the legacy 4 MiB / 10,000
guidance constants as hard maxima; the Hyper minimum (8192 bytes) and positive
header count remain enforced, and defaults remain 64 KiB / 100. An embedder
may transfer only the aggregate post-parse header-byte ceiling with
`with_request_header_bytes_owner(PolicyOwner::External)`. This does not bypass
Hyper parser limits or other canonical request validation.

`with_response_metadata_ownership(ResponseMetadataOwnership { .. })` can
transfer Date and/or Server metadata for successful service responses. The
runtime still owns canonical framing and the explicit stripped-header
denylist. Runtime-generated responses (including service errors, timeouts,
rejections, and invalid external Date fallback) continue using `ResponsePolicy`.
External Date values must be one valid HTTP-date; absence is preserved,
duplicates or invalid values become generic 500 responses, and a future
`Last-Modified` is removed. An embedding origin that owns Date is responsible
for emitting it on response classes required by RFC 9110. All these ownership
switches default to EggServe; high-level `Server` does not enable them.

`RuntimeRejectionPresenter` is a synchronous direct-H1 response-presentation
hook. Its input contains only a typed category and runtime-selected status.
Its output is bounded and cannot choose status, framing, privacy headers, or
connection disposition. Presenter panics and invalid/oversized output fall
back to the configured generic representation. Hyper parser failures raised
before canonical request conversion remain outside the hook.

**Invariants retained:** Hyper HTTP/1.1 parsing, and the feature-gated Hyper
HTTP/2 driver with explicit `Http2Config`, framing validation
(duplicate-CL rejection; lone TE+CL normalizes to TE-wins per RFC 9112 §6.1), TRACE/body policy, canonical Request conversion, handler timeout
ceiling, panic containment, canonical response normalization, runtime-owned
framing (`Content-Length`, `Transfer-Encoding`, reuse), Plan 165 response
privacy, file/stream admission via shared semaphore, lifecycle-aware
incomplete-body close (Complete reusable, Active deferred without forced
close, Abandoned/Failed forced close; Hyper pinned to prevent next-request
parsing until the framing boundary), and shutdown/drain semantics. H2 keeps
response-producer poll accounting per stream so sibling writes cannot mask a
stalled application producer; this is not proof of stream-level flow-control
or socket progress after Hyper accepts a frame. The current Hyper public
server API has no safe stream-reset hook at this boundary, so an H2 stall
uses the conservative bounded connection-shutdown fallback. All paths share
a single normalization and framing authority.

### Deferred bodies + request lifecycle (Plan 174)

- `RequestBody` shares one `RequestShared` allocation (Active/Complete/
  Abandoned/Failed + cancellation) instead of a boolean; dropping an
  incomplete network body marks Abandoned (ownership-derived, no manual
  flag). In-memory copies never force close.
- `Request::lifecycle()` / `lifecycle_clone()` / `into_parts_with_lifecycle()`
  expose the transport-neutral `RequestLifecycle` (`cancelled()`,
  `is_cancelled()`, `cancellation_reason()` with PeerDisconnected /
  ServerShutdown / ConnectionTimeout / TransportFailure, first reason wins).
  It fires on peer loss, forced close, hard timeouts, shutdown after drain,
  and body/transport failure — never merely on Service return, body EOF, or
  normal response completion on keep-alive.
- Stream `Service::call` stays collapsed as `min(body, handler)` for
  compatibility (disambiguated via lifecycle state); after response-start
  with Active body the remaining `body_read_timeout` continues via watchdog
  (Failed + cancel + driver close so pending polls wake). Connection reuse
  waits for both boundaries; abandonment closes via Hyper (pinned by
  `tests/deferred_lifecycle.rs` over TCP, TLS, and duplex).
- `max_in_flight_requests` bounds pre-response `Service::call` only;
  downstream app tasks own a separate budget (Track F). Send-side
  response failure may precede lifecycle cancellation; treat either as
  cancellation.
- H2 Reject uses Hyper's public header-time `Incoming::is_end_stream()`
  state; H1 continues to use its framing headers. H3 performs one bounded
  receive probe when `Content-Length` does not prove presence, discarding the
  first DATA chunk on rejection rather than buffering the request.
- Every H3 request registers its shared lifecycle. QUIC connection loss
  cancels all live observers; body/stream failures cancel only that request,
  and forced graceful-drain termination cancels remaining observers with
  `ServerShutdown` before task abort.

The downstream builder contract (bounded full-duplex bridging, timeout
split, admission split, byte metadata, shutdown ordering) is documented in
`docs/downstream-app-server.md` and qualified externally by
`crates/eggserve-core/tests/app_server_consumer.rs` (Plan 175), which uses
only `primitives` + `server` plus ordinary downstream dependencies.

**Python impact:** No raw Python transports in this plan; `ConnectionInfo`
views expose `None` for both `local_addr` and `remote_addr` on non-socket
transports (Python `PyConnectionInfo` already mirrors this).

### Streaming responses (Plan 162)

Custom services return `ResponseBody::Stream(ResponseStream)` without Hyper:

```rust
ResponseBody::Stream(ResponseStream::with_known_length(stream, len))
ResponseBody::Stream(ResponseStream::new(stream)) // chunked
```

`ResponseStream::new` and `with_known_length` require a `Send + 'static`
producer, but not `Sync`. The producer is one-shot and is exclusively polled
by the owning connection task. `Response` and the erased transport body remain
`Send`; no body is concurrently polled from multiple tasks. Hyper's
`boxed_unsync()` is an internal implementation detail shared by TCP, TLS, and
caller-owned transports.

The public `to_hyper_response()` helper is an explicit outbound conversion
adapter for embedders that choose to integrate at Hyper's transport boundary.
Its body type is opaque and consumers should rely only on the
`http_body::Body<Data = bytes::Bytes, Error = std::io::Error>` contract. The
runtime's semaphore-aware variant is internal; downstream `Service`
implementations and caller-owned connection users do not need to import Hyper.

File responses use the configured `stream_chunk_size` (128 KiB by default)
with bounded `BytesMut`/`read_buf` reads. The adapter passes each full/range
representation's logical `chunk_len` explicitly and bounds the file view with
`AsyncReadExt::take`; allocator capacity cannot cause read-ahead beyond the
representation. Direct H1 write-stall accounting is
based on forward socket writes observed by `ProgressIo`; response producer
polls do not extend that timer. H2 and H3 retain their separately documented
producer/send progress semantics.

- Known lengths emit runtime `Content-Length`; overrun/underrun close the
  connection with `response_stream_length_mismatch` diagnostics.
- Unknown lengths omit `Content-Length`; HTTP/1 selects chunked. Clean
  completion may keep the connection reusable.
- Empty chunks skipped, large chunks split zero-copy (not rejected).
- Producer panics contained at the poll boundary
  (`response_stream_producer_panic`); details never reach the client.
- Cancellation (disconnect/shutdown/HEAD suppression) drops promptly
  (`response_stream_cancelled`). See `streaming_service.rs`.

## Body ingestion pipeline

The runtime handles request body ingestion transparently for services:

1. **Policy selection**: The runtime queries `Service::request_body_policy()` and enforces the global ceiling (`max_request_body_bytes`). The effective policy is the minimum of service preference and runtime ceiling.

2. **Framing validation**: the runtime rejects requests where both Transfer-Encoding and Content-Length survive to its validator before body construction (under Hyper 1.11 a lone Content-Length is discarded during parsing with Transfer-Encoding winning per RFC 9112 §6.1; duplicate Content-Length headers are rejected by Hyper's decoder). Identical duplicate Content-Length values without Transfer-Encoding are rejected as duplicates by the validator (safe default).

3. **Content-Length preflight**: Before reading the body, the runtime validates `Content-Length` against the effective limit. Conflicting or oversized declarations are rejected with 413.

4. **Body consumption**: For `Buffer` policy, the entire body is read under `body_read_timeout` and delivered as an in-memory `RequestBody`. For `Stream` policy, the body is passed through with byte accounting. For `Reject` policy, the body is discarded and the service receives an empty body.

5. **Error mapping**: Body errors map to deterministic HTTP responses:
   - 400: malformed framing, length mismatch
   - 408: body read timeout
   - 413: body too large
   - 500: transport error

6. **Incomplete body handling**: When a service returns with an Abandoned/Failed Stream body, the connection closes. When it returns with an Active body delegated to a downstream task, no forced close occurs; reuse waits for body Complete and abandonment closes via Hyper. Active drain is not safely implementable because the body stream is consumed into the `Request` envelope by value.

## Request body handling

The runtime manages request body lifecycle through the `Request` envelope:

### Body policy

- `Service::request_body_policy(&RequestHead)` — service-declared per-request policy (method-aware)
- `RuntimeConfig::max_request_body_bytes` — hard ceiling no service can exceed
- Incomplete body handling: close on Abandoned/Failed, deferred without close on Active (Hyper-pinned)

### Request envelope

```rust
pub struct Request {
    head: RequestHead,      // immutable request metadata
    body: RequestBody,       // one-shot, bounded body stream (shares lifecycle)
    context: RequestContext, // Plan 197: connection + lifecycle + future opaque capabilities
}
```

### Service trait

```rust
pub trait Service: Send + Sync + 'static {
    fn request_body_policy(&self, head: &RequestHead) -> RequestBodyPolicy {
        RequestBodyPolicy::Reject  // safe default
    }
    fn call(&self, request: Request) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>>;
}
```

Plan 197 keeps `call(Request) -> Response`: no `ServiceOutcome`, no
`poll_ready`. See the Service Trait section above for the normative
commitment/cancellation/admission contract.

### One-shot consumption

- `RequestBody::read_all(self)` — buffer entire body
- `RequestBody::next_chunk(&mut self)` — stream chunks
- `Stream` trait implementation for async iteration
- State machine: Unread → Streaming → Complete | Error (consumption); lifecycle: Active → Complete | Abandoned | Failed (ownership, Drop-derived for network bodies)

### Static service

The built-in `StaticService` declares `Reject` for request bodies. Its
canonical planner still returns `ResponseBody::File` for full and range
responses; the runtime performs admission and streaming.

### Python body policy mapping

Python `Server` constructor parameters map to Rust `RuntimeConfig` fields:

| Python parameter | Rust field | Default |
|------------------|-----------|---------|
| `request_body_mode="reject"` | service-declared via `request_body_policy()` | — |
| `request_body_mode="buffer"` | service-declared via `request_body_policy()` | — |
| `request_body_mode="stream"` | service-declared via `request_body_policy()` | — |
| `max_request_body_bytes` | `max_request_body_bytes` | 0 |
| `body_timeout_secs` | `body_read_timeout` | 30s |

The runtime enforces `max_request_body_bytes` as a hard ceiling. Service-specific limits may only lower it. Body policy is service-declared, not a runtime field.

`eggserve.lowlevel` additionally exposes the Plan 164 admission/parser set
(`max_in_flight_requests`, `max_buf_size`, `max_headers`, `max_header_bytes`,
`max_request_target_bytes`, `keep_alive_idle_timeout_secs`,
`max_requests_per_connection`, `response_write_timeout_secs`) and the safe
Plan 165 privacy subset, plus bounded `Response.stream` over a 16-chunk
bridge (HEAD/body-forbidden never advance the iterator; async rejected).
In-flight admission is held by the connection pipeline before the Python
callback permit, so limits cannot deadlock or invert ownership.

## Python lifecycle mapping

The Python `Server` delegates to the actual Rust `Server` and `ServerHandle`
from `eggserve-core::server` rather than implementing its own accept loop.
The tokio runtime is stored in the `PyServer` struct (not created as a temporary),
ensuring the runtime lives as long as the server.

Lifecycle methods are mapped to the Rust `ServerHandle` API:

- `start()` → creates a bounded Tokio multi-thread runtime (2 worker threads), creates `ServerHandle` via `Server::builder()`, calls `handle.ready().await` so the server is in Running state when `start()` returns. For callback handlers, uses `start_with_service()` instead of `build()`.
- `stop()` → calls `ServerHandle::wait()`, joins thread
- `shutdown()` → calls `ServerHandle::shutdown()` (non-blocking)
- `force_shutdown(deadline)` → calls `ServerHandle::force_shutdown()`, waits with deadline
- `wait()` → blocks on thread join
- `state` → reads `ServerHandle::state()` when a handle exists; returns `"stopped"` if the server was started but the handle is gone; falls back to the lifecycle state tracker otherwise

Policy forwarding: the Python `StaticPolicy` is cloned into the Rust `ServeConfig` (`static_policy` field), so custom policy settings (directory listing, symlinks, dotfiles) are respected by the static service.

Lifecycle states map directly: Python's `ServerState` enum mirrors
`LifecycleState` (Created, Starting, Running, Draining, Stopped, Failed).

Handler timeout (`handler_timeout_secs`, default 30s) is best-effort in
Python; enforced at transport level by the Rust server. Coroutine handlers
are rejected with a 500 response. Signal handling (SIGTERM/SIGINT → graceful
shutdown) is handled by the Python subprocess wrapper, not the Rust server.

## Platform-specific signal limitations

### Unix (Linux, macOS, BSD)

- SIGTERM triggers graceful shutdown (same as Ctrl+C)
- SIGINT (Ctrl+C) triggers graceful shutdown
- Both signals are handled via `tokio::signal::unix`
- Signal handlers are installed once at startup

### Windows

- Ctrl+C (SIGINT) triggers graceful shutdown
- SIGTERM is not supported on Windows
- Service control events (for Windows services) are not handled

### Limitations

- Only one shutdown signal is handled; repeated signals do not escalate to forced shutdown
- Signal handlers cannot be reconfigured after startup
- Python subprocess wrappers handle signal forwarding to the Rust process

## Maintainability convergence notes (Plans 243–258)

- Plan 243: durable direct-server shutdown with runtime-owned JoinSet task draining; see [`release/plan-248-maintainability-convergence-closure.md`](../release/plan-248-maintainability-convergence-closure.md).
- Plans 280–286 (`0.3.0` line): external `PolicyOwner`/`H1PolicyOwnership`
  (six deadlines/ceilings) and `AdmissionOwnership` (service + tunnel gates)
  with the narrow `H1ConnectionPolicy` projection (Plans 280–282), the
  `RuntimeRejectionPresenter` hook (Plan 283), opt-in `OriginOrAbsolute`
  absolute-form dispatch (Plan 278, core projects `OriginOnly`), direct
  opaque `TunnelIo` transport kept (`TunnelIoInner::Direct`, Plan 284),
  the `0.3.0` breaking embedding contract (Plan 285), and registry fixtures
  (Plan 286); see [`release/plan-280-external-policy-ownership-closure.md`](../release/plan-280-external-policy-ownership-closure.md),
  [`release/plan-282-h1-connection-policy-projection-closure.md`](../release/plan-282-h1-connection-policy-projection-closure.md),
  [`release/plan-283-typed-runtime-rejection-closure.md`](../release/plan-283-typed-runtime-rejection-closure.md),
  [`release/plan-284-tunnel-transport-ab-qualification.md`](../release/plan-284-tunnel-transport-ab-qualification.md),
  and [`release/plan-286-embedding-contract-publication-closure.md`](../release/plan-286-embedding-contract-publication-closure.md).
- Plans 244/249–250: single H1 authority — compatibility `Auto` classifies before any Hyper service exists and core executes H2 only, with structured per-connection shutdown; see [crate-topology.md](crate-topology.md) and [`release/plan-250-h1-authority-lifetime-corrective-closure.md`](../release/plan-250-h1-authority-lifetime-corrective-closure.md).
- Plan 245: `eggserve-static::StaticService` owns static request planning/rendering; core keeps a delegating wrapper; see the Plan 248 closure record above.
- Plan 253: every core/server connection overlap is classified (no second H1 implementation); see the ledger in [crate-topology.md](crate-topology.md).
- Async pointers (H1-only, experimental): Plan 254 async lifecycle/streaming parity and Plans 257–258 suppressed-body permit lifetime; see [`release/plan-256-post-convergence-maintenance-interop-closure.md`](../release/plan-256-post-convergence-maintenance-interop-closure.md) and [`release/plan-258-async-suppressed-body-lifetime-corrective-closure.md`](../release/plan-258-async-suppressed-body-lifetime-corrective-closure.md).

## Security Properties

- Response normalization (hop-by-hop stripping, content-length computation) is runtime-owned
- Final response framing is runtime-owned at one Hyper boundary. Date/Server
  are runtime-owned by default; direct H1 can explicitly transfer successful
  service-response metadata while runtime errors remain runtime-owned.
- Services cannot bypass final framing policy through the safe API
- Handler failures map to deterministic generic responses without internal
  leakage (`Minimal` fixed bodies or `Empty`; application `Ok` bodies never
  rewritten; hostile values sanitized before logs)
- Filesystem policy belongs to the service, not the runtime; static validators
  (`ETag`/`Last-Modified`) are explicitly configurable via
  `StaticMetadataPolicy`
