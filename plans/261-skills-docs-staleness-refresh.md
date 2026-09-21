# Plan 261 — Skills, agent guide, and docs staleness refresh

## Purpose

Fix the stale and omission drift found during the session review of agent-facing
skills, `AGENTS.md`, `architecture/`, `docs/`, `README.md`, `plans/ROADMAP.md`,
and `conformance/` so future agents work from accurate, non-stale sources.
Docs, metadata, and skill text only.

Planning baseline:

```text
7bce227 docs: systematic architecture deep-dive refresh (260)
```

## Constraints

- Docs/metadata/skill text only. No Rust/Python source, test, script, config,
  dependency, or lockfile change.
- No public API, ownership, topology, feature-gate, or support-tier change
  (H1 + canonical `primitives` supported; `server`/H2/H3/tunnel/trailer/
  adapter/listener/proxy/TLS-identity/async-Python remain experimental).
- No rewrite of `plans/` or `release/` evidence. Cite them, don't duplicate.
- Every factual claim verified against the code cited (Read/Grep), not memory.
- Keep the direct `rustls` `0.23.45` caret floor and `0.2.0` pre-1.0 line
  statements intact.
- Keep all `architecture/overview.md` index links resolving.
- There is no `.skills/` directory in this workspace. The single skill source
  is `.opencode/skills/eggserve-dev/SKILL.md`; `.agents/skills/eggserve-dev`
  is a symlink to it. Do not create `.skills/`; document the real locations.

## Track A — Skill (`eggserve-dev`)

File: `.opencode/skills/eggserve-dev/SKILL.md` (symlinked from `.agents/`).

- Fix the post-convergence range: "Plans 251–256" → "Plans 251–258",
  naming Plan 257 (suppressed-body permit corrective) alongside the existing
  Plan 258 closure citation.
- Note Plans 259 (overview as bird's-eye index) and 260 (deep-dive refresh)
  as docs-only; no behavior/tier change.
- Fix the topology-gate scope: "Plan 211–247" → "Plans 211–253 rules
  (+254–258 maintenance/async-lifetime notes)".
- Add the `verify.sh full` gotcha already in `AGENTS.md`: needs Python 3.14 +
  maturin (`PYTHON=` overrides, default `python3.14`).
- Leave the CI command list otherwise intact (order difference vs
  `ci.yml` TLS-before-H2 is a harmless summary, not a wrong command).

## Track B — `AGENTS.md`

- Fix "`plans/` + `ROADMAP.md`" → "`plans/ROADMAP.md`".
- Add one skills-location note (canonical `.opencode/skills/`, symlinked
  `.agents/skills/`; no `.skills/` directory) and keep the
  `architecture/overview.md` index pointer.
- Otherwise prune-only: AGENTS.md was compressed in `a755ad1`; do not
  re-expand tripwires or duplicate the skill.

## Track C — `architecture/`

- `overview.md`: extend the plan-context line to name docs-only Plans 259–260;
  fix the two "Plans 211–253" gate scopes (Tool Map row + Deep Dive Index
  `crate-topology` row) to "Plans 211–253 rules (+254–258 notes)".
- `crate-topology.md`: header already names 251–252 + 254–258; no rule change.
  No other deep-dive edits (Plan 260 just refreshed them).

## Track D — `docs/`, `README.md`, `plans/ROADMAP.md`, `conformance/`

- `docs/tls.md`: fix both `eggserve 0.1.0` startup blocks to `0.2.0`.
- `docs/http-interop.md`: fix both `version = "0.1"` Cargo snippets to `"0.2"`.
- `conformance/app_server_conformance.toml`: fix the phantom
  `benchmarks/207-conformance/` evidence path to the on-disk
  `benchmarks/170-closure/` matrix (stored path when executed).
- `plans/ROADMAP.md`: append a short "Docs-only refresh — Plans 259–261"
  section marking 259/260 complete (commits `2ae733a`/`7bce227`) and 261 as
  this session; do not reopen any campaign result.
- `README.md`: extend the Plans 249–250 paragraph to name 251–258 (async
  suppressed-body permit fix, no API/tier change) and add the existing
  `benchmarks/241-fixed-cost-evidence-corrective/` to the evidence list.
- `plans/RELEASE-READINESS-ROADMAP.md`: untouched (historical Phase 31–44
  scheme; superseded by `ROADMAP.md`).

## Verification

- `python3 scripts/verify-conformance-matrix.py` passes.
- `python3 scripts/check-crate-topology.py` passes.
- `python3 scripts/check-python-release-metadata.py` passes.
- `cargo fmt --all -- --check` passes.
- `git status` shows only intended skill/AGENTS/architecture/docs/README/
  plans/ROADMAP/conformance files + this plan.
