# Runtime Architecture

## Overview

The `server` module provides a reusable, transport-owning HTTP runtime that downstream Rust projects can embed without importing internal modules or depending directly on Hyper. It includes a lifecycle state machine (Created → Starting → Running → Draining → Stopped/Failed), readiness signaling, graceful and forced shutdown with configurable drain deadlines, and connection/task tracking.

## Components

### Server

The main entry point. Created via `Server::builder()`, configured with a `RuntimeConfig` and a service, then started with `.start()`. The `start()` call transitions the server from Created → Starting → Running through the lifecycle state machine. Double-start is prevented by atomic state guards and returns `ServerError::AlreadyStarted`.

### ServerBuilder

Configures and constructs a `Server` via a fluent builder API:

- `runtime(config)` — set the `RuntimeConfig`
- `serve_config(config)` — set a pre-built `ServeConfig` (bridges CLI/Python config)
- `bind(addr)` — override the bind address; the server will bind to this address on `start()`
- `from_listener(listener)` — use a pre-bound `TcpListener` instead of binding on start; ownership transfers to the runtime after `start()`, and nonblocking mode is normalized automatically
- `build()` — build with the built-in `StaticService`
- `build_with_service(service)` — build with a custom `Service` implementation
- `static_service(root)` — convenience: create a `StaticService` rooted at the given path

### RuntimeConfig

Transport-level configuration separate from service-level concerns:
- Bind address
- Connection limits
- File-stream limits
- Timeouts (header read, response write, handler, graceful shutdown)
- Keep-alive policy

Safe defaults match or strengthen CLI defaults. Configuration is validated at builder time.

### Service Trait

```rust
pub trait Service: Send + Sync + 'static {
    fn call(
        &self,
        request: RequestHead,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>>;
}
```

- Receives canonical `RequestHead` (no Hyper types)
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
- File-stream semaphore-gated concurrency

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

## Connection/Task Tracking

- Each accepted connection spawns a tokio task, tracked in a bounded `Vec<JoinHandle<()>>`
- Graceful drain waits for each task up to the configured deadline; remaining tasks are dropped (aborted)
- Forced shutdown abandons remaining tasks immediately
- RAII permits ensure connection and file-stream permits are released on drop, even under cancellation
- Normal peer resets do not terminate the server; only fatal runtime errors transition to Failed

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
5. Service invocation with timeout
6. Canonical response normalization
7. Transport-body conversion
8. Write timeout enforcement
9. Permit release and connection termination

## Python lifecycle mapping

The Python `Server` wraps the same tokio runtime and accept loop pattern as
the Rust `Server`. Lifecycle methods are bridged:

- `start()` → creates tokio runtime, binds TcpListener, spawns accept loop on `std::thread`
- `stop()` → sends shutdown signal, joins thread
- `shutdown()` → sends shutdown signal (non-blocking)
- `force_shutdown(deadline)` → sends shutdown, waits with deadline
- `wait()` → blocks on thread join
- `state` → tracks Created → Running → Stopped transitions

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
