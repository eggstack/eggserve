# Plan 292 — Planning-convention migration to CodeGG style

Status: **ACTIVE** (scaffolding in progress this session).

Planning baseline: `af0fcb5` (docs(release): close direct H1 ownership publication).

Source conventions reviewed (local `/home/sugarwookie/projects/codegg`):

- `plans/README.md` — durable vs interim separation, hierarchy, directory roles, lifecycle, classification, naming.
- `plans/003-planning-process.md` — normative governance (document classes, work classification, dependency model, sizing, handoff, corrective passes, registry, review, anti-patterns).
- `plans/registry.md` — compact control surface with status vocabulary.
- Templates: `plans/implementation/README.md` (16-section handoff), `plans/closure/README.md` (12-section closure gate), `plans/subsystems/README.md` (12-section roadmap), `plans/adrs/README.md` (ADR lifecycle + template).

## 1. Gap analysis (eggserve current vs CodeGG target)

EggServe current style:

- Flat `plans/NNN-*.md` (~300 files, `000`–`291` plus `ROADMAP.md`, `RELEASE-READINESS-ROADMAP.md`, `CORRECTIVE-CLOSURE-PHASES-31-35.md`, program indices like `217-225-...`, `280-286-...`, `288-291-...`).
- Each plan carries its own `Status/Goal/Tracks/baseline SHA` shape but there is **no shared template**, no required work classification, no dependency-type vocabulary, no standard status vocabulary, no agent-handoff contract, no closure-evidence gate definition.
- Closure/qualification evidence lives in `release/plan-*.md` (per-plan reports) rather than a `closure/<subsystem>/NNN-status.md` gate paired 1:1 with its implementation plan.
- `plans/ROADMAP.md` mixes long-term direction, milestone history, and live status in one narrative file. There is no `registry.md` compact control surface; no `subsystems/` dependency-ordered roadmaps; no `adrs/` durable-decision log (ADRs exist inline as `architecture/adr-*.md` only); no `implementation/` vs `closure/` vs `archive/` split.
- Governance (`AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md`, `architecture/overview.md`, `CONTRIBUTING.md`) says "every change backed by a plan in `plans/`" and "plans/ are change-trace, not normative", but does not define document classes, authority order, corrective-pass rules, or archive policy.

CodeGG target style (adopted, eggserve-adapted):

- Hierarchy: canonical long-term docs → ADRs → master roadmap → subsystem roadmaps → milestone implementation plans → implementation/verification → closure records → archive; `registry.md` as the compact control surface.
- Directory roles: `adrs/`, `subsystems/`, `implementation/<subsystem>/`, `closure/<subsystem>/`, `archive/` (traceability-preserving moves), `registry.md` (links, not duplicates).
- Lifecycle: identify spec sections → record ADR if needed → subsystem roadmap → dependency-ready milestone → bounded handoff plan → implement/verify → closure record → registry + roadmap update → archive when inactive. No milestone is complete on code-landing alone.
- Classification (every roadmap/plan item): **invariant / capability / infrastructure / polish**; infrastructure/polish are never presented as completed user capability.
- Naming: `adrs/ADR-NNNN-*`, `subsystems/<subsystem>-roadmap.md`, `implementation/<subsystem>/NNN-*`, `closure/<subsystem>/NNN-status.md`; stable subsystem names; no dates in filenames unless inherently time-bound.
- Status vocabulary: proposed / ready / active / blocked / closing / closed / conditionally closed / superseded / archived.
- Dependency model: hard / interface / soft / operational; dependency-ready = all hard closed + all interface under stable written contract.
- Corrective passes are **new plans**, never silent amendments; closures stay immutable except factual corrections.

## 2. Objective

Stand up the CodeGG-style planning hierarchy in this repo, seeded with eggserve content, without rewriting the ~300 legacy plan files or any `release/` evidence:

- New governance + templates + registry + canonical docs + directory READMEs.
- Two exemplar subsystem roadmaps proving the pattern against real closed work.
- Pointer updates in `AGENTS.md`, skill, `architecture/overview.md`, `CONTRIBUTING.md`, `plans/ROADMAP.md` so agents land on the new system.
- Legacy flat `plans/NNN-*.md` + `release/plan-*.md` declared **archived in place** (immutable trace); all new work (293+) uses the new hierarchy.

Explicit non-goals for this plan:

- No mass `git mv` of legacy plans into `archive/` (follow-up may do the mechanical move; history preservation matters more than directory purity today).
- No rewrite of legacy plan contents, statuses, or `release/` evidence.
- No product, API, dependency, topology, or support-tier change.
- No `docs/non-goals.md` / `docs/threat-model.md` update (planning-process change crosses no product non-goal).

## 3. Target structure (Phase 1 — this session)

