# Plan 262 — Agent-docs staleness follow-up (skills, ROADMAP, topology header)

## Purpose

Close the small residual drift left after Plan 261 (`1e42563` +
`28b8b2f`, tree clean): skill CI order vs `ci.yml`, `ROADMAP.md`
261 status, `RELEASE-READINESS-ROADMAP.md` historical signal, and the
`architecture/crate-topology.md` header range. Docs/metadata/skill
text only.

Planning baseline:

```text
28b8b2f docs: reconcile AGENTS.md CI order, fast/full scope, and policy pointers
```

## Constraints

- Docs/metadata/skill text only. No Rust/Python source, test, script,
  config, dependency, or lockfile change.
- No public API, ownership, topology, feature-gate, or support-tier
  change (H1 + canonical `primitives` supported; `server`/H2/H3/
  tunnel/trailer/adapter/listener/proxy/TLS-identity/async-Python
  remain experimental).
- No rewrite of `plans/` or `release/` evidence. Cite them, don't
  duplicate.
- Every factual claim verified against the code cited (Read/Grep),
  not memory.
- Keep the direct `rustls` `0.23.45` caret floor and `0.2.0` pre-1.0
  line statements intact.
- Keep all `architecture/overview.md` index links resolving.
- There is no `.skills/` directory in this workspace. The single skill
  source is `.opencode/skills/eggserve-dev/SKILL.md`;
  `.agents/skills/eggserve-dev` is a symlink to it. Do not create
  `.skills/`; document the real locations.

## Track A — Skill (`eggserve-dev`)

File: `.opencode/skills/eggserve-dev/SKILL.md` (symlinked from
`.agents/`).

- Fix the CI command order to match `.github/workflows/ci.yml`
  exactly: TLS-only `eggserve-bin` lint+tests before the H2-gated
  `eggserve-core`/`eggserve-bin` lint+tests (Plan 261 knowingly left
  H2-before-TLS as a "harmless summary"; executable source wins per
  `AGENTS.md` conflict rule).
- No other skill change (topology-gate `(+254–258 notes)` wording
  stays: the notes live in the `architecture/crate-topology.md`
  Plan 253 ledger, not as script rules).

## Track B — `AGENTS.md`

- No content change needed: CI order (TLS-bin before H2),
  `fast`/`full` scope, and the skills-location note
  (`.opencode/skills/`, symlinked `.agents/skills/`, no `.skills/`)
  already match executable source after `28b8b2f`.
- This plan is the trace record for that "verified current, left
  intact" decision (prune-only; no re-expansion).

## Track C — `architecture/`

- `crate-topology.md`: header "Plans 211–247 establish dependency
  layers" → "Plans 211–253 establish dependency layers" (script
  header is Plan 211–253; the doc already covers 249/250/253
  below). Clarify the "Plans 251–252 and 254–258" campaign line
  with a pointer to the Plan 253 overlap ledger section so future
  agents don't read 253 as skipped.
- No other deep-dive edits (Plans 259–260 just refreshed them;
  `eggserve-primitives.md` stays a concise leaf stub by design).

## Track D — `docs/`, `README.md`, `plans/ROADMAP.md`

- `plans/ROADMAP.md`: mark 261 COMPLETE (commits `1e42563`,
  `28b8b2f`) and append this Plan 262 entry; do not reopen any
  campaign result.
- `plans/RELEASE-READINESS-ROADMAP.md`: add a short historical /
  superseded-by-`ROADMAP.md` header so readers opening the file
  directly get the staleness signal currently recorded only in
  Plan 261 constraints.
- `docs/`, `README.md`, `conformance/`: verified current
  (`docs/tls.md` + `docs/http-interop.md` already `0.2`,
  `conformance/app_server_conformance.toml` already points at
  on-disk `benchmarks/170-closure/`, README 251–258 + 241-evidence
  pointers complete). No edits.

## Verification

- `python3 scripts/verify-conformance-matrix.py` passes.
- `python3 scripts/check-crate-topology.py` passes.
- `python3 scripts/check-python-release-metadata.py` passes.
- `cargo fmt --all -- --check` passes.
- `git status` shows only intended skill/architecture/plans files +
  this plan.
