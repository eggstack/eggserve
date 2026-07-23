# Migration Guide: Request Body Support

## Overview

Request body support is **experimental** and opt-in. The default body policy is
`reject` — all request bodies are silently dropped unless you explicitly configure
a body mode.

## Quick Start

### Before (no body support)

```python
from eggserve import Server

def handler(req):
    return Response.text(200, "ok")

Server(root=".", handler=handler).start()
```

### After (with body support)

```python
from eggserve import Server, Response

def handler(req):
    if req.has_body:
        data = req.body.read()  # or req.body.iter_chunks()
        return Response.text(200, f"Received {len(data)} bytes")
    return Response.text(200, "no body")

Server(
    root=".",
    handler=handler,
    request_body_mode="buffer",      # "reject" | "buffer" | "stream"
    max_request_body_bytes=10240,    # required for buffer/stream
    body_timeout_secs=30,            # optional, default 30
    incomplete_body_policy="close",  # "close" only
).start()
```

## Body Modes

| Mode | Behavior |
|------|----------|
| `reject` (default) | All bodies silently dropped; handler sees `has_body=False` |
| `buffer` | Entire body buffered in memory up to `max_request_body_bytes` |
| `stream` | Body streamed in chunks up to `max_request_body_bytes` |

## Request Body API

```python
req.has_body      # bool: True if body is present
req.body          # RequestBody or None

# Buffer mode
data = req.body.read()          # bytes: full body
data = req.body.iter_chunks()   # BodyChunkIterator: chunk-by-chunk

# Properties
req.body.declared_length   # int | None: from Content-Length header
req.body.bytes_received    # int: bytes read so far
req.body.complete          # bool: True after full consumption
```

## One-Shot Enforcement

Body objects can only be consumed once:

```python
def handler(req):
    if req.has_body:
        data = req.body.read()        # consumes the body
        # req.body.read()             # raises RequestBodyConsumedError
        # req.body.iter_chunks()      # raises RequestBodyConsumedError
```

## Error Hierarchy

```
RequestBodyError
├── RequestBodyRejectedError      # policy is reject
├── RequestBodyTooLargeError      # body exceeds max_request_body_bytes
├── RequestBodyTimeoutError       # body read timeout
├── RequestBodyDisconnectedError  # client disconnected
├── RequestBodyIncompleteError    # premature EOF
├── RequestBodyConsumedError      # body already consumed
└── RequestBodyCancelledError     # request cancelled
```

All errors inherit from `EggserveError`.

## Static Service

The built-in static service (no handler) always uses `reject` policy regardless
of constructor settings. POST/PUT/DELETE/PATCH return 405.

## Connection Behavior

- **Full consumption**: Connection kept alive (if keep-alive)
- **Partial consumption**: Connection closed (close policy)
- **Body exceeds limit**: 413 returned, connection closed
- **Body timeout**: 408 returned or connection closed

## Framing Strictness

eggserve enforces hardened HTTP/1 framing before any handler invocation:

- **TE+CL rejection**: Requests containing both `Transfer-Encoding` and any `Content-Length` field are rejected with 400 before the service is called. This applies to all methods, not just body-forbidden ones.
- **Duplicate Content-Length rejection**: Requests with more than one `Content-Length` field are rejected with 400, even when values are identical. Conflicting values are rejected at the HTTP/1 wire level by Hyper.
- **Malformed Content-Length**: Non-numeric, negative, signed, overflowing, or non-decimal `Content-Length` values are rejected at the HTTP/1 wire level by Hyper before eggserve processes them.

These checks ensure no ambiguous or conflicting framing signals reach the body ingestion pipeline.

## Backward Compatibility

Existing handlers that don't inspect `req.body` continue working unchanged.
The `has_body` and `body` attributes are additions; no existing attributes
were modified.