```text
plans/
  README.md                        # new system index (durable vs interim, roles, lifecycle)
  000-long-term-specification.md   # canonical end-state + invariants (concise, links to docs/)
  001-terminology-and-domain-model.md
  002-long-term-roadmap.md          # dependency-ordered roadmap (concise, links to ROADMAP.md history)
  003-planning-process.md           # normative governance (eggserve-adapted)
  registry.md                      # compact control surface (status vocab + active/closed tables)
  adrs/README.md                   # ADR lifecycle + template (numbers continue; existing architecture/adr-* stay)
  subsystems/README.md             # roadmap template + rules
  subsystems/static-confinement-roadmap.md      # exemplar (closed)
  subsystems/direct-h1-runtime-roadmap.md       # exemplar (closed, 288-291 evidence)
  implementation/README.md         # 16-section handoff template + rules
  closure/README.md                # 12-section closure template + gate rules
  archive/.gitkeep                 # archive policy placeholder
  292-planning-convention-migration-to-codegg-style.md  # this file (bootstrap, legacy location)
```

Note on `000`–`003` numbering: legacy files (`000-foundation-security-contract.md`, `001-...`, `002-...`, `003-static-file-serving-mvp.md`) keep their names untouched. The four new canonical files use CodeGG-identical titles with different suffixes, so there is no overwrite; this file declares the new four canonical and the legacy four historical.

## 4. Execution tracks

### Track A — governance + templates (no code)

1. Write `plans/README.md`, `plans/003-planning-process.md`, the four directory READMEs, and `archive/.gitkeep`, adapted from CodeGG text with eggserve authorities (crate topology, `verify.sh fast/full`, supply-chain both-lockfiles, `check-crate-topology.py`, conformance corpora, `release/` → `closure/` forward-pointer).
2. Keep CodeGG template section structure verbatim where it already fits (16/12/12/ADR); eggserve-adapt only the examples, commands, and ownership vocabulary (e.g. `RuntimeConfig`/`SecureRoot`/`Service`, H1/H2/H3 tiers, Python wheel).

### Track B — canonical docs + registry (concise, link-don't-duplicate)

1. `000-long-term-specification.md`: product identity, crate authorities, safe-default invariants, protocol tiers, Python facade scope; normative pointers to `docs/` + `architecture/` (which remain the normative user/embedding contracts).
2. `001-terminology-and-domain-model.md`: canonical terms (`ConfinedPath`, `SecureRoot`, `StaticPolicy.symlinks`, `Service`, `RequestBody` one-shot, `ResponsePolicy`, `OpsContext`, error taxonomy, plan/closure/registry vocabulary).
3. `002-long-term-roadmap.md`: dependency-ordered phases ending in the closed 288–291 state; full history stays in `plans/ROADMAP.md` (linked, not copied).
4. `registry.md`: status vocabulary + active subsystem roadmaps table (the two exemplars, closed) + dependency-ready table (empty: no open milestones) + blocked table (none) + legacy-trace pointer + "new work starts at 293 in `implementation/`" rule.

### Track C — exemplar subsystem roadmaps

1. `static-confinement-roadmap.md` (status closed): ownership (`eggserve-static` sole authority), invariants/non-goals, milestones mapped to closed legacy plans + `release/` evidence, completion definition.
2. `direct-h1-runtime-roadmap.md` (status closed): single-H1-authority + boundary-ownership story through Plans 215–217/244/249–250/270/278–291, tiers unchanged, evidence links.

### Track D — pointer updates (agent-facing, minimal diff)

1. `AGENTS.md`: plan-driven bullet names the new hierarchy (`registry.md` → subsystem roadmap → `implementation/` plan → `closure/` record; legacy flat files archived in place).
2. `.opencode/skills/eggserve-dev/SKILL.md`: same pointer update in the non-negotiables item + `plans/` layout line.
3. `architecture/overview.md`: plan-context paragraph notes the new hierarchy + registry.
4. `CONTRIBUTING.md`: plan location sentence points at `plans/README.md` + `registry.md`.
5. `plans/ROADMAP.md`: prepend a short notice pointing to `registry.md` as the live control surface (history below unchanged).

## 5. Acceptance criteria

- All files in §3 exist with the stated names; no legacy `plans/NNN-*.md` or `release/` file modified, moved, or deleted.
- `plans/registry.md` uses the CodeGG status vocabulary and links (not duplicates) source documents.
- New templates preserve CodeGG section structure with eggserve-adapted content.
- `AGENTS.md` + skill + overview + `CONTRIBUTING.md` + `ROADMAP.md` notice all point at the new hierarchy consistently.
- Working tree contains only the intended new/modified files (`git status` review); no Rust/build artifacts.

## 6. Verification

```bash
git status --short
git diff --stat
python3 scripts/check-crate-topology.py   # unaffected; confirms no graph/module drift
cargo fmt --all -- --check                # unaffected (docs-only change); run if toolchain present
```

Topology/fmt are expected-green no-ops (no `crates/` touched). Record outcomes in the final summary; do not claim unrun commands.

## 7. Follow-ups (out of scope, recorded for 293+)

- Mechanical `archive/` migration of legacy flat plans (traceability-preserving `git mv` + redirect index).
- Additional subsystem roadmaps (tls-identity, h2-h3-transports, python-facade, cli-ops-observability, proxy-metadata).
- ADR backfill (`architecture/adr-002/003` → `plans/adrs/` supersede-links, never rewrites).
- First real milestone run through the new handoff → closure → registry loop to prove the workflow.
