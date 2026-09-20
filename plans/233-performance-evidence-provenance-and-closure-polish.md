# Plan 233 — Performance evidence provenance and closure polish

## Status

**COMPLETE / VERIFIED.**

Closed by the commit recording `benchmarks/233-evidence-polish/` (per-trial
native/range/TLS JSON, closure-SHA CI record, mechanically derived
`results.json`) with successful exact-SHA Rust/Python/supply-chain CI.
No production code, default, dependency, API, or tier changed; the 128 KiB
default stands (see `benchmarks/233-evidence-polish/README.md`).

## Purpose

Polish the evidence trail for the completed Plans 227–232 performance campaign
without changing production runtime behavior.

Plan 232 successfully corrected the file-stream read-bound bug, retained the
128 KiB default using same-machine 64/128 KiB evidence, completed
representative TLS measurements, and closed with green Rust/Python/supply-chain
CI. A post-closure review found three remaining provenance/reporting gaps:

1. the Plan 232 README records the successful CI run for implementation SHA
   `380e5dc4a04b5596e58a678fef51ba701a17596e`, while the subsequent
   documentation closure SHA
   `523e84197fffc9b2ca5834580c7b7d696f86901c` also received a successful CI
   run but that stronger final-state evidence is not recorded;
2. the full three-trial native and TLS JSON captures were written under
   `/tmp` and only compact reductions were committed, so individual trial
   values are not independently auditable from the repository;
3. range correctness was recorded as exact and error-free, but the Plan 232
   record does not retain per-range throughput, p50/p95/p99, RSS, and resource
   data even though the plan requested those performance fields.

These are evidence/provenance defects only. The runtime correction and the
128 KiB default decision remain valid unless the reproduced evidence
materially contradicts the existing record.

## Current verified state

At the start of this plan:

- Plan 232 implementation SHA:
  `380e5dc4a04b5596e58a678fef51ba701a17596e`;
- Plan 232 documentation closure/current HEAD:
  `523e84197fffc9b2ca5834580c7b7d696f86901c`;
- implementation CI run:
  `35483634791`, successful Rust/Python/supply-chain;
- current-HEAD CI run:
  `35484285932`, successful Rust/Python/supply-chain;
- selected file-stream default: 128 KiB;
- no production dependency or public API change was introduced by Plan 232.

The Plan 232 source correction is already covered by deterministic
forced-over-capacity, full-file, range, truncation, and permit-release tests.
Plan 233 does not reopen that implementation.

## Goals

- Preserve compact but complete per-trial benchmark evidence in the repository.
- Record the successful CI result for the actual Plan 232 closure SHA.
- Fill the missing range-performance fields.
- Make the 64 KiB vs 128 KiB and TLS conclusions reproducible from tracked
  files rather than from reductions of discarded `/tmp` captures.
- Keep historical Plan 227/231/232 evidence immutable except for narrow
  cross-references or corrections of provenance wording.
- Require a final successful CI run for the exact Plan 233 closing SHA.
- Avoid any production-code change.

## Hard scope boundary

Plan 233 is evidence-only.

Allowed changes:

- benchmark harness changes that only improve evidence serialization or
  reproducibility and are not part of any distributed/runtime target;
- new benchmark result files under `benchmarks/233-evidence-polish/`;
- documentation, roadmap, and benchmark-index updates;
- narrow comments/cross-references in Plan 232 evidence.

Not allowed:

- changes under production runtime/static/primitives/H3/TLS source trees;
- changing `stream_chunk_size` or any runtime default;
- performance optimization;
- dependency upgrades;
- new production dependency;
- public API changes;
- support-tier changes;
- security-policy changes;
- release-version changes.

If reproducing evidence reveals a production defect or a materially different
64/128 KiB result, stop and write a separate corrective plan rather than
smuggling code changes into Plan 233.

## Required evidence layout

Create:

```text
benchmarks/233-evidence-polish/
  README.md
  results.json
  raw/
    native-64k-trials.json
    native-128k-trials.json
    ranges-64k-trials.json
    ranges-128k-trials.json
    tls-established-trials.json
    tls-handshake-trials.json
    environment.json
  ci/
    plan232-closure-ci.json
    plan233-closing-ci.json
```

The exact file split may vary slightly, but the repository must retain
per-trial values rather than only medians.

Do not commit huge profiler captures, certificates, private keys, sockets, or
temporary working directories.

## Work

### 1. Record Plan 232 final-state CI provenance

Capture the already-completed GitHub Actions run for
`523e84197fffc9b2ca5834580c7b7d696f86901c`:

