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
pub enum ResponseStatus {
    Ok,              // 200
    NotModified,     // 304
    PartialContent,  // 206
    RangeNotSatisfiable, // 416
    // ... error variants
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
    File { file: ResolvedFile, range: FileRange },
    DirectoryListing { html: String },
    Empty,
    Error { body: String },
}
```

### `FileRange`

```rust
pub enum FileRange {
    Full,
    Satisfiable { start: u64, end: u64 },
    NotSatisfiable,
}
```

## Planning Functions

### `plan_file_response()`

Main entry point for file responses:

```rust
pub fn plan_file_response(
    file: &ResolvedFile,
    method: &ReadOnlyMethod,
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

Strict ETag comparison. Supports weak validators (`W/"..."`) but prefers strong comparison.

### `evaluate_range_header()`

Parses `Range: bytes=START-END` header:

- Valid range within file bounds → `206 Partial Content`
- Range beyond file size → `416 Range Not Satisfiable`
- Multiple ranges → single range only (first range served)

### `evaluate_if_range()`

Validates `If-Range` validator against current ETag/Last-Modified. If mismatch → serve full response.

### `generate_etag()`

Generates ETag from file metadata:

```rust
pub fn generate_etag(metadata: &std::fs::Metadata) -> String
```

Format: `"<inode>-<size>-<mtime>"` (strong validator).

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

## See Also

- [primitives-api.md](primitives-api.md) — Public API for response planning
- [eggserve-core.md](eggserve-core.md) — Core library context
- [architecture/overview.md](overview.md) — Data flow diagram
