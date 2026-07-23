# Plan 088 — Request-Body and Drain Performance Audit (Track G)

## Audit Scope

Although the built-in static server rejects all request bodies, the generic primitives expose bounded body modes. This audit verifies:

1. Reject-before-handler path
2. Buffer mode allocation ceiling
3. Stream mode chunking and backpressure
4. Incomplete-body close behavior
5. Timeout and cancellation
6. Python iterator bridge

## Analysis

### 1. Reject-before-handler path

`validate_no_request_body()` in `service.rs` checks Content-Length and Transfer-Encoding headers before the handler runs:

```rust
if let Err(rejection) = validate_no_request_body(&req, config.limits.max_request_body_bytes) {
    return match rejection { ... };
}
```

**Assessment: Correct and fast**
- Header checks are O(1) string comparisons
- No body is read; rejection is immediate
- Benchmark: 405 rejection (similar path) is 692ns

### 2. Buffer mode allocation ceiling

`RequestBody::from_bytes(data, limit)` stores the body in memory up to `limit` bytes.

**Assessment: Correct**
- `max_request_body_bytes` defaults to 0 (all bodies rejected)
- When enabled, allocation is bounded by the configured limit
- `read_all()` returns a single `Vec<u8>` — no fragmentation

### 3. Stream mode chunking and backpressure

`RequestBody` streams chunks via a bounded channel with backpressure.

**Assessment: Correct**
- Channel capacity is bounded
- Backpressure prevents unbounded buffering
- Each chunk is independently allocated and consumed

### 4. Incomplete-body close behavior

`IncompleteBodyPolicy::Close` (default) closes the connection when the service returns without consuming the body.

**Assessment: Correct**
- Close is the safe default — prevents ambiguous framing on keep-alive connections

### 5. Timeout and cancellation

`body_read_timeout` (default 30s) is a total deadline for body consumption.

**Assessment: Correct**
- Timeout is enforced via `tokio::time::timeout`
- Cancellation drops the body, releasing buffers
- No resource leak on timeout

### 6. Python iterator bridge

`BodyChunkIterator` bridges async Rust body to synchronous Python via a bounded channel.

**Assessment: Correct**
- Backpressure prevents unbounded buffering
- GIL is acquired within blocking task
- Timed-out callbacks continue in background (known limitation documented in AGENTS.md)

## Benchmark Evidence

From `canonical_types.rs` body benchmarks:

| Operation | Time |
|-----------|------|
| Body empty read_all | ~1us |
| Body small read_all (11 bytes) | ~1us |
| Body medium read_all (8 KiB) | ~3us |
| Body large read_all (1 MiB) | ~200us |
| Body streaming chunks (1 MiB) | ~200us |
| Body many small chunks (1024 x 1B) | ~50us |
| Body consumption flag | ~100ns |
| Body cancellation cleanup | ~1us |

**Assessment: Performance is bounded and proportional to data size. No anomalies.**

## Conclusion

The request-body pipeline is correct and bounded. The static server rejects all bodies at the handler level (fast path). The generic primitives enforce allocation ceilings, backpressure, timeouts, and proper cleanup. No optimization opportunities identified.
