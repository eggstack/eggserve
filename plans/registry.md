# EggServe Active Planning Registry

This file is the compact control surface for active interim planning. Detailed requirements and completed history remain in source roadmaps, implementation plans, `closure/` (new) and `release/` (legacy), and Git history.

Canonical direction remains in:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

New milestone numbers continue from `293`. Legacy flat plans (`000`–`292`) and `release/plan-*.md` are archived in place and immutable.

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
| Static confinement + static service | closed | `plans/subsystems/static-confinement-roadmap.md` | All milestones closed (sole authority in `eggserve-static`) | None. Trace: legacy Plans 002/007/219/224/245 + `release/plan-225-compatibility-facade-closure.md`. |
| Direct H1 runtime + service contract | closed | `plans/subsystems/direct-h1-runtime-roadmap.md` | 299 closed (H1 response-trailer wire correctness) | 294–297 polish campaign closed; 299 F3 repair closed (`plans/closure/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md`). No broader adapter/performance reopening. |
| Planning governance | closed | `plans/subsystems/planning-governance-roadmap.md` | 298 closed (planning control-surface reconciliation) | Plan 292 bootstrap retired without rewriting immutable legacy plans; stale 294–297 status reconciled (`plans/closure/planning-governance/298-planning-control-surface-reconciliation.md`). |
| Docs architecture (overview index + deep dives) | closed | `plans/subsystems/docs-architecture-roadmap.md` | 293 closed (overview + all deep dives refreshed) | Trace: `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md`. |

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

## Blocked work

No blocked milestones. Plans 298 and 299 are closed.

## Closure work and current control points

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Direct H1 boundary ownership (288–291) | closed | `plans/288-291-direct-h1-boundary-ownership-followup-program.md`; `release/plan-290-direct-h1-boundary-ownership-qualification.md`; `release/plan-291-direct-h1-boundary-ownership-publication-closure.md` (registry-qualified `eggserve-server 0.3.1`). |
| Embedding contract (280–286) | closed | `plans/280-286-direct-h1-embedding-policy-ownership-program.md`; `release/plan-286-embedding-contract-publication-closure.md` (`primitives 0.2.1`, `server/static/h3/core 0.3.0`, `bin 0.2.1`, wheel `0.2.3`). |
| Post-convergence maintenance (251–258) | closed | `release/plan-256-post-convergence-maintenance-interop-closure.md`; `release/plan-258-async-suppressed-body-lifetime-corrective-closure.md`. |
| Docs architecture overview + deep dives (293) | closed | `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md` (docs-only; overview index + all 23 deep dives refreshed). |
| Direct application-server polish (294–297) | closed | `plans/closure/direct-h1-runtime/294-direct-tower-footprint-baseline.md`; `295-direct-tower-hotpath-optimization.md`; `296-direct-profile-footprint-capability-split.md`; `297-direct-application-server-qualification-closure.md`. P1/P2/WP-A retained; file/tunnel splits NO-GO; publication deferred. |
| Response-trailer F3 | closed | `plans/closure/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md` (wire repair landed; deferred Tower rendezvous untouched; no publication). |

## Legacy trace pointer

Full history: `plans/ROADMAP.md` + flat `plans/NNN-*.md` + `release/`. Cite, don't duplicate. A future mechanical move may relocate legacy files under `plans/archive/` with a redirect index; until then they stay addressable at their current paths.
