# Plan 155 — Bug Report Closure (2026-08-31)

## Status

**COMPLETE.**

## Scope

Resolve every actionable correctness, security, and consistency bug in the
temporary `bugs.md` report dated 2026-08-31. Keep the patch minimal and make
no optimization or feature additions.

## Planned changes

1. Reject raw and percent-decoded ASCII controls consistently in path and
   request-target validation, including the Python error mapping.
2. Make `Expect: 100-continue` matching case-insensitive, expose chunked
   Python request bodies, flush rechunked data before transport errors, and
   align Python directory-listing escaping with core.
3. Reject backslashes in every low-level child-component validation path,
   strip `Connection`-nominated hop-by-hop response headers, and remove the
   misleading unused `normalize_metadata` HEAD parameter.
4. Add focused regressions for all changed behavior and for the already-fixed
   inverted `FileFull::read_range` behavior.
5. Run formatting, targeted tests, workspace tests/lints, Python wheel checks,
   remove `bugs.md`, review the final diff, commit on `main`, and push to
   `origin/main`.

## Validation

- `scripts/verify-conformance-matrix.py` — validated 51 entries.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings` — passed.
- `cargo test --workspace` — passed.
- `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings` — passed.
- `cargo test -p eggserve-bin --features tls` — passed.
- `PYTHON=python3.14 bash scripts/test-python-wheel.sh` — 752 tests passed.
- `./scripts/verify.sh fast` — passed, including the excluded Python crate check.
