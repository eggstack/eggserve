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
| Direct H1 runtime + service contract | closed | `plans/subsystems/direct-h1-runtime-roadmap.md` | All milestones closed through 288–291 (`eggserve-server 0.3.1`) | None. Trace: `release/plan-250-*`, `plan-256-*`, `plan-286-*`, `plan-290-*`, `plan-291-*`. |
| Planning-convention migration | active | Plan 292 (this migration; no separate roadmap — bootstrap) | 292 scaffolding in progress | None. Closes when §Acceptance in `plans/292-planning-convention-migration-to-codegg-style.md` holds. |
| Docs architecture (overview index + deep dives) | closed | `plans/subsystems/docs-architecture-roadmap.md` | 293 closed (overview + all deep dives refreshed) | Trace: `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md`. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies / handoff note |
|---|---|---|---|---|
| Docs architecture | 293 | closed | `plans/implementation/docs-architecture/293-architecture-overview-deep-dive-refresh.md` | Done; closure: `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md`. |

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| — | — | None. |

## Closure work and current control points

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Direct H1 boundary ownership (288–291) | closed | `plans/288-291-direct-h1-boundary-ownership-followup-program.md`; `release/plan-290-direct-h1-boundary-ownership-qualification.md`; `release/plan-291-direct-h1-boundary-ownership-publication-closure.md` (registry-qualified `eggserve-server 0.3.1`). |
| Embedding contract (280–286) | closed | `plans/280-286-direct-h1-embedding-policy-ownership-program.md`; `release/plan-286-embedding-contract-publication-closure.md` (`primitives 0.2.1`, `server/static/h3/core 0.3.0`, `bin 0.2.1`, wheel `0.2.3`). |
| Post-convergence maintenance (251–258) | closed | `release/plan-256-post-convergence-maintenance-interop-closure.md`; `release/plan-258-async-suppressed-body-lifetime-corrective-closure.md`. |
| Docs architecture overview + deep dives (293) | closed | `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md` (docs-only; overview index + all 23 deep dives refreshed). |

## Legacy trace pointer

Full history: `plans/ROADMAP.md` + flat `plans/NNN-*.md` + `release/`. Cite, don't duplicate. A future mechanical move may relocate legacy files under `plans/archive/` with a redirect index; until then they stay addressable at their current paths.
