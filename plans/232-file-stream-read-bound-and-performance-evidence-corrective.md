# Plan 232 — File-stream read-bound and performance-evidence corrective

## Status

**LOCAL VERIFIED / REMOTE CLOSURE PENDING.**

## Purpose

Close two narrow issues discovered during review of the completed Plans
227–231 performance campaign without reopening the broader optimization work.

The first issue is a correctness problem in the new safe file-read path.
`file_body()` computes the logical response chunk length as:

```rust
let chunk_len = remaining.min(stream_chunk_size as u64) as usize;
```

but `read_file_chunk()` currently derives its read target from
`BytesMut::capacity()`. `BytesMut::with_capacity(n)` guarantees capacity
for *at least* `n` bytes; allocator-visible capacity is not a protocol or
response-length authority. The implementation must therefore never use
capacity as the logical number of bytes to consume from the opened file.

This matters most for range and final partial-chunk responses. If the backing
allocation exposes capacity greater than `chunk_len`, the current loop is
allowed to read beyond the representation/range boundary before the caller
subtracts `bytes_read` from `remaining`. Current allocator behavior may
hide the problem, but the invariant is invalid.

The second issue is evidence closure. Plan 231 correctly established large
static-serving gains and green deterministic/resource tests, but the captured
Plan-170-compatible candidate run used `--skip-tls`; installed-wheel Python,
admission saturation, CPython substitution, and TLS sections in that raw
artifact are explicitly marked `not-run`. Routine CI covers Python and
supply-chain correctness, but the performance campaign specifically requested
representative TLS performance evidence. In addition, the new 128 KiB default
substantially increases concurrent file-stream residency compared with the
former 8 KiB default and should receive one direct 64 KiB versus 128 KiB
network/resource comparison before being treated as the final default.

This plan is a narrow corrective and evidence pass. It does not authorize
another general optimization campaign.

## Findings motivating the plan

At Plan 231 closure:

- baseline SHA: `4fda8a3872684a0f5d1a6ca80de30c452f135080`;
- optimized candidate SHA:
  `bdb247907a2e565e80ecb0c623d630a53264cbf2`;
- closure/documentation SHA:
  `23df18440e9ffeb023b5104492e08161f8bc9bcc`;
- direct H1/static/custom deterministic qualification passed;
- remote CI on `23df184` passed Rust, Python, and supply-chain jobs;
- 128 KiB became the default `stream_chunk_size`;
- native static 128 KiB and 1 MiB throughput improved materially;
- small buffered and application-stream paths remained broadly neutral.

The 128 KiB configuration also raised observed peak RSS under the native
static qualification at high concurrency. That does not establish a leak—the
streaming memory remains bounded—but it makes the 64 KiB versus 128 KiB
tradeoff worth resolving explicitly.

## Goals

1. Make the logical file-response chunk length explicit and independent of
   allocator capacity.
2. Prove full-file and range bodies cannot read, emit, or account beyond the
   current representation boundary even when the backing buffer has excess
   capacity.
3. Preserve the Plan 229 no-zero-fill, no-unsafe, no-buffer-pool design.
4. Re-run a focused live-network comparison of 64 KiB and 128 KiB file chunks
   with throughput, tail latency, and RSS/resource evidence.
5. Complete the missing representative TLS performance evidence.
6. Keep 128 KiB only if the live evidence justifies its extra per-stream
   residency; otherwise select 64 KiB and update all current documentation.
7. Correct the Plan 231 closure record so its claims match the evidence
   actually captured.
8. Preserve all Plan 227–231 security, framing, timeout, and crate-ownership
   invariants.

## Non-goals

Do not add or investigate in this plan:

- `sendfile`, `splice`, TransmitFile, mmap, or io_uring;
- a reusable/global buffer pool;
- unsafe spare-capacity manipulation solely for performance;
- descriptor/path caches;
- custom allocators;
- new benchmark dependencies;
- public `Service` API changes;
- H2/H3 support-tier changes;
- new Python server behavior;
- unrelated dependency upgrades;
- additional crate extraction.

## Work

### 1. Correct the file-read authority

Refactor `crates/eggserve-server/src/adapters.rs` so
`read_file_chunk()` receives the intended logical target length explicitly.

The implementation must satisfy:

```text
logical bytes requested = min(remaining representation bytes,
                              configured stream_chunk_size)
```

and that value—not `BytesMut::capacity()`—must be the read authority.

A preferred safe shape is:

- construct/reserve a `BytesMut` with enough capacity;
- pass `chunk_len` explicitly into the helper;
- place an explicit async-read limit of `chunk_len` around the file read
  (for example an `AsyncReadExt::take(chunk_len as u64)` view), or otherwise
  present only the intended remaining target to `read_buf`;
- loop through short reads until exactly `chunk_len` bytes have been read or
  EOF/error occurs;
- freeze only the initialized bytes.

