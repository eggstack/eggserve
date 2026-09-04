# Timeout Reference

This document defines every timeout and lifecycle deadline in the eggserve runtime, its semantics, and enforcement behavior.

## Timeout catalog

| # | Name | Config field | Default | Clock starts | Progress resets | What constitutes progress | Enforcement owner | Terminal behavior | Cleanup |
|---|------|-------------|---------|-------------|----------------|--------------------------|-------------------|-------------------|---------|
| 1 | Listener backoff | *(internal)* | 1–50ms exponential | On accept error | On each backoff step | Successful accept or different error kind | `classify_accept_error()` in accept loop | Error classified as fatal → break accept loop | Backoff state resets on success |
| 2 | TLS handshake timeout | `tls_handshake_timeout` | 10s | TCP accept (TLS path) | No | N/A | `accept_tls()` in accept loop | Handshake aborted, connection dropped | Stream/acceptor dropped |
| 3 | Request-header timeout | `header_read_timeout` | 10s | HTTP/1 connection created | No | Complete header block received | Hyper `http1::Builder::header_read_timeout` | 408 Request Timeout | Hyper closes connection |
| 4 | Request-body timeout | `body_read_timeout` | 30s | Body ingestion begins | No | Body fully consumed (all frames read) | `serve_connection_with_service()` | 408 Request Timeout response | Body dropped, connection kept alive |
| 5 | Handler timeout | `handler_timeout` | 30s | Service `call()` invoked | No | Service future completes | `tokio::time::timeout` in `serve_connection_with_service()` | 504 Gateway Timeout response | Service future dropped |
| 6 | Connection total timeout | `connection_total_timeout` | 60s | HTTP/1 connection created | No | N/A | Connection driver deadline loop | Graceful shutdown of Hyper connection | Connection dropped |
| 7 | Graceful shutdown timeout | `graceful_shutdown_timeout` | 10s | Shutdown requested | No | All connection tasks complete | `accept_loop()` drain loop | Abort remaining tasks, transition to Stopped | JoinSet aborted and joined |
| 8 | Keep-alive idle timeout | `keep_alive_idle_timeout` | 60s | Last request/transport activity | Yes — every request completion and socket read/write | Completed response, new request bytes, any socket progress | Connection driver deadline loop | Graceful shutdown of Hyper connection | Connection dropped |
| 9 | Response write no-progress timeout | `response_write_timeout` | 30s | Response handed to Hyper | Yes — every forward socket write | Socket bytes written | Connection driver + `ProgressIo` transport wrapper | Graceful shutdown of Hyper connection, producer cancelled | Connection dropped, permits released |

Lifecycle controls that are not timeouts but bound connection use: `max_requests_per_connection` (default unlimited; when reached, the current response completes with `Connection: close`) and `max_in_flight_requests` (default 64; exhaustion answers 503 before service invocation).

## Per-field semantics

Timeout values have no absolute upper ceiling. They must be positive and the
request/handler/body budgets must not exceed `connection_total_timeout`, but a
very large valid duration can effectively disable that deadline. Operators
should choose an explicit finite timeout appropriate for the deployment rather
than using an unbounded value.

### 1. Listener backoff

- **Clock starts**: Immediately after an accept error.
- **Progress resets**: Yes — on successful accept, `backoff_idx` resets to 0. On a different error kind, `error_repeat_count` resets to 1 and `backoff_idx` resets to 0 so a new kind starts its own ramp.
- **Progress definition**: Successful `listener.accept()` call.
- **Enforcement**: `classify_accept_error()` applies bounded exponential backoff: `[1, 2, 4, 8, 50]` ms. The backoff is interruptible by the shutdown broadcast channel.
- **Terminal behavior**: Fatal errors (persistent non-transient errors) break the accept loop, transitioning to Draining → Stopped.
- **Cleanup**: Backoff state (`backoff_idx`, `error_repeat_count`, `last_error_kind`) resets on success; `backoff_idx` also resets when the error kind changes.

### 2. TLS handshake timeout

