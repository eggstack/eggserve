# Plan 124 — Python Tokio Runtime Worker Right-Sizing

## Status

**READY FOR HANDOFF — 2026-08-11.**

Parent roadmap: Plan 120.
Depends on: Plan 121; should be measured after Plan 123 if both alter the common Python request path.

Reviewed baseline:

```text
main = bae3dce5f8be876a083434918cdfc974b9781c75
```

---

## Problem statement

Every native Python `Server` currently owns a `tokio::runtime::Runtime` and constructs it with:

```rust
let rt = tokio::runtime::Runtime::new()?;
```

Tokio's normal multi-thread runtime chooses its worker count from the host parallelism. This is reasonable for a standalone server process, but potentially excessive for an embeddable Python API where one process may create several EggServe servers and where typical workloads are I/O-bound and separately bounded by:

```text
max_connections
max_file_streams
max_python_callbacks
```

On a high-core-count machine, multiple Python servers can therefore create substantially more scheduler threads and stack reservation than the workload requires.

The obvious alternative—one process-global runtime—also has nontrivial lifecycle, isolation, fork, shutdown, and interpreter-finalization implications. This plan must prefer simple per-server ownership unless measurements show that a global runtime is clearly necessary.

---

## Goal

Measure the actual thread/memory cost of the current Python runtime strategy and, if material, replace implicit host-core-sized runtimes with an explicitly bounded per-server Tokio runtime configuration that preserves throughput and lifecycle correctness.

Optimization is evidence-gated. A smaller thread count is not automatically better if it causes meaningful static streaming or Python-handler regressions.

---

## Non-goals

Do not:

- introduce a process-global Tokio runtime by default;
- add async Python APIs;
- expose Tokio internals as public configuration merely because the implementation has workers;
- change `max_connections`, `max_file_streams`, or `max_python_callbacks` semantics;
- add a thread-pool dependency;
- tune kernel/socket parameters in this plan;
- introduce NUMA/CPU pinning;
- optimize the standalone Rust CLI unless the same builder helper naturally applies;
- add runtime-worker benchmarking to routine CI.

---

## Track A — Establish current runtime ownership and blocking behavior

Inspect the production path before tuning:

```text
PyServer::start()/stop()/wait()/force_shutdown()
PythonCallbackService::call()
spawn_blocking usage
file body streaming path
TLS path
core Server runtime assumptions
```

Determine which work executes on Tokio workers versus the blocking pool:

- network accept/HTTP connection futures;
- file reads/stream conversion;
- Python callbacks (`spawn_blocking` currently involved);
- request-body reads;
- TLS handshakes;
- shutdown coordination.

This determines whether one, two, or another small worker count is a viable candidate.

### Acceptance criteria

- tuning is based on the actual execution model;
- no worker reduction is made under the false assumption that blocking file I/O is asynchronous if it is not;
- Python callback blocking pool behavior is accounted for separately from Tokio async worker count.

---

## Track B — Measure thread and memory scaling

Use an installed wheel and a small repeatable Python harness to create:

```text
1 server
2 servers
4 servers
8 servers, when host resources allow
```

Use port `0` and loopback so the measurement does not depend on fixed ports.

For each case, record at minimum:

```text
logical CPU count
process thread count before servers
process thread count after each server starts
RSS after each server starts
RSS after shutdown
thread count after shutdown
```

Run the measurement on at least one multi-core Linux host. If the available host has only a few cores, record that limitation rather than extrapolating large-host behavior as fact.

Also test repeated start/stop cycles to ensure runtime threads are reclaimed rather than accumulated.

### Acceptance criteria

- current worker/thread scaling is measured, not inferred from Tokio documentation alone;
- measurements distinguish base Python threads from EggServe-created threads;
- shutdown returns thread count/RSS approximately toward baseline without monotonic leakage;
- evidence is stored as closure notes, not a permanent telemetry subsystem.

---

## Track C — Benchmark bounded per-server runtime candidates

Prefer evaluating explicit per-server builders, for example conceptually:

```rust
Builder::new_multi_thread()
    .worker_threads(N)
    .enable_all()
    .build()
```

Candidate values should stay small and evidence-driven. At minimum compare:

```text
current Runtime::new() default
1 worker
2 workers
min(4, host parallelism) if useful
```

Do not assume that `max_python_callbacks` should equal Tokio workers; Python callbacks are already dispatched through `spawn_blocking` and a separate semaphore.

Benchmark representative workloads:

```text
small static GET at concurrency 1 and moderate concurrency
larger file streaming
HEAD/304 low-body path
custom Python handler at concurrency 1 and max_python_callbacks-sized concurrency
TLS handshake/request sample if feasible
```

Record throughput/latency and CPU/RSS/thread counts.

### Decision rule

Retain a bounded runtime configuration only if it materially reduces resource overhead and does not cause a meaningful representative performance regression.

For this plan:

