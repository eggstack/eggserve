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
from eggserve.server import Server

def my_observer(event):
    print(f"[{event['severity']}] {event['event']}: {event['message']}")

server = Server(root="/path", observer=my_observer)
```

The observer receives a dict with the same schema as the JSON Lines output. Observer errors are caught and printed to stderr; they do not crash the server.

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
| `keep_alive_closed` | DEBUG | Keep-alive connection closed cleanly |
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
| `service_timeout` | WARN | Handler timed out (504) |
| `service_error` | ERROR | Handler returned error |
| `parser_rejection` | DEBUG | HTTP framing rejection |

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
| `connections_rejected` | Rejected by admission limit |
| `active_connections` | Currently active |
| `active_file_streams` | Currently streaming file responses |
| `parser_rejects` | HTTP parsing failures |
| `header_timeouts` | Header read timeouts |
| `body_read_timeouts` | Body read timeouts |
| `connection_total_timeouts` | Total connection lifetime timeouts |
| `bytes_sent` | Total bytes sent |
| `graceful_shutdowns` | Clean shutdowns |
| `forced_shutdowns` | Shutdowns with timeout |
| `listener_errors` | Accept loop errors |
| `dropped_log_events` | Events dropped due to sink failures |

## Troubleshooting

### Log flooding from listener errors

Repeated accept errors (e.g., file descriptor exhaustion) are rate-limited:
- First occurrence is always emitted
- Subsequent identical errors emit a summary every 10 occurrences
- Counter resets on successful accept or different error kind

### Python observer blocking

The observer callback runs with the GIL acquired. Long-running observers block event processing. Keep observer logic minimal or offload to a background thread.

### Log sink failures

If a log sink panics, `CompositeLogSink` catches the panic, increments `dropped_log_events`, and emits a `log_sink_failure` event through the remaining sinks. The server continues operating.

### JSON parse errors

If `event_to_json` output fails to parse, file a bug. The output is guaranteed to be valid UTF-8 JSON. Control characters in messages are escaped (`\n`, `\t`, `\u0000`-style).
