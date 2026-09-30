# Planning Governance Milestone 302 — Agent/Skill/Docs Staleness Sweep (0.4.0 line)

Status: active

Repository baseline: `main` post-Plan-301 (docs-architecture closed at the
0.4.0 baseline; Plan 300 active publication: `eggserve-server`/`static`/
`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`).

Source roadmap:

- `plans/subsystems/planning-governance-roadmap.md#7` (agent-facing control
  surface: registry, skill/agent-guide pointers)
- Interface: `plans/subsystems/docs-architecture-roadmap.md` (mechanical
  version-string sync in `docs/`/`architecture/`/`README.md`; no normative
  rewrite, roadmap stays closed)

Long-term requirements:

- `plans/000-long-term-specification.md` (product identity, safe-default
  invariants)
- `plans/001-terminology-and-domain-model.md` (crate/module vocabulary)

Primary class: polish

## 1. Objective

Sweep version drift left after Plans 293–301 moved the tree to the 0.4.0
line, so future agents work from accurate sources. Skill text, `AGENTS.md`,
`README.md`, `docs/`, `architecture/`, and planning metadata only.
Docs-only; no behavior, API, dependency, or tier change.

## 2. Why this milestone is ready

No hard dependencies: the 0.4.0 code baseline is fixed (Plan 300 bump
landed; publication is operational versions/metadata work). Interface
dependency (current manifests + registry as review input) is stable.
Follows the Plan 287 agent-skill-refresh precedent (same file set, same
docs-only class).

## 3. Current implementation evidence

Subagent staleness audits (verified against manifests, registry, CI config —
not memory) found, all post-0.4.0-bump drift:

- Skill (`.opencode/skills/eggserve-dev/SKILL.md`, symlinked from
  `.agents/skills/`; there is no `.skills/` directory, no `opencode.json`):
  ends at Plans 288–291, cites the 0.3.x set as the current registry path,
  omits Plans 293–301 + active Plan 300 + the 0.4.0 line entirely. CI
  sequence, crate layout, dependency pins, and referenced paths verified
  current.
- `AGENTS.md`: per-crate versions + `verify.sh fast` scope already corrected
  this session (5 insertions, 2 deletions).
- `README.md:91,103,113-114` + `docs/http-interop.md:18-21`: `version = "0.3"`
  snippets predate the 0.4.0 leaves (`"0.3"` caret never matches `0.4.0`).
- `docs/release-contract.md:5-17`: development metadata + release-line
  wording predate the 0.4.x line.
- `docs/tls.md:206,215`: startup banner `eggserve 0.2.1` predates bin `0.2.2`
  (binary prints `env!("CARGO_PKG_VERSION")`).
- `docs/migration-guide.md:351`: `0.2.0` transition framed as "next";
  historical since Plan 226.
- `architecture/overview.md:27-29`: Plan 301 framed as active; registry says
  closed (300 active).
- `architecture/eggserve-core.md:16`: stale "now at `0.3.0`" sentence beside
  the current "now at `0.4.0`".
- `architecture/structured-logging.md:234`: example event `0.3.0` matches
  neither bin `0.2.2` nor the 0.4.0 line.
- `architecture/eggserve-bin.md:164`: Plan 286 closure mislabeled as
  "`0.2.2` publication evidence (Plan 300)"; it is 0.3.0-line evidence.
- `plans/registry.md:12`: "continue from `293`" stale; 293–301 taken.
- `plans/subsystems/docs-architecture-roadmap.md:75`: Milestone 301 marked
  active; closed.

## 4. Invariants that must not regress

- Safe defaults; no serving outside root; `eggserve-static` sole path/FS
  authority; `eggserve-server` single H1 runtime; QUIC only behind `http3`.
- H1 + canonical `primitives` supported; remainder experimental.
- Legacy flat plans `000`–`292` + `release/` records immutable (cite, don't
  duplicate or rewrite).
- No `println!`/`eprintln!` in library code; docs-only diff touches no
  production source.

## 5. Scope

### In scope

- SKILL.md 0.4.0-line paragraph + 286 version correction (server `0.3.0`,
  not `0.3.1`; `0.3.1` arrived with Plan 291).
- `README.md`, `docs/` (interop, release-contract, tls, migration-guide),
  `architecture/` (overview, eggserve-core, structured-logging,
  eggserve-bin, docs-architecture roadmap) version/status corrections above.
- Registry + planning-governance roadmap status updates + closure record.

### Explicitly out of scope

- Any production-code edit (findings become follow-up plans).
- `docs/` normative-contract rewrites; `plans/000`–`003` canonical edits.
- Tier promotions, new features, dependency changes, `non-goals.md` crossing.
- Mass archive migration; new roadmap sections beyond the 302 entries.

## 6. Required production changes

None (docs-only). Code wins on any code/doc contradiction found mid-sweep.

## 7. Ordered work packages

### Work package A — Skill + agent guide (main agent)

SKILL.md 286-version fix + 293–302 paragraph; `AGENTS.md` already corrected
this session (versions + `fast` scope). No new agent-facing conventions to
add: the sweep corrects versions, it does not change architecture.

### Work package B — Docs/README/architecture corrections (main agent)

Apply the §3 file:line corrections verbatim; each new version string proven
by `crates/*/Cargo.toml` before edit.

### Work package C — Plan trace + commit

Registry update (next-number `302`, 302 row), roadmap status, closure
record, verification gates, commit + push to `main`.

## 8. Failure semantics

Docs-only: no runtime failure modes. Any item that cannot be manifest-proven
stays untouched and is recorded as an open finding.

## 9. Compatibility and migration

No compatibility effect. Version strings move to manifests-proven 0.4.0
values (`server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`,
`bin 0.2.2`, wheel `0.2.4`, `eggnet-tls` workspace `0.2.4`).

## 10. Required tests

None (docs-only).

## 11. Required verification commands

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
```

## 12. Documentation updates

- `.opencode/skills/eggserve-dev/SKILL.md` (0.4.0-line currency).
- `AGENTS.md` (already corrected this session).
- `README.md` + `docs/*.md` (version-string sync only).
- `architecture/*.md` (status/version corrections only).
- `plans/subsystems/planning-governance-roadmap.md` (302 milestone).
- `plans/registry.md` (next-number 302 + 302 row).
- `plans/closure/planning-governance/302-agent-skill-docs-staleness-sweep-040.md`
  (closure record).

## 13. Acceptance criteria

- Every §3 item corrected with manifest/registry evidence; nothing else
  touched in those files.
- All touched-file internal links resolve; no legacy plan file changed.
- Topology + conformance-matrix gates pass; `cargo fmt --check` passes.
- Registry + roadmap + closure record complete; commit pushed to `main`.

## 14. Stop conditions

Stop and report rather than improvise when ownership changes, canonical
invariants are contradicted, scope expands into code changes, or manifest
evidence is unavailable.

## 15. Closure evidence required

Requirement-to-evidence matrix (each §3 item → manifest/registry line);
exact verification commands with outcomes; roadmap disposition.

## 16. Handoff notes

Work only in `/home/sugarwookie/projects/eggserve`. Staleness audits for
skill/docs/architecture were completed by subagents this session; apply
their verified findings, do not re-audit.
