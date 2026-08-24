# Plan 142 — Bug List Closure

## Status

**COMPLETE — 2026-08-24.**

## Scope

Resolve every actionable item recorded in the temporary `bugs.md` list with
minimal changes to existing behavior and focused regression coverage. This
includes the listed correctness, contract, portability, protocol-validation,
and directly local performance issues; the service-branch refactor note is
informational and remains out of scope.

## Validation

Run formatting, workspace lint/tests, TLS lint/tests, targeted core tests, and
the Python wheel test path. Review the final diff, delete `bugs.md`, then
commit and push the completed closure from `main`.

## Completion criteria

- [x] all actionable findings are fixed;
- [x] focused regression tests cover changed behavior;
- [x] required verification passes;
- [x] `bugs.md` is deleted;
- [x] changes are committed and pushed from `main`.

## Verification evidence

- `cargo fmt --all -- --check` passed.
- Workspace clippy passed with `-D warnings`.
- `cargo test --workspace` passed with 1,401 tests and 2 expected ignored;
  TLS tests passed with 106 tests.
- Core documentation/examples checks, both CLI dist builds, and all-mode
  Cargo package dry-runs passed.
- An installed wheel build and targeted affected Python modules passed. The
  complete wheel harness was stopped after its integration suite made no
  progress for more than twelve minutes; no test failure was reported before
  the hang, and the targeted affected modules passed independently.
