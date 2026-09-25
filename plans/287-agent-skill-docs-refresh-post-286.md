# Plan 287 — Agent skill/docs refresh past Plan 286

## Purpose

Close the stale drift left after Plans 280–286 shipped `0.3.0`, so future
agents work from accurate, non-stale sources. Docs, metadata, and skill text
only.

Planning baseline:

```text
467a92d docs(architecture): refresh overview index past Plan 286, expand thin leaves, correct deep dives
```

## Findings (verified against code, not memory)

- Skill (`.opencode/skills/eggserve-dev/SKILL.md`, symlinked from
  `.agents/skills/`; there is no `.skills/` directory, no agents
  directory, no `opencode.json`): stale at Plans ≤277. Still says the
  `0.2.3` source candidate is "publication-pending" and tells agents not
  to describe the direct registry path as available. Both contradict
  `release/plan-286-embedding-contract-publication-closure.md`
  (server/static/h3/core `0.3.0` + primitives `0.2.1` + bin `0.2.1`
  published 2026-09-24 with registry-only consumer proof) and
  `architecture/overview.md:40-56`. Missing Plans 278–286 entirely
  (absolute-form seam, policy/admission ownership, `H1ConnectionPolicy`,
  typed rejection, TunnelIo KEEP, `0.3.0` version decision).
- `AGENTS.md:146`: same stale `0.2.3` publication-pending claim.
- `README.md:91,103` + `docs/http-interop.md:18-21`: `version = "0.2"`
  snippets predate the published `0.3.0` leaves; the "0.2.3 release
  candidate … becomes available after publication" framing is spent.
- `docs/http-interop.md:12-15`, `docs/release-process.md:97-108`:
  publication-pending language; the standalone `0.2.3` was folded into
  Plan 286 and never published alone.
- `CHANGELOG.md`: top entry still `0.2.3 — release candidate;
  publication pending`; no `0.3.0` entry.
- Verified current, left intact: `architecture/` deep dives (refreshed
  past Plan 286 in `467a92d`; all 35 overview index links resolve),
  `docs/` otherwise (telemetry/tracing/`BoxBody`/`follow_symlinks`/
  `0.1.x` hits are all intentional negative-scope guards, confirmed via
  grep), `plans/ROADMAP.md` campaign sections, `conformance/` inventory.
- No new skill, agent, or micro-crate is created: the single
  `eggserve-dev` skill remains the right shape (Plan 261/262 precedent);
  splitting it would add sync burden with no concrete consumer blocker.

## Constraints

- Docs/metadata/skill text only. No Rust/Python source, test, script,
  config, dependency, or lockfile change.
- No public API, ownership, topology, feature-gate, or support-tier change
  (H1 + canonical `primitives` supported; `server`/H2/H3/tunnel/trailer/
  adapter/listener/proxy/TLS-identity/async-Python remain experimental).
- No rewrite of `plans/` or `release/` evidence. Cite them, don't duplicate.
- Every factual claim verified against the code cited (Read/Grep).
- Keep the direct `rustls` `0.23.45` caret floor and `0.1.x` pre-1.0 line
  statements intact.
- Keep all `architecture/overview.md` index links resolving.
- There is no `.skills/` directory in this workspace. The single skill source
  is `.opencode/skills/eggserve-dev/SKILL.md`; `.agents/skills/eggserve-dev`
  is a symlink to it. Do not create `.skills/`; document the real locations.

## Tracks

### Track A — Skill (`eggserve-dev`)

- Replace the stale Plans 276–277 "0.2.3 publication-pending / do not
  describe the direct registry path as available" paragraph with the
  published outcome: standalone `0.2.3` folded into Plan 286, never
  published alone; `eggserve-server` owns the optional `http-interop`/
  Tower adapters in the published `0.3.0` artifact set (core forwards as
  compatibility re-exports; direct H1 + Tower graph stays free of
  `eggserve-static`/PHF, proven by registry-only consumers).
- Add a Plans 278–286 paragraph: opt-in absolute-form seam (Plan 278,
  `OriginOnly` default, static stays origin-only), external
  deadline/ceiling ownership (`PolicyOwner`, Plan 280), external
  service/tunnel admission (`AdmissionOwnership`, Plan 281), narrow
  `H1ConnectionPolicy` projection (Plan 282), presentation-only typed
  rejection (`RuntimeRejectionKind`/`RuntimeRejection`, Plan 283),
  TunnelIo direct-transport KEEP (Plan 284), `0.3.0` version decision
  (Plan 285: exhaustive `RuntimeConfig` literals + semaphore accessors
  are source-incompatible), publication + registry-only proof (Plan 286).
- CI command list stays intact (TLS-bin before H2 already matches
  `ci.yml` per Plan 262).

### Track B — `AGENTS.md`

- Fix the stale `0.2.0`/`0.2.1`/`0.2.2` + `0.2.3` publication-pending
  tripwire: standalone `0.2.3` folded into Plan 286, never published
  alone; current line is server/static/h3/core `0.3.0`, primitives
  `0.2.1`, bin `0.2.1`, wheel `0.2.3`.
- Update the Plans 276–277 tripwire to the published outcome (server owns
  adapters in `0.3.0`; core forwards; direct Tower graph excludes
  static/PHF). Prune-only otherwise; do not re-expand tripwires or
  duplicate the skill. Keep the skills-location note and the
  `architecture/overview.md` index pointer as the doc index.

### Track C — `architecture/`

- `overview.md` plan-context line only: extend "Plans 259–260 are
  docs-only refreshes" to name 261–262 + this Plan 287. No other deep-dive
  edits (Plan 260 + `467a92d` just refreshed them; links verified
  resolving).

### Track D — `docs/`, `README.md`, `CHANGELOG.md`, `plans/ROADMAP.md`

- `README.md`: `eggserve-core = "0.2"` → `"0.3"`,
  `eggserve-server version = "0.2"` → `"0.3"`; replace the spent
  "0.2.3 release candidate … after publication" framing with the
  published `0.3.0` direct Tower outcome + registry-only proof pointer.
- `docs/http-interop.md`: same `0.2` → `0.3` snippet bump; replace the
  publication-pending paragraph with the published `0.3.0` outcome.
- `docs/release-process.md`: replace the `0.2.3` publication-pending
  paragraph with the folded-into-286 outcome + `0.3.0` set; fix
  "Continue on the `0.2.x` line" to the current leaf line.
- `CHANGELOG.md`: add the `0.3.0 — published` entry (adapter ownership in
  server, absolute-form seam, policy/admission ownership,
  `H1ConnectionPolicy`, typed rejection, TunnelIo KEEP, checksums +
  registry proof in the Plan 286 closure); re-mark the `0.2.3` entry as
  folded, never published alone.
- `plans/ROADMAP.md`: extend the "Docs-only refresh — Plans 259–261"
  section to 259–262 + 287 (this session), marking 287 alongside the
  existing COMPLETE marks.
- `plans/RELEASE-READINESS-ROADMAP.md`: untouched (historical; header
  already signals superseded-by-`ROADMAP.md` per Plan 262).

## Verification

- `python3 scripts/verify-conformance-matrix.py` passes.
- `python3 scripts/check-crate-topology.py` passes.
- `python3 scripts/check-python-release-metadata.py` passes.
- `cargo fmt --all -- --check` passes.
- `git status` shows only intended skill/AGENTS/architecture/docs/README/
  CHANGELOG/plans files + this plan.
