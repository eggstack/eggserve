# Operations Logging Guide

This guide covers configuring, consuming, and troubleshooting eggserve's structured logging output.

## Configuration

### CLI flags

```sh
eggserve --log-format json /path/to/root    # JSON Lines (machine-consumable)
eggserve --log-format text /path/to/root    # Text (human-readable)
eggserve --log-format none /path/to/root    # Silent (fatal startup diagnostics only)
```

All output goes to **stderr**. stdout is reserved for serving content.

### Python API

```python
from eggserve.server import HTTPServer, SimpleHTTPRequestHandler

handler = SimpleHTTPRequestHandler
server = HTTPServer(("127.0.0.1", 8000), handler)
```

The Python server logs to stderr via the CLI's structured logging. The server does not accept observer callbacks; operational events are emitted to stderr by the Rust runtime.

## JSON Lines Schema

Every line is a self-contained JSON object:

```json
{
  "schema_version": 1,
  "severity": "INFO",
  "event": "connection_accepted",
  "timestamp": "2026-07-22T10:00:00.123Z",
  "message": "connection accepted",
  "connection_id": 42,
  "fields": []
}
```

### Required fields

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | number | Always `1` |
| `severity` | string | `DEBUG`, `INFO`, `WARN`, `ERROR` |
| `event` | string | Stable event kind name (snake_case) |
| `timestamp` | string | RFC 3339 format: `YYYY-MM-DDTHH:MM:SS.mmmZ` |
| `message` | string | Human-readable description |

### Optional fields

| Field | Type | Description |
|-------|------|-------------|
| `connection_id` | number | Unique per-process connection identifier |
| `request_seq` | number | Request sequence number within connection |
| `fields` | array | Structured key-value pairs |

### Fields array

Each element is an object with a single key-value pair. Values preserve their type:

```json
"fields": [
  {"declared_bytes": 1048576},
  {"limit_bytes": 524288},
  {"error_kind": "WouldBlock"}
]
```

## Event Reference

### Process lifecycle

| Event | Severity | When |
|-------|----------|------|
| `process_starting` | INFO | Server binary starting |
| `root_initialized` | INFO | Root directory opened and pinned |
| `listener_ready` | INFO | Accept loop bound and polling |
| `shutdown_requested` | INFO | Graceful shutdown initiated |
| `draining_started` | INFO | Draining in-flight connections |
| `forced_shutdown_started` | WARN | Drain deadline exceeded |
| `shutdown_complete` | INFO | Server stopped |

### Connection lifecycle

| Event | Severity | When |
|-------|----------|------|
| `connection_accepted` | DEBUG | New TCP connection accepted |
| `connection_rejected` | DEBUG | Admission limit reached |
| `tls_handshake_success` | DEBUG | TLS handshake completed |
| `tls_handshake_failure` | WARN | TLS handshake failed |
| `tls_handshake_timeout` | WARN | TLS handshake timed out |
| `header_timeout` | WARN | Header read timed out (also bounds idle keep-alive gaps; see timeout reference) |
| `body_read_timeout` | WARN | Body read timed out |
| `parser_rejection` | DEBUG | HTTP framing rejection (includes Hyper `max_buf_size`/`max_headers` parse failures) |
| `header_bytes_rejected` | DEBUG | Aggregate request-header bytes exceeded (431, pre-service) |
| `request_target_too_long` | DEBUG | Request target exceeded (414, pre-service) |
| `service_admission_rejected` | WARN | In-flight service budget exhausted (503) |
| `keep_alive_closed` | DEBUG | Keep-alive connection closed cleanly |
| `keep_alive_idle_timeout` | DEBUG | Idle keep-alive connection closed after inactivity |
| `max_requests_close` | DEBUG | Request limit reached; response completed with `Connection: close` |
| `write_stall_timeout` | WARN | Response outstanding with no socket progress; connection closed |
| `connection_total_timeout` | WARN | Total connection lifetime exceeded |
| `client_disconnect` | DEBUG | Client disconnected |
| `connection_panic` | ERROR | Handler panic contained |

### Request/service

