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
ProcessStarting, RootInitialized, ListenerReady, ShutdownRequested, DrainingStarted, ShutdownComplete

### Connection
ConnectionAccepted, ConnectionRejected, TlsHandshakeSuccess/Failure/Timeout, HeaderTimeout, ParserRejection, KeepAliveClosed, ResponseWriteTimeout, ClientDisconnect, ConnectionPanic

### Request/Service
RequestCompleted, FileNotFound, FileDenied, FileError, DotfileDenied, SymlinkDenied, RootEscapeDenied, BodyPolicyRejection, ServiceTimeout, ServiceError, DirectoryListingLimit

### Operational
ListenerTransientError, ListenerPersistentError, ResourceExhaustion, BlockingWorkerSaturation

## Output Modes

- **JSON Lines** (`--log-format json`): One valid JSON object per line on stderr
- **Text** (`--log-format text`): `[severity] event_name: message` on stderr
- **None** (`--log-format none`): Only fatal startup diagnostics

## Privacy

- Request paths are sanitized/truncated (last component only, max 128 chars)
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

## Operational Counters

`OpsCounters` tracks: connections accepted/rejected, active connections/streams, parser rejects, timeouts, bytes sent, shutdown counts, listener errors, dropped log events.

## Listener Error Classification

Accept errors are classified by `io::ErrorKind`:
- **Transient** (Interrupted, ConnectionRefused, etc.) → Debug severity, bounded backoff
- **Resource exhaustion** (EMFILE/ENFILE) → Error severity, rate-limited retry
- **Persistent** (unknown errors) → Error severity, no backoff

Backoff uses bounded exponential: 1ms → 2ms → 4ms → 8ms → 50ms cap.
