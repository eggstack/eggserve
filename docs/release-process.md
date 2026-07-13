# Release Process

This is an operator reference for cutting an eggserve release. It describes the
human workflow, evidence model, and failure handling. Gate definitions live in
[`release/criteria.toml`](../release/criteria.toml) — that file is the single
source of truth. This document explains how to use it.

## Evidence philosophy

A workflow job existing in YAML is not evidence. A green local command is not
evidence of a cross-platform CI run. Every release gate is satisfied only by
concrete, recorded evidence tied to one evaluated commit SHA. Evidence expires
when the candidate SHA changes, when invalidating files change, or when the
producing workflow itself changes.

The operator must not mark a gate complete based on an assumption, a prior run,
or a workflow definition. Every gate entry in the checklist must point to a
run ID, artifact digest, or command transcript that a third party could
independently locate and verify.

## Evidence classes

Every gate declares one or more acceptable evidence classes. The classes are
closed and machine-validated against `release/criteria.toml`.

### LOCAL

Command executed on a specific host with recorded tool versions, OS, exit code,
and timestamps. Local evidence is useful during development but must not satisfy
gates that require cross-platform CI or GitHub execution.

Record: command, exit status, stdout/stderr summary or artifact reference, tool
versions, OS/arch, commit SHA, dirty-tree state, timestamps, environment flags.

### GITHUB

A GitHub Actions workflow run with recorded repository, workflow path, run ID
and URL, job name, commit SHA, event type, conclusion, timestamps, runner
OS/arch, and artifact IDs. Static workflow YAML is not GITHUB evidence.

Record: repository, workflow name/path, run ID/URL, job name/ID, commit SHA,
event type, conclusion, timestamps, runner OS/arch, artifact IDs/digests.

### ARTIFACT

A built, downloadable artifact with recorded filename, platform, package
version, SHA-256 digest, size, producing workflow/run, provenance reference,
independent install/smoke result, and contents inventory where relevant.

### HUMAN

A recorded approval decision with approver identity, date/time, exact commit
SHA, evidence-bundle digest, release version, accepted limitations, waivers,
and rationale.

### CONFIG

Documentation and static review only. CONFIG never satisfies an execution gate.
Tooling rejects any required gate whose only accepted evidence is CONFIG unless
the gate is explicitly a documentation/policy gate.

## Release operator sequence

The sequence below assumes the operator has a clean working tree and a candidate
commit. Steps are ordered; some require manual judgment.

### 1. Choose candidate version and exact SHA

Pick the version number and record the full commit SHA. All subsequent evidence
is tied to this SHA. If you push a fix, the SHA changes and all prior evidence
for that fix is invalidated.

### 2. Verify clean tree

```sh
git status --porcelain
```

Must be empty. Staged-but-uncommitted changes, untracked files in `dist/`, or
dirty lockfiles invalidate the candidate.

### 3. Run/inspect required CI

Trigger or locate the CI run for the candidate SHA. Every gate listed in
`release/criteria.toml` with `evidence_classes = ["GITHUB"]` must have a
corresponding run. Record the workflow URL and run ID.

Do not inspect only the summary badge. Open each job, verify the commit SHA
matches, check the runner OS, and confirm the tool versions printed in the log
match the pinned versions declared in the criteria or `scripts/install-cargo-tools.sh`.

### 4. Trigger manual dry run

Dispatch the release workflow with `dry_run=true` (the default). The dry run
must not publish to crates.io, PyPI, or create a GitHub release, and must not
require production registry tokens.

Record the workflow URL and run ID.

### 5. Download aggregate evidence and artifacts

Download the release artifact bundle from the dry-run workflow. Verify:
- All expected binary archives are present (Linux x86_64/aarch64, macOS arm64/x86_64, Windows x86_64).
- Python wheels are present for each platform.
- `checksums.sha256` covers every distributable artifact.
- `provenance.json` references the correct commit SHA and workflow run.
- No source-tree-only files or stale artifacts are included.
- Filenames and target triples are correct.

### 6. Verify evidence manifest against criteria

For each gate in `release/criteria.toml`:
1. Confirm the gate is required (or waived with documented rationale).
2. Confirm the evidence class matches the recorded evidence type.
3. Confirm the evidence references the candidate SHA exactly.
4. Confirm the evidence is not stale (within `max_age_days`, no invalidating file changes).
5. Confirm gate dependencies are satisfied (topological order).

### 7. Generate checklist from criteria + evidence

Use the criteria validator to generate the release checklist skeleton:

```sh
python scripts/release_criteria.py generate-checklist
```

Fill in the evidence references from steps 3–5. The generated checklist must be
checked in or recorded alongside the release artifacts.

### 8. Inspect checksums/provenance/artifact contents

Independently verify:
- `sha256sum` of each artifact matches `checksums.sha256`.
- `provenance.json` commit SHA matches the candidate.
- Binary archives contain the expected binary, README, and license.
- Wheel contents include the native extension and Python package files.
- No unintended files (tests, docs sources, build scripts) are in the artifacts.

