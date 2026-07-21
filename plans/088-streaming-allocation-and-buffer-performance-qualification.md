# Plan 088 — Streaming Allocation and Buffer Performance Qualification

## Goal

Measure and improve eggserve’s static-file streaming, range serving, request-body handling, and connection-loop allocation behavior without weakening HTTP correctness, filesystem confinement, backpressure, cancellation, timeout, or shutdown guarantees.

This is an evidence-driven optimization phase. It must not turn performance targets into claims of edge-server parity, introduce speculative complexity, or alter public behavior solely to improve a benchmark.

## Preconditions

- Plans 075–087 are closed.
- Runtime timeout and shutdown semantics are stable.
- Static file/index paths share the canonical response planner.
- Windows and Unix final files are streamed from validated opened objects.
- Operational counters/events from Plan 087 are available for benchmark and soak instrumentation.

## Non-goals

Do not add:

- HTTP/2, HTTP/3, sendfile platform sprawl, io_uring, kernel bypass, or custom TCP stacks;
- reverse proxy caching;
- application response streaming frameworks;
- compression negotiation or dynamic compression;
- memory mapping by default;
- a benchmark-only unsafe fast path;
- unbounded buffer pools;
- optimization claims against nginx/Caddy without a controlled, relevant methodology.

## Track A — Establish representative performance workloads

Define benchmark classes that match eggserve’s actual product:

### File sizes

- empty;
- 1 KiB;
- 16 KiB;
- 128 KiB;
- 1 MiB;
- 16 MiB;
- 256 MiB or larger where infrastructure permits.

### Request forms

- GET full file;
- HEAD;
- single byte range;
- medium range;
- near-full range;
- conditional 304;
- missing file;
- denied path;
- directory index;
- generated directory listing where enabled;
- keep-alive sequences;
- concurrent short-lived connections.

### Platforms

- Linux x86_64;
- macOS arm64;
- Windows x86_64;
- optional Linux aarch64 for release-target evidence.

### Frontends

- Rust embedded static server;
- CLI binary;
- installed Python wheel/subprocess path;
- Python in-process server where applicable.

Benchmarks must distinguish warm page-cache and cold-ish cache conditions. Do not imply disk throughput from warm-cache tests.

## Track B — Capture a reproducible baseline

Before changing implementation, record:

- source SHA;
- Rust toolchain and optimization flags;
- target CPU/OS;
- feature set, including TLS;
- file system;
- benchmark command and client tool;
- concurrency and connection reuse;
- file sizes and request mix;
- median, p95, and p99 latency;
- throughput;
- CPU time;
- peak and steady-state RSS/working set;
- allocation count/bytes where tooling supports it;
- syscalls or read/write counts where available;
- file descriptor/handle and task counts;
- error rate.

Use repeated runs and report variance. A single best run is not acceptable evidence.

## Track C — Audit response body allocation paths

Inspect:

- full-file chunk creation;
- range chunk creation;
- conversion between `Vec<u8>`, `Bytes`, and transport frames;
- canonical response normalization;
- MIME/path metadata creation;
- per-request header construction;
- error body construction;
- directory listing rendering;
- Python/Rust boundary copies;
- TLS versus plaintext paths.

Classify each allocation as:

- required by ownership/lifetime;
- removable copy;
- reusable bounded buffer;
- metadata-only noise;
- benchmark artifact.

Do not optimize until a profiler or allocation instrument demonstrates material cost in a representative workload.

## Track D — Introduce a bounded reusable streaming buffer strategy

If evidence shows per-chunk allocation is material, implement a bounded strategy.

Requirements:

- fixed or configurable chunk size with validated limits;
- no unbounded global pool;
- buffer ownership clear across async writes;
- cancellation and client disconnect release buffers;
- range boundaries exact;
- no stale-byte exposure between requests;
- no cross-request mutable aliasing;
- no blocking allocator lock on the critical path where avoidable;
- per-connection or bounded shared pool only if measured beneficial.

Compare at least two reasonable chunk sizes across small and large files. Select defaults from latency, throughput, memory, and syscall tradeoffs rather than throughput alone.

## Track E — Eliminate avoidable request-path copies

Potential targets, only when measured:

- avoid rebuilding safe relative paths repeatedly for MIME detection;
- retain precomputed or borrowed metadata where ownership permits;
- avoid cloning response bodies/headers during normalization;
- use `Bytes` for immutable generated/error bodies where appropriate;
- avoid repeated string formatting in disabled logging modes;
- avoid allocating connection/request IDs as strings until emitted;
- avoid collecting headers solely to inspect one value.

Every change must preserve:

- duplicate header behavior;
- normalization order;
- stable public API types;
- sanitized logging;
- file-handle ownership;
- cross-language conformance.

## Track F — Range and seek efficiency

Audit range streaming for:

- exactly one required seek;
- bounded reads that never pass the range end;
- no full-file buffering;
- no metadata reopen;
- correct behavior on short reads;
- cancellation during seek/read/write;
- Windows and Unix parity.

Benchmark many small ranges and large ranges. Confirm adversarial range requests remain bounded by existing connection/file-stream limits and do not create excessive per-request allocations.

## Track G — Request-body and drain performance correctness

Although the built-in static server rejects bodies, generic primitives expose bounded body modes. Measure and verify:

- reject-before-handler path;
- buffer mode allocation ceiling;
- stream mode chunking and backpressure;
- incomplete-body close/drain behavior after Plan 079;
- timeout and cancellation;
- Python iterator bridge.

Optimization must not weaken byte accounting, one-shot consumption, or close-on-ambiguous-framing behavior.

## Track H — Accept-loop and task housekeeping

Audit:

- `Vec<JoinHandle>` retention and scanning;
- finished-task collection;
- semaphore acquisition path;
- listener error backoff;
- broadcast receiver creation;
- per-connection state cloning;
- TLS acceptor construction;
- task and permit cleanup.

Prefer structured task sets such as `JoinSet` only if they simplify lifecycle correctness and benchmark at least neutral. Do not reintroduce detached tasks or false stopped state.

## Track I — TLS overhead characterization

Measure plaintext and native TLS separately.

Record:

- handshake latency;
- resumed versus new connections if supported;
- CPU and memory under handshake churn;
- file throughput over established TLS connections;
- connection admission behavior;
- timeout behavior.

Do not add certificate lifecycle or edge features. The purpose is qualification of the limited native TLS profile.

## Track J — Directory listing and metadata bounds

Benchmark listing generation at:

- 0, 10, 100, 1,000, and configured maximum entries;
- long Unicode filenames;
- filtered reparse/dotfile-heavy directories.

Ensure:

- entry and output limits remain enforced;
- rendering allocation is bounded;
- sorting does not exceed expected complexity/memory;
- HEAD avoids unnecessary body rendering where possible while preserving headers;
- cancellation releases blocking work and buffers.

## Track K — Performance regression thresholds

Define thresholds relative to the captured baseline.

Recommended policy:

- no correctness/security gate may be waived for performance;
- small-file median and p95 latency must not regress materially without rationale;
- large-file throughput must not regress materially;
- steady-state memory and handle counts must remain bounded;
- allocation reduction claims require profiler evidence;
- platform-specific improvements cannot hide severe regression on another supported profile.

Use broad enough thresholds to avoid flaky CI. Scheduled dedicated benchmarks may enforce tighter advisory thresholds; pull-request CI should use stable smoke/regression checks.

## Track L — Comparative context

Optionally compare with:

- Python `http.server` as the replacement baseline;
- one mature static server as contextual reference.

Rules:

- use identical files, clients, network topology, TLS termination, and connection reuse;
- state that feature/security models differ;
- do not optimize solely to win a synthetic benchmark;
- avoid marketing claims unsupported across platforms and workloads.

The primary target is predictable bounded behavior, not maximum headline requests per second.

## Required tests

All optimizations must rerun:

- full workspace tests and doctests;
- raw-wire and canonical conformance;
- direct/index GET/HEAD/range/conditional tests;
- Unix and Windows confinement tests;
- cancellation and shutdown;
- body accounting and framing;
- TLS parity;
- Python parity and installed artifacts;
- log sanitization and counters.

Add targeted tests for:

- exact range end under every buffer size;
- stale-buffer data isolation;
- client disconnect buffer return;
- forced shutdown buffer/task return;
- zero-length and very small files;
- short reads and file truncation;
- concurrent buffer-pool saturation.

## Benchmark artifacts

Store machine-readable benchmark output containing:

- schema version;
- source SHA;
- environment metadata;
- workload definition;
- sample count;
- raw and summarized metrics;
- comparison baseline SHA;
- known noise/limitations.

Do not commit large raw traces unless repository policy allows them. Publish them as workflow artifacts with a small checked-in summary where appropriate.

## Release-gate changes

Add or refine gates such as:

- `perf.static-small-file`;
- `perf.static-large-file`;
- `perf.range-streaming`;
- `perf.keepalive`;
- `perf.allocation-profile`;
- `perf.resource-bounds`;
- `perf.tls-overhead`;
- `perf.directory-listing`;
- `perf.installed-artifact-parity`.

Performance gates must be associated with exact environment classes. Missing benchmark infrastructure should block performance claims, not ordinary correctness builds.

## Documentation changes

Update:

- performance methodology;
- architecture streaming/body sections;
- configuration reference for chunk/buffer limits if public;
- release contract;
- benchmark caveats;
- support-profile evidence requirements.

Avoid publishing unstable microbenchmark numbers in the README.

## Acceptance criteria

- A reproducible cross-platform baseline exists.
- Every optimization is supported by representative measurements.
- Streaming and range paths remain handle-based and bounded.
- No security, protocol, body, or shutdown invariant regresses.
- Buffer reuse cannot expose stale cross-request data.
- Client disconnect and shutdown return buffers/tasks/permits.
- Performance regression thresholds are machine-readable and non-flaky.
- Installed binaries/wheels show behavior consistent with source builds.
- Final benchmark reports include variance and environment metadata.

## Stop conditions

Stop and document rather than merging an optimization if:

- improvement appears only in an unrealistic microbenchmark;
- correctness or confinement becomes harder to audit;
- memory usage rises without a justified production tradeoff;
- a pool or cache cannot be strictly bounded;
- platform behavior diverges materially;
- benchmark variance exceeds the claimed gain;
- an unsafe fast path lacks independent review.

## Handoff

After this plan closes, Plan 089 performs long-duration soak, proxy and TLS qualification, installed-artifact/provenance checks, independent review, and final support-profile decisions.