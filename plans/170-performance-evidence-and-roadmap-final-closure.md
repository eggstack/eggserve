# Plan 170 — Performance Evidence and Roadmap Final Closure

## Status

**READY FOR IMPLEMENTATION AFTER OR ALONGSIDE PLAN 169.**

This is the final evidence/claims closure for the production/embedding/anonymity roadmap. It does not add runtime features unless measurement exposes a correctness or resource-regression bug.

## Goal

Close the gap between Plan 168's broad written performance-qualification intent and the narrower performance evidence currently recorded in `benchmarks/168-qualification/results.json`.

The repository already has strong deterministic correctness/resource/privacy qualification. The remaining need is focused, reproducible evidence for the performance-sensitive claims that motivated this roadmap:

- high-concurrency native HTTP serving;
- streaming application responses;
- the Python low-level service substrate;
- representative TLS overhead;
- the caller-owned connection path used by embedding consumers;
- substitution behavior versus Python `http.server` for the directly comparable static-serving case.

The output should support precise claims, not a benchmark leaderboard.

## Current evidence and gap

The existing Plan 168 performance artifact is useful but narrow:

- Linux x86_64;
- release CLI;
- static GET only;
- 1 KiB and 1 MiB files;
- 16 keep-alive client workers;
- 2 captured runs × 3 trials per size;
- zero errors;
- peak RSS below 7 MiB in the recorded runs.

The artifact correctly records large between-run absolute RPS variance and therefore demonstrates why absolute timing is unsuitable for CI gating.

However, it does not by itself establish the broader performance matrix described in `benchmarks/README.md` or the original Plan 168 acceptance language. In particular it does not record representative native custom-service streaming, Python low-level callback/streaming, TLS, caller-owned-stream, high-concurrency scaling, latency percentiles, CPU, or a same-session `python -m http.server` substitution baseline.

Do not invalidate the existing artifact. Extend the evidence set with a deliberately smaller, representative matrix.

## Claims to qualify

After this plan, evidence may support these bounded statements when accompanied by the recorded profile/machine:

1. EggServe's native HTTP/1 runtime scales across representative keep-alive concurrency without correctness errors or unbounded resource growth.
2. Known- and unknown-length response streaming avoids whole-response buffering and remains throughput-usable under concurrency.
3. `eggserve.lowlevel` provides a bounded synchronous Python service path suitable as a substrate for a downstream application server, with its Python callback cost measured separately from native Rust serving.
4. Caller-owned `AsyncRead + AsyncWrite` connections use the same canonical runtime without a material architectural penalty that would make embedding impractical.
5. Native TLS has measured overhead for representative workloads; no claim of edge-server parity is implied.
6. For simple static serving, a controlled same-machine comparison describes what changes when substituting EggServe for CPython `http.server`.

Do **not** claim universal RPS superiority, nginx/Caddy parity, DDoS resistance, Granian/Gunicorn parity, or anonymity based on these measurements.

## Benchmark harness requirements

Prefer one small reproducible harness family rather than several bespoke scripts.

The harness must record machine-readable results containing at minimum:

- source commit SHA and `Cargo.lock` identity/hash;
- exact EggServe build command/profile/features;
- Rust and Python versions;
- OS, architecture, CPU, logical CPU count, memory;
- server profile and relevant runtime limits;
- workload name and response/request size;
- concurrency and connection-reuse policy;
- requests or duration per trial;
- requests/sec and bytes/sec when meaningful;
- p50/p95/p99 latency;
- error/timeout/rejection counts;
- process CPU time/utilization where available;
- peak and steady-state RSS where available;
- thread/task/fd counts where practical;
- trial variance.

Use standard-library or already-approved lightweight tooling where sufficient. Do not add a large benchmark dependency tree to `eggserve-core` just to produce reports.

## Required benchmark matrix

The matrix below is intentionally narrower than the exhaustive Plan 168 prose. It is enough to test the architectural claims while keeping the work maintainable.

### A. Native static HTTP/1

Response sizes:

- 1 KiB;
- 128 KiB;
- 1 MiB.

Concurrency/reuse points:

- 1 keep-alive worker;
- 16 workers;
- 64 workers;
- 256 workers or the highest stable point the test machine can drive without client-side saturation.

Configure EggServe limits explicitly so the benchmark distinguishes intended admission from accidental default-limit rejection. Record any deliberate saturation test separately.

For the 1 KiB and 1 MiB cases, preserve continuity with the existing Plan 168 artifact.

### B. Native custom `Service`

Measure at least:

- 1 KiB buffered `ResponseBody::Bytes`;
- 1 MiB known-length `ResponseBody::Stream`;
- 1 MiB unknown-length `ResponseBody::Stream`;
- one larger stream (prefer 16 MiB) specifically for RSS/backpressure evidence rather than headline RPS.

Use 16 and 64 concurrent keep-alive clients for the service cases unless the machine cannot drive them cleanly.

Record CPU/RSS and verify streamed responses do not scale memory with representation size in the way a whole-response buffer would.

### C. Python `eggserve.lowlevel`

Use the installed wheel, not `PYTHONPATH` source imports.

Measure separately:

- 1 KiB buffered Python response;
- 1 MiB bounded Python `Response.stream`;
- at least two callback-admission settings representative of normal use (for example 8 and a machine-appropriate higher value).

Record Python process/thread behavior and errors. Do not compare these numbers directly to Rust static serving as if they are equivalent application workloads.

The purpose is to establish that the substrate is bounded and practically usable, not to optimize arbitrary Python callback code inside EggServe.

### D. Caller-owned connection driver

Use a deterministic in-process transport harness based on `tokio::io::duplex` or the existing caller-owned-stream example/test infrastructure.

Measure buffered and streamed service paths over enough iterations to detect gross overhead/regression versus the TCP canonical path. This is a microbenchmark of the embedding seam, not a network throughput score.

Required conclusion shape:

- no duplicated parser/runtime stack;
- no unexpected buffering/allocation cliff;
- any measured delta is recorded and explained.

Do not add I2P as a benchmark dependency.

### E. Representative TLS

Measure at minimum:

- established keep-alive TLS serving for 1 KiB and 1 MiB static responses;
- a handshake-churn case reported separately from established-connection throughput.

Use the existing rustls feature/profile. Do not turn this into certificate automation or TLS-stack competition.

### F. CPython `http.server` substitution baseline

Run current available CPython `python -m http.server` in the same benchmark session and on the same machine for directly comparable static cases.

Match as closely as practical:

- loopback bind;
- source files/page-cache state;
- HTTP/1 protocol mode;
- concurrency/client implementation;
- response sizes;
- request counts/durations.

At minimum compare 1 KiB and 1 MiB GET workloads at concurrency 1, 16, and a higher concurrency that both servers can complete reliably.

Report the comparison as migration evidence:

> “On machine/profile X, under workload Y, EggServe and CPython `http.server` behaved as follows.”

Do not turn ratios into README marketing or imply the Python server is a production performance target.

## High-concurrency resource behavior

Add one focused scaling/resource run that is not merely an RPS test.

Exercise:

- many keep-alive connections with fewer active requests;
- 64+ active request concurrency;
- a deliberate service-admission saturation point;
- a slow-reader subset if the harness can do so deterministically.

Record:

- RSS;
- fd count;
- task/thread count where available;
- active/rejected service counters;
- errors/timeouts;
- recovery after load drops.

The pass condition is bounded recovery and understandable admission behavior, not maximum connections at all costs.

## Trial discipline

For timing-bearing workloads:

- perform a warm-up excluded from results;
- use at least 3 measured trials;
- randomize or alternate A/B ordering for EggServe vs CPython where practical to reduce thermal/frequency drift;
- keep comparisons in the same machine/session;
- record median plus spread/variance, not the best run;
- abort or flag a run when the client itself is saturated or erroring.

If absolute performance changes materially between repeated sessions, retain both results and explain the environmental variance rather than cherry-picking.

## Platform coverage

### Required closure platform

Linux x86_64 is the primary performance-closure platform because the existing Plan 168 artifact and common production deployment profile are there.

### Architecture portability evidence

Because EggServe targets SBC/router deployment, run a smaller representative smoke on an arm64 system when available:

- native static 1 KiB/1 MiB;
- one native streaming case;
- caller-owned-stream correctness/performance smoke;
- RSS/errors.

Linux arm64 is preferred. macOS arm64 is acceptable as supplemental evidence but does not substitute for Linux behavior in deployment claims.

Do not block architecture closure indefinitely on unavailable benchmark hardware. If Linux arm64 cannot be run in this phase, state that performance is unqualified there while retaining existing correctness/platform support claims.

## Evidence layout

Keep the existing `benchmarks/168-qualification/` historical artifact intact.

Create a new evidence directory, recommended:

```text
benchmarks/170-closure/
  README.md                 # exact reproduction commands/environment caveats
  results.json              # normalized summary
  raw/                      # optional raw trial data, if reasonably sized
  <harness files>
```

