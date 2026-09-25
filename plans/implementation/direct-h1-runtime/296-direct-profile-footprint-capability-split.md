# Direct H1 Runtime Milestone 296 — Direct-profile footprint and capability split

Status: closed (closure: `plans/closure/direct-h1-runtime/296-direct-profile-footprint-capability-split.md`; WP-A KEEP, file/tunnel NO-GO)

Repository baseline: `81605fc36b970440d6e3edbfd1e0d1cfa3ec4d91` (planning baseline; implementation must refresh after 294)

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-6--direct-profile-capability-and-dependency-footprint`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/002-long-term-roadmap.md`

Primary class: polish

## 1. Objective

Reduce the compile/link and dependency footprint of direct application-server consumers only where Milestone 294 proves a credible benefit. Evaluate file-body transport capability separation first, tunnel gating second, then remove mechanically unnecessary direct-profile feature/dependency edges and tighten Tokio/futures feature selection.

This milestone may close partially or entirely NO-GO.

## 2. Why this milestone is ready

It is blocked on Milestone 294. The baseline must establish actual graph/linked-size contribution before public feature layout changes are attempted.

## 3. Current implementation evidence

Planning-time candidates:

- `eggserve-server` is intentionally free of `eggserve-static`, but its response adapter still supports canonical file-body variants and therefore uses Tokio `fs`/seek/read machinery and file-stream permits.
- `RuntimeState` owns file-stream admission state even for consumers that never emit file bodies.
- direct Tower/Axum application servers such as EggPool do not need EggServe file-body transport.
- tunnel/upgrade machinery is always compiled into the direct server despite ordinary application APIs not requiring CONNECT/upgrades.
- the `tower` feature currently activates `tower-layer`; production adapter code composes with Tower `Layer`s but does not need to implement the `tower-layer` crate contract itself.
- Tokio `fs` is unconditionally enabled by `eggserve-server`.

## 4. Invariants that must not regress

- `eggserve-static` remains sole filesystem/path/static-policy authority.
- `eggserve-server` remains the sole H1 runtime authority.
- Default/high-level EggServe product behavior remains available under its documented feature/profile selection.
- Existing published direct-server consumers must have a documented migration path for any feature-layout change.
- No feature combination may silently remove a security control while leaving the corresponding capability reachable.
- Topology gate remains the authority for core/static/PHF/QUIC ownership.

## 5. Scope

### In scope

- confirm whether file response transport can be compile-time gated cleanly;
- move Tokio `fs` activation behind that capability when possible;
- avoid file-stream semaphore/state in builds that cannot produce file bodies;
- evaluate tunnel/upgrade feature gating only if linked-size or common-path benefit is material and feature boundaries remain understandable;
- remove `tower-layer` from the direct `tower` feature if confirmed unused;
- audit Tokio and futures feature sets for unnecessary activation;
- add topology/feature-matrix guards;
- classify semver and package publication requirements.

### Explicitly out of scope

- no new micro-crate solely to save bytes;
- no move of static planning into server;
- no sendfile/splice/io_uring/mmap;
- no removal of capabilities from core/bin/Python products without preserving their documented composition;
- no hidden cargo-feature behavior change without docs/tests.

## 6. Required production changes

### Crates and ownership

Prefer feature-gating modules/dependencies inside `eggserve-server`. `eggserve-static`/core/bin may enable the capability explicitly as needed. Do not split a new crate unless 294 plus implementation evidence shows feature gating cannot preserve ownership and the user approves an architecture change.

### Config and policy

If a capability is compile-time absent, runtime configuration referring to that capability must not exist in a misleading half-active state. Prefer cfg-gated API only when semver/version decision explicitly accepts it; otherwise retain inert-compatible fields with documented behavior only if that is clearer and safe.

### Protocol and compatibility

File-body support is transport output machinery, not filesystem policy. Tunnel gating must preserve ordinary HTTP framing and reject/absence semantics consistently.