| Event | Severity | When |
|-------|----------|------|
| `file_not_found` | DEBUG | Path resolved but file not found |
| `file_denied` | DEBUG | Access denied (dotfile/symlink/policy) |
| `file_error` | WARN | File stream I/O error |
| `dotfile_denied` | DEBUG | Dotfile access denied |
| `symlink_denied` | DEBUG | Symlink access denied |
| `root_escape_denied` | WARN | Path escapes root |
| `body_policy_rejection` | DEBUG | Request body rejected by policy |
| `incomplete_body_close` | DEBUG | Connection closed with unconsumed body |
| `service_invocation_suppressed` | WARN | Service call skipped (e.g., timeout already fired) |
| `service_timeout` | WARN | Handler timed out (504) |
| `service_error` | ERROR | Handler returned error |
| `request_completed` | INFO | Request fully processed |
| `directory_listing_limit` | WARN | Directory listing entry or size limit reached |
| `blocking_worker_saturation` | WARN | Blocking worker pool fully utilized |
| `response_stream_started` | DEBUG | Streaming response started |
| `response_stream_completed` | DEBUG | Streaming response completed cleanly |
| `response_stream_length_mismatch` | WARN | Known-length overrun/underrun; connection closed |
| `response_stream_producer_error` | WARN | Producer failed after commitment; connection closed |
| `response_stream_producer_panic` | ERROR | Producer panicked while polling; connection closed |
| `response_stream_cancelled` | DEBUG | Stream dropped before completion (disconnect/shutdown/HEAD suppression) |

### Operational faults

| Event | Severity | When |
|-------|----------|------|
| `listener_transient_error` | DEBUG/WARN | Retryable accept error |
| `listener_persistent_error` | ERROR | Fatal accept error |
| `resource_exhaustion` | ERROR | File descriptor exhaustion |
| `log_sink_failure` | ERROR | Logging backend failed |

## Operational Counters

`global_counters().snapshot()` provides a point-in-time snapshot:

| Counter | Description |
|---------|-------------|
| `connections_accepted` | TCP connections accepted |
| `connections_rejected` | Rejected by connection admission limit |
| `active_connections` | Currently active |
| `active_file_streams` | Currently streaming file responses |
| `active_service_requests` | Requests currently in the service pipeline |
| `parser_rejects` | HTTP parsing failures (incl. Hyper parser-limit errors) |
| `header_bytes_rejected` | Aggregate header-byte rejections (431) |
| `request_target_rejected` | Request-target rejections (414) |
| `service_admission_rejected` | Requests refused by the in-flight service budget (503) |
| `body_rejections` | Request bodies rejected by policy |
| `header_timeouts` | Header read timeouts (incl. idle-gap closes when shorter than the idle timeout) |
| `body_read_timeouts` | Body read timeouts |
| `keepalive_idle_timeouts` | Idle keep-alive closes |
| `max_requests_closes` | Connections closed after reaching the request limit |
| `write_stall_timeouts` | Write no-progress closes |
| `connection_total_timeouts` | Total connection lifetime timeouts |
| `graceful_shutdowns` | Clean shutdowns |
| `forced_shutdowns` | Shutdowns with timeout |
| `listener_errors` | Accept loop errors |
| `dropped_log_events` | Events dropped due to sink failures |
| `streaming_started` | Streaming responses started |
| `streaming_completed` | Streaming responses completed cleanly |
| `stream_length_mismatches` | Known-length mismatches (closed) |
| `stream_producer_errors` | Producer failures after commitment |
| `stream_producer_panics` | Producer panics while polling |
| `stream_cancelled` | Streams cancelled before completion |

## Troubleshooting

### Log flooding from listener errors

Repeated accept errors (e.g., file descriptor exhaustion) are rate-limited:
- First occurrence is always emitted
- Subsequent identical errors emit a summary every 10 occurrences
- Counter resets on successful accept or different error kind

### Python server logging

The Python server delegates logging to the Rust runtime's stderr log sink. There is no Python observer callback; operational events are emitted to stderr in the same structured format as the CLI.

### Log sink failures

If a log sink panics, `CompositeLogSink` catches the panic, increments `dropped_log_events`, and emits a `log_sink_failure` event through the remaining sinks. The server continues operating.

### JSON parse errors

If `event_to_json` output fails to parse, file a bug. The output is guaranteed to be valid UTF-8 JSON. Control characters in messages are escaped (`\n`, `\t`, `\u0000`-style).
