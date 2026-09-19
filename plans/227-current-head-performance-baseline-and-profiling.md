# Plan 227 — Current-HEAD performance baseline and hot-path profiling

## Purpose

Create a trustworthy post-Plan-226 performance baseline before changing the
runtime. Plan 170 remains valuable historical evidence, but its final capture
(`afac949`, 2026-09-04) predates the direct-crate authority extraction,
connection-runtime movement, frontend migration, and Plan 226 release/MSRV
corrections.

This plan is measurement-only. Do not optimize production code here except for
benchmark-only instrumentation that cannot affect normal builds.

## Goals

- Measure current `main` rather than extrapolating from Plan 170.
- Detect client-side saturation in the existing CPython loopback harness.
- Isolate file-stream/body-frame cost from TCP/client cost.
- Profile the H1 small-response path, file-stream path, and application stream
  path sufficiently to decide Plans 228–230.
- Record CPU, allocation/resource, latency, throughput, and error evidence in a
  reproducible format.
- Preserve the existing benchmark claims policy: evidence, not a leaderboard.

## Required evidence layout

Create:

```text
benchmarks/227-current-head/
  README.md
  results.json
  raw/                     # compact raw trial summaries only
  profiles/                # compact profiler summaries, not huge captures
  <small reusable harness files>
```

Every machine-readable result must include:

- source commit SHA and both relevant lockfile identities where applicable;
- exact build command, profile, features, and Rust version;
- OS, architecture, CPU, logical CPUs, memory;
- runtime limits used by the server;
- workload, response/request size, concurrency, connection reuse;
- warm-up policy and number of measured trials;
- RPS/bytes-per-second when meaningful;
- p50/p95/p99 latency;
- process CPU time/utilization where available;
- RSS, fd, and thread/task counts where practical;
- errors, timeouts, parser/service/file-stream rejections;
- notes identifying likely client saturation.

## Work

### 1. Preserve Plan 170 compatibility

Run the existing Plan 170 closure harness against current HEAD for the workload
families that still map cleanly:

- native static H1: 1 KiB, 128 KiB, 1 MiB;
- concurrency 1, 16, 64, and one high-concurrency point;
- native custom 1 KiB bytes response;
- native known-length and unknown-length 1 MiB streams;
- representative established TLS and handshake churn;
- installed-wheel low-level Python smoke where practical.

Do not overwrite `benchmarks/170-closure/`. Record new results only under
Plan 227 and explicitly label Plan 170 values historical.

### 2. Add a native load path for small-response measurements

The Plan 170 CPython `http.client` driver clustered several native 1 KiB
workloads around roughly the same throughput. Treat that as a possible client
ceiling, not proof that the server paths are equivalent.

Add a small reproducible native client/harness that:

- speaks ordinary H1 keep-alive over loopback;
- can run at concurrency 1/16/64;
- validates response status, length, and body bytes;
- records latency distributions and errors;
- does not require adding a production dependency to EggServe;
- is simple enough to audit and preserve as benchmark infrastructure.

Prefer reuse of already-approved workspace crates or a standalone benchmark
utility over introducing a large load-testing dependency. An external tool may
be recorded as supplemental evidence, but the repository must retain at least
one reproducible native path.

### 3. Add in-process body/adapter microbenchmarks

Add targeted measurements that bypass the loopback client and exercise:

- canonical `ResponseBody::Bytes` conversion;
- a 1 MiB file body consumed through the current adapter;
- a 1 MiB known-length `ResponseStream`;
- a 16 MiB stream for bounded-memory evidence;
- file sizes/chunk regimes that expose per-frame/per-read scaling;
- a minimal static request planning/response-construction path.

Record frame/chunk counts along with elapsed CPU/wall time where feasible.

### 4. Profile the H1 response path

On Linux, collect profiler evidence for at least:

- 1 KiB static keep-alive;
- 1 MiB static keep-alive;
- 1 MiB application stream.

Use `perf` or existing platform-native tooling when available. Summaries should
identify time attributed to, at minimum:

- allocation/free;
- Tokio file-read/blocking machinery;
- Hyper body/frame polling;
- `ConnectionActivity` locking/notification;
- response normalization/finalization;
- header/date formatting;
- event construction/log sink dispatch.

Do not commit enormous `perf.data` files. Commit a compact command record plus
folded/top-symbol summary sufficient to reproduce the conclusion.

### 5. Allocation/resource characterization

Measure or count allocations in targeted microbenchmarks when the existing
tooling allows it. At minimum record logically provable allocation counts for:

- file chunks per representation;
- canonical header/value construction;
- request header-block construction;
- response event creation under no-op logging.

If allocator instrumentation is unavailable on the qualification host, state
that explicitly rather than inventing numbers.

### 6. Produce a decision report

The Plan 227 README/results notes must answer these questions before the later
plans begin:

1. Is per-frame `response_poll_progress` visible in H1 stream/file profiles?
2. Does the driver consume that producer-progress state for H1 timeout
   decisions, or is actual `ProgressIo::record_write` the only authority?
3. What file chunk-size range appears worth A/B testing?
4. Is zero-initializing each file chunk visible enough to optimize?
5. Is the native small-response path materially below the Python client
   ceiling?
6. Are per-request shared-state clones/boxing visible or merely theoretical?
7. Is duplicate `Date` formatting measurable?
8. Is disabled/filtered structured logging allocating meaningful work?
9. Which proposed changes should be dropped before implementation because
   evidence does not support them?

## Acceptance criteria

- [ ] A current-HEAD evidence directory exists with source/environment data.
- [ ] The existing Plan 170 families have a clearly labeled current comparison.
- [ ] At least one native, non-CPython small-response load path is reproducible.
- [ ] In-process body measurements isolate chunk/frame behavior.
- [ ] Linux profiler evidence exists for small static, large static, and
      application-stream paths, or the unavailable profiler is documented with
      an equivalent supported method.
- [ ] Client saturation is explicitly assessed.
- [ ] Plans 228–230 receive concrete go/no-go inputs from the measured data.
- [ ] No production optimization is smuggled into the baseline plan.
- [ ] Existing correctness/resource limits show zero unexpected errors or leaks
      during the baseline workloads.

## Handoff

Do not begin broad implementation from intuition alone. Plan 228, 229, or 230
may be narrowed or partially skipped if this baseline disproves its motivating
hypothesis.
