# eggserve-core — Deep Dive

`eggserve-core` is the compatibility and composition layer for EggServe's
direct primitives, runtime, static-serving, TLS, and optional protocol
adapters. Plan 214 moves the canonical model and hardened static
resolver into the direct layers; core remains the compatibility home for
Python-facing adapters, advanced protocol runtime paths, and legacy
configuration. Plan 225 closes the 217–224
program by proving this crate is a compatibility facade rather than an
implementation authority: every production module is classified
(facade / adapter / documented orchestration / transport glue), duplicate
implementations and leftover dependencies are removed, and the topology
gate rejects silent re-expansion (see
`release/plan-225-compatibility-facade-closure.md`). Plan 226 executes
the `0.2.0` version transition and the Rust 1.89 MSRV move with no
ownership change.

Plans 243–258 continue as an API-preserving maintenance index with no
ownership change: compatibility H1 entry points delegate to the single
`eggserve-server` H1 authority while core executes H2 only (Plans
243–244/249–250; see
`release/plan-250-h1-authority-lifetime-corrective-closure.md`); core
`StaticService` is a wrapper over `eggserve-static::StaticService` with no
second renderer (Plan 245); the eight core/server connection parallels are
classified in the overlap ledger in [crate-topology.md](crate-topology.md)
with sharing deferred (Plan 253); wheel typing/registration and
orphan-source/topology-gate cleanup carry no API change (Plans
246–247/251–252/255); and async-Python parity keeps bounded first-pull plus
suppressed-body permit lifetimes (Plans 254/257–258; see
`release/plan-256-post-convergence-maintenance-interop-closure.md` and
`release/plan-258-async-suppressed-body-lifetime-corrective-closure.md`).

The direct layers are [`eggserve-primitives`](crate-topology.md),
[`eggserve-server`](crate-topology.md), and
[`eggserve-static`](crate-topology.md). The compatibility crate exposes them
under `eggserve_core::layers` without changing the established module paths.

TLS identity, trust, client-authentication, and reload policy is implemented by
the neutral `eggnet-tls` crate and re-exported at `eggserve_core::tls` for
compatibility. `eggserve-core` retains only its transport-facing rustls use;
its opt-in HTTP/3 adapter consumes the direct QUIC dependencies through
`eggserve-h3`; see [eggnet-tls.md](eggnet-tls.md) and
[eggserve-h3.md](eggserve-h3.md).

External Rust consumers should start with `eggserve_core::primitives` for the
semver-considered canonical HTTP/security facade. The `eggserve_core::server` module is an
experimental, transport-owning HTTP runtime exposing `Server`,
`RuntimeConfig`, `ServerHandle`, `Service`, `service_fn`, and `StaticService`.
The filesystem, path, response, and MIME implementation modules remain
internal; importing Hyper directly is not required for either static serving or
custom services.

Plan 175 qualifies the HTTP-only downstream application-server bridge using
the public `primitives` and experimental `server` modules. This qualification
does not make the experimental runtime API stable or add upgrade/WebSocket
support; those remain outside the current canonical boundary.

Executable demonstrations live under
[`crates/eggserve-core/examples/`](../crates/eggserve-core/examples/) and are
indexed with the CLI and Python examples in [`examples/README.md`](../examples/README.md):
`static_server` shows the built-in confined service, `custom_service` shows a
small public `service_fn`, `streaming_service` shows known/unknown-length
streams, `caller_owned_stream` drives the canonical pipeline over a
caller-owned stream without a listener, and `primitives` performs response
planning without opening a socket. They are compiled by `scripts/verify.sh full`.

## Composition role (Plan 226)

Keeping the extended orchestration in core is intentional for the current
pre-1.0 line, not an implementation blocker:

- first-party frontends (`eggserve-bin`, `eggserve-python`) may depend on
  core for full composed-server behavior (`ServeConfig` /
  `try_from_serve_config`, full TLS/H2/H3 `Server`, full `StaticService`
  with extra headers and error policy, listing budgets, handle lifecycle);
