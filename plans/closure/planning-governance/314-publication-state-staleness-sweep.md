# Planning Governance Milestone 314 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/planning-governance/314-publication-state-staleness-sweep.md`

Source subsystem roadmap:

- `plans/subsystems/planning-governance-roadmap.md#milestone-314--publication-state-staleness-sweep`

Repository baseline reviewed: `465b80cdabf90801e6ef929563b12886c509c201`

Implementation commits or pull requests:

- The sweep was applied on top of the corrective-line publication commit
  `326aa1e714cdc2e19a0be8eda408d3d1af2c8d02` (tag `v0.5.0`), because the
  sweep's wording asserts a published state and the corrective line
  (Milestone 305) had to be live before the surfaces could truthfully name
  it.

## 1. Executive finding

Every published-state claim across the agent-facing, architecture, and
release surfaces now matches reality. Six surfaces described the 0.4.0
publication as **outstanding** and told agents to advise users to pin
versions that were two patch lines behind what was live. They now name the
published corrective line and record that the 0.4.0 artifact predates the
post-tag fix batch.

No source code, manifest, dependency, API, or tier changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Correct the six stale surfaces | §3.1 | pass |
| Replace the skill's "until then the registry-qualified leaves remain `server 0.3.1` …" instruction and remove the "once Plan 300 publishes" conditional | `.opencode/skills/eggserve-dev/SKILL.md:161-172` | pass |
| Tighten `docs/release-process.md` so it states the publication is complete | `docs/release-process.md:107-126` | pass |
| Record that the published 0.4.0 artifact predates the post-tag corrective batch | `docs/release-process.md:119-126`, `SKILL.md`, `CHANGELOG.md` `## 0.4.0 — published 2026-09-25 (superseded by 0.5.0)` | pass |
| Ensure every version string naming a *current* artifact matches the live registry | §3.2 | pass |
| Update `plans/registry.md`'s next-number line if the highest milestone changed | The next-number line is unchanged at `316` — the sweep registers no new milestone | pass (no change needed) |
| Legacy flat plans and `release/plan-*.md` remain immutable | `git diff --name-only` touches no file under `plans/0*.md`–`plans/2*.md` or `release/` | pass |
| Accepted ADRs are never rewritten | No ADR file touched | pass |
| Historical version statements are preserved verbatim | §3.3 | pass |
| The registry stays the compact control surface; no registry content is duplicated into the architecture documents | The sweep states the published versions and points at the closure records; it does not restate registry rows | pass |
| No source code, manifest, dependency, API, or tier change | `git diff --name-only` — only `.opencode/`, `architecture/`, `docs/`, and `plans/` | pass |
| `plans/000-002` canonical long-term documents are not edited | Untouched | pass |
| The agent-facing guidance introduces no new hard-coded "next plan" number | The sweep names milestone IDs that already exist | pass |
| The sweep must not claim a publication that has not happened | The sweep was applied **after** the six crates were published, the tag was pushed, and the GitHub Release was created (§3.4) | pass |
| No `CHANGELOG.md` 0.4.0 entry written by this milestone | `CHANGELOG.md` was written by Plan 305, which owns it | pass — the sweep made no changelog change |

## 3. Production implementation evidence

### 3.1 The six surfaces

| Surface | Before | After |
|---|---|---|
| `.opencode/skills/eggserve-dev/SKILL.md:161-172` | "300 (active) publishes the 0.4.0 line … Report the direct registry path as available at the 0.4.0-line versions **once Plan 300 publishes**; **until then** the registry-qualified leaves remain `server 0.3.1` + `static`/`h3`/`core 0.3.0` + `primitives 0.2.1` + `bin 0.2.1`" | "300 published the 0.4.0 line …; 304 closed that milestone conditionally; 305 published the corrective `0.5.0` line (`server`/`static`/`h3`/`core 0.5.0`, `primitives 0.2.3`, `bin 0.2.3`, wheel `0.2.5`), which carries the post-0.4.0 fix batch … Report the direct registry path as available at the 0.5.0-line versions (`server 0.5.0` + `primitives 0.2.3`); the compatibility path is `eggserve-core 0.5.0`, the CLI is `eggserve-bin 0.2.3`, and the wheel is `eggserve 0.2.5`." |
| `architecture/crate-topology.md:149` | "Plan 300 **(active)** moves the line to `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`" | "Plan 300 moved the line to `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`, and Plan 305 published the corrective line `server`/`static`/`h3`/`core 0.5.0`, `primitives 0.2.3`, `bin 0.2.3`, wheel `0.2.5`" |
| `architecture/overview.md:65` | "Plan 300 **(active)** publishes the 0.4.0 line" | "Plan 300 published the 0.4.0 line and Plan 305 published the corrective `0.5.0` line that carries the post-0.4.0 fix batch" |
| `architecture/eggserve-core.md:19` | "carried forward on the `0.4.0` line via Plan 300 (**active publication**: …). The crate is now at `0.4.0`." | "carried forward on the `0.4.0` line via Plan 300 and then on the corrective `0.5.0` line via Plan 305 (…). The crate is now at `0.5.0`." |
| `architecture/eggserve-bin.md:164` | "the `0.4.0` publication is Plan 300, **active**" | "the `0.4.0` publication is Plan 300 and the corrective `0.5.0` publication is Plan 305" |

