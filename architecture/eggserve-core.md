# eggserve-core — Deep Dive

The core library crate. Contains all security-critical logic: path confinement, policy enforcement, filesystem traversal, HTTP request handling, response construction, MIME detection, and the public primitives API.

## Module Map

| Module | Visibility | Purpose |
|--------|------------|---------|
| `lib.rs` | pub | Declares all modules; documents the 3-tier stability model |
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` |
| `policy.rs` | **pub** | `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy` |
| `limits.rs` | **pub** | `Limits` — connection count, file streams, header/target/body sizes, timeouts |
| `service.rs` | **pub** (experimental) | `handle_request()` — the HTTP handler. Stability: experimental. See [api-stability.md](../docs/api-stability.md) |
| `error.rs` | pub(crate) | `Error` enum taxonomy |
| `path/` | pub(crate) | Path confinement pipeline |
| `fs/` | pub(crate) | Filesystem confinement |
| `response.rs` | pub(crate) | Response helpers (file streaming, directory listing HTML, error responses) |
| `mime.rs` | pub(crate) | MIME type detection via `phf` map |
| `primitives/` | **pub** | Public facade for embedding consumers |
| `primitives/body.rs` | **pub** | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming abstraction |
| `primitives/canonical.rs` | **pub** | `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response`, `normalize_metadata`, `to_hyper_response` — canonical response types and normalization |
| `primitives/client/` | **pub** (feature-gated: `client`) | HTTP client primitives: `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse` |
| `server/` | **pub** (experimental) | Runtime service boundary: `Server`, `ServerBuilder`, `ServerHandle`, `RuntimeConfig`, `Service` trait, `service_fn`, `StaticService`, `ServiceError`, `ServerError` |
| `server/lifecycle.rs` | **pub** (experimental) | `LifecycleState` — lifecycle state machine (Created → Starting → Running → Draining → Stopped/Failed) |
| `server/connection.rs` | **pub** (experimental) | Body ingestion pipeline, Hyper incoming-body adapter, transfer decoding, error mapping |
| `ops` | **pub** | Operational event model, structured logging, listener error classification, operational counters |

## Key Types

### `ServeConfig` (`config.rs`)

Top-level configuration. Holds bind address, root directory, limits, and static policy. Constructed by the CLI or Python wrapper.

```rust
pub struct ServeConfig {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub limits: Limits,
    pub static_policy: StaticPolicy,
}
```

### `ServeState` (`config.rs`)

Runtime state wrapping `ServeConfig` with a Tokio `Semaphore` for file-stream limiting. Created once at startup, shared across all requests.

```rust
pub struct ServeState {
    pub config: ServeConfig,
    file_stream_semaphore: Semaphore,
}
```

### `Limits` (`limits.rs`)

Resource limits with safe defaults:

| Field | Default | Purpose |
|-------|---------|---------|
| `max_connections` | 64 | Concurrent TCP connections |
| `max_file_streams` | 32 | Concurrent file streams (body transfer) |
| `max_request_body_bytes` | 0 | Request body size (rejected unconditionally) |
| `header_read_timeout` | 10s | Time to read full request headers |
| `response_write_timeout` | 60s | Time to write response body |
| `graceful_shutdown_timeout` | 10s | Drain period after SIGTERM |

### `handle_request()` (`service.rs`)

The HTTP request handler. Steps:

1. Match Hyper `Method` directly against `GET` and `HEAD` (non-read-only methods return 405)
2. Reject request body (metadata-only) via `Content-Length` and `Transfer-Encoding` checks
3. Parse request target → `ConfinedPath`
4. Resolve via the internal `RootGuard` → `ResolvedResource` (the public `SecureRoot` primitive is the embedding-consumer facade; the service uses `RootGuard` directly)
5. For files, call `primitives::planner::plan_file_response()` to evaluate conditional headers (`If-None-Match`, `If-Modified-Since`) and range requests (`Range`, `If-Range`), then convert the resolved file into a `BodySource` via `into_body(&plan)`, and translate the resulting plan into a Hyper response (200 / 206 / 304 / 416)
6. Stream the file body via `body_source_to_response()`, render a directory listing, or return the appropriate error

Returns `Response<BoxBody<Bytes, Infallible>>`.

### Error Taxonomy (`error.rs`)

```rust
pub enum Error {
    PathEscape,
    PathNotAccessible(String),
    Config(String),
    Bind(String),
    Runtime(String),
    RequestRejected(String),
    Io(std::io::Error),
}
```

## Server Module (`server/`)

**Experimental** — API is subject to change without notice.

The `server` module provides a reusable, transport-owning HTTP runtime for embedding. It owns the TCP accept loop, connection management, optional TLS, and HTTP/1 connection handling. Downstream projects provide a `Service` implementation; the runtime handles everything else.

### `Server` and `ServerBuilder`

```rust
let handle = Server::builder()
    .config(RuntimeConfig { bind: addr, ..Default::default() })
    .service(StaticService::new(policy, root))
    .start()
    .await?;
