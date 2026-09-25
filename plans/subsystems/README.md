# Subsystem Roadmaps

Subsystem roadmaps translate the canonical EggServe direction into coherent, dependency-aware workstreams. They are not direct coding-agent checklists. Adapted from the CodeGG convention (Plan 292).

Each roadmap should remain useful across several implementation milestones and repository revisions. Commit-specific mechanics belong in `plans/implementation/`.

## Naming

```text
<subsystem>-roadmap.md
```

Stable subsystem names: `static-confinement`, `direct-h1-runtime`, `tls-identity`, `h2-h3-transports`, `python-facade`, `cli-ops-observability`, `proxy-metadata`.

## Required roadmap structure

```markdown
# <Subsystem> Roadmap

Status: proposed | active | closing | closed | superseded

Long-term references:

- `plans/000-long-term-specification.md#...`
- `plans/001-terminology-and-domain-model.md#...`
- `plans/002-long-term-roadmap.md#...`

Related ADRs:

- `plans/adrs/ADR-NNNN-...md`

## 1. Purpose and ownership boundary

Define what the subsystem owns, what it consumes, and what it must not own (crates + topology-gate impact).

## 2. Work classification

### Invariants

- ...

### Capabilities

- ...

### Infrastructure

- ...

### Polish

- ...

## 3. Non-goals

- ...

## 4. Current state

Summarize repository evidence, existing contracts, compatibility paths, and known gaps. Avoid fragile line-number references unless essential.

## 5. Target architecture

Describe the end-state module, ownership, and lifecycle model for this subsystem.

## 6. Dependency graph

```text
Milestone A
    |
    +--> Milestone B
    |
    `--> Milestone C
             |
             `--> Milestone D
```

Classify each dependency as hard, interface, soft, or operational.

## 7. Milestones

### Milestone 1 — Title

Class: invariant | capability | infrastructure | polish

Objective:

Dependencies:

Deliverable boundary:

User or operator value:

Exit conditions:

Deferred work:

### Milestone 2 — Title

...

## 8. Cross-cutting requirements

### Storage and migration

### Protocol and compatibility

### Security and authorization

### Concurrency, cancellation, and recovery

### Observability and audit

### Performance and resource use

### Documentation and operations

## 9. Verification strategy

Define subsystem-level integration, conformance-corpus, fuzz, contention, restart, and end-to-end evidence (`verify.sh fast/full/deep` mapping).

## 10. Risks and decision points

List unresolved decisions and identify which require ADRs.

## 11. Completion definition

Describe what must be true before the subsystem roadmap is closed.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | not started | — | — | — |
```

## Roadmap rules

A subsystem roadmap MUST:

- link to canonical long-term requirements rather than duplicating them wholesale;
- define ownership boundaries before milestones;
- distinguish infrastructure from completed capability;
- expose dependencies and decision points;
- preserve completed milestone history;
- link each active milestone to one implementation plan and later one closure record;
- state non-goals to prevent scope expansion;
- remain at the subsystem level rather than becoming a file-by-file implementation checklist.

A subsystem roadmap MAY be updated when implementation evidence changes sequencing or decomposition. Material changes must record why the roadmap changed.
