# Structured Logging and Operational Events

## Overview

eggserve uses structured JSON Lines logging for machine-consumable operational events, with a text mode fallback for human readability. The system is defined in `eggserve-core::ops`.

## Module layout (Plan 206 Track H)

`eggserve-core::ops/` — the public observability module:

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `ops/mod.rs` | `OpsContext` authority (sink + counters + correlation IDs), context-local sink-failure accounting |
| `events.rs` | `ops/events.rs` | `Severity`, `EventKind`, `Field`, `Event`, sanitization, JSON rendering |
| `sinks.rs` | `ops/sinks.rs` | `LogSink` implementations (`NopLogSink`, `FilteredLogSink`, `CompositeLogSink`) |
| `counters.rs` | `ops/counters.rs` | `OpsCounters`, `Snapshot` |

Public import paths (`ops::OpsContext`, `ops::Event`, `ops::Severity`, etc.) resolve unchanged through the `mod.rs` facade.

## Ownership: per-runtime contexts with a process-global default

Live server/connection execution resolves observability through an explicit
`OpsContext` carried by runtime ownership, not through process globals:

- `OpsContext` bundles the active `LogSink`, the `OpsCounters`, and the
  connection correlation-ID source. Clones share one inner allocation, so
  handing the context to connection tasks is cheap.
- `RuntimeState` owns the context (`RuntimeState::with_ops` for explicit
  embedders; `new`/`try_new` clone the process-global default for
  CLI/compatibility construction). The accept loop, caller-owned driver,
  connection pipeline, deferred-body supervision, and lifecycle cancellation
  all resolve events, counters, and correlation IDs through it.
- `ServerBuilder::ops_context(..)` attaches a context to a TCP/TLS server;
  the built-in `StaticService` is wired to the same context. `ServerHandle`
  retains it for inspection.
- Connection IDs start at 1 per context and are coherent within the owning
  runtime; explicit caller-supplied IDs (`serve_http1_connection_with_id`)
  still take precedence.
- `RuntimeState::ops_snapshot()` / `ServerHandle::ops_snapshot()` /
  `OpsContext::snapshot()` return bounded, non-blocking `OpsSnapshot` reads
  (never reset-on-read). No exporter, endpoint, or monitoring server exists.
- Failure accounting is context-local: `OpsContext::emit` contains a
  panicking sink and increments that context's `dropped_log_events`; a
  `CompositeLogSink` built with `with_failure_counters` (or via
  `OpsContext::with_sinks`) counts contained child failures in the owning
  context. A composite built with plain `new()` keeps the historical
  process-global accounting for compatibility.
- Intentionally process-global (documented, not accidental): the
  `Logger::global()` / `global_counters()` compatibility shims (CLI startup
  and frontend initialization), and standalone canonical conversions
  (`primitives::to_hyper_response`) which have no runtime owner. The CLI
  adopts its stderr sink into the global default at `Logger::try_init`, so
  default-constructed runtimes keep emitting to the CLI sink.

Canonical request/response types never name observability types; the context
travels with `RuntimeState` and connection activity.

## Event Model

Every operational event has:
- `schema_version` (u32) — currently 1
- `severity` — DEBUG, INFO, WARN, ERROR
- `event` — stable event kind name (ProcessStarting, RequestCompleted, etc.)
- `timestamp` — RFC 3339 format
- `message` — human-readable description
- `connection_id` (optional) — connection identifier, unique within the owning runtime context (sequences start at 1 per context)
- `request_seq` (optional) — request sequence number within connection
- `fields` — array of single-key objects (serialized as `[{"key": value}, ...]`)

## Event Categories

### Process/Config
- `process_starting` — server starting with version, bind, root, policy flags
- `root_initialized` — root directory opened and pinned
- `listener_ready` — accept loop bound and polling
- `shutdown_requested` — graceful shutdown initiated
- `draining_started` — draining in-flight connections
- `forced_shutdown_started` — drain deadline exceeded, aborting connections
- `shutdown_complete` — server stopped with result (clean/timeout/error)

