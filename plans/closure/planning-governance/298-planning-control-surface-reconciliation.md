# Planning Governance Milestone 298 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/planning-governance/298-planning-control-surface-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/planning-governance-roadmap.md#7-milestones`

Repository baseline reviewed: `c1f4348939406c02f9739cdedff3bf7568ce09c6`
plus the Plan 299 implementation closed in the same commit set

Implementation commits or pull requests:

- (this commit) — Plan 298 control-surface reconciliation (planning/docs
  only) + Plan 299 H1 trailer repair with its own closure record

## 1. Executive finding

The Plan 292 bootstrap is retired from active status without touching the
immutable legacy file: every §5 acceptance item is evidenced below, the
registry no longer presents closed campaigns as ready/blocked, the
direct-H1 roadmap distinguishes the closed 294–297 campaign from the
299 repair (itself closed here), and durable agent guidance points at
`plans/registry.md` instead of a hard-coded next-plan number. No
runtime, protocol, API, package, or legacy-history change was made by 298.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Plan 292 §5 acceptance evidenced item-by-item | §3 below | pass | One non-material placeholder note (archive `.gitkeep`) |
| Legacy Plan 292 itself unchanged | `git log --oneline -1 -- plans/292-*.md` → `edd360b` (adoption commit); `git status` shows no `plans/292-*` modification | pass | ACTIVE line retained as history by design |
| Registry no longer describes closed 297 as ready/blocked | `plans/registry.md`: 297 row closed; blocked-work table contains no 297 entry; 299 row closed | pass | Blocked table now names no stale milestones |
| Direct-H1 roadmap: 294–297 closed, 299 separate | `plans/subsystems/direct-h1-runtime-roadmap.md` §4 (campaign closed + 299 repair wording) + §12 table (Milestone 8 closed with closure link) | pass | Status now closed after 299 |
| Agent/skill guidance without stale next-plan number | `AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md` (symlinked `.agents` copy inherits), `architecture/overview.md` point at `plans/registry.md` as the next-number/current-work authority | pass | `CONTRIBUTING.md` already registry-pointed; unchanged |
| 298 closure record + roadmap/registry agreement | This file; roadmap Milestone 1 → closed; registry governance → closed, 298 → closed | pass | — |
| No product/runtime/API/package change from 298 | `git status` file list (§4): planning/docs/agent-pointer files only; no `crates/`, manifests, or legacy plans | pass | 299 production files are separately owned by its closure |

## 3. Plan 292 §5 acceptance matrix

| 292 §5 item | Evidence | Result |
|---|---|---|
| All §3 files exist with stated names | `plans/README.md`, `000/001/002/003` canonical docs, `registry.md`, `adrs/README.md`, `subsystems/README.md` + static-confinement + direct-h1-runtime roadmaps, `implementation/README.md`, `closure/README.md`, `plans/archive/README.md`, `292-*.md` bootstrap — all present (`ls` green) | pass |
| `archive/.gitkeep` placeholder | `plans/archive/` exists with policy `README.md`; no `.gitkeep` | pass with note (non-material: the directory is non-empty so no keep-placeholder is needed; README is the policy placeholder) |
| No legacy `plans/NNN-*.md` or `release/` file modified, moved, or deleted | `git status --short -- 'plans/0*' 'plans/1*' 'plans/2*' release/` empty; 292 file last touched by `edd360b` | pass |
| Registry uses status vocabulary + links, not duplicates | `registry.md` status vocabulary (§proposed…archived) + dependency vocabulary + link-only rows to roadmaps/plans/closures | pass |
| Templates preserve CodeGG section structure, eggserve-adapted | `implementation/README.md` 16-section handoff template; `closure/README.md` 12-section gate; `subsystems/README.md` roadmap rules; `adrs/README.md` lifecycle — all with eggserve authorities (topology, verify.sh, supply-chain, conformance) | pass |
| AGENTS.md + skill + overview + CONTRIBUTING.md + ROADMAP.md notice point at the new hierarchy | `AGENTS.md` plan-driven bullet → hierarchy + registry; skill non-negotiable #4 → same; `architecture/overview.md` plan-context → registry; `CONTRIBUTING.md` → `plans/README.md` + `registry.md`; `plans/ROADMAP.md` live-control-surface notice → `registry.md` | pass |
| Working tree has only intended files, no Rust/build artifacts | 298 diff: `plans/registry.md`, `plans/subsystems/planning-governance-roadmap.md`, `AGENTS.md`, skill, `architecture/overview.md`, this closure (plus separately-owned 299 files); no `target/`, no artifacts | pass |

