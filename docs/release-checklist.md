# Release Checklist

This checklist is a brief overview. The full gate table is machine-generated
from the single source of truth at
[`release/criteria.toml`](../release/criteria.toml) and written to
[`release-checklist-generated.md`](release-checklist-generated.md).

The human operator guide covering evidence philosophy, command sequences,
and failure handling is in [`release-process.md`](release-process.md).

## Regenerating the checklist

```sh
python scripts/release_criteria.py generate-checklist --criteria release/criteria.toml
```

Run this after editing `release/criteria.toml` or when the generated file
falls out of date.

## Evidence record

| Field | Value |
|---|---|
| Evaluated commit SHA | `PENDING — fill at RC approval` |
| CI workflow URL / run ID | `PENDING` |
| Manual release dry-run URL / run ID | `PENDING` |
| Dry-run publication result | `PENDING — must be skipped/blocked` |
| Approver / date | `PENDING` |

Use `LOCAL` for an exact command, host platform, tool versions, and exit code;
use `GITHUB` for a workflow URL/run ID, commit SHA, job result, and artifact
evidence; use `CONFIG` only for static YAML or policy review. `CONFIG` never
closes an execution gate.

The superseded pre-closure dry-run `29258214679` ran against the prior remote
commit and failed because its original workflow invoked `cargo audit` without
installing the plugin. It is intentionally not release evidence; a new run
against the final closure commit is required.

## Publication gates

The manual release workflow defaults to `dry_run=true`. A dry run must not
publish to crates.io or PyPI, create a GitHub release, or require production
registry tokens. The protected `publish` job is entered only by the tagged
push path with publication enabled and is separately environment-gated.

For crates.io, run `bash scripts/verify-cargo-packages.sh`; the binary check
uses a temporary local registry because the core crate must exist in the
registry before a real crates.io publish. The script still runs Cargo's real
publish dry-run against that packaged graph.

For PyPI, build the platform wheel after staging the matching CLI binary and
run `crates/eggserve-python/packaging-tests/run_all.sh` from its clean-venv
workflow. `dist/` outputs must remain untracked.

## Pre-release review

- [ ] Version numbers are synchronized across all crates and the Python package.
- [ ] README, package metadata, release contract, and support classifiers agree.
- [ ] Known accepted limitations are listed in release notes.
- [ ] Windows is classified as functional/parser-hardened only; no Unix-level filesystem claim is made.
- [ ] Follow-symlinks mode is identified as weaker and outside descriptor-relative hardening.
- [ ] No critical/high release blocker remains.
- [ ] Approver has signed off on the exact evaluated commit SHA above.

## Known accepted limitations

- Windows reparse-point/junction hardening is deferred; do not use Windows
  with untrusted mutable public content.
- Follow-symlinks mode is weaker than safe-default Unix traversal.
- HTTP/2, redirects, retries, cookies, proxy support, and multi-range
  responses are outside this release contract.
- Python wheels currently support CPython 3.14 only; PyPy and free-threaded
  CPython are not supported.