### Connection
- `connection_accepted` — new TCP connection accepted with correlation ID
- `connection_rejected` — connection admission limit reached
- `tls_handshake_success/failure/timeout` — TLS events (feature-gated)
- `protocol_negotiated` — selected wire protocol (`http/1.1` or experimental
  `h2`); emitted once per connection without client pseudo-header values
- `header_timeout` — HTTP header read timeout (also idle keep-alive gaps when shorter than the idle timeout)
- `body_read_timeout` — request body read timeout (buffer mode)
- `parser_rejection` — HTTP framing rejection (incl. Hyper parser-limit parse failures)
- `header_bytes_rejected` — aggregate request-header bytes exceeded (431, pre-service)
- `request_target_too_long` — request target exceeded (414, pre-service)
- `service_admission_rejected` — in-flight service budget exhausted (503)
- `keep_alive_closed` — keep-alive connection closed
- `keep_alive_idle_timeout` — idle keep-alive connection closed after inactivity
- `max_requests_close` — request limit reached; H1 closes the connection and
  H2 begins a bounded GOAWAY/drain
- `write_stall_timeout` — response outstanding with no protocol-relevant
  progress; H1 observes forward socket-write progress, while H2 observes
  per-response application-body poll progress only and conservatively closes
  the connection because Hyper exposes no safe public stream reset or
  wire-progress hook
- `connection_total_timeout` — total connection lifetime timeout
- `client_disconnect` — client disconnected (Debug severity)
- `connection_panic` — handler panic contained

### Request/Service
- `request_completed` — request finished with status, bytes, duration
- `file_not_found` — path resolved but file not found (sanitized path field)
- `file_denied` — access denied (dotfile/symlink/policy)
- `file_error` — file stream I/O error
- `dotfile_denied` — dotfile access denied
- `symlink_denied` — symlink access denied
- `root_escape_denied` — path escapes root (Warn severity)
- `body_policy_rejection` — request body rejected by policy
- `service_timeout` — handler timed out (504 response)
- `service_error` — handler returned error
- `directory_listing_limit` — listing entry limit reached
- `incomplete_body_close` — request body connection closed before completion (Abandoned/Failed at return)
- `deferred_body_delegated` — Stream body Active past service return (Debug)
- `deferred_body_completed` — deferred body reached Complete after response-start (Debug)
- `deferred_body_abandoned` — deferred body Abandoned/Failed after response-start (Debug, connection closes via Hyper)
- `deferred_body_timeout` — remaining body_read_timeout fired after response-start (Warn, + legacy `body_read_timeout`)
- `request_lifecycle_peer_disconnect` — lifecycle cancelled with PeerDisconnected (Debug)
- `request_lifecycle_runtime_cancel` — lifecycle cancelled with ServerShutdown/ConnectionTimeout/TransportFailure (Debug)
- `service_invocation_suppressed` — service invocation suppressed (e.g. duplicate or concurrent)

### Operational
- `listener_transient_error` — retryable accept error with backoff
- `listener_persistent_error` — fatal accept error, no backoff
- `resource_exhaustion` — file descriptor or memory exhaustion
- `blocking_worker_saturation` — blocking pool at capacity
- `log_sink_failure` — logging backend failed (retained kind; the composite reports via `dropped_log_events`, not a synthetic event, to avoid re-entering the failing graph)

## Output Modes

- **JSON Lines** (`--log-format json`): One valid JSON object per line on stderr
- **Text** (`--log-format text`): `[severity] event_name: message` on stderr
- **None** (`--log-format none`): Only fatal startup diagnostics

## Privacy

- Request paths are sanitized/truncated (last component only, max 128 chars)
- Control characters, bidi controls, and escape sequences are stripped
- Query strings are omitted by default
- Sensitive headers (Authorization, Cookie) are never logged
- Absolute filesystem paths are startup-only diagnostics

## Python Server Logging

