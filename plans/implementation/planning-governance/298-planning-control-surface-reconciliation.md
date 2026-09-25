# Planning Governance Milestone 298 — Planning control-surface reconciliation

Status: ready for handoff

Repository baseline: `c1f4348939406c02f9739cdedff3bf7568ce09c6`

Source roadmap:

- `plans/subsystems/planning-governance-roadmap.md#7-milestones`

Long-term requirements:

- `plans/README.md`
- `plans/003-planning-process.md#9-registry-requirements`

Applicable legacy bootstrap:

- `plans/292-planning-convention-migration-to-codegg-style.md`

Primary class: polish

## 1. Objective

Reconcile EggServe's planning/closure control surface after Milestones 294–297 and formally retire the Plan 292 bootstrap from active status using the new planning system.

This is planning/docs cleanup only. Do not change runtime code, package versions, features, protocol behavior, or immutable legacy plan contents.

## 2. Why this milestone is ready

Plan 292's hierarchy exists and has been exercised by Plan 293 plus the complete 294–297 campaign. The accepted closure records provide the evidence needed to distinguish historical bootstrap state from active work.

Plan 299 is independently ready and may execute in parallel; 298 only ensures the control surface describes it correctly.

## 3. Current implementation evidence

At the baseline:

- `plans/README.md`, `registry.md`, canonical `000`–`003`, `subsystems/`, `implementation/`, `closure/`, `adrs/`, and `archive/` exist.
- Plan 293 has a new-style implementation plan + closure record.
- Plans 294–297 each have new-style implementation + closure records; Plan 297 says the application-server polish campaign is complete.
- `plans/registry.md` still presents the Plan 292 migration as active scaffolding and contains stale prose saying 297 “is ready”.
- the direct-H1 roadmap is still active after 294–297 because the residual F3 correctness gap is now being separately registered as Plan 299; the roadmap must distinguish “campaign closed” from “subsystem active for 299”.
- `AGENTS.md` and the EggServe dev skill hard-code “new work (294+)”, which became stale as soon as subsequent milestones landed.
- the immutable legacy Plan 292 file still says ACTIVE; current governance says not to rewrite legacy flat plans.

## 4. Invariants that must not regress

- Legacy flat plans through 292 and legacy release evidence remain historical/immutable.
- Registry status follows accepted closure evidence.
- Active/ready/blocked tables contain no contradictory state.
- Each active implementation plan links to a subsystem roadmap and later closure record.
- No planning cleanup changes product/API/security claims.
- Agent guidance points to `plans/registry.md`/planning hierarchy rather than a hard-coded next number.

## 5. Scope

### In scope

- verify each Plan 292 acceptance criterion against current tree;
- create the 298 closure record that records the bootstrap disposition;
- replace the legacy “Plan 292 active bootstrap” registry state with new planning-governance closure state;
- reconcile direct-H1 roadmap/registry wording so 294–297 are closed and 299 is the only active direct-H1 milestone;
- remove stale “297 ready” blocked-work wording;
- update AGENTS/skill/overview/contributing pointers only where they still encode stale bootstrap/next-number state;
- compact registry rows when useful while preserving links to accepted closures;
- link the F3 residual to Plan 299.

### Explicitly out of scope

- no edit to `plans/292-planning-convention-migration-to-codegg-style.md`;
- no legacy plan/archive mass move;
- no Rust/Python/manifest/package change;
- no implementation of Plan 299;
- no modification of historical closure evidence except factual broken-link repair if necessary.

## 6. Required production changes

None.

### Crates and ownership

No crate changes.

### Config and policy

No runtime config changes.

### Protocol and compatibility

No protocol/API changes.

### Runtime and concurrency

Not applicable.

### Frontend or operator surface

Agent/contributor documentation only.

### Security and confinement

Normative security documents remain unchanged unless a stale planning pointer is purely mechanical.

### Documentation and static guards

Prefer removing hard-coded “next plan N+” wording from durable agent guidance; the registry is the next-number/current-work authority.

## 7. Ordered work packages

### Work package A — Verify Plan 292 bootstrap acceptance

Check every §5 acceptance item against current tree. Record pass/fail and exact evidence.

If any material item is absent, do not claim bootstrap closure; correct only missing planning/docs scaffolding within scope or leave a named residual.

### Work package B — Reconcile active control state

Update `plans/registry.md` and affected subsystem roadmaps so:
- 293 and 294–297 remain closed;
- 297 is not described as ready/blocked;
- Plan 292 is historical bootstrap, not active current work, once acceptance is proven;
- Plan 299 is the active direct-H1 correctness milestone;
- blocked-work table is internally consistent.

### Work package C — Durable agent pointers

Review `AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md`, `architecture/overview.md`, and `CONTRIBUTING.md` for stale numeric next-work pointers. Point durable guidance at `plans/registry.md` instead of continually incrementing a number.

### Work package D — Closure record

Create `plans/closure/planning-governance/298-planning-control-surface-reconciliation.md` with:
- Plan 292 acceptance matrix;
- changed planning/docs files;
- proof no legacy plan or production file changed;
- registry/roadmap consistency result;
- residuals, if any;
- disposition of the planning-governance roadmap.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable to runtime. Partial planning cleanup must remain obvious: do not mark 298 closed if the registry still contradicts closure evidence.

## 9. Compatibility and migration

No compatibility effect. The only migration is conceptual: current work is discovered through the registry/new hierarchy; Plan 292 remains historical trace at its old path.

## 10. Required tests

- path/link existence for all new planning pointers;
- Plan 292 acceptance checklist;
- registry status consistency;
- diff review confirming no `crates/`, manifests, or legacy flat plan changes.

## 11. Required verification commands

```bash
git status --short
git diff --check
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
```

If a repository link-check helper exists, run it for touched Markdown. Do not run expensive runtime suites for docs-only changes unless another touched file requires them.

## 12. Documentation updates

Only planning hierarchy and agent/contributor pointers needed for factual reconciliation.

## 13. Acceptance criteria

- Plan 292 §5 acceptance is evidenced item-by-item.
- Legacy Plan 292 itself is unchanged.
- Registry no longer describes closed 297 as ready or blocked.
- Direct-H1 roadmap clearly says 294–297 campaign closed and Plan 299 is separate.
- Agent/skill guidance no longer hard-codes a stale next-plan number.
- A 298 closure record exists and the planning-governance roadmap/registry agree.
- No product/runtime/API/package behavior changes.

## 14. Stop conditions

Stop and report if Plan 292 acceptance is materially incomplete, if reconciliation would require rewriting historical closure evidence, or if cleanup expands into runtime/product work.

## 15. Closure evidence required

`plans/closure/planning-governance/298-planning-control-surface-reconciliation.md` with the exact acceptance matrix, commands/results, changed-file classification, residuals, and final registry/roadmap disposition.

## 16. Handoff notes

This plan intentionally does not “fix” the ACTIVE line inside legacy Plan 292. The new closure record and registry are the authoritative current-state evidence.
