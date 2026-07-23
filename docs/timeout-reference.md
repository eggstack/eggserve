# Timeout Reference

This document defines every timeout and deadline in the eggserve runtime, its semantics, and enforcement behavior.

## Timeout catalog

| # | Name | Config field | Default | Clock starts | Progress resets | What constitutes progress | Enforcement owner | Terminal behavior | Cleanup |
|---|------|-------------|---------|-------------|----------------|--------------------------|-------------------|-------------------|---------|
| 1 | Listener backoff | *(internal)* | 1–50ms exponential | On accept error | On each backoff step | Successful accept or different error kind | `classify_accept_error()` in accept loop | Error classified as fatal → break accept loop | Backoff state resets on success |
| 2 | TLS handshake timeout | `header_read_timeout` | 10s | TCP accept (TLS path) | No | N/A | `accept_tls()` in accept loop | Handshake aborted, connection dropped | Stream/acceptor dropped |
| 3 | Request-header timeout | `header_read_timeout` | 10s | HTTP/1 connection created | No | Complete header block received | Hyper `http1::Builder::header_read_timeout` | 408 Request Timeout | Hyper closes connection |
| 4 | Request-body timeout | `body_read_timeout` | 30s | Body ingestion begins | No | Body fully consumed (all frames read) | `serve_connection_with_service()` | 408 Request Timeout response | Body dropped, connection kept alive |
| 5 | Handler timeout | `handler_timeout` | 30s | Service `call()` invoked | No | Service future completes | `tokio::time::timeout` in `serve_connection_with_service()` | 504 Gateway Timeout response | Service future dropped |
| 6 | Connection total timeout | `connection_total_timeout` | 60s | HTTP/1 connection created | No | N/A | `tokio::time::timeout` in `serve_connection()` | Graceful shutdown of Hyper connection | Connection dropped |
| 7 | Graceful shutdown timeout | `graceful_shutdown_timeout` | 10s | Shutdown requested | No | All connection tasks complete | `accept_loop()` drain loop | Abort remaining tasks, transition to Stopped | JoinSet aborted and joined |

## Per-field semantics

### 1. Listener backoff

- **Clock starts**: Immediately after an accept error.
- **Progress resets**: Yes — on successful accept, `backoff_idx` resets to 0. On a different error kind, `error_repeat_count` resets to 1.
- **Progress definition**: Successful `listener.accept()` call.
- **Enforcement**: `classify_accept_error()` applies bounded exponential backoff: `[1, 2, 4, 8, 50]` ms. The backoff is interruptible by the shutdown broadcast channel.
- **Terminal behavior**: Fatal errors (persistent non-transient errors) break the accept loop, transitioning to Draining → Stopped.
- **Cleanup**: Backoff state (`backoff_idx`, `error_repeat_count`, `last_error_kind`) resets on success.

### 2. TLS handshake timeout

- **Clock starts**: TCP connection accepted on TLS-enabled listener.
- **Progress resets**: No — this is a one-shot deadline.
- **Progress definition**: N/A (single operation).
- **Enforcement**: `tokio::time::timeout(header_read_timeout, tls_acceptor.accept(stream))`.
- **Terminal behavior**: Handshake aborted, connection dropped. No event emitted beyond `TlsHandshakeTimeout`.
- **Cleanup**: TCP stream and TLS acceptor dropped.

### 3. Request-header timeout

- **Clock starts**: Hyper `http1::Builder` creates the connection.
- **Progress resets**: No — this is a one-shot deadline for the initial request line + headers.
- **Progress definition**: Complete header block received (double CRLF).
- **Enforcement**: Hyper's built-in `header_read_timeout` mechanism.
- **Terminal behavior**: Hyper returns an error; connection is closed.
- **Cleanup**: Hyper internally cleans up.

### 4. Request-body timeout

- **Clock starts**: Body ingestion begins (after headers parsed).
- **Progress resets**: No — this is a total deadline for body consumption, not an inactivity timeout.
- **Progress definition**: All body frames consumed (EOF received).
- **Enforcement**: `tokio::time::timeout(body_read_timeout, request_body.read_all())` for Buffer mode; combined `body_read_timeout.min(handler_timeout)` for Stream mode.
- **Terminal behavior**: Returns `408 Request Timeout` response with `Connection: close`.
- **Cleanup**: Body dropped; connection closed (body errors are terminal for the connection).