```

`Server::builder()` returns a `ServerBuilder`. Configure with `.config()` and `.service()`, then `.start()` to begin listening. Returns a `ServerHandle`.

`ServerBuilder` also supports `.bind()` for passing a pre-bound `TcpListener` or `UnixListener` directly, and `.from_listener()` for constructing from an existing listener with additional configuration.

### `RuntimeConfig`

Transport-level configuration separate from service-level concerns (`ServeConfig`):

| Field | Default | Purpose |
|-------|---------|---------|
| `bind` | `127.0.0.1:8000` | Listen address |
| `max_connections` | 64 | Concurrent TCP connections |
| `max_file_streams` | 32 | Concurrent file streams |
| `header_read_timeout` | 10s | Time to read request headers |
| `response_write_timeout` | 60s | Time to write response body |
| `handler_timeout` | None | Per-request handler timeout |
| `graceful_shutdown_timeout` | 10s | Drain period after shutdown signal |
| `keep_alive` | true | TCP keep-alive |
| `max_request_body_bytes` | 0 | Request body size ceiling (0 = reject) |
| `request_body_policy` | `Reject` | Global body policy (Reject/Buffer/Stream) |
| `body_read_timeout` | 30s | Total deadline for body consumption in Buffer mode |
| `incomplete_body_policy` | `Close` | Connection behavior when handler doesn't consume body |

### `Service` Trait

```rust
pub trait Service: Send + Sync + 'static {
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

`service_fn` creates a `Service` from an `Fn(Request) -> Future<Output = Result<Response, ServiceError>> + Send + Sync`.

### `StaticService`

Hardened static file service implementing `Service`:
- Descriptor-relative path confinement (Unix)
- Dotfile, symlink, and directory-listing policy enforcement
- GET/HEAD-only semantics
- Conditional and range request handling
- ETag and Last-Modified generation
- File-stream semaphore-gated concurrency

### Body ingestion

The `server::connection` module implements the body ingestion pipeline:
- Selects effective body policy from service preference and runtime ceiling
- Validates Content-Length against limits before body consumption
- Buffers or streams request bodies through public `RequestBody` primitives
- Enforces body read timeout
- Maps body errors to deterministic HTTP responses
- Handles incomplete body drain/close after service completion

### `ServerHandle`

Control handle returned by `Server::start()`:
- `local_addr()` — listening address
- `shutdown()` — trigger graceful shutdown
- `wait()` — wait for server to finish
- `wait_timeout()` — wait with timeout
- `ready()` — wait for server to be ready to accept connections
- `force_shutdown()` — immediately terminate without draining
- `state()` — query current `LifecycleState`

### Error Types

- `ServerError` — startup/lifecycle errors (Bind, Config, AlreadyStarted, Accept, ShutdownTimeout, Startup, Terminal)
- `ServiceError` — per-request errors (Internal, Rejected, Panic, Timeout)
- `ShutdownResult` — returned by shutdown operations, carries final `LifecycleState`

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
| `rustls` (optional, `client-tls`) | TLS for client HTTPS connections |
| `tokio-rustls` (optional, `client-tls`) | Async TLS stream wrapping for client |
| `webpki-roots` (optional, `client-tls`) | Mozilla CA root certificates for TLS verification |

## See Also

- [policy-system.md](policy-system.md) — Security policy types
- [path-confinement.md](path-confinement.md) — Path validation pipeline
- [filesystem-confinement.md](filesystem-confinement.md) — Filesystem traversal
- [primitives-api.md](primitives-api.md) — Public API boundary
- [response-planning.md](response-planning.md) — HTTP response planning
- [client.md](client.md) — HTTP client primitives
- [runtime.md](runtime.md) — Runtime service boundary (experimental)
- [api-stability.md](../docs/api-stability.md) — API classification by stability tier
- [release-contract.md](../docs/release-contract.md) — Product surface and compatibility commitments
