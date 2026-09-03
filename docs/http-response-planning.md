# HTTP Response Planning

Canonical static planning returns opened-handle file/range bodies directly. The
runtime performs the only file admission and transport conversion. Static request
bodies are rejected by service policy; custom service body policy remains
method-aware.

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
| `If-Match` fails (strong comparison) | 412 Precondition Failed | Empty |
| `If-Unmodified-Since` fails (no `If-Match`) | 412 Precondition Failed | Empty |
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

Preconditions are evaluated in the order mandated by RFC 9110 § 13.2.2:
`If-Match` → `If-Unmodified-Since` → `If-None-Match` → `If-Modified-Since`
→ (`Range` +) `If-Range`.

### If-Match

- Strong comparison per RFC 9110 § 13.1.1: a failed condition yields
  `412 Precondition Failed` with an empty body.
- Wildcard `*` matches whenever a current representation exists.
- Generated metadata ETags are weak, so they never satisfy strong
  comparison — the same rationale that keeps them ineligible for
  `If-Range`.
- Evaluated before all cache-validation conditions; a failed `If-Match`
  takes precedence over a matching `If-None-Match` or a satisfiable range.

### If-Unmodified-Since

- Evaluated only when `If-Match` is absent (RFC 9110 § 13.1.4).
- The condition fails when the file's modification time is newer than the
  provided date; failure yields `412 Precondition Failed`.
- Malformed dates and files without an available modification time are
  ignored, per RFC 9110 § 13.1.4.

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

- Files with pre-epoch (before 1970) modification times receive a weak ETag
  with a negative seconds component, but no `Last-Modified` header:
  HTTP-date formatting supports epoch-or-later timestamps only. Such files
  remain conditionally cacheable via their ETag.
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

- Entity-tags are compared strongly. Weak ETags are not eligible for
  `If-Range`, even when their opaque value matches; they remain valid for
  `If-None-Match`.
- A valid date exactly matching the selected `Last-Modified` validator serves
  the range. Stale, malformed, empty, and nonmatching values serve a full
  `200 OK`; absence of `If-Range` leaves a satisfiable range enabled.

## HEAD parity

HEAD responses use the same status and headers as GET, but with an empty body:

- `200 OK` with full content-length but no body.
- `304 Not Modified` with validator headers but no body.
- `206 Partial Content` with range headers but no body.
- `416 Range Not Satisfiable` with `Content-Range` header but no body.

All origin responses receive exactly zero or one runtime-owned `Date` header
during final response construction per `DatePolicy` (default one system-clock
`Date`; `Suppress` emits zero as an explicit RFC tradeoff; EggServe is the sole
authority with Hyper automatic `Date` disabled). Directory-listing HEAD
preserves the GET representation's `Content-Length`.

## ETag generation

Weak ETags are generated from file size, mtime seconds, and mtime nanoseconds. Returns `None` if metadata has no modification time:

```
W/"<size>-<mtime_secs>-<mtime_nanos>"
```

Nanosecond precision distinguishes rapid same-size modifications where millisecond precision would collide. The ETag is a weak validator — acceptable for static files where strong consistency is not required.

`StaticMetadataPolicy` (`standard()` default emits `ETag` + `Last-Modified`;
`minimal_fingerprint()` suppresses both) controls emission via
`plan_file_response_with_preconditions_and_metadata`. Retained `Last-Modified`
never exceeds `Date` (dropped at the final boundary when it would).

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

## Unified service-layer entry point

Both direct-file and directory-index routes share a single code path (`serve_resolved_file`) that constructs a `StaticRequestInput` from the canonical request and passes it through the planner. This eliminates semantic drift between `/x/` and `/x/index.html`.

### `StaticRequestInput`

```rust
pub(crate) struct StaticRequestInput<'a> {
    pub method: ReadOnlyMethod,
    pub if_none_match: Option<&'a str>,
    pub if_modified_since: Option<&'a str>,
    pub range: Option<&'a str>,
    pub if_range: Option<&'a str>,
}
```

Both routes extract these five fields identically from the incoming request. The helper `serve_resolved_file()` then:

1. Calls `plan_file_response()` with the `StaticRequestInput` fields
2. Constructs the response body from the opened file handle per `BodyPlan`
3. Normalizes the response through the canonical path

No route drops conditional or range headers. No route reconstructs the plan after body construction.

### Parity guarantee

Parity guarantee: `/x/` and `/x/index.html` must produce identical conditional and range behavior for the same file. This is verified by:

- **14 planner parity tests** — identical metadata + headers always produce identical planner outputs (same status, same headers, same `BodyPlan` variant)
- **8 production-path parity tests** — raw TCP comparison of `/subdir/` vs `/subdir/index.html` and `/` vs `/index.html` for full response, HEAD, range, If-None-Match 304, If-Modified-Since 304, and unsatisfiable range 416
- **keep-alive reuse test** — verifies that conditional and range outcomes are correct when requests are sent sequentially on a single keep-alive connection

## Non-goals

- Full HTTP/2 or HTTP/3 semantics.
- General cache policy framework.
- ASGI/WSGI adapters.
- General-purpose request callback server or application runtime.
- Middleware or routing.
- Reverse proxy behavior.
- Multi-range MIME responses.
