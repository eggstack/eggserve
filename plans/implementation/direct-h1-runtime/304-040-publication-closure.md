# Direct H1 Runtime Milestone 304 — 0.4.0 publication closure and registry-state correction

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-10--040-publication-closure`

Original milestone (corrected by this plan):

- `plans/implementation/direct-h1-runtime/300-release-0-4-0-publication.md`
- `plans/registry.md` (Plan 300 row, currently `active`)

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#25-closure-records`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable prior publication precedent:

- `release/plan-286-embedding-contract-publication-closure.md`
- `release/plan-291-direct-h1-boundary-ownership-publication-closure.md`
- `docs/release-process.md`

Primary class: polish

## 1. Objective

Close Plan 300 honestly against the publication that actually happened, and
correct the planning control surface that still reports it as outstanding.

Plan 300 remains `active` in `plans/registry.md:49` and its subsystem
milestone remains `active` in
`plans/subsystems/direct-h1-runtime-roadmap.md:240`, with no closure
record, even though the 0.4.0 line is published on crates.io and PyPI and
the `v0.4.0` tag and GitHub Release exist.

This milestone gathers the missing publication evidence, writes the Plan
300 closure record, and records the one condition that Plan 300 cannot
close on its own: **the published 0.4.0 artifact predates every
post-tag production fix**, so the milestone closes *conditionally* and
hands the corrective republication to Milestone 305.

## 2. Why this milestone is ready

Plan 300's operational prerequisites are satisfied: the crates.io token was
present, the `pypi` environment approval was granted at dispatch time, and
the publication actually completed. There is nothing left to *execute* for
Plan 300 — only evidence to *gather* and state to *reconcile*.

Hard dependencies (294–297, 298, 299) are closed and are not reopened.

## 3. Current implementation evidence

Verified at the baseline, 2026-10-22:

**Published artifacts (live registry state).**

| Artifact | Version present | Published (UTC) |
|---|---|---|
| `eggserve-primitives` (crates.io) | 0.2.2 | 2026-09-25 |
| `eggserve-server` (crates.io) | 0.4.0 | 2026-09-25 |
| `eggserve-static` (crates.io) | 0.4.0 | 2026-09-25 |
| `eggserve-h3` (crates.io) | 0.4.0 | 2026-09-25 |
| `eggserve-core` (crates.io) | 0.4.0 | 2026-09-25 |
| `eggserve-bin` (crates.io) | 0.2.2 | 2026-09-25 |
| `eggserve` (PyPI) | 0.2.4 | — |
| `eggnet-tls` (crates.io) | 0.2.0 | 2026-09-22 |

`eggserve-core 0.4.0` resolves `eggserve-server ^0.4.0`,
`eggserve-static ^0.4.0`, `eggserve-h3 ^0.4.0`,
`eggserve-primitives ^0.2.2`, `eggnet-tls ^0.2.0` — the §6 requirement set
of Plan 300.

**Tag and release.**

- `git tag v0.4.0` → `d537d4dd7e0102c9ffe6101caf7dc2fad8b7f494`
  ("Plan 300: bump to the 0.4.0 release line + wheel 0.2.4", 2026-09-25).
- GitHub Release "0.4.0 release line" tagged `v0.4.0`, published
  2026-09-25T19:28:57Z.

**Local tree.** Manifest versions match Plan 300 §6 exactly. The
`eggnet-tls` workspace version is `0.2.4` while the published crate is
`0.2.0`; Plan 300 §5 explicitly excluded an `eggnet-tls` publication and
recorded the drift as pre-existing. `eggnet-tls` source has not changed
since 2026-09-18, so the published 0.2.0 content matches the local tree.

**Control-surface state that is stale.**

- `plans/registry.md:49` — Plan 300 `active`;
  `plans/registry.md:55` — "Plan 300 is active (operational needs:
  crates.io token present, `pypi` environment approval by maintainer at
  dispatch time)".
- `plans/subsystems/direct-h1-runtime-roadmap.md:240` — Milestone 9
  `active`, blockers "crates.io token + `pypi` approval (operational)".
- `plans/registry.md` has no row for Plan 300's closure record and no
  `plans/closure/direct-h1-runtime/300-*.md` exists.
- `.opencode/skills/eggserve-dev/SKILL.md:163-169` states "300 (active)
  publishes the 0.4.0 line" and "until then the registry-qualified leaves
  remain `server 0.3.1` + `static`/`h3`/`core 0.3.0` + `primitives 0.2.1` +
  `bin 0.2.1`". The second sentence is now false: those exact versions are
  live, and `0.4.x` is the current line.
- `architecture/crate-topology.md:149` states "Plan 300 (active) moves the
  line to `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin
  0.2.2`, wheel `0.2.4`" — the line has moved.
