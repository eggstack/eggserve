# Plan 149 — Bug Report Closure (2026-08-27)

## Status

**COMPLETE — 2026-08-27.**

## Scope

Resolve every actionable B1–B6 finding in the temporary `bugs.md` report dated
2026-08-26. Keep the changes minimal and make no feature additions.

### Fixes

1. Make `Lifecycle::drain()` idempotent while already `Draining`, with a
   direct regression test.
2. Mark zero-length `RequestBody::from_bytes` values consumed for stream
   admission decisions, with unit/property coverage.
3. Reject duplicate `Content-Length` during 304 metadata normalization using
   the existing response-construction error taxonomy; preserve ordinary
   payload normalization behavior.
4. Apply parse-level percent-encoded dot-segment checks to child components.
5. Permit additional `=` characters in attached CLI header values.
6. Reject whitespace-only configured extra response headers while retaining
   support for valid empty HTTP header values in the general header primitive.

The report's optimization-only notes remain out of scope.

## Validation

Validation completed: focused regression tests; conformance matrix;
formatting; workspace clippy; workspace tests (including 708 core unit tests,
all integration suites, 69 CLI tests, 2 expected ignored tests, and 8 doc
tests); and TLS clippy/tests (136 tests). Final diff review, deletion of
`bugs.md`, commit on `main`, and push to `origin/main` follow.