## 4. Production implementation evidence

None by design (298 is planning/docs cleanup only). Changed files in the
298 scope:

- `plans/registry.md` — governance → closed, 298 → closed, blocked-work
  table reconciled (299 closed, no stale ready entries).
- `plans/subsystems/planning-governance-roadmap.md` — Milestone 1 → closed
  with closure link; status → closed.
- `AGENTS.md` — plan-driven bullet points at `plans/registry.md` as the
  next-number/current-work authority (no hard-coded number).
- `.opencode/skills/eggserve-dev/SKILL.md` — same (the
  `.agents/skills/eggserve-dev` symlink inherits it).
- `architecture/overview.md` — plan-context points at `plans/registry.md`.
- This closure record.

Unchanged as required: `plans/292-*.md` (immutable), all legacy flat plans
and `release/` evidence, all `crates/` sources and manifests (299-owned
production files are covered by its closure, not this one).

## 5. Verification executed

### Commands run

```bash
git status --short
git diff --check
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
```

### Results

- `git status --short`: 298 scope shows only the planning/docs files
  listed in §4 (plus the separately-owned 299 production/test/docs files
  in the same commit set); no `plans/NNN-*.md`, `release/`, `target/`, or
  build artifacts.
- `git diff --check`: clean (no whitespace errors).
- `python3 scripts/check-crate-topology.py`: exit 0 (no graph/module drift;
  expected no-op for a docs-only reconciliation plus the separately-gated
  299 changes).
- `cargo fmt --all -- --check`: clean (298 touches Markdown only; the 299
  Rust files were formatted under its own verification).
- Link checks: every new planning pointer verified by path (`registry.md`,
  roadmaps, implementation plans, both closure records, skill symlink
  target). No separate link-check helper exists in the repo.
- No runtime suites required for the 298 docs-only scope (per plan §11);
  299 verification is recorded in its own closure.

## 6. Invariant review

- Legacy flat plans through 292 and `release/` evidence remain immutable:
  `git status` shows none modified; the 292 ACTIVE line stands as history
  while the registry + this closure carry current state.
- Registry status follows accepted closure evidence: 293, 294–297, 299
  rows link their accepted closures and read closed; 298 links this record.
- No contradictory active/ready/blocked state: blocked-work table names no
  milestones; no row reads ready for closed work.
- Each active implementation plan links to a subsystem roadmap and closure:
  vacuously true — no active milestones remain.
- No planning cleanup changed product/API/security claims: 298 diff is
  planning/docs/agent pointers only.
- Agent guidance points at the hierarchy without a stale next number:
  AGENTS.md + skill + overview use the registry authority line.

## 7. Migration and compatibility review

No compatibility effect. The only migration is conceptual: current work is
discovered through `plans/registry.md` and the new hierarchy; Plan 292
remains historical trace at its old path. No configuration, protocol,
dependency, or version change.

## 8. Security review

No security boundary change. Planning edits do not weaken documented
safety invariants; `docs/threat-model.md` / `docs/security-policy.md` /
`docs/non-goals.md` untouched (planning-process change crosses no product
non-goal, per 292 §2 non-goals).

## 9. Documentation and operations

- Updated: `plans/registry.md`, `plans/subsystems/planning-governance-roadmap.md`,
  `AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md`,
  `architecture/overview.md`, this closure record.
- Audit trail: git history + this record; legacy Plan 292 unchanged.
- Static guards: `git diff --check`, topology no-op, `cargo fmt` clean.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `plans/archive/.gitkeep` placeholder from 292 §3 absent (directory holds `README.md` instead) | None — directory is non-empty and documented | None; recorded here as the accepted deviation |

No medium+ findings. Planning governance closes; no corrective plan needed.

## 11. Roadmap disposition

Milestone closed and the planning-governance subsystem returns to closed
status. Plan 292 is historical bootstrap (immutable file retained);
current-state authority is this closure record plus `plans/registry.md`.
Future milestones register in the registry under the next free number with
subsystem roadmap → implementation plan → closure record, per
`plans/003-planning-process.md`.

## 12. Registry updates

- `plans/registry.md`: planning-governance subsystem → closed; 298 →
  closed with closure link; blocked-work table reconciled (no stale
  298/299 ready entries).
- `plans/subsystems/planning-governance-roadmap.md`: Milestone 1 → closed
  with closure link; status → closed.
