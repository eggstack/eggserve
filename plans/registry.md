# EggServe Active Planning Registry

This file is the compact control surface for active interim planning. Detailed requirements and completed history remain in source roadmaps, implementation plans, `closure/` (new) and `release/` (legacy), and Git history.

Canonical direction remains in:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

New milestone numbers continue from `316`. Legacy flat plans (`000`–`292`) and `release/plan-*.md` are archived in place and immutable.

## Status vocabulary

- **proposed** — roadmap or plan exists but is not approved for execution.
- **ready** — dependencies and interfaces are satisfied; plan may be handed off.
- **active** — implementation or closure work is in progress.
- **blocked** — a named dependency or evidence requirement prevents progress.
- **closing** — implementation landed and closure evidence is being gathered.
- **closed** — closure record accepted.
- **conditionally closed** — substantial work landed, but a named correctness or operational evidence condition remains.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

Dependency vocabulary: **hard / interface / soft / operational** (`003-planning-process.md` §4).

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Static confinement + static service | active | `plans/subsystems/static-confinement-roadmap.md` | 310 fuzz-harness repair + build gate | Reopened: the roadmap's "11 fuzz targets" verification claim is false at the baseline (4 of 11 targets do not compile; 1 is inert). Sole static/path/filesystem authority itself is unchanged. |
| Direct H1 runtime + service contract | active | `plans/subsystems/direct-h1-runtime-roadmap.md` | 304 0.4.0 publication closure | 294–297 polish campaign closed; 299 F3 repair closed. Plan 300's publication completed on 2026-09-25 but was never closed; Milestone 304 records it and discloses that the published artifact predates the post-tag corrective batch (Milestone 305). |
| Planning governance | active | `plans/subsystems/planning-governance-roadmap.md` | 314 publication-state staleness sweep | Milestones 1 and 302 closed. Reopened: six surfaces still describe the published 0.4.0 line as pending, including the agent-facing skill. Interface dependency: 304's closure record supplies the facts. |
| Python facade | proposed | `plans/subsystems/python-facade-roadmap.md` | 311 directory-listing renderer consolidation | New roadmap. Depends on the closed `eggserve-static` listing authority and the closed core runtime projection. |
| CLI, ops, and observability | proposed | `plans/subsystems/cli-ops-observability-roadmap.md` | 312 cross-platform compile coverage | New roadmap. 313 (lint policy) and 315 (dependency specification consolidation) are independent of 312. |
| Docs architecture (overview index + deep dives) | closed | `plans/subsystems/docs-architecture-roadmap.md` | 301 closed (overview + deep dives refreshed to 0.4.0 baseline) | Trace: `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md`. Interface: closed 294–299 code state + Plan 300 publication as review input. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies / handoff note |
|---|---|---|---|---|
| Docs architecture | 293 | closed | `plans/implementation/docs-architecture/293-architecture-overview-deep-dive-refresh.md` | Done; closure: `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md`. |
| Direct H1 runtime | 294 | closed | `plans/implementation/direct-h1-runtime/294-direct-tower-footprint-baseline.md` | Evidence gate closed; closure: `plans/closure/direct-h1-runtime/294-direct-tower-footprint-baseline.md`. No production changes. |
| Direct H1 runtime | 295 | closed | `plans/implementation/direct-h1-runtime/295-direct-tower-hotpath-optimization.md` | Done; closure: `plans/closure/direct-h1-runtime/295-direct-tower-hotpath-optimization.md` (P1+P2 KEEP). |
| Direct H1 runtime | 296 | closed | `plans/implementation/direct-h1-runtime/296-direct-profile-footprint-capability-split.md` | Done; closure: `plans/closure/direct-h1-runtime/296-direct-profile-footprint-capability-split.md` (WP-A KEEP; file/tunnel NO-GO). |
| Direct H1 runtime | 297 | closed | `plans/implementation/direct-h1-runtime/297-direct-application-server-qualification-closure.md` | Done; closure: `plans/closure/direct-h1-runtime/297-direct-application-server-qualification-closure.md`. Campaign complete; no publication (next server release minor when cut). |
| Planning governance | 298 | closed | `plans/implementation/planning-governance/298-planning-control-surface-reconciliation.md` | Done; closure: `plans/closure/planning-governance/298-planning-control-surface-reconciliation.md`. Bootstrap retired; agent pointers durable. |
| Direct H1 runtime | 299 | closed | `plans/implementation/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md` | Done; closure: `plans/closure/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md`. F3 wire repair landed; no publication. |
| Docs architecture | 301 | closed | `plans/implementation/docs-architecture/301-architecture-overview-deep-dive-refresh-040.md` | Done; closure: `plans/closure/docs-architecture/301-architecture-overview-deep-dive-refresh-040.md`. |
| Direct H1 runtime | 300 | conditionally closed | `plans/implementation/direct-h1-runtime/300-release-0-4-0-publication.md` | Publication completed 2026-09-25 and is fully evidenced (per-crate crates.io checksums + timestamps, exact-SHA CI run `36176532051`, registry-only consumer proofs, tag `v0.4.0` + GitHub Release, PyPI `eggserve 0.2.4` wheel set + post-publish smokes). Closure: `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`. **Open condition:** the published artifact predates `v0.4.0..465b80c` (B1–B89 + the CI fix), so Milestone 305 must republish it. |
| Planning governance | 302 | closed | `plans/implementation/planning-governance/302-agent-skill-docs-staleness-sweep-040.md` | Done; closure: `plans/closure/planning-governance/302-agent-skill-docs-staleness-sweep-040.md` (docs-only 0.4.0-line sweep). |
| Docs architecture | 303 | closed | `plans/implementation/docs-architecture/303-readme-quickstart-user-docs-accuracy.md` | Done; closure: `plans/closure/docs-architecture/303-readme-quickstart-user-docs-accuracy.md` (README quickstart rewrite + user-docs accuracy). |
| Direct H1 runtime | 304 | closed | `plans/implementation/direct-h1-runtime/304-040-publication-closure.md` | Closes Plan 300 honestly: publication evidence + registry correction. Closure: `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`. **Conditionally closed** outcome — the published 0.4.0 artifact predates `v0.4.0..465b80c` (B1–B89 + CI fix), which Milestone 305 must republish. |
| Direct H1 runtime | 305 | conditionally closed | `plans/implementation/direct-h1-runtime/305-corrective-040x-publication.md` | Corrective publication of the post-0.4.0 fix batch. Version line prepared and gated: `primitives 0.2.3`, `server`/`static`/`h3`/`core 0.5.0`, `bin 0.2.3`, wheel `0.2.5`. Closure: `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`. **Open condition:** the upload/tag/release steps are outstanding — they require the maintainer's crates.io token, the `pypi` environment approval, and a pushed bump commit whose exact-SHA CI is green. |
| Direct H1 runtime | 306 | closed | `plans/implementation/direct-h1-runtime/306-runtime-error-representation-single-authority.md` | The runtime error representation now has one authority (`eggserve-primitives::canonical::runtime_error_with_policy`); the two hand-rolled builders delegate to it and the dead helper cluster is gone. Wire output proven byte-identical across a 44-cell matrix plus a live TCP exchange. Closure: `plans/closure/direct-h1-runtime/306-runtime-error-representation-single-authority.md`. |
| Direct H1 runtime | 307 | closed | `plans/implementation/direct-h1-runtime/307-connection-overlap-classification-accuracy.md` | The Plan 253 ledger is corrected (all eight pairs are classified parallel implementations, not shared kernels), measured, and machine-checked by a drift bound in the topology gate. The H3 `Alt-Svc` advertisement policy is single-sourced in `eggserve-primitives`. Closure: `plans/closure/direct-h1-runtime/307-connection-overlap-classification-accuracy.md`. |
| Direct H1 runtime | 308 | closed | `plans/implementation/direct-h1-runtime/308-compatibility-config-projection-parity.md` | The compatibility `RuntimeConfig` now carries the four direct-H1 ownership values with unchanged defaults, and the compatibility builder delegates 37 of its 38 shared setters to the direct builder. CLI and Python surfaces untouched. Closure: `plans/closure/direct-h1-runtime/308-compatibility-config-projection-parity.md`. |
| Direct H1 runtime | 309 | ready | `plans/implementation/direct-h1-runtime/309-accept-error-classification-parity.md` | One accept-error classification authority for the direct and compatibility accept loops (exhaustion detection, severity mapping, backoff ramp). |
| Static confinement | 310 | ready | `plans/implementation/static-confinement/310-fuzz-harness-repair-and-build-gate.md` | Repair the 4 non-compiling fuzz targets, rebind the Windows directory-buffer target, and build the fuzz crate in routine CI. |
| Python facade | 311 | ready | `plans/implementation/python-facade/311-directory-listing-renderer-consolidation.md` | Remove the bridge's second directory-listing renderer; restore the 1 MiB response-byte ceiling on the Python path. |
| CLI, ops, and observability | 312 | ready | `plans/implementation/cli-ops-observability/312-cross-platform-compile-coverage.md` | Compile the workspace for the Windows and macOS targets in routine CI (compile-only; runtime qualification stays manual). |
| CLI, ops, and observability | 313 | ready | `plans/implementation/cli-ops-observability/313-lint-policy-enforcement.md` | Enable `missing_docs` + `undocumented_unsafe_blocks`; add a `python-bindings-internal` clippy lane. No public API reduction. |
| Planning governance | 314 | ready | `plans/implementation/planning-governance/314-publication-state-staleness-sweep.md` | Correct the six surfaces that still describe the published 0.4.0 line as pending, including the agent-facing skill. Interface dependency: 304. |
| CLI, ops, and observability | 315 | ready | `plans/implementation/cli-ops-observability/315-dependency-specification-consolidation.md` | Introduce a `[workspace.dependencies]` table. **Both lockfiles must be byte-identical before and after.** The excluded Python crate keeps explicit specs. |

