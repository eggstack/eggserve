# Planning Governance Roadmap

Status: closed (Milestones 1, 302, and 314 closed)

Long-term references:

- `plans/README.md` (planning hierarchy and legacy-trace rules)
- `plans/003-planning-process.md` (normative planning governance)
- `plans/002-long-term-roadmap.md` (macro roadmap/control-surface relationship)

Related legacy bootstrap:

- `plans/292-planning-convention-migration-to-codegg-style.md` (archived-in-place bootstrap; immutable under the current rules)

## 1. Purpose and ownership boundary

Owns EggServe's planning control surface: the active registry, subsystem-roadmap status, implementation/closure lifecycle, and agent-facing pointers into that hierarchy.

This subsystem owns no runtime, protocol, static-serving, packaging, or release behavior. It may reconcile stale planning metadata against accepted closure records, but it must not rewrite legacy plan history to make it appear cleaner.

## 2. Work classification

### Invariants

- `plans/registry.md` truthfully represents active/ready/blocked/closed work.
- Legacy flat plans `000`–`292` and legacy `release/plan-*.md` records remain immutable historical trace except factual link repair explicitly allowed by governance.
- New work uses subsystem roadmap → implementation plan → closure record; a code commit alone never closes a milestone.
- Active roadmap status and registry status cannot contradict accepted closure evidence.
- Agent guidance must point to the planning hierarchy without hard-coding a permanently stale “next plan” number.

### Capabilities

None. Planning governance is an internal coordination contract.

### Infrastructure

- `plans/registry.md`
- subsystem roadmaps
- implementation/closure templates and records
- agent/skill pointers to the planning hierarchy

### Polish

- stale-status reconciliation;
- link/pointer cleanup;
- compacting completed work out of active control tables while preserving trace.

## 3. Non-goals

- No Rust/Python/product behavior change.
- No rewrite, move, renumber, or deletion of legacy flat plans.
- No mass archive migration.
- No redesign of the planning convention established by Plan 292.
- No runtime correctness work; Plan 299 separately owns H1 response trailers.

## 4. Current state

Plan 292 created the CodeGG-style hierarchy and its acceptance structure is materially in use: Plan 293 and Milestones 294–297 have implementation plans, closure records, registry entries, and subsystem-roadmap state.

After Plan 297 closure, the control surface still contains stale bootstrap/campaign wording: Plan 292 is still represented as active scaffolding; the direct-H1 registry/roadmap wording retains completed 294–297 campaign language; the blocked-work paragraph refers to 297 as merely ready; and agent guidance contains hard-coded "294+" next-work wording. Milestone 298 reconciled that state.

A second staleness class appeared after the 0.4.0 publication on 2026-09-25. The registry still reported Plan 300 as `active` with a missing crates.io token and `pypi` approval (reconciled by Milestone 304), and six documentation and agent-facing surfaces still describe that publication as pending — most consequentially `.opencode/skills/eggserve-dev/SKILL.md`, which tells agents to pin the pre-0.4.0 versions as current. Milestone 314 sweeps those surfaces.

The immutable Plan 292 file itself still says ACTIVE. Current governance forbids rewriting that legacy bootstrap file, so closure must be represented by new-system evidence and registry/roadmap state.

## 5. Target architecture

One compact truthful registry links to durable subsystem roadmaps and bounded handoff/closure records. Historical plans remain addressable but do not masquerade as current active work.

## 6. Dependency graph

```text
Plan 292 bootstrap (legacy, acceptance to verify)
    |
    +--> Plan 293 first new-style closure (closed)
    |
    +--> Plans 294–297 new-style runtime campaign (closed)
    |
    +--> Milestone 298 control-surface reconciliation (closed)
    |
    +--> Milestone 302 agent/skill/docs staleness sweep (closed)
    |
    `--> Milestone 314 publication-state staleness sweep
