# HTTP Response Planning

## Overview

The response planner (`primitives::planner`) is a pure, Hyper-independent planning layer that determines what response to send for a given request. It produces `StaticResponsePlan` value objects that can be mapped into Hyper, Python stdlib server responses, test assertions, or later adapter layers.

The planner is a standalone primitive — it does not depend on Hyper body types. Callers translate the plan into their HTTP framework of choice.

## Request validation policy

`primitives::http` provides request validation for static/read-only serving:

| Function | Purpose |
|----------|---------|
| `validate_method()` | Restricts to `GET` and `HEAD` (`ReadOnlyMethod`). Returns `MethodNotAllowed` for others. |
| `validate_request_body()` | Rejects requests with bodies under zero-body policy. Checks `Content-Length` and `Transfer-Encoding`. |
| `validate_request_target()` | Validates URI origin-form syntax (starts with `/`, no `*` or authority form). |

### Body validation behavior

Under the default zero-body policy (`max_request_body_bytes: 0`):

- `Content-Length: 0` — allowed
- Positive `Content-Length` — rejected (413-equivalent)
- Malformed `Content-Length` (negative, overflow, non-numeric) — rejected (400-equivalent)
- Non-empty `Transfer-Encoding` — rejected (400-equivalent)
- Both `Content-Length` and `Transfer-Encoding` present — rejected (400-equivalent)

## Status mapping

| Condition | Status | Body |
|-----------|--------|------|
| Normal file GET | 200 OK | Full file |
| Normal file HEAD | 200 OK | Empty |
| `If-None-Match` matches | 304 Not Modified | Empty |
| `If-Modified-Since` matches (no ETag condition) | 304 Not Modified | Empty |
| `Range: bytes=0-` or `bytes=0-99` | 206 Partial Content | Byte range |
| `Range: bytes=-10` (suffix) | 206 Partial Content | Last N bytes |
| Unsatisfiable range | 416 Range Not Satisfiable | Empty |
| `If-Range` validator matches | 206 Partial Content | Byte range |
| `If-Range` validator mismatches | 200 OK | Full file |
| Method not allowed | 405 Method Not Allowed | Empty |
| Body present on GET/HEAD | 400 or 413 | Empty |
| Directory listing | 200 OK | HTML |
| Directory listing HEAD | 200 OK | Empty |

## Conditional request support

### If-None-Match

- Supports weak comparison (acceptable for static files).
- `If-None-Match: *` matches any existing resource.
- Multiple ETag values in one header: if any match, the condition is met.
- Returns `304 Not Modified` with validator headers (ETag, Last-Modified) and empty body.

### If-Modified-Since

- Only evaluated when `If-None-Match` is absent or does not match.
- Parsed via `httpdate::parse_http_date`. Malformed dates are silently ignored (treated as absent).
- Returns `304 Not Modified` when the file's modification time is not newer than the given date.

### Limitations

- No `If-Match` / `If-Unmodified-Since` support (not needed for static file serving).
- No `Vary` header management.
- No full cache framework — the planner evaluates validators and returns the appropriate status, but does not enforce cache-control policy.

## Range request support

### Supported range formats

| Syntax | Meaning |
|--------|---------|
| `bytes=0-99` | First 100 bytes |
| `bytes=0-` | From byte 0 to EOF |
| `bytes=-10` | Last 10 bytes |

### Behavior

- Single byte ranges only. Multiple ranges are not supported — the planner returns `200 OK` (full response) when multiple ranges are present.
- Unsatisfiable ranges (start >= file size) return `416 Range Not Satisfiable` with `Content-Range: bytes */<len>` and `Content-Length: 0`.
- Suffix ranges (`bytes=-N`) where `N` exceeds the file size are satisfied as the whole file (`Content-Range: bytes 0-<last>/<len>`) rather than `416`.
- `206 Partial Content` includes `Content-Range: bytes <start>-<end>/<len>`, `Content-Length`, `Content-Type`, `Accept-Ranges: bytes`, and validators (`ETag`, `Last-Modified`) when available.

### If-Range

- If the validator (ETag or Last-Modified) matches the current resource, the range is served.
- If it does not match, a full `200 OK` is returned.
- Malformed `If-Range` values are treated as absent (full response).

## HEAD parity

HEAD responses use the same status and headers as GET, but with an empty body:

- `200 OK` with full content-length but no body.
- `304 Not Modified` with validator headers but no body.
- `206 Partial Content` with range headers but no body.
- `416 Range Not Satisfiable` with `Content-Range` header but no body.

## ETag generation

Weak ETags are generated from file size and mtime seconds. Returns `None` if metadata has no modification time:

```
W/"<size>-<mtime_secs>"
```

This matches the existing `planner::generate_etag()` behavior. The ETag is a weak validator — acceptable for static files where strong consistency is not required.

## Directory listing planning

`plan_directory_listing()` generates a `StaticResponsePlan` for directory listings:

- `200 OK` with `Content-Type: text/html; charset=utf-8`.
- `Content-Length` based on generated HTML.
- Security headers: `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'`, `Referrer-Policy: no-referrer`.
- Empty body for HEAD requests.

HTML generation is internal to `response::directory_listing_response()`. The planner wraps it with the appropriate status and headers.

## Usage from Rust

```rust
use eggserve_core::primitives::planner::plan_file_response;
use eggserve_core::primitives::http::ReadOnlyMethod;

let plan = plan_file_response(
    ReadOnlyMethod::Get,
    &file_metadata,           // &std::fs::Metadata
    content_type,             // &str, e.g. "text/plain; charset=utf-8"
    if_none_match_header,     // Option<&str>
    if_modified_since_header, // Option<&str>
    range_header,             // Option<&str>
    if_range_header,          // Option<&str>
);

// plan.status, plan.headers, plan.body are Hyper-independent
// Translate to your framework of choice
```

## Usage from Python

The planner produces value objects (`StaticResponsePlan` with `ResponseStatus`, `HeaderMapPlan`, `BodyPlan`) that can be serialized to `(name, value)` header pairs and byte bodies. Python bindings expose these via `ResolvedFile.plan_response()` and `ResolvedFile.plan_conditional_response()`. See [python-api.md](python-api.md) for details.

## Non-goals

- Full HTTP/2 or HTTP/3 semantics.
- General cache policy framework.
- ASGI/WSGI adapters.
- Request callback server.
- Middleware or routing.
- Reverse proxy behavior.
- Multi-range MIME responses.