- **Clock starts**: TCP connection accepted on TLS-enabled listener.
- **Progress resets**: No — this is a one-shot deadline.
- **Progress definition**: N/A (single operation).
- **Enforcement**: `tokio::time::timeout(tls_handshake_timeout, tls_acceptor.accept(stream))`.
- **Terminal behavior**: Handshake aborted, connection dropped. No event emitted beyond `TlsHandshakeTimeout`.
- **Cleanup**: TCP stream and TLS acceptor dropped.

### 3. Request-header timeout

- **Clock starts**: Hyper `http1::Builder` creates the connection.
- **Progress resets**: No — this is a one-shot deadline for the initial request line + headers.
- **Progress definition**: Complete header block received (double CRLF).
- **Enforcement**: Hyper's built-in `header_read_timeout` mechanism.
- **Terminal behavior**: Hyper returns an error; connection is closed.
- **Cleanup**: Hyper internally cleans up.
- **Keep-alive interaction**: Hyper also starts this timeout while a
  keep-alive connection sits idle between requests. When the header timeout
  is shorter than `keep_alive_idle_timeout`, an idle gap is closed by Hyper
  and reported as a header timeout — not as an idle expiry. Operators
  wanting healthy long-lived keep-alive connections must raise the header
  timeout alongside the idle timeout (see the reverse-proxy profile in
  `deployment.md`).

### 4. Request-body timeout

- **Clock starts**: Body ingestion begins (after headers parsed).
- **Progress resets**: No — this is a total deadline for body consumption, not an inactivity timeout.
- **Progress definition**: All body frames consumed (EOF received).
- **Enforcement**: `tokio::time::timeout(body_read_timeout, request_body.read_all())` for Buffer mode; combined `body_read_timeout.min(handler_timeout)` for Stream mode during `Service::call`. In Stream mode the collapsed deadline is distinguished at timeout time via the shared lifecycle state: if the body is still Active the timeout increments `body_read_timeouts` and emits `BodyReadTimeout`; otherwise it is surfaced as `ServiceTimeout`/`handler timed out`. This gives operators distinct counters for body stalls vs handler stalls even though the runtime collapses the deadline during the call for compatibility. After response-start with a deferred (Active) body, the remaining `body_read_timeout` continues via a per-request watchdog that marks the body Failed, cancels the lifecycle with `ConnectionTimeout`, increments both `body_read_timeouts` and `deferred_body_timeouts`, and closes the transport so pending polls wake via transport failure (Plan 174).
- **Terminal behavior**: Before response-start returns `408 Request Timeout` response with `Connection: close`. After response-start the transport closes without a second response (response already committed); Hyper also closes automatically on abandonment.
- **Cleanup**: Body dropped; connection closed (body errors are terminal for the connection).

### 5. Handler timeout

- **Clock starts**: `service.call(request)` invoked.
- **Progress resets**: No — this is a one-shot deadline for response-start (time until the service produces the `Response` object), not for downstream application work that continues after return.
- **Progress definition**: Service future completes (returns `Ok(Response)` or `Err(ServiceError)`).
- **Enforcement**: `tokio::time::timeout(min(body_read_timeout, handler_timeout), service.call(request))` for Stream during the call (compatibility-preserving collapse, disambiguated via lifecycle state); `handler_timeout` alone for Buffer/Reject. After response-start with a deferred body, `handler_timeout` no longer applies to the downstream task; body progress is bounded by the remaining `body_read_timeout` watchdog, response production by `response_write_timeout`, and the connection by `connection_total_timeout`.
- **Terminal behavior**: Returns `504 Gateway Timeout` response. The handler future is dropped.
- **Cleanup**: Service state dropped; connection kept alive for next request when the body is Complete, closed when Abandoned/Failed.

Streaming note: `handler_timeout` bounds only time-to-`Response`, not
the subsequent body stream. Once the service returns
`ResponseBody::Stream`, streaming progress is bounded by
`response_write_timeout` (no-progress) and `connection_total_timeout`
(hard lifetime), not by `handler_timeout`. Do not misuse
`handler_timeout` as a total lifetime for long-lived streams.