```

Milestone 298 has hard evidence dependencies on the actual files/pointers required by Plan 292 acceptance and on accepted 293/294–297 closure records. All are present at planning time but must be verified during execution.

Milestone 314 has an interface dependency on Milestone 304's closure record, which supplies the published-state facts it corrects.

## 7. Milestones

### Milestone 1 — Planning control-surface reconciliation

Class: polish

Objective: verify the Plan 292 bootstrap acceptance criteria, retire its active-bootstrap representation without editing the immutable legacy file, reconcile 294–297 closure state, and remove hard-coded next-number guidance.

Dependencies: Plan 292 artifacts (hard); Plan 293 and 294–297 closure records (hard).

Deliverable boundary: planning/docs only.

User or operator value: handoff agents see one truthful source of active work and cannot mistake closed campaigns for blockers or active scaffolding.

Exit conditions: Milestone 298 closure record proves Plan 292 acceptance, registry/roadmaps agree, stale 297 blocker text is gone, agent guidance uses durable hierarchy pointers, and no legacy plan/runtime file is changed.

### Milestone 302 — Agent/skill/docs staleness sweep (0.4.0 line)

Class: polish

Objective: sweep version drift left after Plans 293–301 moved the tree to
the 0.4.0 line (skill text, `AGENTS.md`, `README.md`, `docs/`,
`architecture/`, registry next-number). Docs-only; no behavior, API,
dependency, or tier change.

Dependencies: closed 293–301 state + active Plan 300 versions as review
input (interface only).

Deliverable boundary: skill/agent-guide/docs/architecture/planning-metadata
text only; implementation plan + closure record + registry update.

User or operator value: future agents work from accurate, non-stale sources
(dependency snippets that resolve, status labels that match the registry).

Exit conditions: every finding in the 302 implementation plan §3 corrected
with manifest/registry evidence; topology + conformance-matrix gates pass;
`cargo fmt --check` passes; registry + closure record complete.

### Milestone 314 — Publication-state staleness sweep

Class: polish

Objective: correct the six surfaces that still describe the published 0.4.0
line as pending — including the agent-facing skill, which instructs
agents to pin `server 0.3.1` / `static`/`h3`/`core 0.3.0` /
`primitives 0.2.1` / `bin 0.2.1` as the current versions — and record that
the published artifact predates the post-tag corrective batch.

Dependencies: Milestone 304's closure record (interface — the facts it
corrects).

Deliverable boundary: skill/agent-guide/architecture/release-process text
only. **No `CHANGELOG.md` entry** — Plan 305 owns the changelog, and it
must describe the artifact consumers can actually reach, including the
corrective line.

User or operator value: agents stop advising two-patch-lines-stale
dependency versions, and consumers are not sent to a published artifact
with known gaps without being told.

Exit conditions: no surface describes the 0.4.0 publication as pending or
conditional; no historical `0.3.x` statement was altered; the skill no
longer contains "until then", "once Plan 300 publishes", or "300
(active)"; the changelog is unchanged.

## 8. Cross-cutting requirements

### Storage and migration

No data/storage migration.

### Protocol and compatibility

No protocol/API effect.

### Security and authorization

No security boundary change; planning edits must not weaken documented safety invariants.

### Concurrency, cancellation, and recovery

Not applicable.

### Observability and audit

Git history plus the 298 closure record is the audit trail. Legacy Plan 292 remains unchanged.

### Performance and resource use

No runtime effect.

### Documentation and operations

Agent-facing pointers must resolve and avoid ephemeral next-plan-number wording.

## 9. Verification strategy

Plan 292 acceptance checklist, link/path existence checks, registry/roadmap consistency review, `git diff --check`, topology no-op guard, and `cargo fmt --all -- --check` as a cheap unaffected-workspace guard.

## 10. Risks and decision points

- Do not “close” Plan 292 by editing its legacy status line; use the new closure record and registry.
- Do not archive/move hundreds of legacy files in this milestone.
- If Plan 292 acceptance is materially incomplete, leave planning governance active and list the exact missing criterion rather than papering over it.

## 11. Completion definition

The planning-governance roadmap closes when Milestone 298 has an accepted closure record and the registry contains no stale bootstrap/campaign state identified by that plan, and when Milestone 314 has an accepted closure record and no surface describes the published 0.4.0 line as pending.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/planning-governance/298-planning-control-surface-reconciliation.md` | `plans/closure/planning-governance/298-planning-control-surface-reconciliation.md` | — |
| 302 | closed | `plans/implementation/planning-governance/302-agent-skill-docs-staleness-sweep-040.md` | `plans/closure/planning-governance/302-agent-skill-docs-staleness-sweep-040.md` | — |
| 314 | closed | `plans/implementation/planning-governance/314-publication-state-staleness-sweep.md` | `plans/closure/planning-governance/314-publication-state-staleness-sweep.md` | Milestone 304 (interface) |
