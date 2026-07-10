# eggserve-core — Deep Dive

The core library crate. Contains all security-critical logic: path confinement, policy enforcement, filesystem traversal, HTTP request handling, response construction, MIME detection, and the public primitives API.

## Module Map

| Module | Visibility | Purpose |
|--------|------------|---------|
| `lib.rs` | pub | Declares all modules; documents the 3-tier stability model |
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` |
| `policy.rs` | **pub** | `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy` |
| `limits.rs` | **pub** | `Limits` — connection count, file streams, header/target/body sizes, timeouts |
| `service.rs` | **pub** (experimental) | `handle_request()` — the HTTP handler |
| `error.rs` | pub(crate) | `Error` enum taxonomy |
| `path/` | pub(crate) | Path confinement pipeline |
| `fs/` | pub(crate) | Filesystem confinement |
| `response.rs` | pub(crate) | Response helpers (file streaming, directory listing HTML, error responses) |
| `mime.rs` | pub(crate) | MIME type detection via `phf` map |
| `primitives/` | **pub** | Public facade for embedding consumers |
| `primitives/body.rs` | **pub** | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming abstraction |
| `primitives/client/` | **pub** (feature-gated: `client`) | HTTP client primitives: `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse` |

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