- new low-level consumers should prefer the direct crates
  (`eggserve-primitives`, `eggserve-server`, `eggserve-static`,
  `eggserve-h3`, `eggnet-tls`), reachable during migration through
  `eggserve_core::layers`;
- removing or deprecating `eggserve-core` requires a separate future
  migration plan with release notes; do not reclassify core as an
  implementation authority unless a future plan intentionally moves
  implementation back into it.

## Compatibility module map

The map below describes the compatibility surface that remains in core. The
canonical values and hardened static implementation now live in the direct
crates; core modules that mirror those names are retained for source
compatibility and advanced protocol/Python integration.

| Module | Visibility | Purpose |
|--------|------------|---------|
| `lib.rs` | pub | Declares all modules; documents the 3-tier stability model |
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` |
| `policy.rs` | **pub** | `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy` |
| `limits.rs` | **pub** | `Limits` — connection count, file streams, header/target/body sizes, timeouts |

| `path/` | — (removed, Plan 219) | Deleted; `ConfinedPath`/`PathPolicy`/`PathRejection`/`percent_decode`/platform helpers live once in `eggserve-static::path`, re-exported via `primitives` |
| `fs/` | — (removed, Plan 219) | Deleted; `PinnedRoot`/`RootGuard`/descriptor- and handle-relative traversal live once in `eggserve-static::fs` (crate-internal), surfaced via `SecureRoot` |
| `response.rs` | pub(crate) | Compatibility response helpers and advanced runtime integration |
| `mime.rs` | — (removed, Plan 219) | Deleted; MIME selection lives once in `eggserve-static`, surfaced via `ResolvedFile::content_type()` |
| `primitives/` | **pub** | Compatibility facade for embedding consumers; direct canonical types live in `eggserve-primitives`, static/path/filesystem authority lives in `eggserve-static` (Plan 219) |
| `primitives/secure_root.rs` | **pub** via re-export | Compatibility facade; `SecureRoot`/`ResolvedFile`/`ResolvedDirectory`/`ResolvedResource`/`resolve_and_plan` are implemented once in `eggserve-static` (Plan 219) |
| `primitives/planner.rs` | **pub** via re-export | Compatibility facade; conditional/range planning functions are implemented once in `eggserve-static` (Plan 219) |
| `primitives/body.rs` | **pub** | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming abstraction |
| `primitives/response_stream.rs` | **pub** | `ResponseStream`, `ResponseStreamError`, `MAX_RESPONSE_STREAM_CHUNK_BYTES` — transport-independent streaming bodies |
| `primitives/canonical.rs` | **pub** via re-export | Compatibility facade (Plan 225 closure): re-exports `eggserve_primitives::canonical::*` preserving `primitives::canonical::X` and `primitives::X` paths, delegates Hyper conversion to `eggserve_server::adapters`, and keeps an inline `adapters` submodule for the 0.1 `canonical::adapters::to_hyper_response` path. There is one normalization authority, not two; the former `primitives/canonical/` submodule copy was deleted and the topology gate rejects its return |
| `primitives/connection_info.rs` | **pub** | `ConnectionInfo` raw peer/local plus Plan 202 effective layer (`proxy_source`/`proxy_destination`/`proxy_provenance`, `effective_client`/`effective_scheme`/`effective_authority`/`forwarded_provenance`) |
| `primitives/proxy.rs` | **pub** | Plan 202 policy and bounded parsers (`IpPrefix`, `ProxySourceKind`, `TrustedProxyConfig`/`ProxyProtocolConfig`/`ForwardedConfig`, PROXY v1/v2, `Forwarded`/`X-Forwarded-*` single-hop) |

| `server/` | **pub** (experimental) | Runtime service boundary: `mod.rs` — `Server`/`ServerBuilder`/re-exports/tests; `runtime.rs` — `RuntimeState`; `accept.rs` — `accept_loop_multi`/handlers/sources/TLS helpers (`pub(super)` where facade needs); `config.rs` — Builder + `try_from_serve_config` + re-exports + tests; re-exports `serve_http1_connection` and feature-gated `serve_http_connection` plus connection context/outcome types |
| `server/lifecycle.rs` | **pub** (experimental) | `LifecycleState` — lifecycle state machine (Created → Starting → Running → Draining → Stopped/Failed) |
| `server/connection/` | **pub** (experimental) | Transport-neutral driver facade (`mod.rs`: strict H1 entry points delegating to the direct `eggserve-server` driver, feature-gated H1/H2 `serve_http_connection` with `Auto` classified before any Hyper service exists, and internal protocol-selected entry); per-connection handling, body ingestion (H1 delegates to direct; core executes H2 only, Plan 249) |
| `server/http3.rs` | internal (`http3`) | Thin facade (Plan 220): projects core `RuntimeConfig`/`RuntimeState` into `eggserve-h3::accept_loop` sharing admission pools/ops; no second state machine |
| `server/config/` | **pub** (experimental) | Submodules (Plan 206 Track E): `runtime.rs` — `RuntimeConfig` single validation authority (delegates to `runtime_limits`); `http1.rs` — `pub(crate)` `Http1Config` projection; `http2.rs` — protocol config (validate `pub(super)`); `http3.rs` — facade re-exporting `eggserve_h3::Http3Config` (Plan 220 authority); `tls.rs` — TLS ownership pointer (no new knobs); facade `config.rs` keeps Builder + `try_from_serve_config` + re-exports + tests |
| `server/connection/context.rs` | pub via facade | `ConnectionContext`, `ConnectionShutdown` (level-triggered, idempotent), `ConnectionOutcome`; `ConnectionContext` carries an optional Plan 202 PROXY layer (`proxy_source`/`proxy_destination`/`proxy_provenance` via `with_proxy_endpoints`) |
| `server/proxy.rs` | pub(crate) | Plan 202 PROXY preamble reader: bounded timeout-protected `read_proxy_preamble` before TLS/HTTP with leftover replay (`PrefixedIo`); `LOCAL`/`UNKNOWN`/`UNSPEC`/UNIX truthful absence; TLVs ignored bounded |
| `server/connection/lifecycle.rs` | pub(crate) | `ConnectionRequests` live-request registry + abnormal-termination cancellation |
| `server/connection/activity.rs` | pub(crate) | `ConnectionActivity` deadlines state, `InFlightGuard` admission guard, `TrackedBody` completion tracking |
| `server/connection/transport.rs` | pub(crate) | `ProgressIo` read/write progress observation |
| `server/connection/driver.rs` | pub(crate) | H2-only Hyper execution plus the `Auto`/replay classifier delegating H1 to `eggserve-server` (Plan 249); graceful close, outcome classification, deadline/select loop, TCP + caller-token adapters |
| `server/connection/pipeline.rs` | pub(crate) | `CanonicalHyperService` + single request/service dispatch |
| `server/connection/request.rs` | pub(crate) | Target/header ceilings, framing checks, body-policy selection, Hyper body bridge |
| `server/connection/response.rs` | pub(crate) | Normalization, panic containment, body-error mapping, final-boundary privacy |
| `server/connection/deferred_body.rs` | pub(crate) | Deferred-body watchdog + terminal-state tracker |
| `ops/` | **pub** (semver-considered pre-1.0 for the event/sink/counter vocabulary and `OpsContext`; runtime attachment experimental with `server`) | Operational observability (Plan 206 Track H): `mod.rs` — `OpsContext` authority; `events.rs` — `Severity`/`EventKind`/`Field`/`Event`, sanitization, JSON rendering; `sinks.rs` — `LogSink` implementations; `counters.rs` — `OpsCounters`/`Snapshot` |

## Key Types

### `ServeConfig` (`config.rs`)

Top-level configuration. Holds bind address, root directory, limits, static
policy, and validated static representation metadata. Constructed by the CLI
or Python wrapper.

```rust
pub struct ServeConfig {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub limits: Limits,
    pub static_policy: StaticPolicy,
    pub default_content_type: String,
    pub extra_response_headers: Vec<(String, String)>,
}
```

### `ServeState` (`config.rs`)

Static state wrapping `ServeConfig` with one static `SecureRoot` capability
(Plan 219: the pinned-root/filesystem implementation lives once in
`eggserve-static`; `ServeState` retains the capability through the
primitives facade instead of a second core resolver). It does not own
transport admission. A running `server::Server` creates `RuntimeState` once;
that runtime state owns the shared Tokio semaphore for all file-backed
responses, including custom-service responses.

```rust
pub struct ServeState {
    pub(crate) config: Arc<ServeConfig>,
    pub(crate) secure_root: SecureRoot,
}
```

### `Limits` (`limits.rs`)

Resource limits with safe defaults:

| Field | Default | Purpose |
|-------|---------|---------|
| `max_connections` | 64 | Concurrent TCP connections |
| `max_in_flight_requests` | 64 | Concurrent service executions, independent of idle keep-alive connections |
| `max_file_streams` | 32 | Concurrent file streams (body transfer) |
| `max_request_body_bytes` | 0 | Runtime hard ceiling; services may opt into bodies only when greater than zero |
| `header_read_timeout` | 10s | Time to read full request headers (also bounds idle gaps when shorter than the idle timeout) |
| `connection_total_timeout` | 60s | Hard maximum connection lifetime (never reset) |
| `handler_timeout` | 30s | Per-request handler timeout |
| `body_read_timeout` | 30s | Total deadline for body consumption |
| `keep_alive_idle_timeout` | 60s | Idle keep-alive close after inactivity (resets on activity) |
| `max_requests_per_connection` | None | Completed requests per connection (`None` = unlimited) |
| `response_write_timeout` | 30s | Response no-progress timeout (steady progress never trips) |
| `graceful_shutdown_timeout` | 10s | Drain period after SIGTERM |
| `max_buf_size` | 64 KiB | HTTP/1 parser/read buffer ceiling (min 8192) |
| `max_headers` | 100 | Request header field count (Hyper answers 431) |
| `max_header_bytes` | 32 KiB | Aggregate header name+value bytes (431 pre-service) |
| `max_request_target_bytes` | 8192 | Request-target length (414 pre-service) |
| `max_listing_entries` | 4096 | Maximum entries to enumerate in a directory listing |
| `max_listing_response_bytes` | 1 MiB | Maximum size in bytes for a directory listing response body |
| `stream_chunk_size` | 128 KiB | Chunk size in bytes for file streaming reads and app-stream framing splits |

## Server Module (`server/`)

**Experimental** — API is subject to change without notice.

The `server` module provides a reusable, transport-owning HTTP runtime for embedding. It owns the TCP accept loop, connection management, optional TLS, and protocol-selection/composition glue. Strict-H1 entry points (`serve_http1_connection`) are compatibility facades delegating to the single `eggserve-server` H1 authority; the feature-gated `serve_http_connection` resolves `Auto` via bounded H2-prior-knowledge detection before any Hyper service exists (H1 delegates the replayable stream to direct, H2 enters core H2 execution). The drivers serve both TCP/TLS connections from the accept loop and caller-owned byte streams. Downstream projects provide a `Service` implementation; the runtime handles everything else.

### `Server` and `ServerBuilder`

```rust
let server = Server::builder()
    .runtime(RuntimeConfig { bind: addr, ..Default::default() })
    .static_service("/srv/www")?
    .build()?;
