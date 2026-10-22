# Planning Governance Milestone 314 — Publication-state staleness sweep across the agent, architecture, and release surfaces

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/planning-governance-roadmap.md#milestone-314--publication-state-staleness-sweep`

Original milestone and its closure (the subject of this sweep):

- `plans/implementation/direct-h1-runtime/300-release-0-4-0-publication.md`
- `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` (conditionally closed by Milestone 304)

Long-term requirements:

- `plans/003-planning-process.md#9-registry-requirements`
- `plans/003-planning-process.md#8-updating-subsystem-roadmaps`
- `plans/README.md` (planning hierarchy and legacy-trace rules)

Applicable prior governance precedent:

- `plans/implementation/planning-governance/302-agent-skill-docs-staleness-sweep-040.md`
- `plans/closure/planning-governance/302-agent-skill-docs-staleness-sweep-040.md`
- `plans/implementation/planning-governance/298-planning-control-surface-reconciliation.md`

Primary class: polish

## 1. Objective

Make every published-state claim across the repository match reality, and
stop the agent-facing surface from instructing agents to pin dependency
versions that are two patch lines behind what is live.

The 0.4.0 line shipped on 2026-09-25. Six surfaces still describe that
publication as outstanding:

| Surface | Line | Stale text |
|---|---|---|
| `.opencode/skills/eggserve-dev/SKILL.md` | 163 | "300 (active) publishes the 0.4.0 line (`server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`)" |
| `.opencode/skills/eggserve-dev/SKILL.md` | 166-169 | "Report the direct registry path as available at the 0.4.0-line versions **once Plan 300 publishes**; **until then** the registry-qualified leaves remain `server 0.3.1` + `static`/`h3`/`core 0.3.0` + `primitives 0.2.1` + `bin 0.2.1`" |
| `architecture/crate-topology.md` | 149 | "and Plan 300 **(active)** moves the line to `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`" |
| `architecture/overview.md` | 65 | "Plan 300 **(active)** publishes the 0.4.0 line" |
| `architecture/eggserve-core.md` | 19 | "carried forward on the `0.4.0` line via Plan 300 (**active publication**: …)" |
| `architecture/eggserve-bin.md` | 164 | "the `0.4.0` publication is Plan 300, **active**" |

The skill sentence is the most consequential: it is the surface every
implementation agent loads before touching this repository, and it
instructs them to advise users to pin `eggserve-server 0.3.1`,
`eggserve-static`/`eggserve-h3`/`eggserve-core 0.3.0`,
`eggserve-primitives 0.2.1`, and `eggserve-bin 0.2.1` — versions that
predate the published line by a full minor.

Additionally, `CHANGELOG.md` contains **no `0.4.0` entry at all**. Its
newest heading is `## 0.3.1 — published (Plans 288–291)`. A reader of the
changelog cannot learn that a 0.4.0 release exists.

This milestone sweeps those surfaces. It does **not** write the
`CHANGELOG.md` 0.4.0 entry — that belongs to the corrective publication
milestone (Plan 305), because the changelog must describe the artifact
users can actually consume, and the corrective line is still pending.

## 2. Why this milestone is ready

- The correct state is established: the 0.4.0 line is published, and
  Milestone 304 records the evidence.
- The sweep is text-only.
- Plan 302 is the direct precedent for a "staleness sweep" milestone in
  this subsystem; this one is narrower and follows the same shape.
- No hard dependency on other open milestones beyond Milestone 304
  (interface dependency: the closure record supplies the facts).

## 3. Current implementation evidence

### 3.1 The published reality (verified 2026-10-22)

| Artifact | Live version | Published |
|---|---|---|
| `eggserve-primitives` | 0.2.2 | 2026-09-25 |
| `eggserve-server` | 0.4.0 | 2026-09-25 |
| `eggserve-static` | 0.4.0 | 2026-09-25 |
| `eggserve-h3` | 0.4.0 | 2026-09-25 |
| `eggserve-core` | 0.4.0 | 2026-09-25 |
| `eggserve-bin` | 0.2.2 | 2026-09-25 |
| `eggserve` (PyPI) | 0.2.4 | — |