- workflow run ID: `35484285932`;
- workflow name/path;
- exact head SHA;
- created/completed timestamps;
- overall conclusion;
- Rust job conclusion;
- Python job conclusion;
- supply-chain job conclusion;
- canonical workflow URL.

Store a compact machine-readable copy under
`benchmarks/233-evidence-polish/ci/plan232-closure-ci.json`.

Update `benchmarks/232-corrective/README.md` with a short cross-reference
stating that both the implementation SHA and subsequent closure SHA passed CI,
without deleting the original implementation-run record.

Do not imply the Plan 232 README was false; it correctly recorded the
implementation run available at the time. Plan 233 adds the stronger
final-state evidence.

### 2. Make benchmark captures repository-retained by default

Review the Plan 227 native harness and Plan 170 TLS harness invocation used by
Plan 232.

Where the harness already supports an output path, use that path directly
under `benchmarks/233-evidence-polish/raw/`.

If range probes or TLS handshake sub-results cannot currently serialize
individual trials, make the smallest benchmark-only harness change needed to
emit deterministic JSON.

Any harness change must:

- remain outside production code;
- add no production dependency;
- preserve existing output semantics;
- record raw per-trial data plus aggregate summaries;
- record errors for every trial;
- include the exact source SHA and runtime configuration;
- avoid timestamps or nondeterministic metadata where unnecessary.

### 3. Reproduce the 64 KiB vs 128 KiB native matrix with per-trial retention

Use current production code and vary only the configured/default chunk regime
through a benchmark-safe mechanism.

Prefer a benchmark/runtime configuration override over editing production
source between runs. If the existing harness can only exercise the compile-time
default, a temporary source override may be used for measurement only, but:

- the exact patch/diff must be recorded;
- the tree must be restored and clean before final evidence is committed;
- the final tracked production default remains 128 KiB.

At minimum retain all three measured trials after warm-up for:

**Static H1**
- 1 KiB: concurrency 1, 16, 64;
- 128 KiB: concurrency 1, 16, 64;
- 1 MiB: concurrency 1, 16, 64;
- 16 MiB: concurrency 16 and 64 if stable.

Per trial retain:

- requests completed;
- elapsed time;
- RPS;
- bytes/s;
- p50;
- p95;
- p99;
- peak RSS;
- end RSS;
- fd count;
- thread count;
- CPU delta/utilization where the harness can obtain it;
- errors/timeouts/truncations.

The aggregate file may report medians/means, but it must be mechanically
derivable from the retained trial rows.

### 4. Capture complete range performance evidence

Run exact range probes for both 64 KiB and 128 KiB chunk regimes.

At minimum:

- 64 KiB range from a larger representation;
- 512 KiB range from a larger representation;
- concurrency 1, 16, 64;
- three measured trials after warm-up.

Each trial must validate:

- status 206;
- exact `Content-Length`;
- exact response bytes;
- no bytes before/after the selected range;
- zero truncation/protocol errors.

Each trial must also retain:

- elapsed time;
- RPS;
- bytes/s;
- p50/p95/p99;
- peak/end RSS;
- fd/thread counts where available;
- error count.

If the range harness currently performs only correctness probes, extend the
benchmark-only harness rather than adding production instrumentation.

### 5. Retain complete TLS trial evidence

Repeat the representative Plan 232 TLS qualification using the final 128 KiB
production default.

Retain individual trial rows for:

**Established keep-alive**
- 1 KiB at concurrency 1, 16, 64;
- 1 MiB at concurrency 1, 16, 64.

**Handshake churn**
- the same connection count used by Plan 232, or a clearly documented
  equivalent;
- at least three trials.

For established TLS retain:

- RPS;
- bytes/s where available;
- p50/p95/p99;
- elapsed time;
- peak/end RSS;
- CPU where available;
- errors.

For handshake trials retain:

- attempted connections;
- successful handshakes;
- elapsed time;
- handshakes/s;
- failures/timeouts.

Record certificate generation parameters but never commit private keys or
short-lived certificate material.

### 6. Capture environment and exact input identity

The evidence must make accidental cross-run comparison difficult.

Record:

- exact source SHA;
- `Cargo.lock` hash;
- Python crate lockfile hash where relevant;
- Rust toolchain;
- OS/kernel;
- architecture;
- CPU;
- logical CPU count;
- memory;
- release profile;
- enabled features;
- runtime limits;
- chunk size;
- max file streams;
- benchmark commands;
- warm-up policy;
- trial count;
- TLS certificate generation command;
- client implementation.

If measurement is run from a dirty tree for the temporary 64 KiB override,
record:

- base SHA;
- exact temporary diff hash/content;
- confirmation that the final committed tree restored 128 KiB.

### 7. Produce one machine-readable aggregate

