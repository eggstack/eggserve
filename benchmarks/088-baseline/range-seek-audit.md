# Plan 088 — Range and Seek Efficiency Audit (Track F)

## Audit Scope

Verify that range streaming:
1. Requires exactly one seek
2. Reads bounded data that never passes the range end
3. Does not buffer the full file
4. Does not reopen metadata
5. Handles short reads correctly
6. Supports cancellation
7. Has Unix/Windows parity

## Code Path Analysis

### Range request flow

1. `handle_request` → validates method, parses path, resolves file
2. `plan_file_response` → evaluates Range header → `RangeRequestOutcome`
3. `file.into_body(&plan)` → returns `BodySource::FileRange { file, range, .. }`
4. `body_source_to_response` → calls `file_response_range()`
5. `file_response_range()`:
   - **One seek**: `file.seek(SeekFrom::Start(start)).await` — exactly one syscall
   - **Bounded reads**: `remaining` counter tracks bytes left; `remaining.min(DEFAULT_CHUNK_SIZE)` caps buffer size
   - **No full-file buffering**: each chunk is read independently; `remaining` decrements
   - **No metadata reopen**: file handle is reused from resolution
   - **Short reads**: `Ok(n)` path applies `let n = (n as u64).min(remaining) as usize;` — correct
   - **Cancellation**: `OwnedSemaphorePermit` is held in the stream state; dropped when stream is dropped → releases file stream slot

### Seek cost

- Single `lseek(2)` syscall per range request
- Cost is O(1) regardless of file size or seek position
- Verified by benchmark: `1m_suffix` (seek to end) is ~same as `1m_first_8k` (seek to start)

### Read bound enforcement

```rust
let mut buf = vec![0u8; remaining.min(DEFAULT_CHUNK_SIZE as u64) as usize];
// ...
let n = (n as u64).min(remaining) as usize;
let remaining = remaining.saturating_sub(n as u64);
```

- Buffer size is capped at `remaining` (never allocates more than needed)
- `remaining` is decremented by exact bytes read
- Stream terminates when `remaining == 0`
- **No over-read possible**

### Short read handling

- `read()` may return fewer bytes than requested (normal for disk I/O)
- The code applies `min(remaining)` to both the truncation and the remaining counter
- If `read()` returns 0 (EOF) before `remaining` reaches 0, the stream terminates early
- This is correct behavior for truncated files

### Cancellation behavior

- The stream holds an `OwnedSemaphorePermit` in its state tuple
- When the stream is dropped (client disconnect, timeout, or explicit drop), the permit is dropped
- The permit drop releases one slot in the `file_stream_semaphore`
- **No resource leak**: file handle is also dropped with the stream

### Unix/Windows parity

- Both platforms use `tokio::fs::File::from_std(std::fs::File)`
- Seek and read operations are identical across platforms
- Windows uses handle-relative opens (Plan 084/085) but the streaming code path is platform-agnostic
- The `OwnedSemaphorePermit` mechanism is platform-independent

## Benchmark Evidence

| Workload | Median (us) | Notes |
|----------|------------|-------|
| 16k first byte (range) | 26.98 | Seek + 1 chunk |
| 16k chunk cross (range) | 26.30 | Seek + 1 chunk spanning 8K boundary |
| 16k full (range) | 27.73 | Seek + 2 chunks |
| 1m first 8k (range) | 27.69 | Seek + 1 chunk |
| 1m suffix (range) | 26.32 | Seek to end + 1 chunk |
| 1m last 8k (range) | 27.85 | Seek to end + 1 chunk |

Range request overhead vs non-range: ~14us (seek + range header parsing + Content-Range computation). Seek position does not affect latency.

## Conclusion

Range streaming is correct and efficient:
- Exactly one seek per range request
- Bounded reads that never exceed the range end
- No full-file buffering
- Short reads handled correctly
- Cancellation releases resources
- Platform-independent code path

No optimization opportunities identified in the range streaming path.
