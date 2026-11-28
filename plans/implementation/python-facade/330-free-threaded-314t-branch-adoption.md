# Python Facade Milestone 330 — Adopt the free-threaded CPython 3.14t family (Beta)

Status: ready for handoff

Repository baseline: `origin/main` `5fdd7da801056e00a8b20bb885aaf9641f3a3c3c`
(worktree `feat/auto-20261128-fb4f0bec`). Adjacent unmerged branch
`feat/auto-20261116-7cc8b1dc`, tip `6e3003f` ("Plan 327: promote the
free-threaded family to Stable"), merge base `4949a77`.

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md#7` (milestones)

Long-term requirements:

- `plans/000-long-term-specification.md#2` (Python `http.server`-shaped
  facade and handler-only `lowlevel` substrate)
- `plans/001-terminology-and-domain-model.md` (`Request`, `Response`,
  `RequestBody`, `Service`)
- `plans/002-long-term-roadmap.md` (phase 4)

Applicable ADRs: none (no accepted ADR governs the Python bridge or the wheel
support tiers).

Primary class: capability (a new wheel family) with invariant obligations.

## 1. Objective

Land the six-commit adjacent free-threaded branch on `main` as a **Beta**
wheel family: one additive `cp314-cp314t` family for CPython 3.14t on Linux
x86_64 (glibc), macOS arm64, and Windows x86_64; an explicit
`#[pymodule(gil_used = false)]` declaration; a free-threaded CI lane; the
free-threaded concurrency tests; family-aware wheel-matrix and release
validators; and documentation that states Beta, not Stable.

The user-facing tier is deliberately **Beta** at the end of this milestone.
The branch tip flips the family to Stable; this milestone keeps everything
else from that commit and reverses only the promotion, because the
qualification evidence required for Stable is not complete (Section 3.4) and
is owned by Milestone 332.

## 2. Why this milestone is ready

- The adjacent branch is code-complete and self-tested: 8 free-threaded tests,
  a `python-free-threaded` CI job, a compile check, family-aware validators
  with self-tests, and three `-ft` matrix entries.
- `git merge-tree --write-tree origin/main 6e3003f` produces a tree with
  conflicts only in `CHANGELOG.md` and `plans/registry.md`; every source,
  workflow, script, test, and doc file auto-merges.
- The current `main` state is strictly worse than the branch: the extension
  already advertises `Py_MOD_GIL_NOT_USED` and runs GIL-free from a source
  build, while the docs claim free-threaded is unsupported and Plan 267's
  rationale for that claim is inverted (Section 3.1).
- Plan 267 Track A is a closed DEFERRED gate; it does not block this slice,
  and the branch supersedes its free-threaded conclusion.

## 3. Current implementation evidence

### 3.1 Baseline audit (2026-11-28, this worktree)

- `crates/eggserve-python/Cargo.toml:45` declares `pyo3 = "0.29.2"`; the
  committed lock resolves **0.29.3**. PyO3 0.28.0 made free-threading
  **opt-out** (`pyo3-0.29.2/CHANGELOG.md`: "Support for free-threaded Python
  is now opt-out rather than opt-in"), so the bare `#[pymodule]` at
  `crates/eggserve-python/src/lib.rs:1670` emits `Py_MOD_GIL_NOT_USED`
  (`pyo3-macros-backend-0.29.3/src/module.rs:394`).
- A clean `git archive` of this commit built with CPython 3.14.5
  free-threading produced `eggserve-0.2.5-cp314-cp314t-macosx_11_0_arm64.whl`;
  `sys._is_gil_enabled()` stayed `False` before import, after
  `import eggserve._native`, and after `import eggserve`.
- Concurrent smoke (`lowlevel.Server`, `max_python_callbacks=8`, 8 client
  threads x 5 requests, buffered and streamed): 40/40 correct, maximum
  concurrent callbacks 8, 0.134 s.
- The full Python suite on 3.14t: 856 tests, `OK`, 123.4 s (stderr carried
  ResourceWarnings, one timeout traceback, and a `RuntimeError: server is
  closed` from a `serve_forever` thread; unittest counted none as failures).
- The `cp311-abi3` wheel built from the same tree is rejected by 3.14t pip
  ("not a supported wheel on this platform"), so released wheels are not
  installable on free-threaded interpreters; only source builds are exposed.
- `release/plan-267-wheel-feasibility.md:30` claims the module "loads
  GIL-required by default"; that is false for the locked PyO3 line.

### 3.2 What the branch adds

Six commits (`ab418f8`..`6e3003f`), `40 files changed, 4530 insertions`:

- `crates/eggserve-python/src/lib.rs`: explicit
  `#[pymodule(gil_used = false)]` with a doc comment.
- `crates/eggserve-python/src/server/tunnel_bridge.rs`: `recv_in_flight`
  CAS so concurrent `Tunnel.recv` raises a typed contention error instead of
  a spurious "already closed".
- `crates/eggserve-python/tests/test_free_threaded.py` (353 lines, 8 tests),
  skipped on GIL builds.
- `scripts/check-python-free-threaded-compile.sh`: asserts
  `Py_GIL_DISABLED` and runs `cargo check --locked` against the `-t`
  interpreter.
- `scripts/soak-python-free-threaded.py`: bounded contention soak
  (`--requests 400 --workers 8 --duration 120`, JSON record, `qualified`).
- `release/wheel-matrix.toml`: three `required` `-ft` entries
  (`cp314-cp314t`) for linux-x86_64-glibc, macos-arm64, windows-x86_64.
- Family-aware `scripts/wheel-matrix.py`, `check-release-wheel-set.py`,
  `check-release-workflow.py`, `check-python-release-metadata.py`
  (including `EXPECTED_FREE_THREADING_TIER`).
- `.github/workflows/ci.yml`: `python-free-threaded` job on
  `setup-python 3.14t`; `.github/workflows/release.yml`: interpreter-driven
  build and a GIL-off assertion in the `-ft` smoke.
- Docs and `plans/` records: toolchain-support, python-packaging,
  release-contract/process, library-capability-matrix, python-api,
  python-http-server-compatibility, SECURITY, CHANGELOG, skill, roadmap,
  Plans 326/327 and their closure records.

### 3.3 Conflict and renumbering map

- Textual conflicts: `CHANGELOG.md`, `plans/registry.md`. Both sides
  append; keep main's entries and add the free-threaded entry in the
  unreleased section.
- Plan-number collision: the branch uses **326** and **327**, which `main`
  already assigns to the public-bind acknowledgment milestone and the TLS
  substrate corrective. The registry's next free number is **330**.
  Renumber the branch's implementation and closure records to **333** and
  **334** during merge resolution, and leave main's 326/327 untouched.

### 3.4 Evidence not yet sufficient for Stable

- `plans/closure/python-facade/327-soak-1.json` (and 2/3) record the
  interpreter build, platform, arm64, and `qualified: true`, but **no commit
  SHA, no wheel filename/tag, and no host identifier**; all three runs are on
  one local macOS arm64 interpreter, two minutes apart.
- The release aggregate proves build + smoke on three targets; the full suite
  runs on Linux only. No contention soak exists for Linux or Windows.
- The branch's own Plan 327 says "wheel filenames are the authority" and "no
  partial promotion", so its own standard is not met by the records as
  written.

Milestone 332 owns that evidence and the Beta→Stable decision. This milestone
must not anticipate it.

## 4. Invariants that must not regress

- The GIL `cp311-abi3` family is unchanged: same 10 required targets, same
  wheel tags, same `requires-python >= 3.11`, same test results.
- The free-threaded family is additive: no `abi3t`, no CPython 3.13t/3.15t,
  no ARMv7, no Windows arm64, no musl free-threaded build.
- `#[pyclass(frozen)]` and the one-shot/single-consumer contracts stay as
  documented; no public Rust crate API changes.
- No HTTP, TLS, confinement, or runtime default changes.
- The user-facing tier statement never exceeds the recorded evidence; Beta
  is the ceiling for this milestone.
- Plan numbers stay unique; main's 326/327 remain the public-bind and TLS
  milestones.
- No publication: the release workflow is exercised with
  `publish_target: none` only.

## 5. Scope

### In scope

- Rebase/merge the adjacent branch onto current `main`.
- Renumber the branch's plan and closure records (326/327 -> 333/334) and
  update every cross-reference.
- Keep the explicit GIL declaration, CI lane, tests, validators, matrix
  entries, tunnel `recv` guard, and docs.
- Set the tier to **Beta** in the classifier, metadata gate, and every doc
  the branch touched.
- Add one executable guard that fails if the declaration and the recorded
  tier drift (Section 7, work package D).
- Re-run the gates and merge.

### Explicitly out of scope

- Beta->Stable promotion (Milestone 332).
- Concurrency-contract fixes beyond what the branch carries (Milestone 331).
- Publication to PyPI/crates.io or a release tag.
- New platforms, `abi3t`, 3.13t/3.15t, or any GIL-family change.
- Rewriting Plan 267; only a short superseded pointer is added (work package
  E). The archived record keeps its original text.

## 6. Required production changes

### Crates and ownership

No crate graph changes. `crates/eggserve-python` stays excluded from the
workspace with its own lockfile. `scripts/check-crate-topology.py` must stay
green.

### Config and policy

None. No default, limit, or policy value changes.

### Protocol and compatibility

The wheel family is additive. Python compatibility remains HTTP/1.1-only.
The `cp311-abi3` compatibility declaration is unchanged.

### Runtime and concurrency

Take the branch's tunnel `recv` CAS guard. No other runtime change.

### Frontend or operator surface (CLI / Python)

The `-ft` family is installable for 3.14t users and documented as Beta.
The GIL surfaces are unchanged.

### Security and confinement

No change. The three-target scope is a support boundary, not a security
relaxation.

### Documentation and static guards

Every doc the branch touched must say Beta; `docs/toolchain-support.md` is
the tier authority. Add the declaration/tier guard.

## 7. Ordered work packages

### Work package A — Prepare the merge branch

Intent: land the branch on current `main` with a reviewable history.

Required changes:

- Create an integration branch from `origin/main`.
- Merge (not rebase) `6e3003f` so the branch's six commits and closure
  records keep their identity, or rebase with `--rebase-merges` if the merge
  proves unreadable; record the choice.
- Resolve `CHANGELOG.md` by keeping both sides and adding one free-threaded
  entry under the unreleased section.
- Resolve `plans/registry.md` by keeping main's 320-329 rows and adding the
  renumbered rows (work package B).

Acceptance evidence: `git log --oneline origin/main..HEAD` shows the merge;
`git diff --check` is clean; `git merge-tree` has no remaining conflict.

### Work package B — Renumber the branch records

Intent: remove the 326/327 collision without losing traceability.

Required changes:

- `git mv` the branch's implementation records to
  `plans/implementation/python-facade/333-free-threaded-cpython-314t-staged-support.md`
  and `.../334-free-threaded-314t-qualification-and-tier-decision.md`.
- `git mv` the closure records to
  `plans/closure/python-facade/333-...` and `.../334-...`.
- Update every heading, status line, cross-reference, registry row, roadmap
  entry, doc reference, and skill reference from 326/327 to 333/334.
- Rename the soak records' `plan` field if it names 327 (the JSON
  `plan` field is trace metadata, not a filename authority).
- Confirm no file or registry row remains at 326/327 for free-threading; the
  existing 326 (public bind) and 327 (TLS) records must be byte-identical to
  `origin/main`.

Acceptance evidence: `git grep -n "326-free-threaded\|327-free-threaded"`
returns nothing; `python3 scripts/check-crate-topology.py` passes; the
registry has exactly one row per number.

### Work package C — Beta tier

Intent: make the user-facing claim match the evidence.

Required changes:

- `crates/eggserve-python/pyproject.toml`: classifier
  `Programming Language :: Python :: Free Threading :: 2 - Beta`.
- `scripts/check-python-release-metadata.py`:
  `EXPECTED_FREE_THREADING_TIER = 2`.
- `docs/toolchain-support.md`: the 3.14t row and prose say **Beta**, three
  targets only, and name Milestone 332 as the promotion owner.
- `docs/python-packaging.md`, `docs/release-contract.md`,
  `docs/release-process.md`, `docs/library-capability-matrix.md`,
  `docs/python-api.md`, `docs/python-http-server-compatibility.md`,
  `SECURITY.md`, `CHANGELOG.md`, and
  `.opencode/skills/eggserve-dev/SKILL.md`: same Beta statement.
- Keep `release/wheel-matrix.toml` `tier = "required"` for the three `-ft`
  entries. Matrix `tier` means "must appear in every release aggregate"; it
  is not the user support tier. State that distinction in
  `docs/release-process.md` so the two vocabularies cannot be confused.

Acceptance evidence: `git grep -n "Free Threading :: 3\|Stable"` over the
touched docs returns only unrelated matches; the metadata gate passes with the
Beta constant; the classifier in the built wheel is Beta.

### Work package D — Declaration guard

Intent: make the GIL declaration and the documented tier impossible to drift
silently.

Required changes:

- Keep `#[pymodule(gil_used = false)]` explicit (do not rely on the PyO3
  default).
- `crates/eggserve-python/tests/test_free_threaded.py` (or a dedicated
  test): on a free-threaded interpreter, assert
  `sys._is_gil_enabled() is False` after `import eggserve._native`, and
  fail if the interpreter is `-t` and the GIL was re-enabled.
- `scripts/check-python-free-threaded-compile.sh` stays the compile-time
  half (`Py_GIL_DISABLED` asserted).
- Extend `scripts/check-python-release-metadata.py` so the declared tier
  constant must match the classifier and, when a built artifact is available,
  the WHEEL tag family (`cp314-cp314t` only for the Beta family).
- Add a short note to `docs/toolchain-support.md` that the declaration is
  explicit and guarded.

Acceptance evidence: the guard fails when the attribute is removed or the
classifier is moved to Stable, proven by a local mutation; CI runs the test in
the `python-free-threaded` lane.

### Work package E — Record the supersession

Intent: leave the planning surface truthful without rewriting history.

Required changes:

- Add a one-line "Superseded for the free-threaded slice by Milestone 333"
  pointer to `release/plan-267-wheel-feasibility.md` **only if** the file's
  archival status permits a pointer; otherwise record the supersession in
  `plans/subsystems/python-facade-roadmap.md` and the new closure records and
  leave the archived record untouched.
- Do not edit `plans/000`-`003`.

Acceptance evidence: the roadmap or closure record states which Plan 267
conclusion is superseded; the archived record is either untouched or carries
only the pointer.

### Work package F — Gates, merge, and control surface

Intent: prove the integration and close it honestly.

Required changes:

- Run every command in Section 11 and record the exact outcomes.
- Merge to `main` (or open the PR for the owner to merge, per repository
  practice).
- Update `plans/registry.md` (rows 330/331/332, current milestone, next
  number) and `plans/subsystems/python-facade-roadmap.md` (status, graph,
  milestone table) in the same change.

Acceptance evidence: gate log with exact commands and results; merge SHA;
registry next number advanced to 335 after 330 consumes 333/334.

## 8. Failure, cancellation, restart, and contention semantics

- A conflict outside `CHANGELOG.md` and `plans/registry.md` means the
  branch base has drifted further than observed; stop and rebase-resolve file
  by file rather than guessing.
- If the `python-free-threaded` CI job cannot be provisioned (`3.14t`
  unavailable), the milestone is **blocked**; a GIL-interpreter substitute is
  not acceptable evidence.
- If a gate fails, fix the integration or stop; never merge with a known red
  gate and never mark the closure closed.
- No publication is attempted; `publish_target: none` only.

## 9. Compatibility and migration

- Additive wheel family; GIL users see no change.
- 3.14t users move from an unqualified source build to a documented Beta
  wheel.
- `requires-python` stays `>=3.11`; the family is selected by interpreter
  tag, not by metadata.
- Removal criterion: if Milestone 332 cannot produce the evidence, demote the
  family to `candidate` in the matrix and record the decision; do not leave a
  Beta claim without an owner.

## 10. Required tests

### Focused unit tests

- The branch's 8 free-threaded tests, unchanged except numbering/comments.
- The declaration guard (work package D).

### Integration tests

- Full Python suite on 3.14t (`PYTHON=<3.14t> bash scripts/test-python-wheel.sh`).
- Full Python suite on the GIL lane, unchanged, with the free-threaded tests
  skipped.

### Restart and recovery tests

- The branch's stop/start phase in the contention tests; no new restart
  surface.

### Contention and cancellation tests

- The branch's concurrent callbacks, stream producers, body-read race,
  iterator race, and tunnel contention tests.
- The bounded soak is **not** required here; it belongs to Milestone 332.

### Security and negative tests

- Validator rejection of a GIL wheel in the `-ft` slot and vice versa, via
  the existing self-tests.
- No new negative security surface.

### Migration and compatibility tests

- GIL wheel bytes/tags unchanged across the 10 required targets.
- Metadata gate passes for both families.

## 11. Required verification commands

```bash
# 1. Integration hygiene
git diff --check
git log --oneline origin/main..HEAD

# 2. Static gates
python3 scripts/check-crate-topology.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
python3 scripts/check-python-release-metadata.py

# 3. Rust + excluded Python crate
cargo fmt --all -- --check
cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked

# 4. GIL lane (unchanged contract)
PYTHON=python3.14 bash scripts/test-python-wheel.sh

# 5. Free-threaded lane (Beta family)
PYTHON=python3.14t bash scripts/check-python-free-threaded-compile.sh
PYTHON=python3.14t bash scripts/test-python-wheel.sh

# 6. Repository fast gate
./scripts/verify.sh fast
```

Do not claim a command that was not actually run in the closure record.

## 12. Documentation updates

- `docs/toolchain-support.md` (tier authority; Beta, three targets only,
  ownership by Milestone 332).
- `docs/python-packaging.md`, `docs/release-contract.md`,
  `docs/release-process.md` (matrix `tier` vs support tier),
  `docs/library-capability-matrix.md`, `docs/python-api.md`,
  `docs/python-http-server-compatibility.md`.
- `SECURITY.md`, `CHANGELOG.md`, `README.md` (only if the branch already
  touched them; do not broaden).
- `.opencode/skills/eggserve-dev/SKILL.md` and `AGENTS.md` only where the
  branch already states a tier.
- `plans/registry.md` and `plans/subsystems/python-facade-roadmap.md`.

## 13. Acceptance criteria

1. `main` contains the branch's free-threaded code, tests, CI lane, matrix
   entries, validators, and docs, with no plan-number collision.
2. The wheel classifier and every touched doc say **Beta**; no touched
   surface says Stable.
3. The GIL `cp311-abi3` family is byte-for-byte unchanged in tags, targets,
   metadata, and tests.
4. The declaration guard fails if `gil_used = false` is removed or the tier
   is moved to Stable without the evidence plan.
5. The 3.14t suite and the GIL suite pass; the static gates and self-tests
   pass.
6. The closure records the deferred contention evidence and names Milestone
   332 as the owner.

## 14. Stop conditions

Stop and report rather than improvise when:

- a merge conflict reaches a file outside `CHANGELOG.md` and
  `plans/registry.md` and cannot be resolved without choosing between
  unrelated main work and branch work;
- a plan number other than 333/334 would be consumed;
- the `3.14t` interpreter or CI job is unavailable;
- the GIL family would change in any observable way;
- a repository gate fails for reasons outside this scope;
- the owner asks for Stable in this milestone (that is Milestone 332's
  decision, and it requires the evidence).

## 15. Closure evidence required

- Merge commit SHA and the exact commands run, with outcomes.
- The renumbering diff (`git grep` proving no 326/327 free-threaded
  references remain).
- The classifier/doc diff showing Beta.
- 3.14t suite result and GIL suite result.
- Validator and metadata self-test results.
- A statement that contention evidence is deferred to Milestone 332 and that
  the family is Beta, with the condition named.

## 16. Handoff notes

- **Nested worktree hazard:** building `crates/eggserve-python` from a
  worktree nested under the main checkout fails with "current package believes
  it's in a workspace when it's not" because Cargo walks past the worktree's
  excluded workspace to the outer checkout. Build from the primary checkout,
  or export with `git archive` and append an empty `[workspace]` table to a
  temporary copy. Do not add `[workspace]` to the repository manifest.
- **Local free-threaded interpreter:** `python3.14t` is available on this
  machine; maturin 1.9.5 builds a `cp314-cp314t` wheel with the dev profile.
  A dev-profile wheel is sufficient for GIL/tag/behavior evidence; release
  claims still require the dist profile.
- **Do not merge the branch's Stable commit as-is.** `6e3003f` is a
  documentation/classifier flip layered on the Beta decision in `a42edc0`;
  keep its text improvements and reverse only the tier.
- **Do not copy the branch's plan files into this tree before the merge**;
  that duplicates 326/327 and guarantees conflicts. Renumber during merge
  resolution.
- **Do not reopen Plan 331/332 scope here.** The BodySource lost-update and
  the per-target soak are separate milestones with separate evidence.
