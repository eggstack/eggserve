# Python Facade Milestone 330 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/python-facade/330-free-threaded-314t-branch-adoption.md`

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md` (Milestone 330)

Executed 2026-11-28 in worktree `feat/auto-20261128-fb4f0bec`. Baseline
`origin/main` `5fdd7da`; merge commit `87bbb522de6477462279febf94181dd0506cf883`.

## 1. Outcome

The adjacent free-threaded branch `feat/auto-20261116-7cc8b1dc` (tip
`6e3003f`, merge base `4949a77`) is merged onto `main` as an additive
`cp314-cp314t` wheel family for CPython 3.14t on Linux x86_64 (glibc), macOS
arm64, and Windows x86_64, with an explicit `#[pymodule(gil_used = false)]`
declaration, a free-threaded CI lane, the concurrency tests, family-aware
wheel-matrix and release validators, the tunnel `recv` contention guard, and
corrected documentation. The user-facing tier is **Beta**.

Merge size: 42 files, 4564 insertions, 238 deletions.

## 2. Commits

| Commit | Content |
|---|---|
| `e30427d` | Plans 330-332, registry and roadmap records (the merge precondition) |
| `87bbb52` | Merge of `6e3003f`: three conflicts resolved, records renumbered to 333/334, tier set to Beta |

The six branch commits (`ab418f8`..`6e3003f`) keep their identity in the merge
history.

## 3. Conflicts and resolution

`git merge-tree --write-tree origin/main 6e3003f`, run before the plan-set
commit, reported conflicts only in `CHANGELOG.md` and `plans/registry.md`.
At merge time `plans/subsystems/python-facade-roadmap.md` also conflicted,
because the plan-set commit edited the same roadmap the branch edits. That is
local, not branch drift: the branch base did not move.

- `CHANGELOG.md`: kept main's section header ("Plans 320-329", widened to
  320-334) and the branch's free-threaded entry; the entry's tier text became
  Beta.
- `plans/registry.md`: kept main's 320-329 rows; the branch's Python facade
  row and its 326/327 implementation rows were renumbered and the duplicate
  facade row dropped in favour of one merged row.
- `plans/subsystems/python-facade-roadmap.md`: kept the 330-332 milestones the
  plan-set commit added and re-added the branch's milestone sections as
  333/334 with amendment blocks.

No conflict reached product source; every source, workflow, script, test, and
other doc file auto-merged.

## 4. Renumbering

`main` already assigns 326 to the public-bind acknowledgment milestone and 327
to the TLS substrate corrective, so the branch records were renumbered
326 -> 333 and 327 -> 334:

- implementation records, closure records, and the three soak records
  (`334-soak-{1,2,3}.json`, including the JSON `plan` field);
- every cross-reference in `docs/`, `scripts/`, `.github/workflows/`,
  `crates/eggserve-python/`, `release/wheel-matrix.toml`,
  `release/plan-267-wheel-feasibility.md`, and
  `.opencode/skills/eggserve-dev/SKILL.md`;
- the 333/334 records carry an amendment block naming the original number.

Proof: `git grep -nE '32(6|7)-free-threaded' -- .` returns nothing. The
existing 326 (public bind) and 327 (TLS) records are untouched.

## 5. Tier

Beta, and the branch's Stable promotion is reversed.

- `crates/eggserve-python/pyproject.toml`:
  `Programming Language :: Python :: Free Threading :: 2 - Beta`.
- `scripts/check-python-release-metadata.py`:
  `EXPECTED_FREE_THREADING_TIER = 2`.
- `docs/toolchain-support.md` (tier authority), `docs/python-packaging.md`,
  `docs/release-contract.md`, `docs/release-process.md`,
  `docs/library-capability-matrix.md`, `docs/python-api.md`,
  `docs/python-http-server-compatibility.md`, `SECURITY.md`, `CHANGELOG.md`,
  and `.opencode/skills/eggserve-dev/SKILL.md` all state Beta.

Reason: the branch's three soak records name no commit, wheel filename, or
host, and all three were taken on one macOS arm64 host, so the Stable claim
is not reproducible from the record and does not meet the branch's own "wheel
filenames are the authority" standard. `docs/release-process.md` now states
that matrix `tier = "required"` is a release-aggregation property, not the
user support tier.

## 6. Declaration guard

- Runtime half (already on the branch, kept):
  `crates/eggserve-python/tests/test_free_threaded.py` runs a subprocess with
  `-X gil=0 -W error::RuntimeWarning`, imports `eggserve._native`, and asserts
  `sys._is_gil_enabled() is False`. If `gil_used = false` is removed, PyO3
  declares GIL-required and that assertion fails.
