# Plan 154 — Bug Report Closure (2026-08-28)

## Status

**COMPLETE — 2026-08-28.**

## Scope

Close the actionable findings in the temporary `bugs.md` report with minimal
changes and no feature additions. This plan covers premature request-body EOF
state, whitespace-only fallback content types, fallible byte-range
construction, request-target whitespace parity, and deterministic hostname
binding. The logging/allocation notes are optimizations rather than bugs and
remain out of scope; the retained `normalize_metadata` parameter is an
experimental API compatibility choice with no correctness defect.

## Changes

1. Mark `RequestBody` as errored on premature EOF in both consuming APIs and
   add state regressions.
2. Reject whitespace-only default content types during configuration
   validation and add construction/CLI coverage.
3. Keep `FileRange::new` internal, require `try_new` for external callers,
   validate resolver conversions without panicking, and update internal uses
   and tests.
4. Make the public request-target validators use HTTP byte-oriented ASCII
   whitespace checks, and select hostname bind results deterministically.
5. Run the repository verification gates and remove the temporary bug report.

## Validation

Validation completed: focused regressions; formatting; workspace Clippy and
tests (1,495 passed, 2 ignored); TLS Clippy/tests (137 passed); core doc tests
(8 passed); conformance matrix validation (51 entries); `verify.sh fast`; core
examples check; both dist builds; and Cargo package dry-runs with
`ALLOW_DIRTY=true`. The excluded Python crate's locked check passed through
the fast verification script.