Equivalent safe implementations are acceptable, but the helper must not infer
the target from allocator capacity or spare capacity.

Do not reintroduce `vec![0; chunk_len]` merely to make the bound convenient.
Do not use `unsafe { set_len(...) }` or custom uninitialized-buffer code for
this correction.

### 2. Add an over-capacity regression that cannot pass accidentally

Add a deterministic unit-level test for the helper/read primitive that creates
a buffer whose actual capacity is intentionally greater than the logical
target.

The fixture must contain:

```text
[target representation bytes][sentinel bytes beyond target]
```

and prove all of the following:

- the helper returns exactly the requested logical target;
- the emitted/initialized prefix contains exactly the target bytes;
- sentinel bytes are not consumed into the response buffer;
- the file cursor advances by exactly the requested target, not by capacity;
- a second read can still observe the sentinel/remainder.

Do not rely on `BytesMut::with_capacity(target)` happening to over-allocate.
Force excess capacity explicitly so the regression tests the contract.

### 3. Add full/range wire regressions

Add deterministic coverage for the complete `file_body` path:

- full file smaller than `stream_chunk_size`;
- full file with a non-multiple final chunk;
- range shorter than one chunk;
- range ending in the middle of a larger underlying file;
- range length not divisible by the chunk size;
- a range followed by sentinel bytes in the underlying file;
- concurrent truncate/short-read behavior remains the documented
  `UnexpectedEof`/connection termination path;
- cancellation/drop still releases the file-stream permit.

For every successful case assert:

- exact byte count;
- exact bytes;
- exact frame total where deterministic;
- no bytes beyond the advertised full/range representation.

Include at least one test using a deliberately over-capacity internal buffer
path if practical; otherwise keep that proof at the helper level and the wire
tests at the public adapter boundary.

### 4. Reconfirm range and framing authority

Audit the corrected flow end-to-end:

```text
ResolvedFile/BodySource
 -> range/full representation length
 -> chunk_len
 -> bounded read
 -> Bytes frame
 -> Hyper body
 -> runtime framing/content-length enforcement
```

Document that:

- secure-root resolution is unchanged;
- the opened capability is never reopened by path;
- range start/length remain planner-owned;
- `Content-Length` remains runtime-owned;
- overrun cannot be converted into a second HTTP error after commitment;
- no whole-file buffering or read-ahead beyond the selected chunk occurs.

### 5. Run a 64 KiB versus 128 KiB live-network matrix

Use the Plan 227 native Rust client/harness and the same release profile on one
machine/session. Compare builds/configurations that differ only in
`stream_chunk_size`.

At minimum test:

**Plaintext static H1**
- 1 KiB at concurrency 1 and 16;
- 128 KiB at concurrency 1, 16, 64;
- 1 MiB at concurrency 1, 16, 64;
- 16 MiB at concurrency 16 and, if the client remains stable, 64.

**Range**
- representative 64 KiB and 512 KiB ranges from a larger file at concurrency
  1/16/64 where practical.

Record for each case:

- median RPS / bytes per second;
- p50/p95/p99 latency;
- peak and post-run RSS;
- fd/thread counts where available;
- errors/timeouts/truncations;
- file-stream admission/recovery counters where applicable.

Use at least three measured trials after warm-up and alternate 64 KiB / 128 KiB
ordering where practical.

### 6. Decide the default from throughput *and* bounded memory

Treat the default as a configuration decision, not a benchmark trophy.

Nominal maximum simultaneous application chunk payload at the default
`max_file_streams = 32` is approximately:

```text
64 KiB  -> 2 MiB
128 KiB -> 4 MiB
```

before allocator/runtime/TLS overhead.

Decision rule:

- retain **128 KiB** only if it shows a consistent, material live-network
  advantage on medium/large static workloads without a disproportionate p99,
  RSS, or concurrency penalty;
- prefer **64 KiB** if 128 KiB's throughput advantage is small/noisy while
  memory/tail-latency cost is visibly higher;
- do not reduce below 64 KiB unless the focused evidence unexpectedly
  demonstrates a correctness/resource reason.

Record the decision and reasoning in machine-readable evidence plus a short
human-readable note. Do not use one arbitrary percentage cutoff; consider the
whole workload/resource profile.

If the default changes, update the single runtime-limit authority and every
live document/test that names the default. Do not edit historical benchmark
artifacts to make old runs appear to use the new value.

### 7. Complete representative TLS performance evidence

Run the Plan 170/231 TLS benchmark path with an ephemeral/local qualification
certificate and the current rustls stack.

Capture separately:

- established keep-alive static 1 KiB;
- established keep-alive static 1 MiB;
- at least concurrency 1 and 16, plus 64 where stable;
- TLS handshake churn as a separate workload.

Run the selected final chunk default. Where useful for deciding 64 versus
128 KiB, include a smaller focused TLS comparison for the 1 MiB case.

Record:

- exact certificate-generation command or fixture;
- build profile/features;
- TLS protocol/cipher information available from the harness;
- throughput and latency;
- errors;
- RSS/CPU where available.

Do not turn these numbers into edge-server or TLS-stack superiority claims.

### 8. Reconcile the Plan 231 evidence record

Do not rewrite or delete the original Plan 231 raw evidence.

Add a new directory:

```text
benchmarks/232-corrective/
  README.md
  results.json
  decisions.md
  raw/
```

The corrective record must state explicitly:

- the `BytesMut::capacity()` bug and fixed SHA;
- which Plan 231 performance sections were previously not run;
- the 64 KiB versus 128 KiB decision;
- the completed TLS measurements;
- whether installed-wheel Python, CPython substitution, and admission
  saturation remain intentionally inherited from Plan 170 / correctness CI
  rather than rerun.

Only rerun the latter three if they are needed to validate a code path touched
by this correction. Do not manufacture unnecessary benchmark work.

Update:

- `benchmarks/README.md`;
- `architecture/testing-and-conformance.md`;
- `architecture/response-planning.md`;
- `architecture/configuration.md` if the default changes;
- `architecture/runtime.md` if wording about the final chunk regime changes;
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` only for durable
  final facts;
- `plans/ROADMAP.md`.

Append/cross-reference the Plan 232 correction from Plan 231 or its closure
README rather than pretending the original closure had complete TLS evidence.

### 9. Correctness and security verification

At minimum run:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py

cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked

bash scripts/check-supply-chain.sh
```

Also run the focused suites covering:

- static authority/confinement parity;
- range/conditional response behavior;
- response streaming;
- direct H1 parity;
- production controls/file-stream permit recovery;
- TLS static serving.

Because this plan changes response-body reading, a green normal unit test run
alone is insufficient; the new over-capacity regression is mandatory.

### 10. Remote closure gate

After the corrective implementation/evidence commit is pushed, require a
successful GitHub Actions run for the exact closing SHA.

At minimum the current:

- Rust;
- Python;
- supply-chain

jobs must pass.

Record the exact SHA and workflow-run URL/ID in the Plan 232 evidence README or
a release evidence note.

## Security and architecture invariants

Plan 232 must preserve:

- `eggserve-static` as the sole static/path/filesystem semantics authority;
- `eggserve-server` as the file-body/transport adapter authority;
- descriptor/handle-relative confinement;
- no pathname reopen after secure resolution;
- no unsafe Rust introduced for buffer management;
- no whole-file buffering;
- bounded `max_file_streams * stream_chunk_size` residency;
- pull-driven backpressure;
- runtime-owned response framing;
- direct H1 write-stall semantics based on actual socket progress;
- no H2/H3 support-tier changes;
- no new production dependency.

## Rollback

If the safe bounded `BytesMut` implementation cannot be expressed cleanly,
prefer a simpler bounded implementation—even if it gives up a small part of
the Plan 229 zero-fill win—over retaining an ambiguous response-length
authority.

If 128 KiB does not clearly justify its resource cost, change the default to
64 KiB rather than keeping 128 KiB for historical consistency.

If TLS or live-network measurements are too noisy to distinguish 64 KiB from
128 KiB, choose the lower-memory 64 KiB default and document the evidence as
inconclusive rather than selecting the larger buffer from an in-process
microbenchmark alone.

## Acceptance criteria

Plan 232 is complete only when:

- [x] no file-body read target is derived from `BytesMut::capacity()`;
- [x] the helper has an explicit logical byte target;
- [x] a forced-over-capacity regression proves no read past that target;
- [x] full and range responses emit exactly the advertised bytes;
- [x] short-read/truncate/cancellation/permit-release behavior remains correct;
- [x] 64 KiB and 128 KiB have same-machine live-network throughput,
      tail-latency, and RSS evidence;
- [x] the final default has an explicit throughput/resource rationale;
- [x] representative established TLS and handshake-churn performance evidence
      exists for the corrected implementation;
- [x] Plan 231's evidence limitations are truthfully cross-referenced;
- [x] no new production dependency, unsafe buffer code, buffer pool, or
      filesystem shortcut is introduced;
- [x] routine and feature-gated verification passes;
- [ ] remote Rust/Python/supply-chain CI succeeds for the exact closing SHA;
- [x] roadmap and current architecture/configuration docs match the final
      implementation.

## Handoff order

Implement in this order:

1. correctness fix + forced-over-capacity regression;
2. focused full/range/short-read/cancellation tests;
3. 64 KiB versus 128 KiB plaintext network/resource matrix;
4. select/finalize the default;
5. TLS evidence at the selected default;
6. full verification;
7. evidence/doc reconciliation;
8. push the closing commit and record successful remote CI.

Do not begin another optimization track from findings discovered here. Any new
bottleneck requiring a qualitatively different mechanism gets its own future
plan.