let handle = server.start().await?;
```

`Server::builder()` returns a `ServerBuilder`. Configure with `.runtime()` and `.static_service()` (or `.serve_config()` for pre-built configs), then `.build()` to construct the server. Call `.start()` (built-in static service) or `.start_with_service(service)` (custom service) to begin listening. Returns a `ServerHandle`.

`ServerBuilder::bind()` overrides the configured socket address. Use
`ServerBuilder::from_listener()` when transferring ownership of an existing
Tokio `TcpListener` (or `from_std_listener` for a std listener, `from_unix_listener` /
`from_std_unix_listener` for Unix-domain listeners on Unix, `from_systemd_index` /
`from_systemd_name` for socket-activation descriptors, and `http3_socket` for a
prebound H3 UDP socket; Plan 201); the runtime owns acceptance over the single
`accept_loop_multi`, while the strict H1 and feature-gated H1/H2 drivers also
serve caller-owned streams.

### `RuntimeConfig`

Transport-level configuration separate from service-level concerns (`ServeConfig`):

| Field | Default | Purpose |
|-------|---------|---------|
| `bind` | `127.0.0.1:8000` | Listen address |
| `max_connections` | 64 | Concurrent TCP connections |
| `max_in_flight_requests` | 64 | Concurrent service executions (503 on exhaustion) |
| `max_file_streams` | 32 | Concurrent file streams |
| `stream_chunk_size` | 128 KiB | File streaming read chunk size; configurable 64 B–1 MiB |
| `header_read_timeout` | 10s | Time to read request headers |
| `connection_total_timeout` | 60s | Hard maximum connection lifetime (never reset) |
| `handler_timeout` | 30s | Per-request handler timeout |
| `body_read_timeout` | 30s | Total deadline for body consumption |
| `keep_alive_idle_timeout` | 60s | Idle keep-alive close after inactivity |
| `max_requests_per_connection` | None | Completed requests per connection (`None` = unlimited) |
| `response_write_timeout` | 30s | Response no-progress timeout |
| `graceful_shutdown_timeout` | 10s | Drain period after shutdown signal |
| `max_buf_size` | 64 KiB | HTTP/1 parser buffer ceiling, set explicitly on Hyper |
| `max_headers` | 100 | Request header field count, set explicitly on Hyper |
| `max_header_bytes` | 32 KiB | Aggregate header bytes (431 pre-service) |
| `max_request_target_bytes` | 8192 | Request-target length (414 pre-service) |
| `response_policy` | suppressed `Server`, system-clock `Date`, no denylist, minimal errors | Final-boundary privacy; Hyper auto-`Date` disabled, EggServe sole authority |
| `max_request_body_bytes` | 0 | Request body size ceiling (0 = reject) |

Note: shared runtime defaults/validation live once in `crate::runtime_limits`
(Plan 179); `Limits` fields map onto `RuntimeConfig` by
`try_from_serve_config()` via `RuntimeConfig::from_shared_runtime`.
Hyper is currently 1.11.1; `max_buf_size`/`max_headers` are pinned explicitly so upgrades cannot silently widen parser memory. Migration from `server_header`: use `response_policy.server_identification` via `RuntimeConfigBuilder::server_header(..)`; see `docs/migration-guide.md`. Static validators are governed by `StaticPolicy.static_metadata` (`plan_file_response_with_preconditions_and_metadata`); see `response-planning.md`. Static-only listing/extra-header budgets stay outside the runtime kernel.

### `RuntimeState`

`RuntimeState::try_new(&config)` (validated; preferred) or `new(&config)`
(validates, panics with context) creates shared admission (file-stream and
in-flight-service semaphores)
for caller-owned streams. Callers must share one `Arc<RuntimeState>` across
all their connections rather than constructing one per connection; otherwise
file/response/service budgets become per-connection instead of server-wide. Hand-constructed invalid `RuntimeConfig` is rejected here and at `ServerBuilder::build()` / `RuntimeConfig::validate()` / the caller-owned `serve_http1_connection` boundary before semaphore/Hyper use.
It owns only transport-runtime admission (file-stream and in-flight service
permits); it never owns static filesystem state
or routing. The TCP/TLS `Server` constructs this internally and shares it
across connections.

### `Service` Trait

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
}
```

