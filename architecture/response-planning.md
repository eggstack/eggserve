# HTTP Response Planning — Deep Dive

The response planner produces framework-independent response descriptions. It handles conditional requests, range requests, ETag generation, and directory listing planning — all without depending on Hyper types.

## Module Location

`eggserve-core::primitives::planner` — exposed via `primitives` public facade.

## Key Types

### `StaticResponsePlan`

The output of response planning. A pure value object:

```rust
pub struct StaticResponsePlan {
    pub status: ResponseStatus,
    pub headers: HeaderMapPlan,
    pub body: BodyPlan,
}
```

### `ResponseStatus`

```rust
pub struct ResponseStatus(pub u16);

impl ResponseStatus {
    pub const OK: Self = Self(200);
    pub const NOT_MODIFIED: Self = Self(304);
    pub const PARTIAL_CONTENT: Self = Self(206);
    pub const NOT_RANGE_SATISFIABLE: Self = Self(416);
    pub const METHOD_NOT_ALLOWED: Self = Self(405);
    // ... other status constants
}
```

### `HeaderMapPlan`

A list of headers to include in the response:

```rust
pub struct HeaderMapPlan {
    pub headers: Vec<ResponseHeader>,
}

pub struct ResponseHeader {
    pub name: String,
    pub value: String,
}
```

### `BodyPlan`

```rust
pub enum BodyPlan {
    Empty,
    FullBytes(Vec<u8>),
    FileFull,
    FileRange { start: u64, end_inclusive: u64 },
}
```

### `FileRange`

```rust
pub struct FileRange {
    pub start: u64,
    pub end_inclusive: u64,
}
```

## Planning Functions

### `plan_file_response()`

Main entry point for file responses:

```rust
pub fn plan_file_response(
    method: ReadOnlyMethod,
    metadata: &std::fs::Metadata,
    content_type: &str,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    range_header: Option<&str>,
    if_range: Option<&str>,
) -> StaticResponsePlan
```

Steps:
1. Check `If-None-Match` (ETag comparison)
2. Check `If-Modified-Since` (Last-Modified comparison)
3. Check `If-Range` (validator matching)
4. Evaluate `Range` header
5. Plan body based on method (GET → full body, HEAD → empty)

### `evaluate_conditional_headers()`

Handles `If-None-Match` and `If-Modified-Since`:

- If ETag matches → `304 Not Modified`
- If `If-Modified-Since` is in the future or file is newer → proceed
- If `If-Modified-Since` is valid and file is not newer → `304 Not Modified`

### `evaluate_if_none_match()`

Weak ETag comparison. Supports the `W/"..."` weak prefix and the `*` wildcard, and matches comma-separated ETag lists by inner-quoted value.

### `evaluate_range_header()`

Parses `Range: bytes=START-END` header:

- Valid range within file bounds → `206 Partial Content`
- Range beyond file size → `416 Range Not Satisfiable`
- Multiple ranges → fall through to full `200 OK` (single-range only is served, and the planner currently does not select the first range)

### `evaluate_if_range()`

Validates `If-Range` validator against current ETag/Last-Modified. If mismatch → serve full response.

### `generate_etag()`

Generates ETag from file metadata:

```rust
pub fn generate_etag(metadata: &std::fs::Metadata) -> Option<String>
```

Returns `None` if metadata has no modification time. Format when present: `W/"<size>-<mtime_secs>"` (weak validator).

### `plan_directory_listing()`

Generates directory listing HTML with CSP headers.

## HEAD Parity

For `HEAD` requests, the planner produces the same `StaticResponsePlan` as `GET` but with `BodyPlan::Empty`. This ensures:
- Same status code
- Same headers (including `Content-Length`)
- No body transfer

## Conditional Request Flow

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

## Framework Independence

All types in the planner are pure value objects with no Hyper dependency. This enables:

1. **Python bindings** — The Python layer consumes `StaticResponsePlan` directly via `plan_to_python()`
2. **Testing** — Plans can be asserted without starting an HTTP server
3. **Embedding** — Other HTTP frameworks can consume the plan without Hyper coupling

## Canonical Response Types

The canonical response types (`primitives::canonical`) provide a transport-independent response model that all response producers converge on before transport conversion.

### Key Types

- `StatusCode` — validated HTTP status code (100–999, three-digit only) with classification helpers
- `ResponseHead` — status + `HeaderBlock` (duplicate-preserving headers)
- `ResponseBody` — `Empty` or `Bytes` body representation
- `Response` — complete response with one-shot body consumption

### Normalization Algorithm

The `normalize_response()` function is the single final normalization path. It applies:

1. **HEAD suppression** — body bytes discarded, representation headers preserved
2. **Body-forbidden enforcement** — 1xx, 204, 304 bodies discarded
3. **Hop-by-hop stripping** — `Transfer-Encoding` removed (runtime-owned)
4. **Content-Length computation** — set to actual body length
5. **Duplicate preservation** — end-to-end duplicate headers preserved

### Conversion Flow

All response producers converge on a single normalization path. There are two
allowed sequences depending on whether the body is in-memory or file-backed:

```
For in-memory bodies:
  producer → Response → normalize_response() → to_hyper_response()

For file-backed bodies:
  producer → normalize_metadata(headers, body_len) → streaming transport
```

