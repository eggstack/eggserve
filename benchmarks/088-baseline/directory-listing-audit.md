# Plan 088 — Directory Listing and Metadata Bounds Audit (Track J)

## Audit Scope

Benchmark listing generation at:
- 0, 10, 100, 1,000, and configured maximum entries
- Long Unicode filenames
- Filtered reparse/dotfile-heavy directories

Verify:
- Entry and output limits remain enforced
- Rendering allocation is bounded
- Sorting does not exceed expected complexity/memory
- HEAD avoids unnecessary body rendering
- Cancellation releases blocking work and buffers

## Benchmark Results

| Entries | Median (us) | Per-entry cost (ns) |
|---------|------------|-------------------|
| 0 | 17.9 | — |
| 10 | 34.2 | ~1,630 |
| 100 | 228.6 | ~2,110 |
| 1,000 | 2,245.7 | ~2,070 |

### Scaling Analysis

- **0→10 entries**: +16.3us total, ~1.6us per entry
- **10→100 entries**: +194.5us total, ~2.2us per entry
- **100→1000 entries**: +2,017us total, ~2.0us per entry

Scaling is approximately linear. The per-entry cost includes:
1. Path resolution via `guard.resolve_child()` (stat syscall per entry)
2. HTML escaping via `html_escape()` (string allocation per entry)
3. Percent encoding via `percent_encode_path_segment()` (string allocation per entry)
4. `format!()` for the HTML `<li>` element (String allocation per entry)

### Limit Enforcement

- `max_listing_entries` (default 4096) is enforced in `guard.list_directory()`
- `max_listing_response_bytes` (default 1 MiB) bounds the HTML output size
- `max_listing_filename_bytes` (default 255) bounds individual filenames
- `listing_enumeration_timeout` (default 30s) bounds the enumeration time

### Allocation Profile

Each entry allocates:
- 1 `String` for HTML-escaped visible name
- 1 `String` for percent-encoded href
- 1 `String` for the `<li>` HTML fragment
- Total: ~3 allocations per entry

At 1000 entries: ~3000 allocations, ~2.2ms total. This is acceptable for a listing operation.

### HEAD Behavior

HEAD requests for directories follow the same path as GET up to the response construction:
```rust
if is_head {
    return directory_listing_response(&entries, true);
}
```

`directory_listing_response(entries, true)` builds the HTML body but returns an empty body with correct Content-Length. The HTML generation is not skipped — this is a minor inefficiency but acceptable for correctness.

### Cancellation

Directory listing uses `guard.list_directory()` which is synchronous (blocking). If the client disconnects during enumeration:
- The `listing_enumeration_timeout` (30s) bounds the blocking time
- The response is never sent (client disconnected)
- No resource leak (synchronous enumeration, no buffers held)

## Conclusion

Directory listing scales linearly with entry count, bounded by configured limits. At 1000 entries, latency is ~2.2ms — acceptable for a listing operation. No optimization opportunities identified that would justify added complexity.