- `CHANGELOG.md` contains **no `0.4.0` entry at all**; its newest heading
  is `## 0.3.1 — published (Plans 288–291)`.

**The condition that Plan 300 cannot close on its own.**

The `v0.4.0` tag points at `d537d4d`, and **eight commits landed after
that tag**, five of which change production code:

| Commit | Subject | Production scope |
|---|---|---|
| `cb68c02` | Fix bugs.md findings: IoError 500, conditional combine, try_into_duplex, trailer mapping | `eggserve-h3`, `eggserve-python`, `eggserve-server`, `eggserve-static` |
| `c00e1b4` | Fix B1-B9 bugs from bugs.md interrogation | `eggserve-python`, `eggserve-server`, `eggserve-static` |
| `1f10632` | Fix B10-B43 bugs from bugs.md interrogation | server + core + static + Python |
| `ef51fa8` | Fix B44-B89 bugs from bugs.md interrogation | server + core + static + Python |
| `715281b` | Fix CI: restore queued Python callback permits + collision-proof test temp files | `eggserve-python`, `eggserve-server` |

`git diff --stat v0.4.0..HEAD -- crates/` shows **53 files changed, 1,646
insertions, 522 deletions**. The published 0.4.0 crates therefore do not
contain any of B1–B89 nor the CI fix. Representative published-artifact
defects, by the commits' own classification:

- B1 — `If-Match *` succeeds when the file exists even without an ETag
  (RFC 9110 §13.1.1).
- B14 — an unsatisfiable `Range` combined with a failed `If-Range` serves
  200 full instead of the RFC 9110 §13.1.5 behavior.
- B18 — header combining used last-wins for `Range`/`If-Range`/dates instead
  of first-wins (comma-join is correct only for `If-Match`/`If-None-Match`).
- B21 — `Http1Upgrade` accepted a bare `101` without a protocol.
- B31 — the Tower adapter did not require the authority to agree with a
  mutated `Host`.
- B35 — child resolution did not inherit the confined backslash policy.
- B36 — fd-relative resolution did not re-apply platform component checks.
- B39 — `PrematureEof`/`LengthMismatch` did not map to `IncompleteError`.
- B48 — a buffer-export failure truncated the stream silently.
- B51 — shutdown drain was not bounded by `graceful_shutdown_timeout`.
- B58 — a listener/address pair could be mispaired and panic.
- B64/B71 — `version_to_http` / `from_state` could panic or silently
  downgrade instead of failing closed.

## 4. Invariants that must not regress

- Published crate versions are immutable: nothing already published is
  re-uploaded, yanked, or altered by this milestone.
- The `RUSTSEC-2026-0285` `rustls` `0.23.45` caret floor stays in every
  constraining manifest, including the excluded Python crate.
- `deny.toml` wildcard and `native-tls`/`openssl-sys` bans stay green for
  both lockfiles.
- Legacy flat plans and `release/` records stay immutable; the Plan 300
  closure lives in `plans/closure/`.
- A closure record may not be written to imply the published artifact
  contains post-tag fixes. The record must state exactly what the published
  artifact does and does not contain.
- No runtime, API, dependency, or tier change lands in this milestone.
- The manual publication workflow stays manual: no push, tag, or merge
  publishes anything.

## 5. Scope

### In scope

- Gather Plan 300 §15 evidence that is not already recorded: per-crate
  crates.io SHA-256 checksums and publication timestamps, the exact-SHA
  hosted CI run ID and conclusion, the registry-only consumer manifests and
  ancestry output, and the wheel workflow run IDs (TestPyPI and PyPI) plus
  the aggregate manifest.
- Write `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`
  with status **conditionally closed** and the post-tag fix batch named as
  the explicit open condition.
- Reconcile the control surface: `plans/registry.md` (Plan 300 row,
  "Blocked work" section, "Closure work and current control points" table,
  next-number line), and
  `plans/subsystems/direct-h1-runtime-roadmap.md` (Milestone 9 status and
  the Milestone 10 row).
- Record the `eggnet-tls` 0.2.0-vs-0.2.4 version observation as a known
  limitation, consistent with Plan 300 §5.
- Hand the corrective republication to Milestone 305 (Plan 305) with the
  exact commit range `v0.4.0..465b80c` as its input.

### Explicitly out of scope

- Publishing anything. This milestone records and corrects; it does not
  upload.
- Fixing, reverting, or cherry-picking the post-tag commits.
- The doc/skill/architecture wording sweep (that is Plan 314).
- The correctness/consolidation work in Plans 306–309 and Milestone 311.
- Editing the immutable Plan 300 implementation plan. Material deviations
  are recorded in the closure record, never by rewriting the plan.
