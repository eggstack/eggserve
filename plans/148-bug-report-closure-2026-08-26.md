# Plan 148 — Bug Report Closure (2026-08-26)

## Status

**COMPLETE — 2026-08-26.**

## Scope

Resolve all confirmed findings in the temporary `bugs.md` report dated
2026-08-26, with minimal changes and no new product features. This includes
BUG-01 through BUG-08: fallback short-name policy enforcement, Windows
fail-closed handle/metadata handling, public range-length overflow behavior,
CLI header parsing, accept backoff, planner documentation, and the
`rustls-pemfile` migration and the corresponding dependency documentation.

The report's optimization-only notes are out of scope. Defense-in-depth gaps
will be addressed only where the existing behavior is an actionable security,
availability, or correctness defect without changing the product contract;
intentional cross-platform policy choices remain documented rather than
redesigned.

## Validation

Validation completed: focused core/TLS tests; workspace tests (1,477 passed,
2 ignored); TLS tests (136 passed); doc tests (8 passed); format and clippy;
conformance and `verify.sh fast`; Windows non-TLS target check; examples;
both dist builds; package verification; Python wheel smoke/tests (748 passed);
`cargo audit`; and `cargo deny check`. Windows + TLS cross-compilation was
attempted but is unavailable in this Linux environment because `ring` rejects
the GNU compiler for the `*-pc-windows-msvc` target. Delete `bugs.md`, commit
on `main`, and push to `origin/main` after the final diff review.