- Receives canonical `Request` envelope (RequestHead + RequestBody + `RequestContext`)
- Returns canonical `Response` or `ServiceError` — no `ServiceOutcome` (Plan 197 Track C keeps `Response`-only; trailers → message body (Plan 198), interim → request-scoped capability (Plan 198), tunnel → `TunnelCapability::accept` returning handshake `Response` + `TunnelIo` (Plan 199))
- Must be `Send + Sync` for sharing across connections; no `poll_ready` (Plan 197 Track E; Tower readiness belongs in the `tower` adapters, Plan 200 implemented — see `docs/http-interop.md`)
- Panics caught at tokio task boundary
- Commitment/cancellation normative in `docs/downstream-app-server.md` + `architecture/runtime.md`: final head commits on `Ok(Response)` + normalization; never a second HTTP error after commitment

`service_fn` creates a `Service` from an `Fn(Request) -> Future<Output = Result<Response, ServiceError>> + Send + Sync`. `service_fn_head` creates a service from a closure that only receives the request head (discarding the body); it uses `Reject` body policy. `service_fn_with_policy` creates a service with an explicit `RequestBodyPolicy`.

### `StaticService`

Hardened static file service implementing `Service`:
- Descriptor-relative path confinement (Unix)
- Dotfile, symlink, and directory-listing policy enforcement
- GET/HEAD-only semantics
- Conditional and range request handling
- ETag and Last-Modified generation
- Unknown-suffix fallback content type and ordered safe extra headers on final
  status-200 responses; runtime-owned and hop-by-hop fields cannot be replaced
