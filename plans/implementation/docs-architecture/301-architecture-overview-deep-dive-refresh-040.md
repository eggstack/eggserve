# Docs Architecture Milestone 301 — Overview + Deep-Dive Refresh (0.4.0 baseline)

Status: active

Repository baseline: `main` post-Plan-299 (closed 294–299 code state, Plan 300
active publication: `eggserve-server`/`static`/`h3`/`core 0.4.0`,
`primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`).

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
dive once, correcting stale 0.3.x claims against the 0.4.0 baseline
(Plans 294–299 closed, Plan 300 active). Docs-only; no behavior, API,
dependency, or tier change.

## 2. Why this milestone is ready

No hard dependencies: code baseline (Plans 294–299) is closed with closure
evidence; Plan 300 publication is operational (versions/metadata only).
Interface dependency (current code as review input) is stable. Docs-only so
no migration or platform gate blocks.

## 3. Current implementation evidence

- `architecture/overview.md` (713 lines): Plan 293 index shape with crate
  overviews, capability map, tool map, deep-dive index, lifecycle diagrams,
  module maps. Version lines still at 0.3.x; needs 0.4.0 refresh plus
  294–297 benchmark and 299 trailer-F3 pointers.
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
  corpora, and tier labels (0.4.0 versions, 294–297 benchmarks, 299 F3,
  296 tower WP-A edge).
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

## 7. Ordered work packages

### Work package A — Overview verification (main agent)

Refresh `architecture/overview.md` sections (workspace layout, crate table,
feature flags, capability map, tool map, module maps, error taxonomy).

### Work package B — Systematic deep dives (subagents)

Six parallel groups walked all deep dives (leaf crates, composition
frontends, security/policy, runtime/transports, ops/config, tools/quality).
Main agent applies minimal doc corrections.

### Work package C — Plan trace + commit

Registry update, closure record, commit + push.

## 8. Failure semantics

Docs-only: no runtime failure modes. Partial completion leaves some deep
dives unwalked — recorded as open findings, never claimed complete.

## 9. Compatibility and migration

No compatibility effect. Version strings move to manifests-proven 0.4.0
values (`server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`,
`bin 0.2.2`, wheel `0.2.4`, `eggnet-tls` workspace `0.2.4`, PyO3 `0.29.2`).

## 10. Required tests

None (docs-only).

## 11. Required verification commands

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
```

## 12. Documentation updates

- `architecture/overview.md` (index refresh to 0.4.0).
- `architecture/*.md` deep dives (stale-claim corrections only).
- `plans/subsystems/docs-architecture-roadmap.md` (status).
- `plans/registry.md` (register 301).
- `plans/closure/docs-architecture/301-architecture-overview-deep-dive-refresh-040.md`
  (closure record).

## 13. Acceptance criteria

- Overview gives a 2–4 sentence bird's-eye per discrete module/tool/
  capability with a working link to the owning deep dive.
- Every deep dive walked once; stale claims corrected; open findings listed.
- All `architecture/` internal links resolve.
- Topology + conformance-matrix gates pass; `cargo fmt --check` passes.
- Registry + roadmap + closure record complete; commit pushed to `main`.

## 14. Stop conditions

Stop and report rather than improvise when ownership changes, canonical
invariants are contradicted, scope expands into code changes, or external
evidence is unavailable.

## 15. Closure evidence required

Requirement-to-evidence matrix; exact verification commands with outcomes;
per-group subagent walk report; link-resolution evidence; unresolved
findings by severity; roadmap disposition.

## 16. Handoff notes

Work only in `/home/sugarwookie/projects/eggserve`. Use subagents per work
package B groups to preserve context.
