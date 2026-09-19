# Plan 231 — Post-optimization performance and regression qualification

## Prerequisites

- Plan 227 baseline is complete.
- Candidate implementations from Plans 228–230 have landed on a qualification
  branch/commit, or have explicit NO-GO records.

## Purpose

Close the performance campaign with reproducible evidence and keep only changes
that improve the intended workload or materially simplify the hot path without
regressing security, correctness, tail latency, or bounded-resource behavior.

This plan is deliberately a qualification/decision gate. It does not authorize
new optimizations discovered during final measurement. New findings receive a
narrow follow-up plan.

## Goals

- Compare the optimized candidate against the Plan 227 baseline on the same
  hardware/session where practical.
- Separate throughput gains from client/tool saturation.
- Verify resource bounds, cancellation, timeout, and protocol behavior under
  the optimized code.
- Record per-change KEEP / REVERT / DEFER conclusions.
- Refresh performance documentation without universal superiority claims.
- Ensure binary/package/dependency footprint did not regress unexpectedly.

## Work

### 1. Freeze the candidate

Record:

- baseline Plan 227 SHA;
- optimized candidate SHA;
- exact diff/commit set attributable to Plans 228–230;
- build profile/features/toolchain;
- runtime limits.

Do not mix dependency upgrades, new features, or unrelated refactors into the
qualification candidate.

### 2. Same-machine A/B matrix

Run baseline and candidate in alternating order with warm-up plus at least
three measured trials.

Required native H1 matrix:

**Static**
- 1 KiB, 128 KiB, 1 MiB;
- concurrency 1, 16, 64;
- one high-concurrency point if the client remains unsaturated.

**Custom service**
- 1 KiB `ResponseBody::Bytes`;
- 1 MiB known-length stream;
- 1 MiB unknown-length stream;
- 16 MiB bounded-memory stream.

**Range/conditional**
- representative first/middle/suffix ranges;
- 304 via ETag/date validator;
- HEAD for small and large files.

**TLS**
- established keep-alive 1 KiB and 1 MiB;
- handshake churn separately.

Use the Plan 227 native client for small-response conclusions. The CPython
harness remains migration/context evidence only.

### 3. Resource and tail-latency qualification

At minimum record:

- p50/p95/p99;
- CPU;
- peak/steady RSS;
- fd/thread counts;
- file-stream permits/rejections;
- service admission rejections/recovery;
- errors/timeouts/truncations.

Exercise:

- 64+ active requests;
- many idle keep-alive connections;
- deliberate service/file-stream saturation;
- slow reader;
- stalled reader;
- shutdown under active streaming.

The candidate fails if it improves headline throughput by allowing resource
growth, losing permit recovery, or weakening timeout enforcement.

### 4. Optimization-specific proofs

#### Plan 228
Verify:

- no per-frame producer-progress mutex/notification remains unless the final
  design proved a consumer;
- write-stall still fires on lack of forward socket progress;
- no false stalls under steady writes;
- request/response lifecycle accounting remains exact;
- reduced shared-state clone/dynamic-dispatch path is exercised by direct and
  compatibility H1 tests.

#### Plan 229
Verify:

- chosen chunk size is documented;
- memory implication under `max_file_streams` is bounded;
- short-read/EOF/range/truncate/cancellation behavior is unchanged;
- no whole-file buffering;
- common MIME/header path allocation reductions are observable or at least
  mechanically proven.

#### Plan 230
Verify:

- runtime response has exactly zero/one Date according to policy with no
  throwaway intermediate Date;
- custom/suppress Date semantics are unchanged;
- disabled lazy events do not construct event payloads;
- enabled events/counters/sink panic handling remain unchanged;
- request metadata duplicate/order semantics remain exact.

### 5. Keep/revert rules

For each nontrivial optimization, write one result entry:

```text
change:
target workload:
measured effect:
CPU/allocation/resource effect:
correctness/security result:
decision: KEEP | REVERT | DEFER
rationale:
```

Use confidence intervals/spread or repeated-trial consistency rather than one
best run.

Guidance:

- **KEEP** when improvement is reproducible and no meaningful regression exists,
  or when the change removes dead/redundant work and is simpler while
  performance-neutral.
- **REVERT** when the result is noise, worsens tail/resource behavior, or
  increases complexity without clear benefit.
- **DEFER** when the host/tooling cannot establish the effect or the next step
  would require a qualitatively different design such as sendfile/buffer pools.

Do not set one arbitrary percentage threshold for all workload classes.

### 6. Full correctness/security verification

Run at least the current repository routine matrix:

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
bash scripts/verify-cargo-packages.sh --mode all
```

Also run the direct/compatibility/static authority suites and the production
controls/response streaming tests touched by this campaign. Run Python wheel,
TLS/H2/H3 feature checks according to the current CI matrix.

### 7. Footprint/dependency check

Confirm:

- no new production dependency was introduced;
- default and optional dependency package counts remain understood;
- release/dist binary sizes do not regress unexpectedly;
- the default graph remains free of H3/QUIC;
- `eggserve-primitives` remains transport/runtime neutral.

A small code-size increase may be accepted for a demonstrated runtime win, but
record it.

### 8. Evidence and documentation

Create:

```text
benchmarks/231-optimization-closure/
  README.md
  results.json
  decisions.md
  raw/             # compact trial summaries only
```

Update:

- `benchmarks/README.md` evidence index and interpretation;
- `architecture/testing-and-conformance.md` mapping;
- `architecture/runtime.md` only if internal runtime behavior changed in a
  way maintainers must know;
- `docs/timeout-reference.md` only if wording is needed to clarify the
  transport-write progress authority;
- `plans/ROADMAP.md` with the final program status;
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` only for durable
  implementation facts, not benchmark marketing.

Do not rewrite the historical Plan 170 evidence.

## Explicit deferred ideas

Do not opportunistically add these during closure:

- `sendfile`/TransmitFile/`splice`;
- mmap file cache;
- io_uring;
- custom allocator;
- global buffer pool;
- descriptor/path cache;
- public Service future redesign;
- scheduler/runtime replacement;
- platform-specific socket tuning as a new default.

Any of these requires a separate plan based on a remaining measured bottleneck.

## Acceptance criteria

- [ ] Baseline and candidate SHAs are recorded.
- [ ] Same-machine A/B measurements cover small/medium/large static and custom
      streaming paths.
- [ ] Native small-response conclusions are not based solely on CPython.
- [ ] Tail latency, CPU, RSS, errors, and resource recovery are included.
- [ ] Every Plan 228–230 optimization has KEEP/REVERT/DEFER disposition.
- [ ] Timeout/cancellation/confinement/framing invariants pass.
- [ ] Full routine/feature/package/supply-chain/Python verification passes.
- [ ] No unexpected production dependency was added.
- [ ] Closure evidence and benchmark index are updated.
- [ ] No unsupported universal performance claim is introduced.