Deferred note (Plan 174): a service may return response-start while a
downstream task still owns an Active `RequestBody`. Connection reuse then
waits for both the request framing boundary (body Complete) and the
response boundary; abandonment forces safe close via Hyper (pinned by
`deferred_lifecycle` regression tests). EggServe `max_in_flight_requests`
bounds pre-response `Service::call` execution only; downstream
application-task admission is downstream-owned.

### 6. Connection total timeout

- **Clock starts**: HTTP/1 connection created (after TCP accept, optional TLS handshake).
- **Progress resets**: No — this is a total connection lifetime limit, not an inactivity timeout.
- **Progress definition**: N/A (timer never resets).
- **Enforcement**: The connection driver compares `Instant::now()` against `start + connection_total_timeout` on every wake; on expiry the Hyper connection is gracefully shut down (`conn.graceful_shutdown()`), then awaited. The post-shutdown drain is bounded by `min(graceful_shutdown_timeout, 5s)` so a stalled client cannot hold its admission permit indefinitely.
- **Terminal behavior**: Hyper connection is gracefully shut down, then awaited within the bounded drain.
- **Cleanup**: Connection dropped; permits released.

**Design note**: This was originally named `response_write_timeout` but was renamed to `connection_total_timeout` because it wrapped the entire Hyper connection future, not just response writes. The per-write no-progress control now exists separately as `response_write_timeout` (#9); the old name was reused for the new semantic, which is documented here rather than hidden.

**Precedence**: This is the hard ceiling for a connection. When it expires before the handler or body budget, the request dies mid-flight regardless of those wider budgets. Setting `handler_timeout` or `body_read_timeout` above an explicit `connection_total_timeout` via the `RuntimeConfig` builder is rejected as dead configuration; lowering only the total below the default budgets is accepted with this documented precedence. The Python facade caps forwarded handler/body budgets to the total and logs when adjustment occurs.

**Migration**: `connection_total_timeout` keeps its name, type (`Duration`), and hard-lifetime semantics — nothing is reinterpreted. What changes is that it is no longer the *only* way to bound idle or stalled clients: set `keep_alive_idle_timeout` for idle keep-alive turnover, `response_write_timeout` for stalled responses, and `max_requests_per_connection` for request-count bounds, and raise the total for deployments that want healthy long-lived keep-alive connections (see the per-profile defaults in `deployment.md`). The stdlib compatibility facade keeps the conservative 60-second default.

### 7. Graceful shutdown timeout

- **Clock starts**: `shutdown_rx` broadcast received (shutdown requested).
- **Progress resets**: No — this is a one-shot deadline for the drain phase.
- **Progress definition**: All connection tasks in the JoinSet complete.
- **Enforcement**: `tokio::time::timeout(graceful_shutdown_timeout, tasks.join_next())` in a loop.
- **Terminal behavior**: `tasks.abort_all()` → join all aborted tasks → `lifecycle.mark_stopped()` → `ShutdownResult::Timeout`.
- **Cleanup**: All permits released, JoinSet empty, lifecycle in `Stopped` state.

### 8. Keep-alive idle timeout

- **Clock starts**: Last request/transport activity (connection start, request completion, inbound bytes, socket writes).
- **Progress resets**: Yes — every completed response, every new request byte, and every socket write moves the deadline.
- **Progress definition**: Any request completion or transport activity.
- **Enforcement**: The connection driver sleeps until the next applicable deadline and recomputes on every activity state change. Applies only when no request is in flight and no response body is outstanding; a connection mid-request or mid-response is never idle-closed.
- **Terminal behavior**: Graceful shutdown of the Hyper connection (`KeepAliveIdleTimeout` event, `keepalive_idle_timeouts` counter, `IdleTimeout` outcome — a clean close).
- **Cleanup**: Connection dropped; permits released.
- **Interaction**: See the header-timeout note above: with default settings an idle gap is usually closed first by Hyper's 10-second header timeout and counted as a header timeout. Set the idle timeout *shorter* than the header timeout for distinct idle accounting, or raise both for long-lived keep-alive.

### 9. Response write no-progress timeout

- **Clock starts**: A response is handed to Hyper for transmission (all responses, including errors and rejections, arm the budget).
- **Progress resets**: Yes — every forward socket write (`AsyncWrite::poll_write` / `poll_write_vectored` with `n > 0`) moves the deadline. Steady progress, however slow, never triggers it: this is a no-progress timer, not a total duration.
- **Progress definition**: Socket bytes written, observed by the `ProgressIo` transport wrapper at the transport boundary (transparent to Hyper framing; identical for TCP, TLS, and caller-owned transports).
- **Enforcement**: The connection driver closes the connection when a response body is outstanding and no socket progress was made for the interval. Distinguishing "stalled response" from "idle keep-alive" is exact: every response body is wrapped so its end-of-stream, failure, or drop (disconnect/shutdown/cancellation) releases the outstanding slot. Idle connections (nothing outstanding) never trip this timer.
- **Terminal behavior**: Graceful shutdown of the Hyper connection (`WriteStallTimeout` event, `write_stall_timeouts` counter, `WriteTimeout` outcome), producer/file work cancelled via body drop. No secondary response is attempted after partial commitment; nothing is buffered to avoid the timeout.
- **Cleanup**: Connection dropped; file-stream, service, and connection permits released.

### Maximum requests per connection

Not a timeout, but the third per-connection lifecycle bound alongside the idle and write timers:

- **Semantics**: Counts every completed response on the connection — GET, HEAD, errors, and requests rejected before service invocation all count. After the configured count, the current response completes correctly with `Connection: close` (`MaxRequestsClose` event, `max_requests_closes` counter); framing is never corrupted.
- **Default**: Unlimited (`None`). The control exists for anonymity-sensitive and resource-constrained profiles; the reverse-proxy and direct-TLS profiles leave it unlimited and rely on the idle and write timers instead.
- **Configuration**: `max_requests_per_connection: Option<u64>` (`None` = unlimited); CLI `--max-requests-per-connection <N>` with `0` = unlimited.

## Known limitations

None open from the original production-lifecycle set. The former
progress-aware write-enforcement limitation is closed: `response_write_timeout`
is implemented via the `ProgressIo` transport wrapper plus response-body
completion tracking, as the Plan 164 design spike prescribed. The remaining
intentional non-goals (no per-IP/client rate limiting, no request routing or
middleware, no custom parser solely for a finer request-line knob) are
unchanged: Hyper still exposes no aggregate header-byte, request-target, or
request-line knob, so those ceilings are enforced post-parse in
`convert_request_head` (431/414 before service invocation) while
`max_buf_size`/`max_headers` are set explicitly on every Hyper builder.

## Interaction diagram

```text
TCP Accept
  │
  ├─ [TLS path] TLS handshake timeout (tls_handshake_timeout)
  │
  ▼
HTTP/1 Connection Created ──────────────────────────────────────────┐
  │                                                                  │
  ├─ Header-read timeout (header_read_timeout)                       │
  │   └─ 408 if headers incomplete (also bounds idle gaps when       │
  │      shorter than keep_alive_idle_timeout)                       │
  │                                                                  │
  ├─ Body-read timeout (body_read_timeout)                           │
  │   └─ 408 if body incomplete                                      │
  │                                                                  │
  ├─ Handler timeout (handler_timeout)                               │
  │   └─ 504 if handler slow                                         │
  │                                                                  │
  ├─ Service admission (max_in_flight_requests)                      │
  │   └─ 503 before service invocation, no queue                     │
  │                                                                  │
  ├─ Keep-alive idle timeout (keep_alive_idle_timeout)               │
  │   └─ graceful close after inactivity (resets on activity)        │
  │                                                                  │
  ├─ Response write no-progress timeout (response_write_timeout)     │
  │   └─ close after no socket progress with body outstanding        │
  │                                                                  │
  ├─ Max requests per connection (max_requests_per_connection)       │
  │   └─ current response completes with Connection: close           │
  │                                                                  │
  ├─ Connection total timeout (connection_total_timeout) ────────────┘
  │   └─ Hard ceiling: graceful shutdown of connection
  │
  ▼
Connection closed
  │
  └─ Permit released (connection + file-stream + in-flight service)
```
