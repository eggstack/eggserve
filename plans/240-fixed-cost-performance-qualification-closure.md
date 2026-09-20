# Plan 240 — Fixed-cost performance qualification and closure

## Prerequisites

- Plan 234 baseline is complete.
- Candidate implementations or explicit NO-GO records exist for Plans 235–239.

## Purpose

Qualify the second performance campaign against its own current-head baseline,
retain only changes that remove proven fixed costs without semantic/resource
regression, and close the program with auditable evidence.

This plan does not authorize new optimizations discovered during closure.

## Candidate freeze

Record:

- Plan-234 baseline SHA;
- candidate SHA;
- exact commits belonging to Plans 235–239;
- root and Python lockfile hashes;
- compiler/profile/features;
- runtime limits;
- any tracks that closed NO-GO/DEFER before implementation.

Do not mix dependency upgrades or unrelated features into the candidate.

## Same-machine A/B matrix

Run baseline and candidate in alternating order with warm-up and at least three
measured trials.

### Native H1

- custom 1 KiB bytes: c1/c16/c64;
- static 1 KiB: c1/c16/c64;
- static 128 KiB and 1 MiB representative points;
- HEAD and 304;
- short/nested/query/percent-encoded static paths;
- representative range request;
- caller-owned H1 smoke/perf sanity.

### TLS

- established keep-alive 1 KiB;
- representative metadata-light session;
- opt-in peer-certificate exposure case where practical;
- handshake churn only as a regression check, not as evidence for per-request
  metadata changes.

### Python

- trivial no-body handler;
- handler reading only method;
- text-header and byte-header consumers;
- metadata-heavy consumer;
- bytes response;
- synchronous streamed response;
- 10/100/N slow-stream resource points selected by Plan 234.

## Metrics

Record:

- RPS/throughput where meaningful;
- p50/p95/p99;
- CPU;
- allocation counts or equivalent retained evidence;
- RSS;
- fd count;
- thread count for Python;
- syscall counts for static resolver samples;
- admission rejections/recovery;
- errors/timeouts/truncations.

## Optimization-specific closure

### Plan 235

Prove:

- one-buffer request target preserves all accessors and parsing semantics;
- complete/lazy body state removes the intended allocation/sync work;
- header unique lookup is allocation-free on the common path;
- no object-size/resource regression offsets the gain.

### Plan 236

Prove:

- non-root hardened Unix lookup no longer duplicates the pinned root FD;
- the security-required pre/post-open checks remain;
- path fast path is byte/decision-equivalent;
- range behavior is unchanged.

### Plan 237

Prove:

- the internal outer dynamic-dispatch layer is removed if retained;
- direct and compatibility H1 still share one pipeline;
- timeout/tunnel behavior is unchanged;
- tunnel fast path and metadata sharing have independent KEEP/NO-GO decisions.

### Plan 238

Prove:

- any request-state consolidation actually removes an allocation rather than
  moving it;
- common object size/locking remains acceptable;
- interim/lifecycle concurrency semantics are intact.

### Plan 239

Prove:

- Python properties are behavior-compatible;
- lazy views reduce real construction cost;
- stream resource scaling is no worse;
- any changed producer architecture retains bounded backpressure, GIL
  isolation, cancellation, and shutdown behavior.

## Keep/revert policy

For every nontrivial change record:

```text
change:
target cost/workload:
baseline evidence:
candidate evidence:
allocation/syscall/resource effect:
correctness/security result:
decision: KEEP | REVERT | DEFER
rationale:
```

KEEP when a change:

- removes a mechanically unnecessary fixed cost with equal/simpler code and no
  regression; or
- shows a reproducible workload/resource improvement with no meaningful
  tail/correctness/security regression.

REVERT when it increases complexity, common object size, synchronization,
resource use, or maintenance burden without a clear benefit.

DEFER when the clean implementation would require a public API redesign,
qualitatively different I/O architecture, cache, allocator, or unsupported
platform assumption.

## Full verification

At minimum run the current repository routine matrix:

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
bash scripts/test-python-wheel.sh
```

Also run targeted path/confinement, direct-H1 parity, tunnel, proxy/TLS,
request-body/trailer/interim, and Python stream/shutdown suites affected by the
candidate.

Run platform qualification when a retained change touches shared filesystem
behavior in a way relevant to Windows, even though the Unix root-FD optimization
itself is cfg-unix.

## Footprint/API check

Confirm:

- no production dependency added;
- public documented Rust/Python API remains source-compatible;
- no default H3/QUIC graph expansion;
- release/dist sizes have no unexplained regression;
- `eggserve-primitives` remains transport neutral;
- `eggserve-static` remains the sole confinement authority.

## Evidence layout

Create:

```text
benchmarks/240-fixed-cost-closure/
  README.md
  results.json
  decisions.md
  raw/
  profiles/
```

Update `benchmarks/README.md`, `architecture/testing-and-conformance.md`,
`plans/ROADMAP.md`, and durable architecture/agent docs only for facts that
actually landed. Do not describe planned optimizations as current behavior.

## Acceptance criteria

- [ ] Baseline/candidate SHAs and environment are recorded.
- [ ] Small native/static, path-heavy, TLS, and Python cases are compared.
- [ ] Allocation/syscall/resource effects accompany timing results.
- [ ] Every Plan 235–239 track has KEEP/REVERT/DEFER or NO-GO disposition.
- [ ] Security/confinement/framing/lifecycle/timeout tests pass.
- [ ] Routine/package/supply-chain/Python-wheel checks pass.
- [ ] No unsupported universal performance claim is introduced.
- [ ] Final docs distinguish retained behavior from deferred ideas.
