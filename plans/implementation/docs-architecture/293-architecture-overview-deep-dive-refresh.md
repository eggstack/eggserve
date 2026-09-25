# Docs Architecture Milestone 293 — Overview + Deep-Dive Refresh

Status: active

Repository baseline: `main` at Plan 292 hierarchy adoption (post-288–291 code
state: `eggserve-server 0.3.1`, `static`/`h3`/`core 0.3.0`, `bin 0.2.1`,
wheel `0.2.3`).

Source roadmap:

- `plans/subsystems/docs-architecture-roadmap.md#7`

Long-term requirements:

- `plans/000-long-term-specification.md` (product identity, ownership
  boundaries, safe-default invariants)
- `plans/001-terminology-and-domain-model.md` (crate/module vocabulary)

Applicable ADRs:

- `architecture/adr-002-windows-handle-relative-filesystem.md`
- `architecture/adr-003-custom-service-ownership.md`

Primary class: polish

## 1. Objective

Verify `architecture/overview.md` as the bird's-eye index over every discrete
module/tool/capability and systematically walk each `architecture/*.md` deep
dive once, correcting stale claims against current code. Docs-only; no
behavior, API, dependency, or tier change.

## 2. Why this milestone is ready

No hard dependencies: code baseline (Plans 280–291) is closed with
registry-qualified publication evidence. Interface dependency (current code as
review input) is stable. Docs-only so no migration or platform gate blocks.

## 3. Current implementation evidence

- `architecture/overview.md` (707 lines): Plan 259–260 index shape with crate
  overviews, capability map, tool map, deep-dive index, lifecycle diagrams,
  module maps. Plan 270–291 claims present; needs systematic verification.
- 26 files in `architecture/` (overview + 23 deep dives + 2 ADRs).
- Ownership enforced by `scripts/check-crate-topology.py`; conformance
  corpora validated by `scripts/verify-conformance-matrix.py`.

## 4. Invariants that must not regress

- Safe defaults (loopback, no symlinks/dotfiles/listing unless opt-in).
- No serving outside root; `eggserve-static` sole path/FS authority;
  `eggserve-server` single H1 runtime; QUIC only behind `http3`.
- H1 + canonical `primitives` supported; `server`/H2/H3/tunnel/trailer/
  adapter/listener/proxy/TLS-identity/async-Python remain experimental.
- Library code emits via `OpsContext`, never `println!`/`eprintln!`.
- `architecture/overview.md` remains the index; every link resolves.

## 5. Scope

### In scope

- Verify/refresh `architecture/overview.md` crate, capability, and tool
  sections against current manifests, source layout, scripts, conformance
  corpora, and tier labels.
- Walk each deep dive once via subagents; fix stale versions, paths, feature
  flags, counts (fuzz targets, matrix entries, scenarios), and broken links.
- Registry + roadmap status updates + closure record.

### Explicitly out of scope

- Any production-code edit (findings become follow-up plans).
- `docs/` normative rewrite; `plans/000`–`003` canonical edits.
- Tier promotions, new features, dependency changes, `non-goals.md` crossing.

## 6. Required production changes

None (docs-only). If a subagent finds code/doc contradiction, code wins and
the finding is recorded for a future corrective plan.

### Crates and ownership

No ownership change; verify topology claims match
`scripts/check-crate-topology.py`.

### Config and policy

No config change; verify field/policy names against source.

### Protocol and compatibility

No protocol change; keep experimental labels on H2/H3/tunnel/adapter/
listener/proxy/TLS-identity/async-Python.

### Runtime and concurrency

No runtime change; lifecycle wording checked, not redesigned.

### Frontend or operator surface (CLI / Python)

No surface change; CLI-grammar and Python-facade claims verified.

### Security and confinement

No security change; confinement claims re-verified.

### Documentation and static guards

Update `architecture/*.md` only. No guard-script changes.

## 7. Ordered work packages

### Work package A — Overview verification (main agent)

Intent: confirm the bird's-eye map matches reality.

Required changes: refresh `architecture/overview.md` sections (workspace
layout, crate table, feature flags, capability map, tool map with
script/corpora/fuzz/benchmark/test/example/CI tables, module maps, error
taxonomy, platform support, testing strategy, release process).

Acceptance evidence: every claim traceable to a manifest, source file,
script `--help`/header, or corpus file; all internal links resolve.

### Work package B — Systematic deep dives (subagents)

Intent: walk each component once without exhausting main context.

Required changes: subagents review grouped deep dives against code and
report findings; main agent applies minimal doc corrections.

Groups: (1) primitives+server+static, (2) eggnet-tls+tls+H2+H3,
(3) core+bin+python, (4) path+filesystem+policy+security,
(5) runtime+response-planning+primitives-api+configuration+errors+logging,
(6) crate-topology+testing-and-conformance. Findings that need code fixes
leave the docs unchanged and file a follow-up plan instead.

Acceptance evidence: each of the 23 deep dives walked once; per-group
report with walked files, stale claims fixed, and open findings.

### Work package C — Plan trace + commit

Intent: close the plan-driven loop.

Required changes: registry update, closure record, commit + push.

Acceptance evidence: `registry.md` links roadmap/plan/closure; closure
record has requirement-to-evidence matrix with exact verification commands;
clean `git status`; pushed `main`.

## 8. Failure, cancellation, restart, and contention semantics

Docs-only: no runtime failure modes. Partial completion leaves some deep
dives unwalked — recorded as open findings, never claimed complete.
Contradictory evidence stops that section's edit pending maintainer review.

## 9. Compatibility and migration

No compatibility effect. Version strings stay at published values
(`eggserve-server 0.3.1`, `static`/`h3`/`core 0.3.0`, `bin 0.2.1`, wheel
`0.2.3`, PyO3 `0.29.2`) unless manifests prove otherwise.

## 10. Required tests

Docs-only: no new unit/integration tests. Static gates substitute:

### Focused unit tests

None.

### Integration tests

None.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

None.

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
```

Broader `./scripts/verify.sh fast` only if time permits (docs-only change;
`full`/`deep` not required — needs Python 3.14 + maturin and expensive
suites).

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `architecture/overview.md` (index refresh).
- `architecture/*.md` deep dives (stale-claim corrections only).
- `plans/subsystems/docs-architecture-roadmap.md` (status).
- `plans/registry.md` (register 293).
- `plans/closure/docs-architecture/293-architecture-overview-deep-dive-refresh.md`
  (closure record).

## 13. Acceptance criteria

- Overview gives a 2–4 sentence bird's-eye per discrete module/tool/
  capability with a working link to the owning deep dive.
- Every deep dive walked once; stale claims corrected; open findings listed.
- All `architecture/` internal links resolve.
- Topology + conformance-matrix gates pass; `cargo fmt --check` passes.
- Registry + roadmap + closure record complete; commit pushed to `main`.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- an unresolved architecture decision materially changes ownership;
- repository evidence contradicts a canonical invariant;
- scope would expand into code changes or a `docs/non-goals.md` crossing;
- external evidence is required but unavailable (registry, platform, browser).

## 15. Closure evidence required

Requirement-to-evidence matrix; exact verification commands with outcomes;
per-group subagent walk report; link-resolution evidence; unresolved
findings by severity; roadmap disposition.

## 16. Handoff notes

Work only in `/home/sugarwookie/projects/eggserve`. Use subagents per work
package B groups to preserve context. Preserve unrelated user changes; repo
baseline is clean (`main` at Plan 292). Keep edits docs-only.