### 5. Handler timeout

- **Clock starts**: `service.call(request)` invoked.
- **Progress resets**: No — this is a one-shot deadline for the entire handler invocation.
- **Progress definition**: Service future completes (returns `Ok(Response)` or `Err(ServiceError)`).
- **Enforcement**: `tokio::time::timeout(handler_timeout, service.call(request))`.
- **Terminal behavior**: Returns `504 Gateway Timeout` response. The handler future is dropped.
- **Cleanup**: Service state dropped; connection kept alive for next request (keep-alive).

### 6. Connection total timeout

- **Clock starts**: HTTP/1 connection created (after TCP accept, optional TLS handshake).
- **Progress resets**: No — this is a total connection lifetime limit, not an inactivity timeout.
- **Progress definition**: N/A (timer never resets).
- **Enforcement**: `tokio::time::timeout(connection_total_timeout, &mut conn)` wrapping the entire Hyper connection future.
- **Terminal behavior**: Hyper connection is gracefully shut down (`conn.graceful_shutdown()`), then awaited.
- **Cleanup**: Connection dropped; permits released.

**Design note**: This was originally named `response_write_timeout` but was renamed to `connection_total_timeout` because it wraps the entire Hyper connection future, not just response writes. Hyper does not expose a reliable per-write hook at the current abstraction level, so progress-aware write enforcement cannot be safely implemented without a transport wrapper. See "Known limitations" below.

### 7. Graceful shutdown timeout

- **Clock starts**: `shutdown_rx` broadcast received (shutdown requested).
- **Progress resets**: No — this is a one-shot deadline for the drain phase.
- **Progress definition**: All connection tasks in the JoinSet complete.
- **Enforcement**: `tokio::time::timeout(graceful_shutdown_timeout, tasks.join_next())` in a loop.
- **Terminal behavior**: `tasks.abort_all()` → join all aborted tasks → `lifecycle.mark_stopped()` → `ShutdownResult::Timeout`.
- **Cleanup**: All permits released, JoinSet empty, lifecycle in `Stopped` state.

## Known limitations

### Progress-aware response write enforcement

**Status**: Not implemented. Documented as a known limitation.

The original `response_write_timeout` was intended to enforce a per-write inactivity deadline — closing a stalled writer while allowing steadily progressing responses. However, Hyper does not expose a reliable per-write hook at the current abstraction level. The `serve_connection` future bundles the entire connection lifecycle (multiple requests, keep-alive, response writing) into a single future, and there is no way to observe individual write completions from outside Hyper's internals.

**Current behavior**: `connection_total_timeout` acts as a total connection lifetime limit. A response that steadily streams longer than the configured duration will be terminated when the total connection lifetime expires. This is documented and accurate — the field name matches the behavior.

**Future work**: A transport-level wrapper (e.g., wrapping `TokioIo` with a progress-tracking layer) could provide per-write progress callbacks. This would allow an inactivity-based timeout that resets on successful write completion. The wrapper would need to:
- Intercept `AsyncWrite::poll_write` calls
- Reset an inactivity timer on each successful write
- Close the connection when the inactivity timer expires
- Be transparent to Hyper's HTTP/1 framing

This is out of scope for the current plan and would require a new plan to implement.

## Interaction diagram

```text
TCP Accept
  │
  ├─ [TLS path] TLS handshake timeout (header_read_timeout)
  │
  ▼
HTTP/1 Connection Created ──────────────────────────────────────────┐
  │                                                                  │
  ├─ Header-read timeout (header_read_timeout)                       │
  │   └─ 408 if headers incomplete                                   │
  │                                                                  │
  ├─ Body-read timeout (body_read_timeout)                           │
  │   └─ 408 if body incomplete                                      │
  │                                                                  │
  ├─ Handler timeout (handler_timeout)                               │
  │   └─ 504 if handler slow                                         │
  │                                                                  │
  ├─ Connection total timeout (connection_total_timeout) ────────────┘
  │   └─ Graceful shutdown of connection
  │
  ▼
Connection closed
  │
  └─ Permit released (connection + file-stream)
```
