# Direct H1 Runtime Milestone 296 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/296-direct-profile-footprint-capability-split.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-6--direct-profile-capability-and-dependency-footprint`

Repository baseline reviewed: `f2d581e0ce1fb18540f63175c319242162c48b6d`
plus 294/295 work and this milestone's manifest change

Implementation commits or pull requests:

- (this closure commit) — WP-A deactivation + docs, NO-GO evidence for
  WP-B/WP-C, benchmark evidence

## 1. Executive finding

The milestone closes with one kept pruning change and two evidence-backed
NO-GOs, exactly the outcome the plan anticipates ("may close partially or
entirely NO-GO"). `tower-layer` activation is removed from the direct
`tower` feature (−1 graph node, link-neutral, gate-green). File-body and
tunnel splits were experimented to the point of measured attribution
(~35 KB / ~30 KB upper bounds) and stopped before API polish: the benefit
does not justify public-accessor/contract/config churn. No new crate, no
static/server ownership move, no hidden behavior change, no publication.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Every retained split has 294 justification | WP-A: mechanically unnecessary edge (production never uses Layer APIs) | pass | Link-neutral hygiene, not a size win |
| Direct Tower graph core/static/PHF-free and no larger | no-dev tree 38 lines; topology gate | pass | `tower-layer` absent; nothing added |
| File-disabled builds avoid fs machinery if kept | N/A (NO-GO) | pass | Stopped before polish; static/core/bin file paths untouched and green |
| Static/high-level product paths retain file serving | `eggserve-static` 345 tests; `static_authority_conformance` 7 tests | pass | No composition change needed |
| Tunnel gating only with clear benefit + simple semantics | NO-GO by 294 gate (no attributed cost) | pass | Contract fork rejected with rationale |
| Unused dependency/feature edges removed when safe | `tower-layer` deactivated; Tokio/futures audit finds all edges used | pass | Audit recorded in `raw/footprint.txt` |
| Semver/publication impact classified | Minor-level at most (feature-edge removal); no bump, no publish | pass | Final call deferred to 297 |
| Feature-matrix checks | default / no-default / tower / http-interop checks + MSRV 1.89 lanes | pass | — |
| Package dry-run | `verify-cargo-packages.sh --mode all` (direct consumer 45 pkgs/110 nodes/579568 B) | pass | — |

## 3. Production implementation evidence

- `crates/eggserve-server/Cargo.toml`: `tower` feature drops
  `dep:tower-layer`; `tower-layer` added to dev-dependencies; ownership
  comment. The optional dependency stays declared (Plan-276 gate rule).
- `crates/eggserve-server/src/tower.rs`: middleware-boundary docs
  reworded (no dangling `tower_layer::` intra-doc link).
- `docs/http-interop.md`, `docs/dependency-policy.md`: composition
  documented (downstream `Layer` authors depend on `tower-layer` directly).
- No cfg gates, no new features, no config/API/behavior change. No new
  crate (explicitly rejected per scope).

Candidate decisions: **WP-A KEEP; WP-B NO-GO; WP-C NO-GO.** Tokio/futures
pruning: nothing unused (audit only). Topology-gate change: deliberately
not made (existing rules guard ownership; deactivation proven by trees).

## 4. Verification executed

### Commands run

```bash
cargo +1.89 check -p eggserve-server --all-targets --no-default-features
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
cargo check -p eggserve-server --all-targets --no-default-features --features http-interop
cargo check -p eggserve-server --all-targets
cargo test -p eggserve-server --no-default-features --features tower --test interop_http_tower
cargo test -p eggserve-server --no-default-features --features tower --test tunnel_upgrade
cargo test -p eggserve-static
cargo test -p eggserve-core --test static_authority_conformance
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
bash scripts/verify-cargo-packages.sh --mode all
```

### Results

All green: MSRV lanes clean; feature matrix clean; tower interop 16,
tunnel 10, static 345, conformance 7 passed; topology gate + 25 self-tests
pass; package dry-run passes with registry-consumer proof (direct Axum
consumer resolves and runs against the changed manifest).

## 5. Invariant review

- `eggserve-static` sole filesystem authority; `eggserve-server` sole H1
  authority: no ownership moved (NO-GOs); docs reaffirm the split.
- Default/high-level product behavior available: unchanged (no default
  change; file serving proven by suites).
- No silent security-control removal: the only change removes an
  *activation edge* for an unused trait crate; middleware still composes
  after parsing/validation and before normalization (doc contract kept).
- Topology gate remains authoritative and green.

## 6. Failure and recovery review

- Missing-capability-at-compile-boundary: WP-A introduces no missing
  capability (services never implemented `Layer`); downstream authors get a
  clear one-line manifest recipe in docs.
- Feature-combination resource behavior: unchanged (no combinations added).
- Tunnel/file suites green with no change (regression tripwires intact).

## 7. Migration and compatibility review

WP-A is a manifest-level edge removal: anyone relying on transitive
` tower-layer` via `eggserve-server/tower` adds `tower-layer = "0.3"`.
Source compatibility otherwise complete. Pre-1.0 experimental line:
minor-level at most, **not patch**; no version bump and no publication in
this milestone. 297 makes the release call. No migration guide page needed
(one-line recipe lives in `docs/http-interop.md`).

## 8. Security review

Dependency-graph narrowing only (−1 node). No confinement, framing, or
default change. Supply-chain posture unchanged or marginally narrower.

## 9. Documentation and operations

- `benchmarks/296-direct-profile-footprint/` (README, results.json, raw
  trees/sizes/attribution/consumer numbers).
- `benchmarks/README.md` index row.
- `docs/http-interop.md` + `docs/dependency-policy.md` composition updates.
- Architecture pages need no change (no ownership/architecture delta).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | ~35 KB file-body + ~30 KB tunnel code linked into file/tunnel-free direct binaries (public-API reachability defeats LTO) | 3% order link overhead for narrowest consumers | Accepted; revisit only with a cheaper seam (e.g. post-1.0 API redesign), never as silent cfg churn |
| low | Symbol attribution is an upper bound (shared blocking-pool monomorphizations counted) | True separable saving is smaller than quoted | Recorded; supports rather than weakens the NO-GO |

## 11. Roadmap disposition

Milestone closed. Retained WP-A change plus 295's P1/P2 proceed to
Milestone 297 qualification. 297 is now unblocked (all hard dependencies
closed or explicitly NO-GO).

## 12. Registry updates

- `plans/registry.md`: 296 → closed; 297 → ready.
- `plans/subsystems/direct-h1-runtime-roadmap.md`: milestone 6 → closed
  with closure link; milestone 7 → ready.
- `plans/implementation/direct-h1-runtime/296-*.md`: status → closed.
