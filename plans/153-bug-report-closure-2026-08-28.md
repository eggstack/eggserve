# Plan 153 — Bug Report Closure (2026-08-28)

## Status

**COMPLETE — 2026-08-28.**

## Scope

Resolve every actionable finding in the temporary `bugs.md` report dated
2026-08-28 with minimal changes and no feature additions. This covers B1–B7
and the concrete low-risk optimizations O1–O3.

## Planned changes

1. Remove unexplained fallible-construction unwraps from canonical error
   responses and prevent overflowing `FileRange` values from being created.
2. Make bounded file reads tolerate short reads, preserve control characters
   distinctly in directory-listing text, and make lifecycle draining retry
   state races.
3. Unify the path sanitizer range expression, make truncation single-pass,
   reduce text-log and body-framing allocations, and avoid the dead unlimited
   body-limit fallback for `Reject`.
4. Add focused regression coverage where behavior is externally observable.
5. Run the repository verification gates, delete `bugs.md`, and commit/push
   the closure on `main`.

## Dispositions

O4 requires a buffer-pool design beyond this minimal maintenance patch. O5 is
not material for the bounded small-header lists this server accepts, and O6
and O7 are already bounded/appropriate linear operations. No feature work or
scope expansion is included.

## Validation

Validation will include focused tests for changed modules, formatting,
workspace Clippy/tests, TLS Clippy/tests, conformance validation, and the
available fast verification gates before the final diff review and push.

Validation completed: focused regressions; formatting; workspace Clippy and
tests (1,492 passed, 2 ignored); TLS Clippy/tests (136 passed); 8 core doc
tests; conformance matrix validation (51 entries); `verify.sh fast`; Rust
example smoke checks; both dist builds; and Cargo package verification. The
Python wheel gate was not available because Python 3.14 and maturin are not
installed in the environment.