- Changing the semver line or cutting a new version (Plan 305's decision).

## 6. Required production changes

### Crates and ownership

None. This milestone produces planning and documentation artifacts only.

### Config and policy

None.

### Protocol and compatibility

None. The published artifact's behavior is whatever it is; the closure
record states it.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and confinement

The closure record must enumerate the published-artifact defects with
security or confinement relevance (B31, B35, B36, B48, B58 and any others
the implementing agent classifies) so that downstream consumers of the
published 0.4.0 crates can judge their exposure before Milestone 305
supersedes it.

### Documentation and static guards

- `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`
  (new).
- `plans/registry.md` (updated).
- `plans/subsystems/direct-h1-runtime-roadmap.md` (updated).
- `CHANGELOG.md` — a `0.4.0` entry is **not** written here; the changelog
  entry must describe the artifact that users can actually consume, which
  is the subject of Plan 305. Write the entry there.

## 7. Ordered work packages

### Work package A — Collect the missing publication evidence

Gather, and record verbatim in the closure record:

1. Per-crate crates.io publication timestamp and SHA-256 checksum for
   `eggserve-primitives 0.2.2`, `eggserve-server 0.4.0`,
   `eggserve-static 0.4.0`, `eggserve-h3 0.4.0`, `eggserve-core 0.4.0`,
   `eggserve-bin 0.2.2`.
2. The exact-SHA hosted CI run ID and conclusion for the version-bump commit
   `d537d4d`, for the rust, supply-chain, and python jobs.
3. Registry-only consumer evidence per the 286/291 precedent: fresh `/tmp`
   manifests pinned to the exact published versions (direct H1, direct H1
   with TLS, Tower/Axum, core compatibility), locked builds, focused test
   runs, and `cargo tree -e no-dev` ancestry showing no
   `eggserve-core`/`eggserve-static`/`phf` on the direct graph.
4. Wheel workflow run IDs for `publish_target=testpypi` and
   `publish_target=pypi`, the aggregate wheel manifest, and the post-publish
   smoke result for `eggserve 0.2.4`.

If a hosted CI run ID, a consumer proof, or a workflow run ID cannot be
recovered from the environment, record it as **not run / not recoverable**
with the reason. Do not infer it. A closure record may state
"conditionally closed" with named outstanding evidence; it may not state
"closed" against evidence that was never collected.

Acceptance evidence: every Plan 300 §15 bullet is either present in the
closure record or explicitly marked outstanding with a reason.

### Work package B — Write the Plan 300 closure record

Create `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`
using the `plans/closure/README.md` template, with:

- Status: **conditionally closed**.
- Requirement-to-evidence matrix over Plan 300 §13's acceptance criteria,
  each marked pass / partial / not run with a pointer to the evidence.
- A dedicated subsection listing the post-tag fix batch: the commit range
  `v0.4.0..465b80c`, the 53-file / 1,646-insertion / 522-deletion diff
  summary, the five production-fix commits, and the published-artifact
  defects the implementing agent classifies as user-visible or
  security-relevant.
- The semver rationale for the published line restated from Plan 300 §6.
- A "Roadmap disposition" of *conditionally closed with a named corrective
  publication outstanding* and an explicit pointer to Plan 305.
- A "Registry updates" section listing the exact edits this milestone makes
  to `plans/registry.md` and the source roadmap.

Acceptance evidence: the closure record states the published artifact's
actual contents and does not claim the post-tag fixes are shipped.

### Work package C — Reconcile the control surface

Edit `plans/registry.md`:

- Plan 300 row: `active` → `closed (conditionally)`, with the closure
  record link and the open condition named.
- "Blocked work": replace the "Plan 300 is active (operational needs:
  crates.io token present, `pypi` environment approval)" sentence with the
  truthfully blocked item — the corrective republication (Plan 305).
- "Closure work and current control points": add the Plan 300 closure row.
- Add rows for the milestones this correction registers (304–315).
- Update the "New milestone numbers continue from" line to the number after
  the highest registered milestone.

Edit `plans/subsystems/direct-h1-runtime-roadmap.md`:

- Milestone 9 status: `active` → `closed (conditionally)`, blockers
  updated.
- Add Milestone 10 (this plan) and the subsequent milestones with their
  plans.
- Update §4 "Current state" so it no longer describes Milestone 9 as the
  active work, and instead records that the 0.4.0 line is published and
  that the corrective republication is outstanding.

Acceptance evidence: no line in `plans/registry.md` or the direct-H1
roadmap asserts that Plan 300 is awaiting publication.

### Work package D — Handoff to the corrective republication

Record in the closure record and in the Plan 305 implementation plan:

- The exact corrective input: `v0.4.0..465b80c`.
- The list of published-artifact defects that Milestone 305 must ship.
- The semver question Plan 305 must answer, including the behavior-changing
  fixes that may warrant a minor bump rather than a patch.

## 8. Failure, cancellation, restart, and contention semantics

