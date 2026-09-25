# Planning Governance Roadmap

Status: active

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

After Plan 297 closure, the control surface still contains stale bootstrap/campaign wording: Plan 292 is still represented as active scaffolding; the direct-H1 registry/roadmap wording retains completed 294–297 campaign language; the blocked-work paragraph refers to 297 as merely ready; and agent guidance contains hard-coded “294+” next-work wording.

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
    `--> Milestone 298 control-surface reconciliation
```

Milestone 298 has hard evidence dependencies on the actual files/pointers required by Plan 292 acceptance and on accepted 293/294–297 closure records. All are present at planning time but must be verified during execution.

## 7. Milestones

### Milestone 1 — Planning control-surface reconciliation

Class: polish

Objective: verify the Plan 292 bootstrap acceptance criteria, retire its active-bootstrap representation without editing the immutable legacy file, reconcile 294–297 closure state, and remove hard-coded next-number guidance.

Dependencies: Plan 292 artifacts (hard); Plan 293 and 294–297 closure records (hard).

Deliverable boundary: planning/docs only.

User or operator value: handoff agents see one truthful source of active work and cannot mistake closed campaigns for blockers or active scaffolding.

Exit conditions: Milestone 298 closure record proves Plan 292 acceptance, registry/roadmaps agree, stale 297 blocker text is gone, agent guidance uses durable hierarchy pointers, and no legacy plan/runtime file is changed.

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

The planning-governance roadmap closes when Milestone 298 has an accepted closure record and the registry contains no stale bootstrap/campaign state identified by that plan.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | ready | `plans/implementation/planning-governance/298-planning-control-surface-reconciliation.md` | — | — |