- a **material resource reduction** is >=25% fewer EggServe-created steady-state threads for a single server on the measurement host, or clearly sublinear improvement when several servers are created;
- a **meaningful performance regression** is >5% median throughput loss or >10% p95 latency increase in a representative workload, unless the resource reduction is large enough to justify and document the trade-off.

Use several runs and compare medians; do not make the decision from one noisy sample.

If the host's default runtime already creates a small bounded worker set and the savings are negligible, close the plan with no production change.

### Acceptance criteria

- candidate comparison includes current behavior;
- worker count is not chosen solely from intuition;
- any retained configuration meets the decision rule or documents a deliberate trade-off;
- no public API knob is added unless a genuine downstream need is demonstrated.

---

## Track D — Prefer a small per-server builder

If measurement justifies a change, keep ownership equivalent to the current design:

```text
PyServer owns Runtime
start creates it
stop/wait tears it down
server instances are isolated
```

Use an internal helper/builder so runtime construction is not duplicated.

The default should be fixed/bounded or derived from a small cap rather than raw host CPU count. Choose the value from Track C evidence.

Do not couple worker count directly to `max_connections`; 64 connections do not require 64 runtime workers.

### Acceptance criteria

- per-server runtime ownership remains explicit;
- worker count is bounded and documented internally;
- all Tokio facilities required by the runtime are enabled;
- shutdown drops the runtime cleanly;
- multiple server instances remain independent;
- no process-global mutable singleton is added.

---

## Track E — Global runtime is a fallback hypothesis, not the default plan

Consider a shared runtime only if **both** are true:

1. bounded per-server workers still produce unacceptable thread/memory scaling for realistic multi-server use; and
2. a shared runtime provides a substantial additional benefit that cannot be achieved with the simpler design.

If evaluated, explicitly analyze:

```text
Python interpreter shutdown/finalization
module unload behavior
fork-after-import / multiprocessing implications
server-specific shutdown without shutting down peers
runtime lifetime after the last server
panic/error isolation
TLS/server task cancellation
thread-safe initialization
```

A process-global runtime that is never shut down may be acceptable in some Rust applications but is a significant embedding behavior change in Python. Do not introduce it casually.

### Acceptance criteria for any shared-runtime proposal

- dedicated tests cover two simultaneous servers and independent shutdown;
- interpreter process exits cleanly;
- no server can tear down another server's runtime;
- resource improvement over bounded per-server runtimes is substantial and recorded;
- architecture remains simpler enough to justify the lifecycle coupling.

If these conditions are not met, reject the shared-runtime approach.

---

## Track F — Regression coverage

Required tests after any runtime-construction change:

```text
start/ready/stop one server
repeated start/stop lifecycle allowed by current API contract
multiple simultaneous servers on port 0
independent shutdown of two servers
stock static service
custom Python callback service
request body buffered/stream path where supported
TLS server startup/request
force shutdown
server drop/interpreter process exit smoke
```

Do not add tests that assert an exact OS thread count in routine CI; scheduler/runtime implementation details vary. Thread counts belong in the manual evidence harness. Production tests should assert behavior/lifecycle.

### Acceptance criteria

- lifecycle remains correct after Plan 121;
- multiple servers do not interfere;
- no deadlock occurs around runtime mutex/GIL interaction;
- Python callback semaphore semantics remain unchanged;
- no file-stream permit behavior changes.

---

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Run the manual thread/RSS/performance harness from an installed wheel before and after any production change.

Routine CI must not gain resource-measurement jobs.

---

## Explicit acceptance criteria

Plan 124 is complete when either an optimization or evidence-only closure is recorded.

### Optimization closure

- [ ] current thread/RSS scaling is measured for one and multiple servers;
- [ ] current default is benchmarked against bounded worker candidates;
- [ ] chosen worker configuration materially reduces resource overhead;
- [ ] representative throughput/latency remains within the plan's regression bounds or an explicit trade-off is justified;
- [ ] per-server runtime ownership remains the default architecture;
- [ ] no new public worker-count configuration is added without need;
- [ ] start/stop/multi-server/TLS/custom-handler tests pass;
- [ ] no thread/runtime leak is observed over repeated lifecycle cycles;
- [ ] routine CI remains unchanged in shape.

### Evidence-only closure

- [ ] measurements show current behavior is already acceptable or bounded candidates do not provide a worthwhile trade-off;
- [ ] no speculative runtime refactor remains;
- [ ] the result is documented and the plan is closed.

---

## Rejection conditions

Reject the implementation if it:

- introduces a global runtime solely because it seems more efficient;
- uses one worker without measuring larger-file/custom-handler/TLS behavior;
- exposes an undocumented public Tokio tuning knob;
- conflates `max_python_callbacks` with async worker count;
- adds a permanent performance CI gate;
- causes cross-server shutdown coupling;
- weakens runtime admission/security semantics for resource savings.