- Produces canonical file-backed responses; the server runtime applies shared file-stream admission during transport conversion

### Body ingestion

The `server::connection` module implements the body ingestion pipeline:
- Selects effective body policy from service preference and runtime ceiling
- Validates Content-Length against limits before body consumption
- Buffers or streams request bodies through public `RequestBody` primitives
- Enforces body read timeout
- Maps body errors to deterministic HTTP responses
- Handles incomplete body close after service completion

### `ServerHandle`

Control handle returned by `Server::start()`:
- `local_addr()` — listening address
- `shutdown()` — trigger graceful shutdown
- `wait()` — wait for server to finish
- `ready()` — wait for server to be ready to accept connections
- `force_shutdown(deadline)` — trigger graceful shutdown and wait with a deadline; forcibly abort if deadline exceeded
- `state()` — query current `LifecycleState`

### Error Types

- `ServerError` — startup/lifecycle errors (Bind, Config, AlreadyStarted, NotStarted, Accept, TlsSetup, Transport, ShutdownTimeout, Startup, Terminal; `#[non_exhaustive]`, match with wildcard)
- `ServiceError` — per-request errors (Internal, Rejected, Panic, Timeout; struct with private kind, inspect via `is_panic`/`is_timeout`)
- `ShutdownResult` — returned by shutdown operations, carries final `LifecycleState` (variants: `Clean`, `Timeout`, `Forced`)