- Not applicable: no runtime code changes.
- If the publication evidence cannot be recovered, the closure record
  states *conditionally closed* with the exact outstanding evidence and the
  milestone does not claim full closure. Evidence loss must not be
  papered over with an inferred run ID or checksum.
- If a published version turns out to differ from Plan 300 §6 (for example
  a version was published with different contents than the tag), stop and
  report: that is a publication-integrity incident, not a documentation
  task.

## 9. Compatibility and migration

No consumer-facing change. Downstream consumers of the published 0.4.0
crates are affected only by the *disclosure* that the artifact predates
B1–B89; that disclosure is the point of the conditional closure and must be
stated precisely so consumers can decide whether to wait for Milestone 305.

## 10. Required tests

### Focused unit tests

None; no production code changes.

### Integration tests

None; no production code changes.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

Not applicable to the milestone itself. The closure record must classify
the published-artifact defects by security relevance.

### Migration and compatibility tests

- Confirm the published dependency graph resolves as Plan 300 §6 specifies
  (read-only registry inspection).
- Confirm no published version was altered or yanked by this milestone.

## 11. Required verification commands

```bash
# Live registry state (read-only)
curl -sL https://crates.io/api/v1/crates/eggserve-core
curl -sL https://crates.io/api/v1/crates/eggserve-server
curl -sL https://crates.io/api/v1/crates/eggserve-static
curl -sL https://crates.io/api/v1/crates/eggserve-h3
curl -sL https://crates.io/api/v1/crates/eggserve-primitives
curl -sL https://crates.io/api/v1/crates/eggserve-bin
curl -sL https://pypi.org/pypi/eggserve/json

# Local consistency
git rev-list -n1 v0.4.0
git log --oneline v0.4.0..HEAD
git diff --stat v0.4.0..HEAD -- crates/
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
```

The registry reads are informational. The authoritative evidence is the
recorded run ID, checksum, and consumer proof from Work package A.

## 12. Documentation updates

- `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` (new).
- `plans/registry.md` (updated).
- `plans/subsystems/direct-h1-runtime-roadmap.md` (updated).
- No `docs/` or `architecture/` edit in this milestone (Plan 314 owns the
  wording sweep).
- No `CHANGELOG.md` edit in this milestone (Plan 305 owns it).

## 13. Acceptance criteria

- `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` exists
  and carries status `conditionally closed`.
- The closure record states the published versions, timestamps, and
  checksums for all six crates and the wheel, or marks each as outstanding
  with a reason.
- The closure record names the post-tag fix batch (`v0.4.0..465b80c`) and
  the published-artifact defects, and states plainly that the published
  artifact does not contain B1–B89.
- `plans/registry.md` no longer reports Plan 300 as active or awaiting a
  crates.io token / `pypi` approval, and links the closure record.
- `plans/subsystems/direct-h1-runtime-roadmap.md` Milestone 9 is no longer
  `active`.
- No published artifact is altered, re-uploaded, or yanked.
- No production code, manifest, dependency, or API change lands.

## 14. Stop conditions

Stop and report rather than improvise if:

- a published version's contents or checksum cannot be reconciled with the
  `v0.4.0` tag;
- a published crate was yanked or its metadata was altered by a third
  party;
- the hosted CI run, consumer proof, or wheel run cannot be recovered and
  the milestone is being asked to record it as verified;
- closing Plan 300 would require rewriting the immutable Plan 300 plan or
  any legacy `release/` record;
- the reconciliation would require editing `plans/000-002` long-term
  documents.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/304-040-publication-closure.md`
containing:

> **Recorded deviation.** The closure record named here was never created as a
> separate file. Plan 304's closure evidence was recorded inside
> `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`, which
> carries both milestones' status; the registry's Plan 300 and Plan 304 rows
> both point at it. This milestone's condition — that the published 0.4.0
> artifact predated the post-tag fix batch — was discharged by Milestone 305,
> which republished the corrected 0.5.0 line on 2026-10-22. The required
> content is therefore split as follows: the Plan 300 closure record and its
> conditional status are in `300-release-0-4-0-publication.md`; the
> condition's discharge is recorded there and in the registry's Plan 300,
> Plan 304, and 0.4.0-line publication rows.

- the Plan 300 closure record and its conditional status;
- the evidence inventory from Work package A, including every item marked
  outstanding;
- the diff-stat and commit-range evidence for the post-tag fix batch;
- the control-surface diff (registry + roadmap) with the exact edits;
- confirmation that no published artifact and no production file changed;
- the explicit handoff to Plan 305 with the corrective input range and the
  semver question it must answer;
- residual findings and the roadmap disposition for Milestone 10.

## 16. Handoff notes

The publication already happened; this milestone is bookkeeping with a
correctness obligation — the record must not overstate. The single most
important output is the disclosure that the published 0.4.0 artifact
predates B1–B89. Do not soften it, and do not attempt to fix the
published artifact from here.