A residual sweep for the stale markers across `.opencode/`, `architecture/`,
`docs/`, and `README.md` returns nothing:
`grep -rn "Plan 300 (active)\|300 (active)\|once Plan 300 publishes\|until then the registry-qualified"` → no matches.

### 3.2 Current-artifact version strings

Plan 314 §5 also requires that every version string naming a *current*
artifact match the live registry. Four further strings named the
pre-correction versions and were swept:

| Surface | Before | After |
|---|---|---|
| `architecture/crate-topology.md:167` | "The current line is the `eggserve-server`/`static`/`h3`/`core 0.4.0` / `primitives 0.2.2` / `bin 0.2.2` / wheel `0.2.4` line (Plan 300; …)" | "The current line is the `eggserve-server`/`static`/`h3`/`core 0.5.0` / `primitives 0.2.3` / `bin 0.2.3` / wheel `0.2.5` line (Plan 305; Plan 300 published the superseded `0.4.0` line …)" |
| `architecture/overview.md:200` (`eggserve-core` row) | "Compatibility/composition umbrella (`0.4.0`; … 0.4.0 via Plan 300)" | "(`0.5.0`; … 0.4.0 via Plan 300, 0.5.0 via Plan 305)", plus a Plan 308 sentence for the ownership values the compatibility config now carries |
| `architecture/overview.md:201` (`eggserve-bin` row) | "Static-only CLI binary (`0.2.2`; requirement-only bump via Plan 300, no behavior change)" | "(`0.2.3`; requirement-only bump via Plan 305, no behavior change)" |
| `architecture/overview.md:202` (`eggserve-python` row) | "wheel (`0.2.4`; Python distribution, Rust side tracks `0.4.0` leaves)" | "wheel (`0.2.5`; Python distribution, Rust side tracks `0.5.0` leaves)" |

`docs/release-process.md:107-126` now states the publication as complete and
names the superseded 0.4.0 line explicitly, including the instruction that a
consumer on `eggserve-core = "0.4"` must widen the requirement to `"0.5"`
because the caret range does not cross the minor boundary.

### 3.3 Historical statements preserved

The following were deliberately left verbatim: the `0.3.1`/`0.3.0`/`0.2.1`
version references that describe what Plans 286 and 291 published at the
time; the Plan 226 `0.2.0` transition note; the `core-only 0.2.2` Tower
repair reference; and the `release/plan-286-…` and
`release/plan-291-…` closure pointers. They are history, not current-state
claims.

### 3.4 Ordering constraint

The sweep asserts a published state. It was therefore applied only after
the corrective line was live:

- six crates published to crates.io (2026-10-22T14:49:50Z … 14:51:29Z),
- tag `v0.5.0` pushed at `326aa1e714cdc2e19a0be8eda408d3d1af2c8d02`,
- GitHub Release "0.5.0 release line" published 2026-10-22T14:52:41Z.

Applying it earlier would have written "Plan 305 published …" before anything
was published — the precise failure mode the plan forbids.

## 4. Verification executed

| Check | Result |
|---|---|
| `grep -rn "Plan 300 (active)\|300 (active)\|once Plan 300 publishes\|until then the registry-qualified" .opencode/skills/eggserve-dev/SKILL.md architecture/ docs/ README.md CHANGELOG.md` | no matches |
| `git diff --name-only` over the sweep | only `.opencode/skills/eggserve-dev/SKILL.md`, `architecture/crate-topology.md`, `architecture/overview.md`, `architecture/eggserve-core.md`, `architecture/eggserve-bin.md`, `docs/release-process.md` |
| `git diff --name-only` over `crates/`, `Cargo.toml`, `Cargo.lock`, `release/`, `plans/0*.md`–`plans/2*.md` | empty — no code, manifest, dependency, legacy-plan, or `release/` change |
| Live registry read-back of every version the sweep names | `eggserve-primitives 0.2.3`, `eggserve-server`/`eggserve-static`/`eggserve-h3`/`eggserve-core 0.5.0`, `eggserve-bin 0.2.3` — all present, none yanked |
| `plans/registry.md` next-number line | unchanged at `316`; the sweep registers no new milestone |

