# eggserve-core — Deep Dive

`eggserve-core` is the reusable Rust library behind every EggServe surface. It
contains the security-critical path confinement, policy enforcement, HTTP
request handling, response construction, MIME detection, and runtime service
boundary.

External Rust consumers should start with `eggserve_core::primitives` for the
semver-considered canonical HTTP/security facade. The `eggserve_core::server` module is an
experimental, transport-owning HTTP/1 runtime exposing `Server`,
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

## Module Map

| Module | Visibility | Purpose |
|--------|------------|---------|
| `lib.rs` | pub | Declares all modules; documents the 3-tier stability model |
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` |
| `policy.rs` | **pub** | `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy` |
| `limits.rs` | **pub** | `Limits` — connection count, file streams, header/target/body sizes, timeouts |

| `path/` | pub(crate) | Path confinement pipeline |
| `fs/` | pub(crate) | Filesystem confinement |
| `response.rs` | pub(crate) | Response helpers (file streaming, directory listing HTML, error responses) |
| `mime.rs` | pub(crate) | MIME type detection via `phf` map |
| `primitives/` | **pub** | Public facade for embedding consumers |
| `primitives/body.rs` | **pub** | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming abstraction |
| `primitives/response_stream.rs` | **pub** | `ResponseStream`, `ResponseStreamError`, `MAX_RESPONSE_STREAM_CHUNK_BYTES` — transport-independent streaming bodies |
| `primitives/canonical.rs` | **pub** | `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response`, `normalize_metadata`, `to_hyper_response` — canonical response types and normalization |

| `server/` | **pub** (experimental) | Runtime service boundary: `Server`, `ServerBuilder`, `ServerHandle`, `RuntimeConfig`, `Service` trait, `service_fn`, `StaticService`, `ServiceError`, `ServerError`; re-exports `serve_http1_connection`, `ConnectionContext`, `ConnectionShutdown`, `ConnectionOutcome` from `connection` |
| `server/lifecycle.rs` | **pub** (experimental) | `LifecycleState` — lifecycle state machine (Created → Starting → Running → Draining → Stopped/Failed) |
| `server/connection.rs` | **pub** (experimental) | Transport-neutral driver: `serve_http1_connection`, `ConnectionContext`, `ConnectionShutdown`, `ConnectionOutcome`; per-connection HTTP/1 handling, body ingestion |
| `ops` | **pub** | Operational event model, structured logging, listener error classification, operational counters |

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

Static state wrapping `ServeConfig` with one pinned root. It does not own
transport admission. A running `server::Server` creates `RuntimeState` once;
that runtime state owns the shared Tokio semaphore for all file-backed
responses, including custom-service responses.

```rust
pub struct ServeState {
    pub(crate) config: Arc<ServeConfig>,
    pub(crate) pinned_root: Arc<PinnedRoot>,
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
| `stream_chunk_size` | 8 KiB | Chunk size in bytes for file streaming reads and app-stream framing splits |

## Server Module (`server/`)

**Experimental** — API is subject to change without notice.

The `server` module provides a reusable, transport-owning HTTP runtime for embedding. It owns the TCP accept loop, connection management, optional TLS, and the canonical transport-neutral connection driver (`serve_http1_connection`). The driver serves both TCP/TLS connections from the accept loop and caller-owned byte streams sharing the same pipeline. Downstream projects provide a `Service` implementation; the runtime handles everything else.

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
Tokio `TcpListener`; the runtime owns TCP acceptance, but the canonical driver
(`serve_http1_connection`) also serves caller-owned streams.

### `RuntimeConfig`

Transport-level configuration separate from service-level concerns (`ServeConfig`):

| Field | Default | Purpose |
|-------|---------|---------|
| `bind` | `127.0.0.1:8000` | Listen address |
| `max_connections` | 64 | Concurrent TCP connections |
| `max_in_flight_requests` | 64 | Concurrent service executions (503 on exhaustion) |
| `max_file_streams` | 32 | Concurrent file streams |
| `stream_chunk_size` | 8 KiB | File streaming read chunk size |
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

Note: `Limits` fields map onto `RuntimeConfig` by `try_from_serve_config()`. Hyper is currently 1.11.1; `max_buf_size`/`max_headers` are pinned explicitly so upgrades cannot silently widen parser memory. Migration from `server_header`: use `response_policy.server_identification` via `RuntimeConfigBuilder::server_header(..)`; see `docs/migration-guide.md`. Static validators are governed by `StaticPolicy.static_metadata` (`plan_file_response_with_preconditions_and_metadata`); see `response-planning.md`.

### `RuntimeState`

`RuntimeState::new(&config)` creates shared admission (file-stream and
in-flight-service semaphores)
for caller-owned streams. Callers must share one `Arc<RuntimeState>` across
all their connections rather than constructing one per connection; otherwise
file/response/service budgets become per-connection instead of server-wide.
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

- Receives canonical `Request` envelope (RequestHead + RequestBody + ConnectionInfo)
- Returns canonical `Response` or `ServiceError`
- Must be `Send + Sync` for sharing across connections
- Panics caught at tokio task boundary

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

- `ServerError` — startup/lifecycle errors (Bind, Config, AlreadyStarted, NotStarted, Accept, TlsSetup, Transport, ShutdownTimeout, Startup, Terminal)
- `ServiceError` — per-request errors (Internal, Rejected, Panic, Timeout)
- `ShutdownResult` — returned by shutdown operations, carries final `LifecycleState` (variants: `Clean`, `Timeout`, `Forced`)

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `bytes` | Buffer types |
| `futures-util` | Streaming body adapters |
| `http-body-util` | Body combinators |
| `httpdate` | Last-Modified header formatting |
| `hyper` | HTTP/1.1 server, request/response types |
| `hyper-util` | Tokio integration, server utilities |
| `phf` | Compile-time perfect hash function for MIME map |
| `thiserror` | Derive macro for Error types |
| `tokio` | Async runtime |
| `rustix` (Unix only) | Descriptor-relative filesystem syscalls |


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
