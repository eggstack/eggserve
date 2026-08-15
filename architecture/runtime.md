# Runtime Architecture

Custom startup is filesystem-agnostic, static planner metadata is attached to
canonical responses, and the only production file-stream admission pool is
created by `RuntimeState` once per running server. Fully consumed streamed
request bodies remain keep-alive eligible; an incompletely consumed body causes
the response and connection to close.

## Overview

> **Status: Experimental.** The `server` module API is subject to change without notice.

The `server` module provides a reusable, transport-owning HTTP runtime that downstream Rust projects can embed without importing internal modules or depending directly on Hyper. It includes a lifecycle state machine (Created → Starting → Running → Draining → Stopped/Failed), readiness signaling, graceful and forced shutdown with configurable drain deadlines, and connection/task tracking.

The runnable public-API demonstrations are [`static_server.rs`](../crates/eggserve-core/examples/static_server.rs)
and [`custom_service.rs`](../crates/eggserve-core/examples/custom_service.rs).
Both wait for `ready()`, use loopback defaults, and call synchronous
`shutdown()` followed by consuming `wait()` after Ctrl+C. The examples are
intentionally not routers or application frameworks.

## Components

### Server

The main entry point. Created via `Server::builder()`, configured with a `RuntimeConfig` and a service, then started with `.start()`. The `start()` call transitions the server from Created → Starting → Running through the lifecycle state machine. Double-start is prevented by atomic state guards and returns `ServerError::AlreadyStarted`.

### ServerBuilder

Configures and constructs a `Server` via a fluent builder API:

- `runtime(config)` — set the `RuntimeConfig`
- `serve_config(config)` — compatibility convenience for `start()`; it is
  consumed into one `StaticService` during `build()` and is ignored by
  `start_with_service()` custom-service startup
- `bind(addr)` — override the bind address; the server will bind to this address on `start()`
- `from_listener(listener)` — use a pre-bound `TcpListener` instead of binding on start; ownership transfers to the runtime after `start()`, and nonblocking mode is normalized automatically
- `build()` — validate configuration and construct the built-in `StaticService`
  once when `serve_config()` was supplied; invalid static roots fail here
- `static_service(root)` — convenience: create a `StaticService` rooted at the given path

### RuntimeConfig

Transport-level configuration separate from service-level concerns:
- Bind address
- Connection limits
- File-stream limits
- Timeouts (header read, connection total, handler, body read, graceful shutdown)
- Server header
- TLS configuration (feature-gated)
- Maximum request body size (hard ceiling)

Safe defaults match or strengthen CLI defaults. Configuration is validated at builder time.

### Service Trait

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

### ServerHandle

Control handle returned by `Server::start()`. Not `Clone` — there is exactly one handle per server instance.

- `local_addr()` — bound address (useful for port-zero discovery)
- `state()` — current `LifecycleState`
- `ready().await` — wait for Running state; returns error if server failed during startup
- `shutdown()` — trigger graceful shutdown (idempotent; multiple calls are safe)
- `force_shutdown(deadline).await` — graceful shutdown followed by deadline; if the server doesn't stop within `deadline`, remaining tasks are abandoned and `ShutdownResult::Forced` is returned
- `wait().await` — consume handle, trigger graceful shutdown if still running, wait for completion
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

## Lifecycle State Machine

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

Allowed operations per state:

| State     | build | start | ready | shutdown | force_shutdown | wait |
|-----------|-------|-------|-------|----------|----------------|------|
| Created   | yes   | yes   | —     | noop     | noop           | err  |
| Starting  | —     | err   | yes   | pending  | pending        | err  |
| Running   | —     | err   | ok    | ok       | ok             | yes  |
| Draining  | —     | err   | err   | idempot  | ok             | yes  |
| Stopped   | —     | err   | err   | noop     | noop           | ok   |
| Failed    | —     | err   | err   | noop     | noop           | err  |

Race safety: state is stored in an `AtomicU8` with `compare_exchange` for all transitions. Channel notifications (`watch` for readiness, `broadcast` for terminal state) ensure waiters are awakened without polling.

## Listener Error Classification

Listener errors are classified by `io::ErrorKind` into transient, resource-exhaustion, and persistent categories. Transient errors use bounded exponential backoff (1ms to 50ms cap). All errors emit structured log events via `classify_accept_error()`.

