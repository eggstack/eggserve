# Plan 229 — Static/file response streaming optimization

## Prerequisite

Plan 227 must identify the current file-stream cost profile and provide a
baseline for chunk-count, CPU, latency, RSS, and throughput.

## Purpose

Improve static-file throughput and CPU efficiency while preserving EggServe's
resolver-opened capability model, bounded file-stream admission, exact range
semantics, and backpressure.

The current direct adapter reads file bodies in chunks with:

```rust
let mut buffer = vec![0; chunk_len];
read_file_chunk(&mut file, &mut buffer).await;
Frame::data(Bytes::from(buffer))
```

The default chunk size remains 8 KiB. Each emitted chunk therefore owns a fresh
allocation, is zero-initialized before the file read, crosses Tokio file I/O,
and becomes a Hyper body frame. The old Plan 088 evidence already showed
chunk-count scaling; Plan 227 must establish the current magnitude.

## Goals

- Select an evidence-backed default file-stream chunk regime.
- Avoid zero-initializing bytes that are immediately filled by file I/O.
- Keep one owned immutable byte allocation per emitted frame unless a safe,
  measured design proves a better ownership model.
- Preserve exact EOF/range/error behavior.
- Remove avoidable static-serving request/metadata allocations and repeated
  header scans.
- Keep memory bounded by explicit stream admission and chunk-size limits.

## Work

### 1. Chunk-size matrix

Benchmark candidate chunk sizes at minimum:

- 8 KiB (current baseline);
- 16 KiB;
- 32 KiB;
- 64 KiB;
- 128 KiB;
- 256 KiB.

Test at least:

- 1 KiB file;
- 128 KiB file;
- 1 MiB file;
- 16 MiB file for sustained throughput/RSS;
- representative byte-range responses;
- concurrency 1, 16, 64 where the host can drive them without client
  saturation;
- plaintext and one established TLS case.

Do not choose a new default solely from maximum sequential throughput. Consider
CPU, p99 latency, per-stream memory, fairness under concurrency, and TLS write
behavior.

The chosen default must be documented with its memory implication:

```text
maximum resident chunk payload ~= max_active_file_streams * chosen_chunk_size
```

plus allocator/runtime overhead. This is a bound on simultaneously owned
application chunks, not a claim about total process RSS.

### 2. Replace zero-filled chunk construction

Implement a safe uninitialized-capacity read path using `BytesMut` /
`AsyncReadExt::read_buf` or an equivalent safe API.

Requirements:

- never expose uninitialized bytes;
- never read beyond the remaining response/range length;
- continue reading after short reads until the requested chunk is full or EOF;
- preserve `UnexpectedEof` when the opened file ends before the advertised
  representation length;
- freeze/move the filled buffer into `Bytes` without copying;
- keep cancellation/backpressure pull-driven.

Do not use unsafe `Vec::set_len` solely for this optimization.

### 3. Do not add a buffer pool by default

Because emitted `Bytes` owns the chunk until downstream releases it, a useful
pool requires lifecycle/reclamation complexity. Do not introduce a global or
per-runtime buffer pool in this plan unless Plan 227/229 measurements prove
allocation itself remains a dominant cost after chunk-size and zero-fill
changes.

If a pool is still proposed, stop and write a separate design plan covering
memory caps, cancellation, poisoning/stale-byte safety, TLS interaction, and
cross-runtime isolation.

### 4. Preserve range behavior

Verify full and range file bodies:

- perform at most the existing required seek behavior;
- never reopen a resolved path by pathname;
- never buffer the whole representation;
- enforce exact range length;
- handle concurrent truncate/replace behavior according to the existing
  opened-handle contract;
- release file-stream permits on EOF, error, cancellation, HEAD suppression,
  and client disconnect.

### 5. Static request header lookup cleanup

`StaticService::file_response` currently queries several conditional/range
headers independently through the ordered `HeaderBlock`.

Replace repeated whole-block scans with a single bounded pass that captures the
first values needed for:

- `If-Match`;
- `If-Unmodified-Since`;
- `If-None-Match`;
- `If-Modified-Since`;
- `Range`;
- `If-Range`.

Preserve duplicate-header semantics and all existing planner behavior. Do not
convert the canonical ordered duplicate-preserving representation into a
`HashMap`.

### 6. MIME lookup fast path

Avoid allocating `to_ascii_lowercase()` for the common already-lowercase
extension case.

Requirements:

- lowercase common extensions remain allocation-free;
- mixed/uppercase extensions remain case-insensitive;
- unknown extensions still use the configured/default octet-stream behavior;
- do not add libmagic or a MIME dependency.

### 7. Header construction/moves

Profile and, where straightforward:

- use `HeaderBlock::with_capacity` when the planned header count is known;
- consume/move static response-plan header values into the canonical response
  rather than cloning them when ownership permits;
- reserve directory-listing output capacity from bounded entry/name-size
  information where it avoids repeated growth.

Do not weaken canonical validation or bypass the canonical response builder for
micro-optimizations.

## Measurement requirements

For each candidate retained change, compare against Plan 227 using identical
build/profile/runtime limits.

Record:

- response throughput and bytes/sec;
- CPU;
- p50/p95/p99;
- RSS at 1/16/64 active streams;
- chunk/frame count;
- allocation count/bytes where available;
- file-stream rejection/recovery behavior.

The chosen chunk default should show a reproducible advantage on medium/large
file workloads without materially worsening 1 KiB latency, high-concurrency
tail latency, or bounded memory.

## Non-goals

- No `sendfile`, `splice`, io_uring, mmap whole-file cache, or platform-specific
  raw-socket bypass.
- No file-descriptor/path cache.
- No weakening of secure-root/capability semantics.
- No unbounded read-ahead.
- No whole-response buffering.
- No new MIME/database dependency.

## Acceptance criteria

- [ ] Chunk-size selection is evidence-backed rather than guessed.
- [ ] File buffers no longer pay avoidable zero-initialization when the safe
      implementation is measurably useful.
- [ ] Full/range/EOF/truncate/cancellation behavior remains correct.
- [ ] File-stream memory remains explicitly bounded.
- [ ] Conditional/range header extraction performs one bounded pass.
- [ ] Common lowercase MIME lookup avoids a temporary lowercase allocation.
- [ ] No canonical validation or confinement boundary is bypassed.
- [ ] A/B performance and resource evidence is recorded.
- [ ] Static authority conformance and direct-vs-compatibility tests remain
      green.