In-memory path (e.g. Python handler responses, error responses):

```
StaticResponsePlan / Python Response
    │
    ▼
canonical::Response  (via From impls or builder)
    │
    ▼
normalize_response(response, request)
    │
    ▼
canonical::to_hyper_response(normalized)
    │
    ▼
hyper::Response<BoxBody>
```

File-backed path (e.g. file streaming):

```
StaticResponsePlan + ResolvedFile
    │
    ▼
normalize_metadata(status, headers, body_len, is_head)
    │
    ▼
Streaming transport (body_source_to_response)
```

### Metadata Normalization (Shared)

`normalize_metadata()` is the shared normalization entry point for both
in-memory and file-backed response producers. It applies the same framing rules
as `normalize_response()` but without consuming a `Response` value:

1. Strip runtime-owned `Transfer-Encoding`
2. HEAD responses: suppress `Content-Length`
3. Body-forbidden statuses (1xx, 204, 304): suppress `Content-Length`
4. Normal payloads: set `Content-Length` to actual body length
5. Preserve all other headers (including duplicates)

File-streaming producers call `normalize_metadata()` directly to apply the
same framing policy without constructing a canonical `Response`.

## Body-Source Conversion

After planning, the resolved file is converted to a `BodySource` that carries the opened file handle forward:

```
ResolvedResource::File(file)
    │
    ▼
plan_file_response(method, metadata, ...)
    │
    ▼
StaticResponsePlan { status, headers, body: BodyPlan }
    │
    ▼
file.into_body(&plan)  →  BodySource
    │
    ▼
body_source_to_response(status, headers, body_source, semaphore)
    │
    ▼
Hyper Response<BoxBody>
```

The `into_body()` conversion is consuming — it takes ownership of the `ResolvedFile` and maps each `BodyPlan` variant:

- `BodyPlan::Empty` → `BodySource::Empty`
- `BodyPlan::FullBytes(bytes)` → `BodySource::Bytes(bytes)`
- `BodyPlan::FileFull` → `BodySource::FileFull { file, len, mime }`
- `BodyPlan::FileRange { start, end_inclusive }` → `BodySource::FileRange { file, range, total_len, mime }`

The service layer's `body_source_to_response()` async function then converts the `BodySource` into a Hyper streaming body, acquiring a semaphore permit for file-backed variants to enforce `max_file_streams`.

## See Also

- [primitives-api.md](primitives-api.md) — Public API for response planning
- [eggserve-core.md](eggserve-core.md) — Core library context
- [architecture/overview.md](overview.md) — Data flow diagram

## Streaming Buffer Strategy (Plan 088)

File streaming uses `DEFAULT_CHUNK_SIZE` (8 KiB) as the read buffer size for both full-file and range responses. Each chunk allocates a fresh `Vec<u8>`, reads into it, truncates to actual bytes read, and wraps in `Bytes::from(buf)` (zero-copy transfer of ownership). No buffer pool or reuse strategy is currently employed — each chunk allocation is bounded by the fixed chunk size and released when consumed by the transport layer.

The `stream_chunk_size` field in `Limits` is reserved for future configurability. The `normalize_metadata()` function uses in-place `retain` for hop-by-hop header stripping and Content-Length removal, avoiding the previous clone+rebuild pattern.

Key allocation classification per request:
- **Required by ownership/lifetime**: chunk `Vec<u8>` (bounded by chunk size), ETag `String`, `HeaderMapPlan` headers
- **Removable copy (eliminated)**: `normalize_metadata` header filtering (now uses `retain`)
- **Benchmark artifact**: per-chunk allocation is bounded and cheap at 8 KiB

## Test coverage

The response planner has extensive test coverage:

- **Unit tests** (`planner.rs`): 52 tests covering ETag generation, conditional headers, range parsing, If-Range evaluation, HEAD parity, directory listing planning, and edge cases.
- **Integration tests** (`integration.rs`): tests for the full request handling path including method validation, body rejection, conditional requests, range requests, and HEAD parity.
- **Live HTTP tests** (`http_primitives_integration.rs`): 15 tests exercising real TCP connections through hyper's client/server stack, covering GET, HEAD, POST (405), 404, 403, 400, 413, 206, 416, and 304 responses.
- **Python tests** (`test_primitives.py`): comprehensive tests for method validation, body validation, request target validation, response planning, range responses, conditional responses, and HEAD parity through PyO3 bindings.
- **Canonical conformance tests** (`tests/canonical_conformance.rs`, `python/eggserve/test_canonical_conformance.py`): parity tests for canonical HTTP types (Method, HttpVersion, HeaderBlock, RequestTarget, RequestHead, StatusCode, ResponseHead, ResponseBody, Response, normalize_response). Exercises identical behavior across Rust and Python, including normalization rules (HEAD suppression, body-forbidden enforcement, hop-by-hop stripping, content-length computation).
- **Buffer qualification tests** (`tests/streaming_buffer_qualification.rs`): 18 tests for Plan 088 covering exact range boundaries (first byte, last byte, full file, chunk-crossing, chunk-start), zero-length file handling (GET, HEAD, range 416), buffer isolation between sequential requests, suffix/open-ended ranges, Content-Range header accuracy, sequential range requests across a full file, and large-file range content preservation.