## Connection/Task Tracking

- Each accepted connection spawns a tokio task, tracked in a `JoinSet` with bounded concurrency
- Graceful drain waits for each task up to the configured deadline; remaining tasks are dropped (aborted)
- Forced shutdown abandons remaining tasks immediately
- RAII permits ensure connection and file-stream permits are released on drop, even under cancellation. Canonical file-backed responses acquire the shared file-stream permit at transport conversion, so custom Rust services and the Python static façade share the same ceiling.
- Normal peer resets do not terminate the server; only fatal runtime errors transition to Failed
- Python callback failures are converted to generic service errors with fixed diagnostic categories; handler exception text and response data are not logged.

### Runtime ownership corrective contract

Each running server creates exactly one `RuntimeState`, including one
`max_file_streams` semaphore. The accept loop clones that state into every
connection; `StaticService` owns only its pinned root, policy, and listing
limits. All canonical file and range responses acquire the same permit at the
single Hyper conversion boundary. Custom Rust and Python services have no
implicit root or static state.

The runtime asks the service for the body policy for the actual request. GET,
HEAD, DELETE, OPTIONS, and extension methods are not globally body-forbidden;
TRACE content remains rejected. An unconsumed streamed body marks the response
`Connection: close`, drops the body, and prevents connection reuse.

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

1. TCP accept with connection permit
2. Optional TLS handshake (feature-gated)
3. HTTP/1 connection setup via Hyper
4. Request conversion to canonical types
5. Body ingestion (policy selection, Content-Length preflight, transfer decoding)
6. Service invocation with timeout
7. Canonical response normalization
8. Transport-body conversion
9. Permit release and connection termination

## Body ingestion pipeline

The runtime handles request body ingestion transparently for services:

1. **Policy selection**: The runtime queries `Service::request_body_policy()` and enforces the global ceiling (`max_request_body_bytes`). The effective policy is the minimum of service preference and runtime ceiling.

2. **Framing validation**: The runtime rejects requests containing both Transfer-Encoding and Content-Length before body construction. Duplicate Content-Length headers with conflicting values are also rejected at the HTTP/1 wire level. Identical duplicate Content-Length values are normalized by Hyper.

3. **Content-Length preflight**: Before reading the body, the runtime validates `Content-Length` against the effective limit. Conflicting or oversized declarations are rejected with 413.

4. **Body consumption**: For `Buffer` policy, the entire body is read under `body_read_timeout` and delivered as an in-memory `RequestBody`. For `Stream` policy, the body is passed through with byte accounting. For `Reject` policy, the body is discarded and the service receives an empty body.

5. **Error mapping**: Body errors map to deterministic HTTP responses:
   - 400: malformed framing, length mismatch
   - 408: body read timeout
   - 413: body too large
   - 500: transport error

6. **Incomplete body handling**: When a service returns without fully consuming a Stream body, the connection closes. Active drain is not safely implementable because the body stream is consumed into the `Request` envelope by value.

## Request body handling

The runtime manages request body lifecycle through the `Request` envelope:

### Body policy

- `Service::request_body_policy(&RequestHead)` — service-declared per-request policy (method-aware)
- `RuntimeConfig::max_request_body_bytes` — hard ceiling no service can exceed
- Incomplete body handling: always close (hardcoded, not configurable)

### Request envelope

```rust
pub struct Request {
    head: RequestHead,      // immutable request metadata
    body: RequestBody,       // one-shot, bounded body stream
    connection: ConnectionInfo, // transport metadata
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

### One-shot consumption

- `RequestBody::read_all(self)` — buffer entire body
- `RequestBody::next_chunk(&mut self)` — stream chunks
- `Stream` trait implementation for async iteration
- State machine: Unread → Streaming → Complete | Error

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
| `body_read_timeout_secs` | `body_read_timeout` | 30s |

The runtime enforces `max_request_body_bytes` as a hard ceiling. Service-specific limits may only lower it. Body policy is service-declared, not a runtime field.

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

## Security Properties

- Response normalization (hop-by-hop stripping, content-length computation) is runtime-owned
- Services cannot bypass final framing policy through the safe API
- Handler failures map to deterministic responses without internal leakage
- Filesystem policy belongs to the service, not the runtime
