# Structured Logging and Operational Events

## Overview

eggserve uses structured JSON Lines logging for machine-consumable operational events, with a text mode fallback for human readability. The system is defined in `eggserve-core::ops`.

## Event Model

Every operational event has:
- `schema_version` (u32) — currently 1
- `severity` — Debug, Info, Warn, Error
- `event` — stable event kind name (ProcessStarting, RequestCompleted, etc.)
- `timestamp` — RFC 3339 format
- `message` — human-readable description
- `connection_id` (optional) — unique per-process connection identifier
- `request_seq` (optional) — request sequence number within connection
- `fields` — structured key-value pairs

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
- `connection_rejected` — admission limit reached
- `tls_handshake_success/failure/timeout` — TLS events (feature-gated)
- `header_timeout` — HTTP header read timeout
- `body_read_timeout` — request body read timeout (buffer mode)
- `parser_rejection` — HTTP framing rejection
- `keep_alive_closed` — keep-alive connection closed
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

### Operational
- `listener_transient_error` — retryable accept error with backoff
- `listener_persistent_error` — fatal accept error, no backoff
- `resource_exhaustion` — file descriptor or memory exhaustion
- `blocking_worker_saturation` — blocking pool at capacity
- `log_sink_failure` — logging backend failed

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

## Python Observer

The Python `Server` accepts an optional `observer` callback that receives structured event dictionaries:

```python
def my_observer(event):
    print(f"[{event['severity']}] {event['event']}: {event['message']}")

server = Server(root="/path", observer=my_observer)
```

Observer callback errors are caught and printed to Python stderr. The observer runs with the GIL acquired, so long-running observers may block event processing. Observer errors are not counted.

## Operational Counters

`OpsCounters` (accessible via `global_counters()`) tracks:
- `connections_accepted` — TCP connections accepted
- `connections_rejected` — connections rejected by admission limit
- `active_connections` — currently active connections
- `active_file_streams` — currently streaming file responses
- `parser_rejects` — HTTP parsing failures
- `header_timeouts` — header/body read timeouts
- `connection_total_timeouts` — total connection lifetime timeouts
- `bytes_sent` — total bytes sent to clients
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
- Failed sink events increment `dropped_log_events` counter
- `Logger::try_init()` returns `Err(())` if already initialized (Python coexistence)
- `NopLogSink` is the default when no logger is configured

## Example Events

```json
{"schema_version":1,"severity":"Info","event":"process_starting","timestamp":"2026-07-22T10:00:00Z","message":"eggserve 0.1.0 starting","fields":{"version":"0.1.0","bind":"127.0.0.1:8000","root":"./public","symlinks":"denied","dotfiles":"denied"}}
```

```json
{"schema_version":1,"severity":"Info","event":"request_completed","timestamp":"2026-07-22T10:00:01Z","message":"GET /style.css 200","connection_id":42,"request_seq":1,"fields":{"method":"GET","path":"/style.css","status":200,"bytes":1024,"duration_ms":3}}
```

```json
{"schema_version":1,"severity":"Warn","event":"listener_transient_error","timestamp":"2026-07-22T10:00:02Z","message":"accept error, retrying in 2ms","fields":{"error":"connection refused","backoff_ms":2}}
```
