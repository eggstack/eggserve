# Plan 150 — Bug Report Closure (2026-08-27)

## Status

**COMPLETE — 2026-08-27.**

## Scope

Resolve every actionable finding in the temporary `bugs.md` report dated
2026-08-27 with minimal changes and no feature additions. This covers B-01,
B-02, B-03, B-04, B-05, and B-07. B-06 is explicitly an audit note with no
present defect; latent tracking notes, optimization proposals, and coverage
gaps without a demonstrated failure remain out of scope.

## Changes

1. Add routine CI and fast local verification for the pinned supply-chain
   tools and the excluded Python crate's locked Cargo check.
2. Make file streaming tolerate a concurrent short read/EOF by yielding the
   bytes obtained and ending the stream cleanly.
3. Clarify the retained HEAD argument on the metadata-only normalizer while
   preserving HEAD length computation at the response normalizer boundary, and
   update its contract docs/tests.
4. Make directory-listing planning explicitly two-stage until the renderer
   supplies the GET bytes, with docs/tests that prevent treating its placeholder
   body as wire data.
5. Review the test-only request-head helper finding; it has no fuzz call path
   and no production impact, so no production-facing change is warranted.

## Validation

Validation completed: focused regressions; formatting; conformance; workspace
clippy/tests (1,483 passed, 2 expected ignored); TLS clippy/tests (136 passed);
8 core doc tests; `verify.sh fast`; and pinned `cargo audit`/`cargo deny check`.
The excluded Python crate's locked `cargo check` also passed. `bugs.md` was
deleted; the final diff is ready for commit on `main` and push to
`origin/main`.