- Compile half: `scripts/check-python-free-threaded-compile.sh` asserts
  `Py_GIL_DISABLED` and runs `cargo check --locked`.
- Tier half: the release metadata gate requires exactly
  `Free Threading :: 2 - Beta` while the `cp314t` family is required.
- Mutation proof: flipping the classifier to `3 - Stable` made
  `check-python-release-metadata.py` exit 1 with "the free-threading
  classifier must be exactly ... 2 - Beta"; restoring it passed.

## 7. Gate log

Run on 2026-11-28 unless noted. "Export" means the committed tree was
extracted with `git archive` to `/tmp/eggserve-330-src` and an empty
`[workspace]` table was appended to the excluded crate's manifest (see
Section 8); the table is not committed.

| Command | Result |
|---|---|
| `git diff --check` | clean |
| `python3 scripts/check-crate-topology.py` | pass |
| `python3 scripts/wheel-matrix.py validate` | pass |
| `python3 scripts/wheel-matrix.py self-test` | pass |
| `python3 scripts/check-release-wheel-set.py --self-test` | pass |
| `python3 scripts/check-release-workflow.py` | pass |
| `python3 scripts/check-release-workflow.py --self-test` | pass |
| `python3 scripts/check-python-release-metadata.py` | pass (tier 2) |
| metadata gate with classifier mutated to Stable | fails as designed; restored |
| `cargo fmt --all -- --check` | pass |
| `PYTHON=<3.14t> bash scripts/check-python-free-threaded-compile.sh` (export) | pass; `Py_GIL_DISABLED` asserted |
| `cargo clippy --lib -- -D warnings` in `crates/eggserve-python` (export) | pass |
| `PYTHON=<3.14t> bash scripts/test-python-wheel.sh` (export) | **864 tests, OK**, 123.9 s |
| `PYTHON=<3.14> bash scripts/test-python-wheel.sh` (export) | **864 tests, OK (skipped=8)**, 122.3 s |

The GIL lane skips exactly the eight free-threaded tests; the two lanes are
otherwise identical, which is the intended contract.

## 8. Deviations

1. **Nested worktree.** `cargo check/clippy --manifest-path
   crates/eggserve-python/Cargo.toml` cannot run from this worktree: Cargo
   resolves the outer checkout and reports "current package believes it's in a
   workspace when it's not". The Python-crate gates and both wheel suites ran
   from a `git archive` export of `87bbb52` with an empty `[workspace]` table
   appended to a temporary copy. The repository manifest is unchanged.
2. **`./scripts/verify.sh fast` was not run end-to-end.** It invokes the same
   `--manifest-path crates/eggserve-python/Cargo.toml` checks and fails for the
   nested-worktree reason above. Its constituent gates were run individually
   (rows above). Workspace-member clippy and tests were not run, because no
   workspace crate is touched by this merge:
   `git diff --name-only origin/main..HEAD -- crates | grep -v
   crates/eggserve-python` is empty.
3. **maturin provisioning.** `scripts/test-python-wheel.sh` requires
   `maturin` importable by the target interpreter. The shipped 3.14t
   interpreter has none, and the standalone binary is 1.9.5, so
   `maturin==1.14.1` was installed into a throwaway venv used as `PYTHON`. The
   GIL lane used the already-installed 1.9.5 module, which the script accepts.
4. **Wheel-tag half of the guard.** The plan asked the metadata gate to check
   the WHEEL tag family "when a built artifact is available".
   `scripts/check-release-wheel-set.py` already enforces family-to-tag-triple
   agreement for release sets, so the check was not duplicated in the metadata
   gate; the gate owns the tier constant and classifier only.
5. **Third conflict.** See Section 3.
6. **`--ours` resolution gap, caught and repaired.** Taking the pre-merge
   version of `plans/subsystems/python-facade-roadmap.md` also discarded the
   branch's edits that sat outside the conflict hunks: the §8 free-threading
   concurrency semantics, the §9 free-threaded CI-lane paragraph, and the
   three free-threading bullets in §10. A branch-added-line comparison
   (`git diff --unified=0 origin/main 6e3003f`, then each added line against
   the merged file, with the renumbering applied) found them, and a follow-up
   commit restores them verbatim. The registry had no such loss: its only
   absent branch lines are the superseded next-number line, the superseded
   duplicate facade row, the branch's CLI row that omits main's 326, and the
   326/327 rows that now exist as 333/334.

## 9. Deferred evidence

The per-target contention evidence and every Beta-to-Stable decision belong to
**Milestone 332**. Milestone 331 owns the concurrency-contract fixes the audit
found. This milestone asserts Beta only.

## 10. Not done

No publication, no version bump, no tag, no push. The branch's Stable commit
text survives in history; only the tier was reversed on the merged tree.