Do not commit huge packet captures, generated binaries, virtual environments, or multi-gigabyte raw logs.

`results.json` should distinguish workload families and profiles so later releases can compare like-for-like.

## Regression interpretation

Use results to establish a reproducible baseline for future same-machine/same-method comparisons. Do not create PR gates on absolute RPS or latency.

Treat these as hard failures during qualification:

- protocol errors;
- unexpected 5xx/connection truncation;
- unexplained memory/fd/thread growth;
- streaming buffering proportional to whole representation size when backpressure should prevent it;
- failure to recover permits/resources after saturation;
- performance collapse caused by an obvious implementation bug.

A performance tradeoff from an explicit security/resource bound may be accepted when documented.

## Documentation and final roadmap closure

After evidence is captured, update:

- `benchmarks/README.md` evidence index and claims text;
- `architecture/testing-and-conformance.md` Plan 170 mapping;
- `docs/deployment.md` only where measured profile guidance changes;
- README only if a performance statement can now be made precisely without marketing language;
- `AGENTS.md` / `.opencode/skills/eggserve-dev/SKILL.md` with the evidence location and no-absolute-CI-gate invariant if needed.

Then reconcile the historical roadmap:

### Plan 168

Append a closure record distinguishing:

- deterministic correctness/resource/privacy qualification;
- performance evidence provided by the original 168 snapshot;
- expanded representative performance evidence from Plan 170.

If the required Plan 170 matrix is complete, mark Plan 168 **CLOSED**. Do not mechanically check historical criteria that were intentionally narrowed; explain the closure interpretation explicitly.

### Plan 161

Once Plan 169 and this plan are complete, mark Plan 161 **CLOSED** and append a short final outcome mapping:

- response streaming → Plan 162;
- caller-owned transport → Plan 163;
- resource controls → Plan 164;
- privacy policy → Plan 165;
- Python substrate → Plan 166;
- CGI/FastCGI → Plan 167 no-go;
- qualification → Plans 168/170;
- final closure/API polish → Plan 169.

Do not rewrite the original roadmap body.

## Verification

Before final closure run at least:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
cargo test -p eggserve-core --test response_streaming
cargo test -p eggserve-core --test transport_driver
cargo test -p eggserve-core --test production_controls
cargo test -p eggserve-core --test response_privacy
```

Run the installed Python wheel tests and TLS checks. Use the existing manual/deep qualification paths for slow suites rather than expanding routine CI.

If Plan 169 changes the erased body type / relaxes `ResponseStream: Sync`, all Plan 170 benchmarks must measure the post-169 implementation so the closure evidence corresponds to the final code.

## Non-goals

Do not add:

- a permanent benchmarking service/daemon;
- public-cloud absolute performance gates;
- Granian/Gunicorn/nginx/Caddy benchmark marketing;
- HTTP/2/3;
- ASGI/WSGI;
- CGI/FastCGI;
- I2P dependencies;
- per-client rate limiting/WAF behavior;
- runtime optimizations solely to win a microbenchmark without profiling evidence.

## Acceptance criteria

- [ ] A reproducible machine-readable Plan 170 evidence set exists with source SHA and environment metadata.
- [ ] Native static serving has representative 1/16/64/high-concurrency scaling evidence.
- [ ] Buffered plus known/unknown-length native service responses have throughput/latency/resource evidence.
- [ ] A large streamed response demonstrates bounded-memory behavior under backpressure.
- [ ] The installed Python low-level buffered/streaming paths have bounded performance/resource evidence.
- [ ] Caller-owned-stream performance is measured sufficiently to rule out a gross embedding-path regression.
- [ ] Representative established TLS and handshake-churn costs are recorded separately.
- [ ] A same-session CPython `http.server` substitution baseline exists for directly comparable static workloads.
- [ ] Errors, timeouts, RSS, and resource recovery are recorded for a high-concurrency/admission-stress run.
- [ ] No absolute RPS CI gate or unsupported superiority claim is introduced.
- [ ] `benchmarks/README.md` and testing/deployment docs point to the new evidence accurately.
- [ ] Plan 168 has an explicit final closure record.
- [ ] Plan 161 has an explicit final roadmap closure record after Plan 169 is complete.

## Handoff

Implement Plan 169 first if it changes the response-body erasure/bounds, then run this qualification against that final code. This plan should be the last work item in the 161–170 line unless measurement exposes a concrete correctness/resource bug; any such bug should receive a narrowly scoped corrective plan rather than expanding this qualification phase.