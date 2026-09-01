# Plan 153 — Bug Report Closure (2026-09-01)

## Status

**COMPLETE — 2026-09-01.**

## Scope

Resolve the actionable findings in the temporary `bugs.md` report with
minimal changes and no feature additions.

## Planned changes

1. Constrain `ServiceError::rejected` status codes to the canonical HTTP
   range, remove the unreachable 501 body mapping, and add regression coverage.
2. Make sanitized root and trailing-slash paths retain a useful final field,
   updating the focused unit and integration expectations.
3. Preserve the originating pinned `SecureRoot` in Python resolved
   directories so `list()` and `resolve_child()` do not reopen or re-resolve
   the root by pathname.
4. Run the relevant Rust and Python checks, delete `bugs.md`, and commit/push
   the closure on `main`.

## Dispositions

- The reported request-body clone issue is not reachable: `RequestBody` does
  not implement `Clone`, and `read_all(self)` already rejects a streaming
  state on the same owned value. The shared atomic is only the connection
  pipeline's completion flag.
- The follow-symlink double-swap race remains the documented limitation of
  the explicitly weaker `SymlinkPolicy::Follow` profile; safe defaults never
  enter that path.
- The report's optimization, information, and coverage-only notes remain out
  of scope for this focused bug closure.

## Validation

- `cargo fmt --all -- --check`: clean.
- `python3 scripts/verify-conformance-matrix.py`: 51 entries validated.
- Workspace Clippy and tests: clean; 1,513 passed, 2 ignored.
- TLS Clippy and tests: clean; 138 passed.
- Core doc tests: 8 passed.
- Core examples check: clean.
- Python wheel smoke and test suite: 753 passed.