## Blocked work

The corrective republication is **prepared but not published**. The 0.4.0
line is published (crates.io `primitives 0.2.2`, `server`/`static`/`h3`/
`core 0.4.0`, `bin 0.2.2`, `eggnet-tls 0.2.0`; PyPI wheel `0.2.4`; tag
`v0.4.0`; GitHub Release "0.4.0 release line"), and Milestone 304 closed
Plan 300 conditionally against it — see
`plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`.
The published artifact predates `v0.4.0..465b80c` (B1–B89 plus the CI
fix), so the live crates still carry those defects. Milestone 305 prepared
the corrective `0.5.0` line (`primitives 0.2.3`, `server`/`static`/`h3`/
`core 0.5.0`, `bin 0.2.3`, wheel `0.2.5`) and gated it locally, but the
upload, tag, GitHub Release, and wheel steps are **outstanding**: they
require the maintainer's crates.io token, the `pypi` environment approval,
and a pushed bump commit whose exact-SHA hosted CI is green. See
`plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`.

Milestone 314 has an interface dependency on 304's closure record (the
facts it corrects).

## Closure work and current control points

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Direct H1 boundary ownership (288–291) | closed | `plans/288-291-direct-h1-boundary-ownership-followup-program.md`; `release/plan-290-direct-h1-boundary-ownership-qualification.md`; `release/plan-291-direct-h1-boundary-ownership-publication-closure.md` (registry-qualified `eggserve-server 0.3.1`). |
| Embedding contract (280–286) | closed | `plans/280-286-direct-h1-embedding-policy-ownership-program.md`; `release/plan-286-embedding-contract-publication-closure.md` (`primitives 0.2.1`, `server/static/h3/core 0.3.0`, `bin 0.2.1`, wheel `0.2.3`). |
| Post-convergence maintenance (251–258) | closed | `release/plan-256-post-convergence-maintenance-interop-closure.md`; `release/plan-258-async-suppressed-body-lifetime-corrective-closure.md`. |
| Docs architecture overview + deep dives (293) | closed | `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md` (docs-only; overview index + all 23 deep dives refreshed to 0.3.x baseline). |
| Docs architecture overview + deep dives (301) | closed | `plans/closure/docs-architecture/301-architecture-overview-deep-dive-refresh-040.md` (docs-only; overview index + all deep dives refreshed to 0.4.0 baseline). |
| Direct application-server polish (294–297) | closed | `plans/closure/direct-h1-runtime/294-direct-tower-footprint-baseline.md`; `295-direct-tower-hotpath-optimization.md`; `296-direct-profile-footprint-capability-split.md`; `297-direct-application-server-qualification-closure.md`. P1/P2/WP-A retained; file/tunnel splits NO-GO; publication deferred. |
| Response-trailer F3 | closed | `plans/closure/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md` (wire repair landed; deferred Tower rendezvous untouched; no publication). |
| 0.4.0 line publication (300) | conditionally closed | `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` (publication fully evidenced: per-crate crates.io checksums, exact-SHA CI run `36176532051`, registry-only consumer proofs, tag + GitHub Release, PyPI wheel set + post-publish smokes). Open condition: the published artifact predates `v0.4.0..465b80c` → Milestone 305. |
| Runtime error representation (306) | closed | `plans/closure/direct-h1-runtime/306-runtime-error-representation-single-authority.md` (one authority in `eggserve-primitives`; 44-cell byte-equivalence + wire-level proof; dead helper cluster removed; topology gate rule `check_plan306_error_authority`). |
| Connection-overlap classification (307) | closed | `plans/closure/direct-h1-runtime/307-connection-overlap-classification-accuracy.md` (ledger corrected and drift-bounded by `check_plan253_drift`; Alt-Svc advertisement single-sourced; six dead helpers removed). |
| Compatibility config projection (308) | closed | `plans/closure/direct-h1-runtime/308-compatibility-config-projection-parity.md` (four ownership values carried with unchanged defaults; 37/38 shared builder setters delegate; topology gate rule `check_plan308_builder_delegation`). |
| Corrective 0.5.0 line (305) | conditionally closed | `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md` (version line prepared and gated locally; the upload, tag, release, and wheel steps are outstanding and require the maintainer's registry credentials and a green exact-SHA hosted CI run). |
| Agent/skill/docs staleness sweep (302) | closed | `plans/closure/planning-governance/302-agent-skill-docs-staleness-sweep-040.md` (docs-only; skill + AGENTS.md + docs/README + architecture version strings onto the 0.4.0 line; registry next-number → 303). |
| Corrective program (304–315) | active | `plans/implementation/direct-h1-runtime/304-040-publication-closure.md`; `305-corrective-040x-publication.md`; `306-runtime-error-representation-single-authority.md`; `307-connection-overlap-classification-accuracy.md`; `308-compatibility-config-projection-parity.md`; `309-accept-error-classification-parity.md`; `plans/implementation/static-confinement/310-fuzz-harness-repair-and-build-gate.md`; `plans/implementation/python-facade/311-directory-listing-renderer-consolidation.md`; `plans/implementation/cli-ops-observability/312-cross-platform-compile-coverage.md`; `313-lint-policy-enforcement.md`; `315-dependency-specification-consolidation.md`; `plans/implementation/planning-governance/314-publication-state-staleness-sweep.md`. Closure records land under `plans/closure/<subsystem>/`. |

## Legacy trace pointer

Full history: `plans/ROADMAP.md` + flat `plans/NNN-*.md` + `release/`. Cite, don't duplicate. A future mechanical move may relocate legacy files under `plans/archive/` with a redirect index; until then they stay addressable at their current paths.
