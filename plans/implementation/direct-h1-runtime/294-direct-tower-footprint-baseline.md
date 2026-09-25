# Direct H1 Runtime Milestone 294 — Direct Tower and footprint baseline

Status: ready for handoff

Repository baseline: `81605fc36b970440d6e3edbfd1e0d1cfa3ec4d91`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-4--direct-tower--footprint-baseline`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `architecture/adr-003-custom-service-ownership.md`

Primary class: polish

## 1. Objective

Establish a reproducible current-HEAD baseline for the direct `eggserve-server --no-default-features --features tower` application-server profile before changing production code. Measure the exact request/response adaptation and linked footprint relevant to downstream Axum/Tower consumers such as EggPool, including long-lived streaming responses.

This milestone changes no production behavior. It decides, with evidence, which Milestone 295/296 candidates are worth implementing.

## 2. Why this milestone is ready

The direct H1 capability and direct Tower publication are already closed and registry-qualified through Plans 274–277 and 288–291. `eggserve-server 0.3.1` is the published baseline. No architecture decision is required to measure existing behavior.

## 3. Current implementation evidence

At this baseline:

- Hyper requests are converted into canonical `RequestHead`/`RequestBody`, then `TowerToEggserve` converts the canonical head back into `http::Request<HttpRequestBody>`.
- canonical header names/values own storage; the inbound conversion preallocates `HeaderBlock` but still validates/materializes the canonical form before Tower reconstruction.
- Tower responses are converted into canonical responses; non-empty streaming bodies use an adapted stream and trailer rendezvous before final Hyper-body conversion.
- `eggserve-server` contains generic file-body transport support and enables Tokio `fs` even for direct Tower consumers.
- the `tower` feature names `tower-layer` although production adapter code does not currently require Layer APIs directly.
- Plans 227–241 optimized the native H1/common runtime before Plans 274–277 established the current direct Tower path; no equivalent retained profiling campaign exists for the published direct Tower composition.

## 4. Invariants that must not regress

- One H1 parser, request-validation, framing, response-policy, timeout, admission, cancellation, and shutdown authority.
- Direct Tower graph remains free of `eggserve-core`, `eggserve-static`, and PHF unless the baseline itself disproves current topology documentation.
- Request bodies remain one-shot and bounded; streaming preserves backpressure and cancellation.
- Response framing remains runtime-owned; HEAD/body-forbidden semantics and trailers remain correct.
- No performance claim without a named workload/profile and retained raw evidence.

## 5. Scope

### In scope

- native H1 versus direct Tower/Axum allocation and CPU profiling;
- small buffered request/response cases plus SSE-like/unknown-length streaming;
- header-count sensitivity;
- response trailer/no-trailer cases;
- dependency/feature ancestry and stripped binary measurements;
- current Tokio/futures/Tower feature activation;
- file-body/tunnel code reachability/linked-size investigation;
- establishing keep/no-go gates for 295 and 296.

### Explicitly out of scope

- no runtime implementation change;
- no public API/feature change;
- no static-server optimization;
- no H2/H3 promotion;
- no sendfile/splice/io_uring/mmap/cache/custom allocator.

## 6. Required production changes

None.

### Crates and ownership

Measure `eggserve-server`, `eggserve-primitives`, and identical external consumer fixtures. Do not move ownership.

### Config and policy

Use explicit, recorded `RuntimeConfig` values matching a direct application-server profile. Include the published defaults and an EggPool-shaped high body ceiling/long streaming lifetime fixture where appropriate.

### Protocol and compatibility

H1 only for primary timing conclusions. H2/H3 are regression context, not optimization targets.

### Runtime and concurrency

Cover concurrency 1, 16, and 64 where the client remains unsaturated. Include established keep-alive and connection churn separately. For streaming, include multiple concurrently active slow streams and cancellation.

### Frontend or operator surface

Use one minimal Tower service and one Axum router fixture. Add a downstream-like fixture that returns SSE-style incremental chunks without depending on EggPool source code.

### Security and confinement

Malformed request/framing/security cases are correctness controls, not benchmark inputs.

### Documentation and static guards

Retain evidence under a new benchmark directory and record exact source SHA, toolchain, CPU/OS, profile, feature set, dependency graph, and commands.

## 7. Ordered work packages

### Work package A — Freeze fixtures and graph

Intent: create identical native and Tower/Axum workloads.

Required changes: benchmark/test-only fixtures only; no production changes.

Acceptance evidence: resolved no-dev graphs; `cargo tree -e features`; stripped fixture sizes; proof direct Tower excludes core/static/PHF.

### Work package B — Allocation and hot-path profiling

Measure at least:

- GET/bodyless request → 1 KiB response;
- POST with small body → small response;
- 1/16/64 request-header fields;
- 1 MiB known-length streaming response;
- SSE-like unknown-length response with 32–256 small chunks;
- no-trailer and terminal-trailer streaming responses.

Record allocations/request where tooling permits, CPU profiles/flamegraphs or equivalent samples, throughput, p50/p95/p99, and RSS.

### Work package C — Footprint decomposition

Compare:

1. `eggserve-server` native direct profile;
2. direct `tower`;
3. direct `tower` + Axum fixture.

Attribute graph/feature/code candidates, specifically file-body support, Tokio `fs`, tunnel support, `tower-layer`, and any futures/http-body helpers. Distinguish package-count savings from final linked bytes.

### Work package D — Decision record

For each candidate, record:

```text
candidate:
measured/mechanical cost:
target workload:
expected change:
risk:
decision for 295/296: PROCEED | NO-GO | DEFER
```

Milestone 295 may proceed only for confirmed direct-Tower hot-path targets. Milestone 296 may proceed only for graph/link candidates with a credible direct-profile benefit or a mechanically unnecessary dependency edge.

## 8. Failure, cancellation, restart, and contention semantics

Baseline fixtures must verify cancellation of an in-flight stream, client disconnect, server shutdown with active streams, and admission recovery. Measurement harness failure or client saturation must be reported rather than interpreted as server improvement/regression.

## 9. Compatibility and migration

None; evidence only.

## 10. Required tests

Focused fixture correctness, Tower/Axum qualification tests, direct/native parity controls, streaming/trailer cancellation controls, topology checks.

## 11. Required verification commands

```bash
cargo test -p eggserve-server --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
python3 scripts/check-crate-topology.py
./scripts/verify.sh fast
```

Benchmark/profile commands must be recorded exactly in closure evidence.

## 12. Documentation updates

- benchmark evidence index;
- source subsystem roadmap status;
- `plans/registry.md`;
- no normative API docs unless measurement discovers an existing documentation defect.

## 13. Acceptance criteria

- Native and direct Tower/Axum fixtures are behaviorally comparable.
- Allocation/CPU/latency evidence identifies or rejects the canonical↔Tower conversion cost.
- Streaming response adapter cost is separately characterized.
- Direct-profile graph and stripped executable sizes are recorded.
- File-body/Tokio-fs, tunnel, and Tower dependency candidates have explicit decisions.
- 295/296 are not unblocked by assumption.

## 14. Stop conditions

Stop if benchmark fixtures require production semantic changes, if current source violates a closed security invariant, or if measurement cannot distinguish server cost from client/tool saturation.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/294-direct-tower-footprint-baseline.md` with exact commands, environment, raw-evidence paths, result tables, candidate dispositions, and explicit unblock/block decisions for 295/296.

## 16. Handoff notes

This is an evidence milestone. Do not “optimize while measuring.” Keep fixture code outside production authority where practical and preserve unrelated repository changes.
