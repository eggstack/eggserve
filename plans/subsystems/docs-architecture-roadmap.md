# Docs Architecture Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md` (product identity, crate-ownership boundaries, safe-default invariants)
- `plans/001-terminology-and-domain-model.md` (canonical crate/module vocabulary)
- `plans/002-long-term-roadmap.md` (phase 6 planning-convention migration; docs stay traceable, not normative)

Related ADRs:

- `architecture/adr-002-windows-handle-relative-filesystem.md` (accepted)
- `architecture/adr-003-custom-service-ownership.md` (accepted)

## 1. Purpose and ownership boundary

Owns the bird's-eye `architecture/overview.md` index plus the per-component
deep-dive documents in `architecture/`. Consumes every leaf and composition
crate as review input. Must not own: runtime behavior, public API shapes,
dependency graph, or tier promotions. All doc claims must match executable
source (`scripts/check-crate-topology.py` ownership, `docs/` normative
contracts); when docs conflict with config/scripts, the executable source
wins.

## 2. Work classification

### Invariants

- `architecture/overview.md` stays the entry point: 2–4 sentence bird's-eye
  per module/tool/capability plus a link to the owning deep dive.
- Every deep-dive link in the overview resolves to an existing file.
- No doc change weakens safe defaults, confinement, or crate authority.

### Capabilities

- Reviewer can navigate from the overview to any discrete component review
  without reading unrelated subsystems.

### Infrastructure

- Subagent-assisted systematic review so each deep dive is checked against
  current code without exhausting main-context window.

### Polish

- Overview + deep-dive refresh against the Plan 288–291 baseline
  (`eggserve-server 0.3.1`, `static`/`h3`/`core 0.3.0`, `bin 0.2.1`, wheel
  `0.2.3`); stale version/tier/feature claims corrected.

## 3. Non-goals

- No behavior, API, dependency, or support-tier change.
- No `docs/` normative-contract rewrite; no `plans/000`–`003` canonical edit.
- No new crate, feature flag, or topology rule.

## 4. Current state

`architecture/overview.md` already has the Plan 259–260 index shape (crate
overviews, capability map, tool map, deep-dive index, lifecycle diagrams)
over 26 architecture files. Plans 270–291 (supervisory completion, adapter
extraction, embedding policy ownership, boundary-ownership follow-up) need a
systematic freshness pass across the deep dives.

## 5. Target architecture

Overview remains the stable index; each deep dive owns its component detail.
Plan 293 refreshes the overview claims and walks every deep dive once,
recording discrepancies as findings rather than silent scope expansion.

## 6. Dependency graph

```text
Milestone 293 (overview + deep-dive refresh)
```

No hard dependencies (docs-only, closed code baseline). Interface dependency:
closed Plans 280–291 code state as review input. Soft: subsystem owners may
correct findings in follow-up plans.

## 7. Milestones

### Milestone 293 — Architecture overview + deep-dive refresh

Class: polish

Objective: verify `architecture/overview.md` claims against current code and
systematically refresh each deep dive's stale claims.

Dependencies: none (docs-only; code baseline is closed 288–291 state).

Deliverable boundary: updated `architecture/*.md` + implementation plan +
closure record + registry update.

User or operator value: accurate bird's-eye map plus reliable per-component
review entry points.

Exit conditions: every overview section verified; every deep dive walked once
with findings recorded; all links resolve; topology + conformance-matrix
gates pass.

Deferred work: any code defect found becomes a new corrective plan, not
silent scope expansion here.

## 8. Cross-cutting requirements

### Storage and migration

Docs-only; no storage or migration effect.

### Protocol and compatibility

No protocol change; tier labels (H1 + primitives supported, remainder
experimental) must stay consistent.

### Security and authorization

No confinement/policy change; security-model claims re-verified, not
rewritten.

### Concurrency, cancellation, and recovery

No runtime change; lifecycle wording checked against code.

### Observability and audit

No ops-model change; logging-doc claims checked.

### Performance and resource use

Benchmark claims keep profile + evidence-file naming; no new perf claims.

### Documentation and operations

This roadmap IS the docs work; `docs/` normative contracts untouched except
broken-link fixes if found.

## 9. Verification strategy

`python3 scripts/verify-conformance-matrix.py`,
`python3 scripts/check-crate-topology.py`, link-resolution check over
`architecture/`, `cargo fmt --all -- --check`. `verify.sh fast` scope where
practical; docs-only so no `full`/`deep` requirement beyond existing gates.

## 10. Risks and decision points

Risk: reviewer corrects code to match stale docs (inverted). Mitigation:
code wins; findings become follow-up plans. No ADR expected.

## 11. Completion definition

Overview verified as index; each deep dive walked once; closure record with
requirement-to-evidence matrix accepted; registry + roadmap status updated.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 293 | closed | `plans/implementation/docs-architecture/293-architecture-overview-deep-dive-refresh.md` | `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md` | — |
