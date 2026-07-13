# Release Checklist

This checklist is evidence-driven. A workflow declaration or a green local
command is not evidence of a GitHub matrix or release-workflow run. Every
release-candidate decision must identify one evaluated commit SHA; changes to
release workflows, manifests, lockfiles, packaging tests, or support claims
invalidate the affected evidence.

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

## Release-gate evidence

| Gate | Status | Evidence class | Command/job and result | Run ID / artifact digest |
|---|---|---|---|---|
| Rust format, lint, tests, doctests, feature matrices | `PENDING` | `GITHUB` | CI `rust-check`, required matrix green | `PENDING` |
| Raw-wire and production-path tests | `PENDING` | `GITHUB` | CI `wire-tests`, release validation | `PENDING` |
| Corpus replay, including client feature | `PENDING` | `GITHUB` | CI `corpus-replay` | `PENDING` |
| Pinned `cargo-audit` / `cargo-deny` installation and checks | `PENDING` | `GITHUB` | CI/release `scripts/install-cargo-tools.sh`, versions `0.22.2` / `0.19.0` | `PENDING` |
| `eggserve-core` package/list/publish dry-run | `PENDING` | `GITHUB` | `scripts/verify-cargo-packages.sh` | `PENDING` |
| `eggserve-bin` local-registry package/list/publish dry-run | `PENDING` | `GITHUB` | Exact packaged core crate staged in temporary registry | `PENDING` |
| Python metadata and CPython 3.14 support | `PENDING` | `GITHUB` | `Requires-Python >=3.14,<3.15`, imports/API/exception tests | `PENDING` |
| Installed wheel on Linux, macOS, Windows | `PENDING` | `GITHUB` | Native wheel build, clean venv, `PYTHONPATH` unset, callback/client/CLI smoke | `PENDING` |
| Release artifacts and checksums | `PENDING` | `GITHUB` | Expected four Unix/one Windows archives and wheels inspected | `PENDING` |
| Provenance | `PENDING` | `GITHUB` | Commit and workflow run match `provenance.json` | `PENDING` |
| Latest fuzz campaign | `PENDING` | `GITHUB` | Date, workflow URL, result, and corpus replay result | `PENDING` |

## Pre-release review

- [ ] Version numbers are synchronized across all crates and the Python package.
- [ ] README, package metadata, release contract, and support classifiers agree.
- [ ] Known accepted limitations are listed in release notes.
- [ ] Windows is classified as functional/parser-hardened only; no Unix-level filesystem claim is made.
- [ ] Follow-symlinks mode is identified as weaker and outside descriptor-relative hardening.
- [ ] No critical/high release blocker remains.
- [ ] Approver has signed off on the exact evaluated commit SHA above.

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

## Known accepted limitations

- Windows reparse-point/junction hardening is deferred; do not use Windows
  with untrusted mutable public content.
- Follow-symlinks mode is weaker than safe-default Unix traversal.
- HTTP/2, redirects, retries, cookies, proxy support, and multi-range
  responses are outside this release contract.
- Python wheels currently support CPython 3.14 only; PyPy and free-threaded
  CPython are not supported.
