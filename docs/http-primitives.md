# HTTP Primitives Contract

eggserve exposes a documented, reusable HTTP/1.1 primitive contract for downstream projects. This document defines the supported server-side HTTP subset and the behavior guarantees that embedding consumers can depend on.

## Supported protocol subset

- HTTP/1.1 server behavior through Hyper.
- GET and HEAD for the static CLI path.
- Explicit method validation primitive for downstream code (`ReadOnlyMethod`).
- Origin-form request targets for static path parsing.
- No request bodies for the static CLI path.
- Configurable body metadata validation primitive for downstream code.
- Static full-file responses.
- Static range responses.
- Empty responses.
- Byte responses for future dynamic primitive work.
- Conditional GET/HEAD via `If-None-Match` and `If-Modified-Since`.
- Range requests via `Range` and `If-Range`.
- Generic 400/403/404/405/413/416/500/503 behavior for the CLI path.

### Unsupported in this contract

- Request body streaming into Python callbacks.
- Multipart range responses.
- Chunked response construction as a public primitive.
- HTTP trailers.
- Upgrade semantics.
- Absolute-form proxy requests.
- Authority-form CONNECT requests.
- Asterisk-form OPTIONS requests.

## Request method validation

`primitives::http` provides a `ReadOnlyMethod` enum restricted to `GET` and `HEAD`:

```rust
pub enum ReadOnlyMethod {
    Get,
    Head,
}
```

`validate_method(method: &str)` returns `Ok(ReadOnlyMethod)` for `"GET"` and `"HEAD"`, or `Err(RequestValidationError::MethodNotAllowed)` for all other methods.

### Error mapping

| Method | Result |
|--------|--------|
| `GET` | `ReadOnlyMethod::Get` |
| `HEAD` | `ReadOnlyMethod::Head` |
| Any other | `RequestValidationError::MethodNotAllowed` → HTTP 405 |

## Request target validation

`validate_request_target(target: &str)` validates origin-form request targets:

- Must start with `/`
- Must not be empty
- Must not contain whitespace
- Rejects absolute-form (`http://...`), authority-form (`host:port`), and asterisk-form (`*`)

Error: `RequestValidationError::InvalidRequestTarget` → HTTP 400 (via path parsing layer).

Note: Full path confinement (traversal, dotfiles, percent-encoding) is handled by `ConfinedPath::parse()`, not by `validate_request_target()`. The target validation is a coarse check; path confinement is the fine-grained check.

## Request body metadata policy

`validate_request_body()` validates body-framing headers for GET/HEAD requests:

```rust
pub fn validate_request_body(
    content_length: Option<&str>,
    transfer_encoding: Option<&str>,
    max_body_bytes: u64,
) -> Result<(), RequestValidationError>
```

### Behavior under zero-body policy (max_body_bytes = 0)

| Input | Result |
|-------|--------|
| No headers | OK |
| `Content-Length: 0` | OK |
| `Content-Length: 1024` | `BodyTooLarge` → HTTP 413 |
| `Content-Length: not-a-number` | `InvalidContentLength` → HTTP 400 |
| `Content-Length: -1` | `InvalidContentLength` → HTTP 400 |
| `Content-Length: 99999999999999999999` | `InvalidContentLength` → HTTP 400 |
| `Transfer-Encoding: chunked` | `UnsupportedTransferEncoding` → HTTP 400 |
| `Content-Length: 0` + `Transfer-Encoding: chunked` | `ConflictingBodyHeaders` → HTTP 400 |
| `Transfer-Encoding: ` (empty/whitespace) | OK (treated as absent) |

### Configurable body limits

The `max_body_bytes` parameter allows downstream projects to set non-zero limits. When set to a positive value, `Content-Length` values up to that limit are accepted; values above it trigger `BodyTooLarge`.

## Header handling rules

Response headers are constructed as a `HeaderMapPlan` (ordered list of name/value pairs). The planner produces these headers:

### Full response (200 OK)

- `Content-Length` — file size
- `Content-Type` — MIME type from file extension
- `Accept-Ranges: bytes`
- `X-Content-Type-Options: nosniff`
- `Last-Modified` — from file metadata (when available)
- `ETag` — weak validator from size + mtime (when available)

### Range response (206 Partial Content)

- `Content-Length` — range size
- `Content-Type` — MIME type
- `Content-Range: bytes START-END/TOTAL`
- `Accept-Ranges: bytes`
- `X-Content-Type-Options: nosniff`
- `Last-Modified` — (when available)
- `ETag` — (when available)

### Not modified (304)

- `ETag` — current validator
- `Last-Modified` — (when available)

### Not range satisfiable (416)

- `Content-Length: 0`
- `Accept-Ranges: bytes`
- `Content-Range: bytes */TOTAL`

### Method not allowed (405)

- `Allow: GET, HEAD`

## Static response planning

The response planner (`primitives::planner`) is a pure function with no Hyper dependency. It produces `StaticResponsePlan` values from file metadata and request headers.

### Planning flow

