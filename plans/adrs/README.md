# Architecture Decision Records

This directory contains durable decisions that affect EggServe architecture across milestones or subsystems. Adapted from the CodeGG convention (Plan 292).

Use an ADR when a question cannot be answered safely inside one implementation plan without establishing a reusable architectural contract.

## Naming

```text
ADR-NNNN-short-title.md
```

Numbers are monotonically increasing and never reused. The existing `architecture/adr-002-*` and `architecture/adr-003-*` records stay where they are; the first new ADR here is `ADR-0001` only if it does not collide in meaning — prefer continuing the conceptual sequence and linking back to the architecture ADRs explicitly.

## Status lifecycle

```text
proposed -> accepted -> deprecated or superseded
         `-> rejected
```

Accepted ADRs are historical records. Do not rewrite an accepted ADR to make a later decision appear original. Create a new ADR and mark the old one superseded.

## ADR template

```markdown
# ADR-NNNN: Title

Status: proposed

Date: YYYY-MM-DD

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#...`
- `plans/001-terminology-and-domain-model.md#...`

Affected subsystem roadmaps:

- `plans/subsystems/...`

## Context

Describe the architectural problem, existing implementation, constraints, and why the decision is required now.

## Decision drivers

- ...

## Considered options

### Option A — Name

Description, benefits, costs, and failure modes.

### Option B — Name

Description, benefits, costs, and failure modes.

## Decision

State the selected option precisely, including crate ownership and interface boundaries (checked by `scripts/check-crate-topology.py` where applicable).

## Consequences

### Positive

- ...

### Negative

- ...

### Neutral or deferred

- ...

## Compatibility and migration

Describe storage, protocol, configuration, API, and operational migration requirements.

## Security and reliability implications

Describe confinement, safe defaults, timeout/admission, cancellation, restart, recovery, and denial-of-service effects (`docs/threat-model.md`).

## Verification

Describe the evidence required to prove implementations conform to this decision (`verify.sh` scope, conformance corpora, topology gate, supply-chain where applicable).

## Supersession

None.
```

## ADR threshold

An ADR is normally required when a decision:

- changes a crate-ownership or confinement boundary;
- introduces a new listener, proxy-metadata, TLS, or tunnel contract;
- selects a durable external standard or dependency;
- changes authentication/authorization semantics (mTLS, proxy trust);
- changes admission, timeout, or shutdown semantics;
- establishes a public compatibility contract (Rust, Python, CLI);
- materially changes a long-term non-goal (requires `docs/non-goals.md` + `docs/threat-model.md` in the same change).

An ADR is usually unnecessary for local refactors, internal naming cleanup, implementation-specific data structures, or reversible optimizations that preserve established contracts.
