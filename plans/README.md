# EggServe Planning System

This directory separates durable architectural direction from temporary execution planning.
Adopted from the CodeGG planning convention (Plan 292); adapted to EggServe authorities.

## Canonical long-term documents

The following files define the intended product and architecture and MUST NOT be edited as part of ordinary implementation work:

- `000-long-term-specification.md` — normative end-state specification and invariants.
- `001-terminology-and-domain-model.md` — normative language and identity model.
- `002-long-term-roadmap.md` — dependency-ordered long-term capability roadmap.
- `003-planning-process.md` — rules for deriving and managing interim plans.

The first three documents are stable architectural references. Changes to them require an explicit long-term architecture decision, not an implementation convenience. Interim plans MUST reference them rather than copying or silently revising their requirements.

Normative user/embedding contracts live in `docs/` and `architecture/`; the canonical planning docs point at them and MUST NOT duplicate them wholesale.

## Planning hierarchy

```text
Long-term specification and terminology
        |
        v
Architecture decision records
        |
        v
Master long-term roadmap
        |
        v
Subsystem roadmaps
        |
        v
Milestone implementation plans
        |
        v
Implementation and verification
        |
        v
Closure records and archive
```

## Directory roles

- `adrs/` — durable architecture decisions. Accepted decisions are superseded, not rewritten.
- `subsystems/` — subsystem specifications and dependency-ordered roadmaps. These translate the long-term documents into coherent workstreams.
- `implementation/` — focused milestone plans handed to implementation agents. These are operational and may evolve as code changes.
- `closure/` — verification, evidence, residual-risk, and completion records for implemented milestones.
- `archive/` — completed or superseded interim planning retained for traceability.
- `registry.md` — compact index of active subsystem roadmaps, implementation plans, closure work, and dependencies.

## Legacy trace (archived in place)

The flat `plans/NNN-*.md` files (`000`–`292`), `plans/ROADMAP.md`, `plans/RELEASE-READINESS-ROADMAP.md`, and `release/plan-*.md` are immutable historical trace. They are **archived in place**: cite them, do not rewrite, move, or delete them under this plan. A future mechanical `archive/` move may relocate them with a redirect index; until then the registry links to them where they stand.

All new work (293+) starts in the hierarchy above and registers in `registry.md`.

## Core rule

Long-term documents state **what EggServe is becoming and what must remain true**. Interim documents state **what an agent should implement next against a specific repository baseline**.

Implementation agents MUST NOT add commit-specific steps, transient file lists, current test counts, or short-lived corrective work to the canonical long-term documents.

## Planning lifecycle

1. Identify the relevant long-term specification sections and invariants.
2. Record any unresolved architectural decision in `adrs/`.
3. Create or update a subsystem roadmap in `subsystems/`.
4. Select one dependency-ready milestone.
5. Write a bounded handoff plan under `implementation/`.
6. Implement and verify the milestone.
7. Write a closure record under `closure/`.
8. Update `registry.md` and the subsystem roadmap status.
9. Move completed or superseded interim documents to `archive/` when they no longer represent active work.

No milestone is complete merely because code landed. Completion requires the closure evidence defined by its implementation plan and subsystem roadmap.

## Required classification

Every subsystem roadmap and implementation plan MUST distinguish:

- **Invariant** — a property that must always remain true.
- **Capability** — user- or operator-visible behavior.
- **Infrastructure** — internal machinery required by capabilities.
- **Polish** — ergonomics, diagnostics, performance tuning, cleanup, or documentation.

Infrastructure and polish MUST NOT be presented as completed user capability unless the user-visible acceptance criteria are actually satisfied.

## Naming conventions

- ADR: `adrs/ADR-NNNN-short-title.md`
- Subsystem roadmap: `subsystems/<subsystem>-roadmap.md`
- Milestone implementation plan: `implementation/<subsystem>/NNN-short-title.md`
- Closure record: `closure/<subsystem>/NNN-status.md`
- Archived document: retain its original relative structure beneath `archive/`

New milestone numbers continue from `293` (legacy flat sequence ends at `292`). Use stable subsystem names (`static-confinement`, `direct-h1-runtime`, `tls-identity`, `h2-h3-transports`, `python-facade`, `cli-ops-observability`, `proxy-metadata`). Do not encode dates in filenames unless the document is inherently time-bound.

## Starting a new workstream

Begin with `subsystems/README.md`, then use the templates and rules in:

- `adrs/README.md`
- `implementation/README.md`
- `closure/README.md`

Register active work in `registry.md` before handing implementation plans to agents.