```
Request with conditional headers
    │
    ▼
┌─────────────────────────────────┐
│ If-None-Match (ETag)           │
│  Match? → 304 Not Modified     │
└─────────────────┬───────────────┘
                  │ No match
                  ▼
┌─────────────────────────────────┐
│ If-Modified-Since               │
│  Not modified? → 304            │
└─────────────────┬───────────────┘
                  │ Modified
                  ▼
┌─────────────────────────────────┐
│ If-Range                        │
│  Mismatch? → serve full         │
└─────────────────┬───────────────┘
                  │ Match
                  ▼
┌─────────────────────────────────┐
│ Range header                    │
│  Valid? → 206 Partial           │
│  Invalid? → 416                 │
└─────────────────┬───────────────┘
                  │ No range
                  ▼
           200 OK (full body)
```

## Conditional request behavior

### If-None-Match

- Weak comparison: `W/"abc"` matches `W/"abc"` (inner value comparison).
- Wildcard: `If-None-Match: *` matches any resource with an ETag.
- Multiple ETags: comma-separated list; any match triggers 304.
- Returns 304 with `ETag` and `Last-Modified` headers, empty body.

### If-Modified-Since

- Only evaluated when `If-None-Match` is absent or does not match.
- Parsed via `httpdate::parse_http_date`. Malformed dates are silently ignored (treated as absent).
- Returns 304 when file modification time ≤ the given date.
- Returns 200 when file is newer.

## Range request behavior

### Supported formats

| Syntax | Meaning |
|--------|---------|
| `bytes=0-99` | First 100 bytes |
| `bytes=0-` | From byte 0 to EOF |
| `bytes=-10` | Last 10 bytes |
| `bytes=50-99` | Bytes 50 through 99 |

### Range evaluation rules

- **Suffix range (`bytes=-N`)**: Returns last N bytes. If N exceeds file size, returns the whole file.
- **Open-ended range (`bytes=START-`)**: Returns from START to EOF. If START ≥ file size, returns 416.
- **Closed range (`bytes=START-END`)**: Returns START through END (clamped to file size). If START > END, returns 416. If START ≥ file size, returns 416.
- **Multiple ranges**: Falls through to full 200 OK response (single-range only).
- **Unsupported unit**: Falls through to full 200 OK response.
- **Empty file**: All range requests return 416.

### If-Range

- Validates against current ETag or Last-Modified.
- If validator matches, the range is served (206).
- If validator mismatches, full 200 OK is returned.
- Malformed If-Range is treated as absent (range proceeds normally).

## HEAD/GET parity

HEAD responses produce the same `StaticResponsePlan` as GET but with `BodyPlan::Empty`:

- Same status code
- Same headers (including `Content-Length`)
- No body transfer

This is mechanically enforced by the planner: `ReadOnlyMethod::Head` produces `BodyPlan::Empty` while `ReadOnlyMethod::Get` produces `BodyPlan::FileFull` or `BodyPlan::FileRange`.

## Error mapping

| Error | HTTP Status | Description |
|-------|-------------|-------------|
| `RequestValidationError::MethodNotAllowed` | 405 | Method not in {GET, HEAD} |
| `RequestValidationError::InvalidRequestTarget` | 400 | Target not origin-form |
| `RequestValidationError::InvalidContentLength` | 400 | Malformed Content-Length |
| `RequestValidationError::BodyTooLarge` | 413 | Content-Length exceeds limit |
| `RequestValidationError::UnsupportedTransferEncoding` | 400 | Non-empty Transfer-Encoding |
| `RequestValidationError::ConflictingBodyHeaders` | 400 | Both CL and TE present |
| Path traversal / dotfile denial | 403 | Path policy violation |
| Malformed percent encoding | 400 | Bad %xx sequence |
| File not found | 404 | No file at resolved path |

## Downstream use by app-server/adapter projects

eggserve's primitive layer is designed for embedding. Downstream projects may build ASGI/WSGI/app servers externally using these primitives, but eggserve does not implement those protocols in-tree.

### Rust embedding

```rust
use eggserve_core::primitives::planner::plan_file_response;
use eggserve_core::primitives::http::ReadOnlyMethod;

let plan = plan_file_response(
    ReadOnlyMethod::Get,
    &file_metadata,
    "text/plain; charset=utf-8",
    if_none_match_header,
    if_modified_since_header,
    range_header,
    if_range_header,
);

// plan.status, plan.headers, plan.body are Hyper-independent
// Translate to your framework of choice
```

### Python embedding

```python
from eggserve import SecureRoot, StaticPolicy

root = SecureRoot("public", policy=StaticPolicy())
resource = root.resolve_path("/assets/app.css")
if resource.is_file:
    plan = resource.file.plan_response("GET")
    print(plan.status, plan.body_kind)  # 200 file_full
```

## See also

- [http-response-planning.md](http-response-planning.md) — detailed planner behavior
- [python-api.md](python-api.md) — Python API reference
- [architecture/response-planning.md](../architecture/response-planning.md) — architecture deep dive
