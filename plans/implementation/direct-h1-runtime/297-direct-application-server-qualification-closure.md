# Direct H1 Runtime Milestone 297 — Direct application-server qualification and closure

Status: blocked

Repository baseline: `81605fc36b970440d6e3edbfd1e0d1cfa3ec4d91` (planning baseline; refresh after retained 295/296 implementation)

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-7--downstream-like-qualification-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/002-long-term-roadmap.md`

Primary class: polish

## 1. Objective

Close the direct-application-server optimization campaign by qualifying every retained Milestone 295/296 production change against native H1, Tower/Axum, and an EggPool-shaped streaming consumer. Record keep/revert/defer decisions, dependency/binary impact, correctness/security evidence, and the exact semver/publication strategy.

## 2. Why this milestone is ready

Blocked until Milestones 295/296 have implementation/closure candidates or explicit NO-GO dispositions. It may proceed with only one retained production track if the other closes NO-GO.

## 3. Current implementation evidence

To be refreshed from the 294 baseline and 295/296 closure candidates. The qualification must not rely on microbenchmarks alone.

## 4. Invariants that must not regress

All direct-H1 roadmap invariants, plus:
- native `Service` and direct Tower remain behaviorally convergent at the transport-policy boundary;
- no feature/profile weakens framing/body ceilings/timeout/shutdown semantics;
- core compatibility remains forwarding/composition rather than duplicate H1 authority;
- dependency/binary wins are not purchased with unbounded buffering or detached tasks.

## 5. Scope

### In scope

- same-machine A/B using 294 fixtures;
- EggPool-shaped Axum routes: health/small JSON, request body admission, SSE/token stream, cancellation;
- allocation/CPU/p50/p95/p99/RSS under c1/c16/c64;
- long-lived concurrent streams and shutdown;
- dependency trees, feature trees, stripped fixture sizes;
- package/build matrix;
- compatibility and semver/publication decision;
- registry-only consumer proof if publication occurs.

### Explicitly out of scope

- real LLM provider/network latency;
- claims that EggServe is universally faster than Hyper/Axum alternatives;
- unrelated static/H2/H3/Python optimization;
- new production optimization discovered during closure: open a corrective/new milestone instead.

## 6. Required production changes

None unless required to revert a candidate that fails qualification. New optimization work is prohibited in this closure milestone.

## 7. Ordered work packages

### Work package A — Freeze baseline/candidate

Record exact 294 baseline SHA, candidate SHA(s), diff/commit set, compiler, build profile, enabled features, runtime limits, OS/CPU, and client harness.

### Work package B — Same-machine performance/resource matrix

At minimum:
- small bodyless request/1 KiB response;
- small POST/JSON response;
- 1 MiB known stream;
- SSE-like 128-chunk response;
- 10/100 concurrent slow streams where resource bounds permit;
- cancellation after first/middle chunk;
- established keep-alive and connection churn separately.

Alternate baseline/candidate trials with warm-up and repeated samples. Record allocation counts where available, CPU, throughput, p50/p95/p99, RSS, fd/thread counts, errors/timeouts.

### Work package C — Footprint matrix

Record:
- lockfile package count;
- no-dev dependency nodes;
- feature ancestry;
- stripped direct fixture bytes;
- stripped Axum application fixture bytes;
- compile-time capability combinations retained by 296.

Use the same toolchain/profile and clean-build conditions for comparisons.

### Work package D — Correctness/security/regression

Run direct parity, Tower/Axum qualification, request-smuggling/framing controls, body/trailer suites, cancellation/shutdown, core compatibility, topology, package/supply-chain gates appropriate to changed manifests.

### Work package E — Downstream-like consumer

Build a registry/path-local consumer shaped like EggPool's boundary:

```text
caller-bound TcpListener
  -> eggserve-server direct Server + tower
  -> Axum Router
  -> small JSON route
  -> streaming SSE route
  -> bounded request body
  -> controlled shutdown
```

Do not import EggPool application logic. The point is the same integration geometry.

### Work package F — Release decision

For every retained change record KEEP/REVERT/DEFER. Then classify exact version/publication set. If publishing, follow current manual release contract, package dry-runs, exact-SHA hosted CI, and fresh registry-only direct Tower/Axum proof before claiming downstream availability.

## 8. Failure, cancellation, restart, and contention semantics

Qualification must include admission saturation/recovery, slow consumer, stalled consumer, producer error, client disconnect, server shutdown while streaming, and repeated start/stop fixture cycles. No resource count may grow unbounded across cycles.

## 9. Compatibility and migration

Record source/feature/runtime compatibility and any required consumer feature change. If 296 introduces an intentional source/feature break, provide migration examples and use the semver level justified by current pre-1.0 policy.

## 10. Required tests

All focused tests from retained 295/296 work plus:
- direct/native parity;
- Axum/Tower qualification;
- core compatibility Tower forwarding;
- static file smoke if file capability selection changed;
- tunnel regression if tunnel capability selection changed;
- full feature/topology package checks.

## 11. Required verification commands

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-server --no-default-features --features tower
cargo test -p eggserve-core --no-default-features --features tower
./scripts/verify.sh fast
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
```

Run `verify.sh full` when retained changes affect packaging/frontends or before publication. Hosted CI is required for a publication candidate.

## 12. Documentation updates

Update benchmark evidence, direct embedding docs, dependency policy, migration guide if needed, architecture runtime/topology docs, source roadmap, and registry. Do not edit legacy `plans/ROADMAP.md`.

## 13. Acceptance criteria

- Same-machine A/B covers buffered and SSE-like streaming workloads.
- Tail latency, CPU, allocations, RSS, errors, and resource recovery are recorded.
- Direct-profile dependency and stripped-binary deltas are recorded.
- Every 295/296 candidate has KEEP/REVERT/DEFER/NO-GO disposition.
- All correctness/security/topology checks relevant to retained changes pass.
- Native H1 and Tower/Axum contracts remain convergent.
- Exact semver/publication decision is recorded.
- If published, registry-only consumer proof names the exact artifact/version.

## 14. Stop conditions

Stop closure and open a corrective plan if any retained optimization regresses framing/security, cancellation/shutdown, resource bounds, or compatibility beyond the planned semver classification. Do not repair by silently expanding 297 into a new optimization milestone.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/297-direct-application-server-qualification-closure.md` containing the requirement-to-evidence matrix, exact benchmark environment and raw-evidence paths, commands/results, keep/revert decisions, residuals, and release/downstream disposition.

## 16. Handoff notes

The qualification fixture should resemble EggPool's transport geometry but remain a generic external consumer. Do not make EggServe depend on EggPool or specialize EggServe APIs for one downstream.