The Python `Server` uses the process-global default (stderr when the CLI
adopts it, no-op otherwise). The Python `Server` does not accept observer
callbacks and does not expose per-server sink selection; Rust embedders that
need isolated sinks use `OpsContext` with `ServerBuilder::ops_context` /
`RuntimeState::with_ops`.

## Operational Counters

`OpsCounters` tracks (per runtime context; `global_counters()` is the
process-global default's set, and `RuntimeState::ops_snapshot()` /
`ServerHandle::ops_snapshot()` read a single runtime's set):
- `connections_accepted` — TCP connections accepted
- `connections_rejected` — connections rejected by admission limit
- `active_connections` — currently active connections
- `active_file_streams` — currently streaming file responses
- `active_service_requests` — requests currently in the service pipeline
- `connection_panics` — handler panics contained
- `body_rejections` — request body rejections by policy
- `parser_rejects` — HTTP parsing failures (incl. Hyper parser-limit errors)
- `header_bytes_rejected` — aggregate header-byte rejections (431)
- `request_target_rejected` — request-target rejections (414)
- `service_admission_rejected` — in-flight service budget refusals (503)
- `header_timeouts` — header read timeouts
- `body_read_timeouts` — request body read timeouts
- `keepalive_idle_timeouts` — idle keep-alive closes
- `max_requests_closes` — connections closed after the request limit
- `write_stall_timeouts` — write no-progress closes
- `connection_total_timeouts` — total connection lifetime timeouts
- `graceful_shutdowns` — clean shutdowns without timeout
- `forced_shutdowns` — shutdowns where drain deadline was exceeded
- `listener_errors` — accept loop errors (all classifications)
- `dropped_log_events` — events dropped due to sink failures

## Listener Error Classification

Accept errors are classified by `io::ErrorKind`:
- **Transient** (Interrupted, ConnectionRefused, etc.) → Debug severity, bounded backoff
- **Resource exhaustion** (EMFILE/ENFILE) → Error severity, rate-limited retry
- **Persistent** (unknown errors) → Error severity, no backoff

Backoff uses bounded exponential: 1ms → 2ms → 4ms → 8ms → 50ms cap.
Backoff is interruptible by shutdown via `tokio::select!`.

## Log Sink Failure Behavior

- `CompositeLogSink` catches panics from individual sinks via `catch_unwind`
  and never lets a sink panic escape into request/connection execution
- Failed sink emissions increment `dropped_log_events` deterministically
  (one per dropped emission) in the owning context — the context whose
  composite was built with `with_failure_counters` / `with_sinks`, or the
  process-global counters for plain `new()` composites; iteration continues
  so healthy siblings still receive the original event
- No synthetic `LogSinkFailure` event is emitted from the failure path: when
  the composite is installed globally that would recursively re-enter the
  same failing sink graph (Plan 178). The counter is the failure signal;
  `flush()` panics are likewise contained without propagation.
  `OpsContext::emit` applies the same non-recursive containment to direct
  (non-composite) sinks, counted in the emitting context.
- `Logger::try_init()` returns `Err(())` if already initialized (Python coexistence)
- `NopLogSink` is the default when no logger is configured

## Example Events

```json
{"schema_version":1,"severity":"INFO","event":"process_starting","timestamp":"2026-07-22T10:00:00Z","message":"eggserve 0.1.0 starting","fields":[{"version":"0.1.0"},{"bind":"127.0.0.1:8000"},{"root":"./public"},{"symlinks":"denied"},{"dotfiles":"denied"}]}
```

```json
{"schema_version":1,"severity":"INFO","event":"request_completed","timestamp":"2026-07-22T10:00:01Z","message":"GET /style.css 200","connection_id":42,"request_seq":1,"fields":[{"method":"GET"},{"path":"/style.css"},{"status":200},{"bytes":1024},{"duration_ms":3}]}
```

```json
{"schema_version":1,"severity":"WARN","event":"listener_transient_error","timestamp":"2026-07-22T10:00:02Z","message":"accept error, retrying in 2ms","fields":[{"error":"connection refused"},{"backoff_ms":2}]}
```