`benchmarks/233-evidence-polish/results.json` should summarize the retained
raw files, not replace them.

Include:

- references to every raw artifact;
- 64/128 KiB median comparisons;
- range aggregate comparisons;
- TLS established aggregates;
- TLS handshake aggregates;
- zero/nonzero error totals;
- peak RSS comparison;
- final default: 128 KiB, unless the reproduced data materially contradicts
  Plan 232;
- an explicit statement that no production code changed under Plan 233.

Do not invent confidence intervals from three trials. Report trial spread or
min/median/max instead.

### 8. Reassess—but do not silently change—the Plan 232 decision

Compare the reproduced data with Plan 232's conclusions.

Expected closure is:

- small responses remain roughly neutral;
- 128 KiB retains a meaningful advantage for 1 MiB/large static responses;
- 64 KiB retains lower high-concurrency RSS;
- 128 KiB remains the selected default because the throughput/resource
  tradeoff is acceptable and bounded.

If results materially reverse those conclusions:

- mark Plan 233 `BLOCKED / NEW CORRECTIVE REQUIRED`;
- do not change runtime defaults in this plan;
- preserve all evidence;
- write a new production corrective plan.

### 9. Documentation reconciliation

Update:

- `benchmarks/README.md` evidence index;
- `benchmarks/232-corrective/README.md` with the final-state CI
  cross-reference and Plan 233 evidence link;
- `architecture/testing-and-conformance.md` with the retained-per-trial
  evidence location;
- `plans/ROADMAP.md` with Plan 233 status;
- `AGENTS.md` / `.opencode/skills/eggserve-dev/SKILL.md` only if a durable
  maintainer rule changes, e.g. future manual performance qualification must
  retain compact per-trial JSON rather than only aggregate reductions.

Do not rewrite historical numeric evidence in Plans 227, 231, or 232.

### 10. Verification

Because production code must not change, verification should prove both
repository cleanliness and documentation/evidence consistency.

Run at minimum:

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

Also verify:

- `git diff` contains no production-runtime source changes;
- final `DEFAULT_STREAM_CHUNK_SIZE` remains 128 KiB;
- no certificate/private-key file is tracked;
- all JSON evidence parses;
- every aggregate row can be traced to retained raw trials;
- raw trial files identify the exact source/configuration they measured.

### 11. Final exact-SHA remote CI gate

After committing Plan 233 evidence/documentation, wait only for normal GitHub
Actions completion; do not create another documentation-only commit merely to
record a run unless the repository's established convention requires it.

The exact final SHA must have successful:

- Rust;
- Python;
- supply-chain

jobs.

Record the final workflow run in
`benchmarks/233-evidence-polish/ci/plan233-closing-ci.json`.

If recording that run necessarily requires a follow-up metadata commit, that
metadata-only commit must itself also receive a successful CI run, or the
README must clearly distinguish the verified implementation/evidence SHA from
the later non-semantic metadata commit. Prefer avoiding an infinite
"record-CI-then-create-new-SHA" loop.

## Acceptance criteria

Plan 233 is complete only when:

- [x] Plan 232 closure SHA `523e841...` and workflow run `35484285932`
      are preserved in repository evidence;
- [x] individual three-trial 64 KiB and 128 KiB native measurements are
      retained under version control;
- [x] individual range trials retain correctness plus throughput, latency,
      and RSS/resource fields;
- [x] individual TLS established and handshake trials are retained under
      version control;
- [x] `results.json` is mechanically traceable to retained raw files;
- [x] environment, lockfile, source-SHA, runtime-limit, and command identity
      are recorded;
- [x] no production runtime/default/API/dependency change is introduced;
- [x] 128 KiB remains selected unless reproduced evidence requires a separate
      corrective plan;
- [x] historical Plan 227/231/232 raw evidence is not rewritten;
- [x] documentation accurately explains the provenance relationship among
      Plans 231, 232, and 233;
- [x] all retained JSON parses and contains zero unexplained benchmark errors;
- [x] routine Rust/Python/supply-chain verification passes;
- [x] the exact final Plan 233 closing SHA has successful remote CI.

## Handoff order

1. Capture the Plan 232 current-HEAD CI metadata.
2. Add/adjust benchmark-only serialization if needed.
3. Re-run and retain 64 KiB / 128 KiB native trials.
4. Re-run and retain range performance trials.
5. Re-run and retain TLS trials.
6. Build aggregate `results.json` directly from retained raw data.
7. Reconcile documentation and benchmark index.
8. Verify no production source/default changed.
9. Run local correctness/supply-chain checks.
10. Commit/push and require exact-SHA CI.

No production optimization should be performed under this plan.
