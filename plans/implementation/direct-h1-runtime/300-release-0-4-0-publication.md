# Direct H1 Runtime Milestone 300 — 0.4.0 release publication

Status: active

Repository baseline: `0048f07`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-9--040-release-publication`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Applicable prior publication precedent:

- `release/plan-286-embedding-contract-publication-closure.md`
- `release/plan-291-direct-h1-boundary-ownership-publication-closure.md`
- `docs/release-process.md`

Primary class: polish

## 1. Objective

Publish the 0.4.0 release line to crates.io (plus wheel 0.2.4 to PyPI via
the existing release workflow), tag `v0.4.0`, and create a GitHub Release —
with registry-only consumer proof and checksums per the 286/291 precedent.
No production behavior ships beyond the already-closed 297 (WP-A feature
edge) and 299 (trailer declaration) work; this milestone moves only
versions, requirements, lockfiles, and release metadata.

## 2. Why this milestone is ready

Hard dependencies are closed: 294–297 polish campaign (297 reserves the
0.4.0 minor for the 296 WP-A `tower` feature edge), 299 wire repair (rides
this release; additive primitives API), 298 control-surface reconciliation.
Live registry state verified 2026-09-25: crates.io holds primitives 0.2.1,
server 0.3.1, static/h3/core 0.3.0, bin 0.2.1, eggnet-tls 0.2.0; PyPI holds
eggserve 0.1.0/0.2.0 only. None of the planned new versions exist.

## 3. Current implementation evidence

At the baseline:

- `eggserve-server 0.3.1` carries the WP-A edge (`tower` no longer
  activates `tower-layer`) and the 299 pipeline/adapter/interop changes.
- `eggserve-primitives 0.2.1` carries `TrailerDeclaration` + additive
  declaration-aware constructors (299, unreleased).
- `eggserve-core 0.3.0` requires `eggserve-server ^0.3.0` (cannot resolve a
  0.4.0 server); `eggserve-static`/`eggserve-h3`/`eggserve-bin` likewise.
  Publishing server 0.4.0 therefore forces companion republishes with
  bumped requirement floors.
- Wheel metadata is 0.2.3 across workspace/python-crate/pyproject (never
  published to prod PyPI); `check-python-release-metadata.py` enforces the
  three-way agreement, so the wheel bump moves all three to 0.2.4.
- `~/.cargo/credentials.toml` exists (crates.io token configured);
  `gh` is authenticated with `workflow` scope (release dispatch allowed);
  prod PyPI uses the protected `pypi` environment (human approval required).

## 4. Invariants that must not regress

- `RUSTSEC-2026-0285` rustls `0.23.45` caret floor stays in every
  constraining manifest including the excluded Python crate.
- `deny.toml` wildcard/native-tls bans stay green for both lockfiles.
- Crate topology ownership unchanged (checked by gate before/after bumps).
- Versions immutable once published: never re-upload changed contents under
  an existing version; on failure cut a new version.
- No push/tag/merge auto-publishes; tag is a historical marker only.
- Legacy flat plans + `release/` records stay immutable (new closure lives
  in `plans/closure/`).

## 5. Scope

### In scope

- Version/requirement bumps + lockfile refresh for the 0.4.0 line and wheel
  0.2.4; stale `docs/release-process.md` leaf-line wording.
- Local gates: fmt, topology, conformance matrix, supply-chain (both
  lockfiles), `verify-cargo-packages.sh --mode all`, per-crate
  `cargo publish --locked --dry-run`, workspace tests, Python metadata
  preflight.
- Exact-SHA hosted CI green on the version-bump commit before any upload.
- Serial crates.io publication in dependency order with index-visibility
  waits, checksums, and registry-only consumer proof (286/291 precedent).
- Tag `v0.4.0` + GitHub Release with notes.
- Wheel workflow dispatch `testpypi` → qualify → `pypi` (maintainer
  approves the `pypi` environment; that approval is out of agent hands).
- Plan 300 closure record + registry/roadmap close.

### Explicitly out of scope

- No source/behavior change beyond version/requirement metadata.
- No `eggnet-tls` publication (unchanged; dependents' `^0.2.0` satisfied by
  published 0.2.0; its workspace-inherited version drift is pre-existing).
- No mass archive move; no canonical-doc revision.
- No approving the `pypi` environment (maintainer-only GitHub action).

## 6. Required production changes

### Crates and ownership

Version + requirement metadata only (no crate logic touched):

| Crate | Version | Requirement changes |
|---|---|---|
| `eggserve-primitives` | 0.2.1 → **0.2.2** | none (additive API = patch, 291 precedent) |
| `eggserve-server` | 0.3.1 → **0.4.0** | `eggserve-primitives` `0.2.1` → `0.2.2` |
| `eggserve-static` | 0.3.0 → **0.4.0** | primitives → `0.2.2`, server `0.3.0` → `0.4.0` |
| `eggserve-h3` | 0.3.0 → **0.4.0** | primitives → `0.2.2`, server `0.3.0` → `0.4.0` |
| `eggserve-core` | 0.3.0 → **0.4.0** | primitives → `0.2.2`, server/static/h3 → `0.4.0` |
| `eggserve-bin` | 0.2.1 → **0.2.2** | primitives → `0.2.2`, server/static → `0.4.0`, core/h3 → `0.4.0` |
| `eggserve-python` (excluded) | 0.2.3 → **0.2.4** | core → `0.4.0`, bin `0.2.0` → `0.2.2`, primitives → `0.2.2`, server → `0.4.0` |
| workspace `package.version` | 0.2.3 → **0.2.4** | keeps the three-way wheel agreement (check script) |
| `pyproject.toml` | 0.2.3 → **0.2.4** | same |

Minor (not patch) for server/static/h3/core because the server requirement
moves across the 0.x minor boundary and the server minor itself is driven
by the 296 WP-A feature edge reserved by 297. Patch for primitives
(additive only) and bin (binary crate, requirement-only change, 286
precedent). Static/h3 lockstep 0.4.0 with server per the 286 style (no
source change; requirement-floor move only).

### Config and policy

None.

### Protocol and compatibility

Resolved graphs move to the 0.4.0 line; `^0.3` pins no longer resolve the
new server. Registry-only consumers must prove the direct graph stays free
of core/static/PHF and the core graph resolves the new companions.

### Runtime and concurrency

Not applicable (no runtime change).

### Frontend or operator surface

Wheel 0.2.4 carries the 0.4.0 Rust code through the unchanged release
workflow (10-target matrix, abi3, preflight gates).

### Security and confinement

Supply-chain gates for both lockfiles before and at publication; no new
dependencies (Cargo.lock package set must be requirement-bump-only).

### Documentation and static guards

Update the stale `docs/release-process.md` leaf-line sentence
(`0.3.x`/`bin 0.2.1`/`wheel 0.2.3` → `0.4.x`/`bin 0.2.2`/`wheel 0.2.4`).
Topology + conformance + packaging gates re-run after bumps.

## 7. Ordered work packages

### Work package A — Plan registration

Register Plan 300 (this file) + roadmap Milestone 9 in `plans/registry.md`.
Commit the registration separately from version bumps.

### Work package B — Version bumps + lockfiles

Apply the §6 table, refresh both lockfiles, update the release-process
leaf line. Verify no source file changed (`git diff --stat` shows only
manifests/lockfiles/docs).

### Work package C — Local gates

`cargo fmt`, topology, conformance matrix, Python metadata preflight,
`install-cargo-tools.sh` + `check-supply-chain.sh` (both lockfiles),
`verify-cargo-packages.sh --mode all`, per-crate publish dry-runs,
workspace clippy/tests. Record outcomes; do not proceed on red.

### Work package D — Hosted CI proof

Push the bump commit; require exact-SHA hosted CI success (rust,
supply-chain, python jobs) before any upload, per 226/272/275/286/291
precedent.

### Work package E — crates.io publication

Serial `cargo publish -p <crate> --locked` in order
primitives → server → static → h3 → core → bin, confirming index
visibility before each dependent. Record UTC timestamps + SHA-256
checksums. On any failure, stop; remediate with a new version, never a
re-upload.

### Work package F — Registry-only consumer proof

Fresh `/tmp` manifests with exact versions (direct, Tower, core), locked
reruns, `cargo tree -e no-dev` ancestry checks (no core/static/PHF on the
direct graph), per 286/291 precedent.

### Work package G — Tag + GitHub Release

`git tag v0.4.0` + push; `gh release create v0.4.0` with notes naming the
artifact set, checksums, wheel status, and evidence links.

### Work package H — Wheel publication

Dispatch `release.yml` `publish_target=testpypi` for wheel 0.2.4; qualify;
then dispatch `publish_target=pypi` (maintainer approves `pypi`
environment). Record run IDs + post-publish smoke.

### Work package I — Closure

`plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` with the
§15 evidence; close roadmap Milestone 9 + subsystem and registry rows.

## 8. Failure, cancellation, restart, and contention semantics

- crates.io versions are immutable: a failed-but-published version is never
  overwritten; cut a new version instead.
- Dependents publish only after the prerequisite is index-visible; a
  visibility timeout stops the sequence (no partial-graph claims).
- A rejected/failed wheel run does not affect crates.io artifacts; recovery
  uses a new wheel version when contents must change.
- Release workflow concurrency group serializes dispatches; do not dispatch
  twice.

## 9. Compatibility and migration

- `eggserve-server 0.3.x` consumers stay on the published 0.3.1 artifact;
  nothing already published is yanked or altered.
- Core/static/h3/bin 0.3.x lines stay published and resolvable; the 0.4.0
  line is additive.
- `docs/migration-guide.md` needs no new entry (no API break beyond the
  already-documented 0.4.0 feature edge; 299 is additive with a documented
  H1 declaration path).

## 10. Required tests

- `cargo publish -p <each> --locked --dry-run` (packaging authority).
- `bash scripts/verify-cargo-packages.sh --mode all` (layered local-registry
  proof).
- Registry-only consumers: direct H1 (+TLS), Tower/Axum, core compat —
  build + focused tests + `cargo tree -e no-dev` ancestry.
- Exact-SHA hosted CI (rust/supply-chain/python) green on the bump commit.
- Wheel matrix qualification via the dispatched workflow (preflight +
  aggregate + smoke), not local emulation.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
cargo publish -p eggserve-primitives --locked --dry-run
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-static --locked --dry-run
cargo publish -p eggserve-h3 --locked --dry-run
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-bin --locked --dry-run
cargo test --workspace
```