Tag `v0.4.0` → `d537d4d`; GitHub Release "0.4.0 release line" published
2026-09-25T19:28:57Z.

### 3.2 The stale skill text

`.opencode/skills/eggserve-dev/SKILL.md:163-169`:

```
wire (F3); 300 (active) publishes the 0.4.0 line (`server`/`static`/`h3`/
`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`); 302 sweeps
skill/agent-guide/docs/architecture version drift onto that line. Report the
direct registry path as available at the 0.4.0-line versions once Plan 300
publishes; until then the registry-qualified leaves remain `server 0.3.1` +
`static`/`h3`/`core 0.3.0` + `primitives 0.2.1` + `bin 0.2.1` (Plans
286/291).
```

The final clause is the actionable defect.

### 3.3 The changelog gap

`CHANGELOG.md` headings, newest first: `0.3.1`, `0.3.0`, `0.2.3`, `0.2.2`,
`0.2.1`. There is no `0.4.0` heading and no `0.4.0` string anywhere in the
file.

### 3.4 What is already correct (do not touch)

- `AGENTS.md:87` — "Per-crate versions differ (e.g. `server/static/h3/core
  0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`)" — accurate.
- `README.md:97` — `eggserve-core = "0.4"` — accurate and resolvable.
- `docs/release-process.md:111` — "Continue on the current `0.4.x` leaf line
  (`eggserve-bin 0.2.2`, wheel `0.2.4`; Plan 300)" — accurate about the
  line, though it does not say the publication is complete; this milestone
  may tighten the wording.
- `architecture/overview.md:190`, `:198`, `:199` — the `0.4.0` crate
  version references are accurate.
- `architecture/eggserve-primitives.md:16` and
  `architecture/eggserve-bin.md:8` — accurate version statements.

### 3.5 Historical references that are correct and must be preserved

Many `0.3.x` / `0.2.1` strings in the documentation are *history*, not
staleness — for example `architecture/crate-topology.md:41-42` ("publish the
additive direct-H1 ownership/range API as `eggserve-server 0.3.1`"),
`docs/release-process.md:100-102` (the published `0.3.0` set),
`architecture/runtime.md:913` (Plans 280–286 on the `0.3.0` line),
`architecture/eggserve-core.md:373` (the `0.3.0` embedding contract).

These must not be rewritten. They describe what was published when.

## 4. Invariants that must not regress

- Legacy flat plans (`000`–`292`) and `release/plan-*.md` records remain
  immutable historical trace. They are never edited to reflect the current
  state.
- Accepted ADRs are never rewritten to conceal history.
- Historical version statements that describe what was published at the
  time are preserved verbatim.
- The planning registry remains the compact control surface; the sweep does
  not duplicate registry content into the architecture documents.
- No source code, manifest, dependency, API, or tier change.
- The `plans/000-002` canonical long-term documents are not edited.
- The agent-facing guidance continues to avoid hard-coding a
  permanently-stale "next plan" number; the sweep must not introduce a new
  one.
- The sweep does not claim a publication that has not happened (the
  corrective line in Plan 305 is still pending).

## 5. Scope

### In scope

- Correct the six stale surfaces listed in §1 to state that the 0.4.0
  line is published, with a pointer to the closure record.
- Replace the skill's "until then the registry-qualified leaves remain
  `server 0.3.1` + …" instruction with the published versions, and remove
  the "once Plan 300 publishes" conditional.
- Tighten `docs/release-process.md:111` so it states the publication is
  complete.
- Record in the closure record (and, if appropriate, in the skill and
  `architecture/overview.md`) that the **published 0.4.0 artifact predates
  the post-tag corrective batch**, so the agent-facing guidance does not
  lead a consumer to believe the live 0.4.0 line carries the B1–B89 fixes.
  The wording must be accurate: the 0.4.0 line is published, and a
  corrective line is pending.
- Ensure every version string that names a *current* artifact matches the
  live registry.
- Update `plans/registry.md`'s next-number line if this sweep changes the
  highest registered milestone.

### Explicitly out of scope

- **No `CHANGELOG.md` entry for 0.4.0.** Plan 305 owns the changelog, and
  it must describe the artifact users can actually consume, which includes
  the corrective line. Writing a 0.4.0 entry here would produce a
  changelog that describes a superseded artifact without saying so.
- No rewriting of the registry rows that Plan 304 owns.
- No rewriting of historical version statements (§3.5).
- No new architecture content beyond the publication-state correction.
- No change to the plans in this corrective series.
- Publication.

## 6. Required production changes

### Crates and ownership

No crate changes. The affected files are:

- `.opencode/skills/eggserve-dev/SKILL.md`
- `architecture/crate-topology.md`
- `architecture/overview.md`
- `architecture/eggserve-core.md`
- `architecture/eggserve-bin.md`
- `docs/release-process.md`
- `plans/registry.md` (next-number line only, if needed)

### Config and policy

None.

### Protocol and compatibility

None.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and confinement

None.

### Documentation and static guards

The sweep is itself the documentation change. It must be precise rather
than merely positive: the corrected text must not imply that the published
0.4.0 artifacts contain the post-tag corrective fixes.

## 7. Ordered work packages

### Work package A — Build the stale-claim inventory

Enumerate every sentence across `docs/`, `architecture/`, `.opencode/`,
`AGENTS.md`, and `README.md` that names a **current** artifact version or
describes the 0.4.0 publication's status. Classify each as:

- **current-state claim** — must be corrected (this is the work);
- **historical claim** — describes what was published at the time; leave
  alone;
- **correct** — no change.

Record the inventory in the closure record.

Acceptance evidence: the inventory table, with the six known stale entries
plus any additional ones found.

### Work package B — Correct the agent-facing skill

Rewrite `.opencode/skills/eggserve-dev/SKILL.md:163-169` so that:

- the 0.4.0 line is described as published, with the closure record
  pointer;
- the "until then the registry-qualified leaves remain `server 0.3.1` …"
  instruction is replaced by the published versions;
- the skill does **not** tell an agent to pin the older versions;
- the skill states that the published 0.4.0 artifact predates the
  post-tag corrective batch and that a corrective line is pending under
  Plan 305, so an agent advising a consumer knows the live line has known
  gaps.

Acceptance evidence: the skill no longer contains "until then", "once
Plan 300 publishes", or "300 (active)".

### Work package C — Correct the architecture surfaces

Update `architecture/crate-topology.md:149`, `architecture/overview.md:65`,
`architecture/eggserve-core.md:19`, and `architecture/eggserve-bin.md:164`
so each states that the 0.4.0 line is published and points to the closure
record.

Do not alter the surrounding historical sentences.

Acceptance evidence: no architecture document describes the 0.4.0
publication as active or pending.

### Work package D — Tighten the release-process wording

Update `docs/release-process.md:111` so the leaf line is described as
published rather than as the target of a pending publication.

Acceptance evidence: the sentence reads as a statement about the current
line.

### Work package E — Consistency check

Re-run the inventory over the corrected tree and confirm:

- no surface describes the 0.4.0 publication as pending;
- no surface instructs a consumer to pin a pre-0.4.0 version as current;
- every *current* version string matches the live registry;
- no historical sentence was altered.

Acceptance evidence: the re-run inventory shows zero stale current-state
claims and zero altered historical claims.

### Work package F — Registry next-number

Update `plans/registry.md`'s next-number line if the sweep changes the
highest registered milestone number.

Acceptance evidence: the next-number line matches the highest registered
milestone plus one.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable; documentation-only.

## 9. Compatibility and migration

No consumer-visible change, no semver event, and no published artifact
change.

The agent-facing effect is material and intentional: agents will stop
advising pre-0.4.0 pins. That is the point of the milestone.

## 10. Required tests

### Focused unit tests

None; documentation-only.

### Integration tests

None.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- The corrected skill must not direct a consumer to a version that lacks
  the corrective fixes without saying so.
- No historical statement about the `0.3.x` line may be altered.

### Migration and compatibility tests

- Every version string that the sweep leaves in place resolves against the
  live registry (checked by inspection against the crates.io and PyPI
  listings).

## 11. Required verification commands

```bash
# The surfaces
grep -rn "until then\|once Plan 300 publishes\|300 (active)\|Plan 300, active" \
  .opencode/skills/ architecture/ docs/ AGENTS.md README.md