### 9. Record human approval

Fill in the HUMAN evidence record: approver name, date, exact SHA,
evidence-bundle digest, release version, accepted limitations, and any waivers.
Approval is invalidated by any change to the candidate SHA or evidence bundle.

### 10. Enter protected publication path

Push the candidate as a tagged commit. The `publish` job in the release
workflow is entered only by the tagged push path with publication enabled and is
separately environment-gated. The manual release workflow defaults to
`dry_run=true`; publication never fires from a manual dispatch unless explicitly
overridden.

For crates.io: run `bash scripts/verify-cargo-packages.sh`. The binary check
uses a temporary local registry because the core crate must exist in the
registry before a real crates.io publish.

For PyPI: build the platform wheel after staging the matching CLI binary and run
`crates/eggserve-python/packaging-tests/run_all.sh`. `dist/` outputs must
remain untracked.

### 11. Run post-publication smoke tests

After publication, verify:
- `cargo install eggserve` succeeds.
- `pip install eggserve` succeeds on a clean environment.
- `eggserve --help` runs without missing-library errors.
- `python -m eggserve --help` works.
- A minimal static-file serve returns expected content.

## Failure handling

### Rerun vs invalidate

If a CI job fails due to a transient infrastructure issue (runner timeout,
network blip), rerunning the same job on the same SHA preserves evidence
validity. If the failure is deterministic (test failure, lint error), the gate
is not satisfied and must not be marked passed.

### Candidate SHA changed

Any push to the candidate branch changes the SHA. All prior evidence is
invalidated. The operator must re-verify every gate against the new SHA. Do not
carry forward evidence from a prior SHA even if the change is "trivial."

### Workflow changed

Changes to `.github/workflows/`, `scripts/install-cargo-tools.sh`, or
`release/criteria.toml` invalidate evidence produced by the affected workflow.
Re-run the affected gates.

### Artifact mismatch

If a downloaded artifact does not match its checksum, the artifact gate fails.
Do not publish with mismatched checksums. Re-run the artifact assembly.

### Stale fuzz evidence

Fuzz campaign evidence has a maximum age (`max_age_days`). If the evidence is
older than allowed, or if parser/security-critical files changed since the last
campaign, the fuzz gate is invalidated. Run a new campaign.

### Failed platform wheel

If a wheel fails on one platform, the cross-platform wheel gate is not
satisfied. Fix the platform-specific issue and re-run the full wheel matrix.
Do not publish with partial platform coverage.

### Partial publication

If crates.io publication succeeds but PyPI fails (or vice versa), do not
consider the release complete. Record the partial state, fix the failing leg,
and re-run publication from the same tagged SHA. Do not create a new tag for the
same version.

### Revoked approval

If a new issue is found after human approval, the approval is revoked. The
operator must re-run the affected gates, update the evidence record, and obtain
fresh approval before proceeding.

## Publication safety

The manual release workflow defaults to `dry_run=true`. A dry run must not
publish to crates.io or PyPI, create a GitHub release, or require production
registry tokens.

The protected `publish` job is entered only by the tagged push path with
publication enabled and is separately environment-gated. This means:

1. Manual dispatches never publish.
2. Publication requires a Git tag push.
3. Publication requires the environment gate to be approved.

This three-layer defense prevents accidental publication from a misconfigured
dispatch, a direct push, or an unreviewed tag.

## Known limitations

These limitations are accepted for the current release and must be disclosed in
release notes:

- **Windows reparse-point hardening is deferred.** Do not use Windows with
  untrusted mutable public content. Windows support is functional and
  parser-hardened only.
- **Follow-symlinks mode is weaker.** The `--follow-symlinks` flag uses
  canonicalize-based resolution, which is outside the descriptor-relative
  hardening guarantee. Safe-default Unix traversal (symlinks denied) uses
  `statat`/`openat` and is strictly stronger.
- **HTTP/2, redirects, retries, cookies, proxy, and multi-range responses are
  outside this release contract.** The server implements HTTP/1.1 with single
  byte ranges only.
- **Python wheels support CPython 3.14 only.** PyPy and free-threaded CPython
  are not supported. The wheel matrix covers Linux, macOS, and Windows on
  CPython 3.14.

## Gate reference

Gate definitions, evidence classes, platform obligations, freshness rules,
invalidation paths, dependencies, and waiver policy are maintained in
[`release/criteria.toml`](../release/criteria.toml). This file is the single
source of truth. Do not duplicate gate definitions in prose or in this
document.

To inspect the criteria:

```sh
python scripts/release_criteria.py validate release/criteria.toml
python scripts/release_criteria.py list
python scripts/release_criteria.py explain <gate-id>
python scripts/release_criteria.py graph
```

Gate IDs are stable. Renaming a gate requires a migration note and a criteria
schema version bump.
