# Runtime Architecture

## Overview

The `server` module provides a reusable, transport-owning HTTP runtime that downstream Rust projects can embed without importing internal modules or depending directly on Hyper.

## Components

### Server

The main entry point. Created via `Server::builder()`, configured with a `RuntimeConfig` and a service, then started with `.start()`.

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

Control handle returned by `Server::start()`:
- `local_addr()` - listening address
- `shutdown()` - trigger graceful shutdown
- `wait()` - wait for server to finish
- `wait_timeout()` - wait with timeout

### Error Types

- `ServerError` - startup/lifecycle errors (Bind, Config, AlreadyStarted, Accept, ShutdownTimeout)
- `ServiceError` - per-request errors (Internal, Rejected, Panic, Timeout)

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

## Security Properties

- Response normalization (hop-by-hop stripping, content-length computation) is runtime-owned
- Services cannot bypass final framing policy through the safe API
- Handler failures map to deterministic responses without internal leakage
- Filesystem policy belongs to the service, not the runtime