## 12. Documentation updates

- `docs/release-process.md` leaf-line sentence.
- Source roadmap (Milestone 9) + registry + Plan 300 closure record.

## 13. Acceptance criteria

- All six workspace crates + wheel metadata carry the §6 versions with
  agreeing lockfiles and a manifests/lockfiles/docs-only diff.
- All §11 gates green locally plus exact-SHA hosted CI green.
- crates.io shows the six new versions with recorded checksums; each
  dependent published only after prerequisite visibility.
- Registry-only consumers prove direct-graph purity and core resolution.
- Tag `v0.4.0` pushed; GitHub Release created with notes.
- Wheel 0.2.4 qualified on TestPyPI and published to PyPI (modulo the
  maintainer's `pypi` environment approval, which is explicitly handed off).
- Closure record exists; roadmap/registry agree; no stale control state.

## 14. Stop conditions

Stop and report rather than improvise if:

- any §11 gate is red (fix the cause, do not publish around it);
- hosted CI on the bump commit is not green;
- a planned version already exists on the live index;
- index visibility for a prerequisite never arrives;
- an upload fails after others succeeded (remediate with new versions);
- the `pypi` environment approval is unavailable (wheels stay at TestPyPI);
- the diff shows source changes beyond manifests/lockfiles/docs.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`
containing:

- bump commit SHA + exact-SHA hosted CI run ID/conclusion;
- per-crate UTC timestamps + crates.io SHA-256 checksums;
- registry-only consumer manifests/results/ancestry evidence;
- tag + GitHub Release links;
- wheel workflow run IDs (testpypi + pypi), aggregate manifest, smoke;
- semver rationale restated; residuals and roadmap/registry disposition.

## 16. Handoff notes

crates.io upload needs the maintainer token (present at
`~/.cargo/credentials.toml`; if absent, stop). `gh` has `workflow` scope
for dispatch. Never claim "published" from a dry-run. The `pypi`
environment approval is a human GitHub action — dispatch `pypi` only after
the user confirms, and report the pending-approval state instead of
waiting silently.
