# Milestone Implementation Plans

This directory contains bounded plans handed directly to implementation agents. Adapted from the CodeGG convention (Plan 292).

Implementation plans are operational documents tied to the current repository state. They may be corrected, superseded, or archived without modifying the canonical long-term documents.

## Layout and naming

```text
implementation/<subsystem>/NNN-short-title.md
```

New milestone numbers continue from `293`. Subsystem names come from `plans/001-terminology-and-domain-model.md`.

## Required implementation-plan template

```markdown
# <Subsystem> Milestone NNN — <Title>

Status: ready for handoff | active | blocked | implemented | superseded

Repository baseline: `<commit SHA or branch state>`

Source roadmap:

- `plans/subsystems/<subsystem>-roadmap.md#...`

Long-term requirements:

- `plans/000-long-term-specification.md#...`
- `plans/001-terminology-and-domain-model.md#...`

Applicable ADRs:

- `plans/adrs/ADR-NNNN-...md`

Primary class: invariant | capability | infrastructure | polish

## 1. Objective

State one bounded outcome.

## 2. Why this milestone is ready

List closed hard dependencies and stable interface dependencies.

## 3. Current implementation evidence

Describe the relevant code, tests, conformance corpora, guards, and known gaps at the repository baseline.

## 4. Invariants that must not regress

- ...

## 5. Scope

### In scope

- ...

### Explicitly out of scope

- ...

## 6. Required production changes

Describe required behavior and ownership changes. Name likely modules where useful, but do not prescribe blind mechanical edits when alternatives are valid.

### Crates and ownership

### Config and policy

### Protocol and compatibility

### Runtime and concurrency

### Frontend or operator surface (CLI / Python)

### Security and confinement

### Documentation and static guards

## 7. Ordered work packages

### Work package A — Title

Intent:

Required changes:

Acceptance evidence:

### Work package B — Title

...

## 8. Failure, cancellation, restart, and contention semantics

State expected behavior for partial failure, duplicate delivery, process restart, cancellation races, stale generations, and concurrent callers.

## 9. Compatibility and migration

Describe backward compatibility, configuration fallback, protocol negotiation, and removal criteria for legacy paths. Publication milestones state the semver strategy.

## 10. Required tests

### Focused unit tests

### Integration tests

### Restart and recovery tests

### Contention and cancellation tests

### Security and negative tests

### Migration and compatibility tests

## 11. Required verification commands

```bash
# narrow tests first, e.g. cargo test -p <crate> <filter>
# ./scripts/verify.sh fast
# static guards: python3 scripts/check-crate-topology.py, conformance-matrix gate
# broader suite appropriate to the change (full/deep only when the plan requires)
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- ...

## 13. Acceptance criteria

Use externally observable or contract-level statements.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- an unresolved architecture decision materially changes ownership;
- a hard dependency is absent;
- required migration cannot be made safely;
- repository evidence contradicts a canonical invariant;
- external evidence is required but unavailable (registry, platform, browser);
- scope would expand into another subsystem roadmap or a `docs/non-goals.md` crossing without the required doc updates.

## 15. Closure evidence required

List the exact evidence the later closure record must contain.

## 16. Handoff notes

Include known hazards, resource constraints, serial-test requirements, environment requirements (e.g. `verify.sh full` needs Python 3.14 + maturin), and preserved user changes.
```

## Handoff rules

Before assigning a plan to an agent:

- confirm the repository baseline is current;
- confirm all hard dependencies are closed;
- confirm unresolved decisions have ADRs or are explicitly out of scope;
- ensure the milestone can be completed in one coherent pass;
- ensure tests and closure evidence are specific;
- register the plan in `plans/registry.md`.

The implementation agent may adjust file-level mechanics based on current code. It may not weaken canonical invariants or silently enlarge scope.

## Corrective plans

Corrective work receives a new plan in the same subsystem directory, for example:

```text
004-correctness-and-recovery-closure.md
```

The corrective plan must reference the original plan and closure record, enumerate unclosed findings, and add regression evidence that would have caught them. Never rewrite the original closure to look successful.