## Dependencies

Plan 225 closure: `eggserve-core` keeps only the dependencies its
compatibility orchestration and transport glue require. The MIME
perfect-hash map lives once in `eggserve-static`; core no longer depends
on `phf`. Confinement syscalls live once in `eggserve-static`; core keeps
`rustix` (Unix only) for listener accept/socket validation (`net` only,
plus ungated `rustix::io::Errno`), never the `fs` feature.

| Dependency | Purpose |
|------------|---------|
| `bytes` | Buffer types |
| `futures-util` | Streaming body adapters |
| `http-body` | `Body` trait for response-completion tracking (Plan 164) |
| `http-body-util` | Body combinators |
| `httpdate` | Last-Modified header formatting |
| `hyper` | HTTP/1.1 server, request/response types (transport glue) |
| `hyper-util` | Tokio integration, server utilities (transport glue) |
| `thiserror` | Derive macro for Error types |
| `tokio` | Async runtime (accept loop, connection pipeline) |
| `rustix` (Unix only, `net` feature) | Listener accept/socket validation; no `fs` (confinement lives in `eggserve-static`) |


## See Also

- [policy-system.md](policy-system.md) — Security policy types
- [path-confinement.md](path-confinement.md) — Path validation pipeline
- [filesystem-confinement.md](filesystem-confinement.md) — Filesystem traversal
- [primitives-api.md](primitives-api.md) — Public API boundary
- [response-planning.md](response-planning.md) — HTTP response planning
- [runtime.md](runtime.md) — Runtime service boundary (experimental)
- [api-stability.md](../docs/api-stability.md) — API classification by stability tier
- [release-contract.md](../docs/release-contract.md) — Product surface and compatibility commitments
- [python-http-server-compatibility.md](../docs/python-http-server-compatibility.md) — Python facade boundary
