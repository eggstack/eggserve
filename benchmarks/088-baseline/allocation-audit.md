# Plan 088 — Allocation Audit (Track C)

## Per-Path Allocation Classification

Each allocation in the static-file serving path is classified as:

- **Required** — necessary for ownership/lifetime correctness
- **Removable** — copy that can be eliminated
- **Bounded buffer** — reusable per-chunk allocation (bounded by chunk size)
- **Metadata noise** — cheap construction that is not on the critical path
- **Benchmark artifact** — allocation that exists only in measurement code

### 1. Full-file chunk creation (`response.rs:112`)

```rust
let mut buf = vec![0u8; DEFAULT_CHUNK_SIZE];
```

**Classification: Bounded buffer**
- Allocates 8 KiB per chunk read
- Ownership transfers to `Bytes::from(buf)` → `Frame::data()` → body stream
- Dropped after each chunk is flushed to the transport
- Cannot be pooled without adding unsafe code; fresh allocation is correct and bounded
- **No stale-byte risk**: each buffer is independently allocated and consumed

### 2. Range chunk creation (`response.rs:233`)

```rust
let mut buf = vec![0u8; remaining.min(DEFAULT_CHUNK_SIZE as u64) as usize];
```

**Classification: Bounded buffer**
- Same as full-file but capped at `remaining` bytes
- Final chunk is smaller than `DEFAULT_CHUNK_SIZE`
- Correct and bounded

### 3. `Bytes::from(buf)` conversion (`response.rs:118, 241`)

```rust
Frame::data(Bytes::from(buf))
```

**Classification: Required**
- `Bytes::from(Vec<u8>)` does not copy; it takes ownership of the Vec's allocation
- Zero-copy conversion from owned buffer to reference-counted bytes
- Required for the `Frame<Bytes>` type used by `StreamBody`

### 4. `HeaderBlock` construction in `file_response` (`response.rs:82-96`)

```rust
let mut headers = HeaderBlock::new();
headers.push_str("content-type", mime).unwrap();
// ... 5-7 headers total
```

**Classification: Metadata noise**
- Constructs 5-7 header fields per request
- Each `push_str` allocates a `String` for name and value
- Required for response correctness; not avoidable without pre-allocating header templates
- Cost: ~6 allocations per file response

### 5. `normalize_metadata` header filtering (`canonical.rs:461-464`)

```rust
strip_hop_by_hop(headers);
remove_header(headers, "content-length");
```

**Classification: Required (optimized)**
- Uses `retain()` for in-place filtering — no clone or rebuild
- Linear scan of header list (typically <10 headers)
- Required to enforce hop-by-hop stripping and Content-Length correctness

### 6. ETag generation (`service.rs:315-321`)

```rust
Some(format!("W/\"{}-{}\"", size, mtime_secs))
```

**Classification: Metadata noise**
- One `String` allocation per file response
- Required for conditional request support
- Could be pre-computed at resolve time (minor optimization)

### 7. MIME detection (`mime.rs`)

```rust
phf_map! { ... }
```

**Classification: Required**
- Compile-time perfect hash — O(1) lookup
- `to_ascii_lowercase()` allocates a new `String` per call
- Could be avoided by lowercasing in-place or using a stack buffer (minor)

### 8. Error body construction (`response.rs:24-41`)

```rust
fn canonical_error(status: StatusCode, body: &'static str) -> Response<BoxBodyInner> {
    // ...
    builder.body(full_body(body)).unwrap()
}
```

**Classification: Required**
- Body is `&'static str` → zero-allocation `Bytes::from(s)`
- Headers allocate via `HeaderBlock::push_str`
- Error responses are infrequent; optimization not warranted

### 9. Directory listing HTML generation (`response.rs:267-286`)

```rust
let mut html = String::from("<!DOCTYPE html>...");
for (name, is_dir) in entries {
    html.push_str(&format!(...));
}
```

**Classification: Required**
- HTML is generated in-memory, then converted to `Bytes::from(body_bytes)`
- `format!` per entry allocates a new `String`
- Could be improved with `write!` to a pre-allocated buffer (minor)
- Bounded by `max_listing_entries` (default 4096)

### 10. Python/Rust boundary copies (`server.rs`)

**Classification: Required (out of scope)**
- Python `Server` primitives cross the FFI boundary
- Body bytes are copied between Python and Rust heap
- Cannot be avoided without zero-copy FFI (not planned)

### 11. TLS vs plaintext paths

**Classification: N/A (no TLS benchmarks in this baseline)**
- TLS adds encryption overhead per write but no additional buffer allocations
- Rustls uses internal buffers; not measurable at the handler level

## Summary

| Path | Classification | Allocations per request | Avoidable? |
|------|---------------|------------------------|------------|
| Chunk buffer (8 KiB) | Bounded buffer | 1 per chunk | No (correct and bounded) |
| Bytes::from(buf) | Required | 0 (zero-copy) | No |
| HeaderBlock headers | Metadata noise | 5-7 strings | Minor (pre-allocate templates) |
| normalize_metadata | Required (optimized) | 0 (in-place retain) | No |
| ETag string | Metadata noise | 1 String | Minor (pre-compute) |
| MIME to_lowercase | Metadata noise | 1 String | Minor (stack buffer) |
| Error body | Required | 0 (static str) | No |
| Directory listing HTML | Required | N entries * format! | Minor (write! macro) |
| Python FFI | Required | body bytes copied | No |

## Conclusion

The allocation profile is dominated by per-chunk buffer creation (`vec![0u8; 8192]`), which is bounded and correct. The per-request overhead is ~7 string allocations (headers + ETag + MIME). No removable copies or stale-byte risks were found. The `normalize_metadata` retain optimization (Plan 088) eliminated the largest removable copy.