## 5. Invariant review

| Invariant | Evidence | Status |
|---|---|---|
| Legacy flat plans and `release/plan-*.md` remain immutable | No such file in the diff | holds |
| Accepted ADRs are never rewritten | No ADR file in the diff | holds |
| Historical version statements are preserved verbatim | §3.3 | holds |
| The registry stays the compact control surface | The sweep states published versions and points at closure records; it does not restate registry rows | holds |
| No source code, manifest, dependency, API, or tier change | The diff touches only prose surfaces | holds |
| `plans/000-002` are not edited | Untouched | holds |
| No new hard-coded "next plan" number is introduced | Only existing milestone IDs are named | holds |
| The sweep does not claim a publication that has not happened | §3.4 | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** The sweep is a prose edit; it can
  be re-applied idempotently, and each anchor was required to match exactly
  once so a second application cannot double-apply.
- **Cancellation races / restart / shutdown.** Not applicable.
- **Partial failure and resource release.** Not applicable — no I/O.
- **Malformed or unauthorized input.** The anchors are exact multi-line
  strings; a drifted anchor makes the edit *fail loudly* rather than
  half-apply (four of the five anchors in the first pass applied and
  `architecture/overview.md` was reported as skipped, then completed with an
  exact anchor).
- **Bounded event and artifact behavior.** Six surfaces, each a bounded
  prose replacement.

## 7. Migration and compatibility review

- **Configuration behavior.** Unchanged.
- **Backward compatibility.** Unchanged — no code moved.
- **Protocol negotiation.** Unchanged.
- **Rollback limitations.** A prose revert is trivial; the sweep carries no
  behavioral change.
- **Semver decision.** **No version impact.** Documentation only.
- **Registry-only consumer proof.** Not applicable to this milestone; the
  corrective line's proof is recorded in
  `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`.
- **Legacy-path status.** Unchanged.

## 8. Security review

- **Confinement enforcement.** Unchanged.
- **Safe-default preservation.** Unchanged.
- **Path validation.** Unchanged.
- **Privilege boundaries.** Unchanged.
- **Denial-of-service bounds.** Unchanged.
- **Redaction and audit.** Unchanged.
- **Agent-facing accuracy.** The sweep removes an instruction that told
  agents to advise users to pin `eggserve-server 0.3.1`,
  `eggserve-static`/`eggserve-h3`/`eggserve-core 0.3.0`,
  `eggserve-primitives 0.2.1`, and `eggserve-bin 0.2.1` — versions that
  predate the live line by a full minor and that do not carry the B1–B89
  fixes. The corrected guidance names the live versions and states that the
  0.4.0 artifact predates the fix batch, so a consumer is not sent to a
  version line that is both stale and defective.

## 9. Documentation and operations

- **Updated:** `.opencode/skills/eggserve-dev/SKILL.md`,
  `architecture/crate-topology.md`, `architecture/overview.md`,
  `architecture/eggserve-core.md`, `architecture/eggserve-bin.md`,
  `docs/release-process.md`.
- **Not touched:** `CHANGELOG.md` (Plan 305 owns it), `plans/registry.md`
  rows (Plan 304 owns them), the legacy flat plans, and `release/`.
- **Operator diagnostics.** None added.
- **Recovery instructions.** Revert the sweep commit; it changes no
  behavior.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The TestPyPI trusted publisher is still unconfigured, so the release path can only publish straight to PyPI | No staging-package verification precedes the production PyPI upload | Configure the TestPyPI trusted publisher; then use `publish_target=testpypi` before `publish_target=pypi` on the next line |
| low | `architecture/overview.md` and `architecture/eggserve-core.md` still contain 0.3.x / 0.2.x version strings | They are **historical** statements about what Plans 286/291/300 published at the time, not current-state claims | None — preserving them is an explicit invariant |

No critical, high, or medium finding is open.

## 11. Roadmap disposition

**Milestone closed and the next dependency may proceed.**

The agent-facing, architecture, and release surfaces now describe reality:
the corrective line is published, the superseded 0.4.0 line is named as
superseded and defective, and no surface instructs a consumer or an agent to
use a version line that is two patch lines behind what is live.

## 12. Registry updates

`plans/registry.md`:

- The Planning governance row for Milestone 314 moves `ready` → `closed`,
  with this closure record linked.

`plans/subsystems/planning-governance-roadmap.md`:

- Milestone 314 moves `ready` → `closed` in §12, with this closure record.
- The next-number line is unchanged at `316`.
