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

Executable demonstrations live under
[`crates/eggserve-core/examples/`](../crates/eggserve-core/examples/) and are
indexed with the CLI and Python examples in [`examples/README.md`](../examples/README.md):
`static_server` shows the built-in confined service, `custom_service` shows a
small public `service_fn`, and `primitives` performs response planning without
opening a socket. They are compiled by `scripts/verify.sh full`.

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
| `primitives/canonical.rs` | **pub** | `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response`, `normalize_metadata`, `to_hyper_response` — canonical response types and normalization |

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

Static state wrapping `ServeConfig` with one pinned root. It does not own
transport admission. A running `server::Server` creates `RuntimeState` once;
that runtime state owns the shared Tokio semaphore for all file-backed
responses, including custom-service responses.

```rust
pub struct ServeState {
    pub config: ServeConfig,
    pinned_root: PinnedRoot,
}
```

### `Limits` (`limits.rs`)

Resource limits with safe defaults:

| Field | Default | Purpose |
|-------|---------|---------|
| `max_connections` | 64 | Concurrent TCP connections |
| `max_file_streams` | 32 | Concurrent file streams (body transfer) |
| `max_request_body_bytes` | 0 | Runtime hard ceiling; services may opt into bodies only when greater than zero |
| `header_read_timeout` | 10s | Time to read full request headers |
| `connection_total_timeout` | 60s | Total connection lifetime timeout |
| `graceful_shutdown_timeout` | 10s | Drain period after SIGTERM |

## Server Module (`server/`)

**Experimental** — API is subject to change without notice.

The `server` module provides a reusable, transport-owning HTTP runtime for embedding. It owns the TCP accept loop, connection management, optional TLS, and HTTP/1 connection handling. Downstream projects provide a `Service` implementation; the runtime handles everything else.

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
Tokio `TcpListener`; the runtime is TCP-only.

### `RuntimeConfig`

Transport-level configuration separate from service-level concerns (`ServeConfig`):

| Field | Default | Purpose |
|-------|---------|---------|
| `bind` | `127.0.0.1:8000` | Listen address |
| `max_connections` | 64 | Concurrent TCP connections |
| `max_file_streams` | 32 | Concurrent file streams |
| `header_read_timeout` | 10s | Time to read request headers |
| `connection_total_timeout` | 60s | Timeout wrapping the entire Hyper connection future |
| `handler_timeout` | 30s | Per-request handler timeout |
| `graceful_shutdown_timeout` | 10s | Drain period after shutdown signal |
| `max_request_body_bytes` | 0 | Request body size ceiling (0 = reject) |
| `body_read_timeout` | 30s | Total deadline for body consumption in Buffer mode |

Note: `Limits::connection_total_timeout` is mapped to `RuntimeConfig::connection_total_timeout` by the `From<&ServeConfig>` impl.

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
