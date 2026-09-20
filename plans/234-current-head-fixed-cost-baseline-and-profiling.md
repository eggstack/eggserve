# Plan 234 — Current-HEAD fixed-cost baseline and profiling

## Purpose

Establish the post-Plan-233 baseline for the remaining fixed per-request costs
before changing production code. Plans 227–233 already qualified the larger
file-stream and H1 bookkeeping changes; this plan must not repeat that campaign
or infer new wins from its old measurements.

This plan is measurement-only except for benchmark-only instrumentation.

## Questions to answer

Before Plans 235–239 implement anything, determine:

1. How many heap allocations are attributable to canonical request-target,
   empty/fixed request-body, trailer-slot, interim-sender, and request-context
   construction on a bodyless H1 request?
2. Does hardened Unix static resolution still perform one root descriptor
   duplication per non-root request, and is it visible in syscall counts?
3. What fraction of static-path planning time/allocation is percent decoding,
   normalization, component ownership, and range/header parsing?
4. Is the remaining `CanonicalHyperService` `Arc<dyn Fn> -> Pin<Box<...>>`
   layer observable in small custom-service workloads?
5. How much per-request copying comes from TLS/connection metadata on
   established keep-alive TLS connections?
6. Does the ordinary zero-tunnel H1 path acquire the tunnel `JoinSet` mutex
   often enough to matter?
7. How many objects/bytes are materialized by the Python request bridge before
   a minimal handler reads any optional compatibility property?
8. Under 10/100/N slow Python response streams, how do thread count, RSS,
   latency, shutdown time, and cancellation behave?

## Evidence layout

Create:

```text
benchmarks/234-fixed-cost-baseline/
  README.md
  results.json
  raw/
  profiles/
```

Retain compact per-trial JSON in accordance with Plans 232–233. Record the
exact source SHA, lockfile hashes, compiler, profile/features, machine, runtime
limits, warm-up, trial count, and errors.

## Work

### 1. Reuse the native Plan-227/231 load harness

Measure at minimum:

- custom 1 KiB bytes response, concurrency 1/16/64;
- static 1 KiB, concurrency 1/16/64;
- static 128 KiB and 1 MiB representative points to ensure later fixed-cost
  changes do not regress the bulk path;
- HEAD and 304;
- short path, nested path, query-bearing path, percent-encoded path;
- established TLS 1 KiB keep-alive;
- caller-owned H1 smoke to keep the direct embedding path represented.

Do not replace the existing native harness with CPython-only timing.

### 2. Allocation characterization

Use an available allocation profiler/counter, heap profiler, or reproducible
instrumented allocator in benchmark-only code. If host restrictions prevent
one method, use another supported method and document the limitation.

At minimum isolate:

- `RequestTarget::parse` with and without query;
- `RequestBody::empty`;
- `RequestBody::from_bytes`;
- runtime request-context/interim construction;
- Hyper-to-canonical request conversion with typical header sets;
- static path confinement for normalized and encoded paths;
- Python request construction for a minimal no-body request.

Record logical allocation counts alongside measured counts when the source
makes an allocation mechanically provable.

### 3. Unix syscall capture

For hardened static serving on Linux, capture per-request or normalized syscall
counts for:

- root file request;
- one-component file;
- nested file;
- directory redirect/listing/index path where supported by current policy.

Pay particular attention to `fcntl`/descriptor duplication, `statat`,
`openat`, metadata/fstat, reads, and closes. The purpose is to prove which
syscalls are security/identity work and which are ownership artifacts.

### 4. H1 dispatch/profile capture

Profile custom 1 KiB keep-alive long enough to distinguish:

- Hyper parsing;
- canonical request conversion;
- `CanonicalHyperService` dynamic dispatch/future boxing;
- service invocation;
- response normalization/finalization;
- activity/tunnel-state checks.

If symbols cannot be resolved sufficiently, add benchmark-only counters rather
than altering production behavior.

### 5. Established-TLS metadata test

Use a long-lived TLS connection so handshake cost is excluded. Compare a
minimal TLS metadata configuration with the opt-in bounded peer-certificate
exposure path where feasible. Record allocations/CPU attributable to
per-request metadata cloning separately from rustls transport work.

### 6. Python callback/request construction

Use an installed wheel or equivalent extension build and measure:

- trivial handler returning empty/bytes response;
- handler reading only `method`;
- handler reading text headers;
- handler reading raw byte views;
- handler reading TLS/proxy/address metadata.

The goal is to identify which compatibility views are eagerly materialized even
when untouched.

### 7. Python slow-stream resource matrix

Exercise synchronous `Response.stream` with slow readers at increasing active
stream counts. Record:

- process thread count;
- RSS;
- Python callback semaphore state;
- stream/channel backpressure;
- disconnect cleanup;
- server shutdown/drain behavior;
- errors/truncation.

Do not change the producer architecture in this plan.

### 8. Produce GO/NO-GO inputs

The Plan-234 report must explicitly decide whether evidence supports:

- Plan 235 request-target/body changes;
- Plan 236 root-FD/path changes;
- Plan 237 generic dispatch, tunnel fast path, and metadata sharing;
- Plan 238 request-state consolidation;
- Plan 239 lazy Python request views and any further stream-executor design.

A mechanically unnecessary allocation/syscall may receive GO even if aggregate
RPS is noisy, provided the proposed replacement is simpler or equivalently
simple and semantics are unchanged.

## Optional compiler/profile experiment

Build the same source with the normal release/benchmark profile and a
performance-oriented `opt-level=3` experiment. Record the delta separately.
Do not change `[profile.dist]` under this plan; distribution remains
size-oriented unless a later, separately justified decision changes it.

## Acceptance criteria

- [ ] Source/environment/profile provenance is retained.
- [ ] Native small-response and static baselines are reproducible.
- [ ] Allocation evidence covers request target/body/context and Python request
      construction.
- [ ] Linux static syscall evidence identifies root-FD duplication separately
      from security-required checks.
- [ ] Established TLS isolates metadata-copy cost from handshake cost.
- [ ] Python slow-stream thread/RSS/shutdown behavior is recorded.
- [ ] Plans 235–239 receive explicit GO/NO-GO inputs.
- [ ] No production optimization lands in this plan.