### Runtime and concurrency

A no-file build should not allocate/initialize file-stream admission state. A no-tunnel build should not carry tunnel JoinSet/permit/state on ordinary requests if gating is retained.

## 7. Ordered work packages

### Work package A — Trivial graph pruning

Confirm and, if safe, remove unused `tower-layer` activation/dependency. Audit exact Tokio/futures feature use. These may proceed only when mechanical proof is clear; record binary impact even if linked bytes are neutral.

### Work package B — File-body feature experiment

Create a branch/fixture proving:
- direct Tower/Axum builds without file transport;
- static/core/bin consumers explicitly regain file transport;
- topology and package builds remain coherent;
- stripped direct fixture size and no-dev graph improve or the design materially narrows activated Tokio/runtime machinery.

Decide KEEP or NO-GO before polishing API.

### Work package C — Tunnel gating experiment

Proceed only if 294 attributes meaningful size/hot-path cost and the feature model is simpler than the unconditional state. Otherwise close DEFER/NO-GO.

### Work package D — Semver and migration

Classify:
- source compatibility;
- Cargo feature compatibility;
- exhaustive config/public enum impact;
- current core/static/bin/Python composition;
- registry publication set.

Do not publish in this milestone unless explicitly directed; record the release candidate strategy for 297.

## 8. Failure, cancellation, restart, and contention semantics

Every retained feature combination must preserve resource release, shutdown, body cancellation, and admission behavior for capabilities present in that build. Missing capabilities must fail at compile/configuration boundaries rather than producing half-implemented runtime behavior.

## 9. Compatibility and migration

Because `eggserve-server` is pre-1.0 and experimental, a feature-layout change may still require a new minor rather than a patch. Determine this from actual public API impact; do not synchronize sibling crate versions aesthetically.

## 10. Required tests

- cargo check/test for server default/no-default/tower/http-interop and new capability combinations;
- static/core/bin composition tests proving file serving still works;
- direct Tower fixture proving file/static ancestry remains absent;
- tunnel suites for enabled builds and compile/config tests for disabled builds;
- topology script self-tests for forbidden ancestry/feature leakage;
- package dry-run for changed manifests.

## 11. Required verification commands

```bash
cargo +1.89 check -p eggserve-server --all-targets --no-default-features
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
cargo test -p eggserve-server --no-default-features --features tower
python3 scripts/check-crate-topology.py
bash scripts/verify-cargo-packages.sh --mode all
./scripts/verify.sh fast
```

Add explicit new feature combinations to CI/static guards if retained.

## 12. Documentation updates

- `docs/dependency-policy.md`;
- direct embedding docs;
- architecture crate-topology/runtime pages;
- migration guide when feature selection changes;
- release contract only after actual version/publication decision.

## 13. Acceptance criteria

- Every retained split has measured/mechanical justification from 294.
- Direct Tower graph remains core/static/PHF-free and is no larger.
- File-disabled direct builds do not activate Tokio filesystem/file-stream machinery if the file split is kept.
- Static/high-level product paths retain file serving with explicit composition.
- Tunnel gating is kept only with clear benefit and simple compatibility semantics.
- Unused dependency/feature edges are removed when mechanically safe.
- Semver/publication impact is explicitly classified.

## 14. Stop conditions

Stop if the split duplicates static/server ownership, requires a new crate without approval, causes combinatorial feature complexity, or yields no meaningful graph/link benefit beyond cosmetic cfg churn. Stop before a breaking public feature change until semver/migration is documented.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/296-direct-profile-footprint-capability-split.md` with candidate-by-candidate KEEP/NO-GO decisions, before/after cargo trees and stripped sizes, feature matrix, semver classification, and verification results.

## 16. Handoff notes

Binary size must be measured with identical stripped release fixtures. Cargo package count alone is insufficient. LTO may erase some unreachable code; treat that as evidence against unnecessary feature complexity.