# Live registry confirmation
curl -sL https://crates.io/api/v1/crates/eggserve-core
curl -sL https://crates.io/api/v1/crates/eggserve-server
curl -sL https://crates.io/api/v1/crates/eggserve-bin
curl -sL https://pypi.org/pypi/eggserve/json

# Unaffected-workspace guards
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
git diff --check
```

## 12. Documentation updates

- `.opencode/skills/eggserve-dev/SKILL.md`
- `architecture/crate-topology.md`
- `architecture/overview.md`
- `architecture/eggserve-core.md`
- `architecture/eggserve-bin.md`
- `docs/release-process.md`
- `plans/subsystems/planning-governance-roadmap.md` — Milestone 314
  status.
- `plans/registry.md` — the milestone row and the next-number line.
- `plans/closure/planning-governance/314-publication-state-staleness-sweep.md`
  (new).

## 13. Acceptance criteria

- No surface in `.opencode/skills/`, `architecture/`, `docs/`, `AGENTS.md`,
  or `README.md` describes the 0.4.0 publication as active, pending, or
  conditional.
- The skill no longer instructs an agent to pin `server 0.3.1`,
  `static`/`h3`/`core 0.3.0`, `primitives 0.2.1`, or `bin 0.2.1` as the
  current versions.
- Every current-state version string matches the live registry.
- No historical statement about the `0.3.x` line was altered.
- The agent-facing guidance states that the published 0.4.0 artifact
  predates the post-tag corrective batch, and names Plan 305 as the
  pending corrective publication.
- `CHANGELOG.md` is unchanged; Plan 305 owns it.
- No source, manifest, dependency, API, or tier change.

## 14. Stop conditions

Stop and report rather than improvise if:

- correcting a surface would require editing an immutable legacy plan, a
  `release/` record, or an accepted ADR — those are not surfaces this
  milestone may touch;
- the correct wording would require asserting that the corrective line is
  published — it is not; describe it as pending;
- the sweep would require editing `plans/000-002`;
- a version string in a historical sentence turns out to be ambiguous
  between "published at the time" and "current" — leave it and record the
  ambiguity rather than guessing.

## 15. Closure evidence required

Create `plans/closure/planning-governance/314-publication-state-staleness-sweep.md`
containing:

- the Work package A inventory table with its classification;
- the diff of every corrected surface;
- the Work package E re-run inventory, showing zero stale current-state
  claims;
- the registry live-state confirmation;
- confirmation that no historical sentence and no immutable record was
  altered (`git diff --stat` over `plans/0*`, `plans/2*`, `plans/29*`, and
  `release/` shows no change);
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

Two traps. First, the sweep must not turn into a general documentation
refresh — Plan 302 already did the 0.4.0-line sweep, and this milestone
only corrects the *publication status*. Second, the corrected text must
stay honest in both directions: the 0.4.0 line **is** published, and it
**does** predate the corrective batch. Saying only the first half would
send consumers to an artifact with known gaps.
