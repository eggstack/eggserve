# Plan 151 — Bug Report Closure (2026-08-27)

## Status

**COMPLETE — 2026-08-27.**

## Scope

Close the actionable findings in the temporary `bugs.md` report with minimal
changes and no feature additions. This includes the request-body completion
flag, callback admission lifetime, 205 documentation/framing behavior,
Python header validation parity, and path/header documentation corrections.
The connection-response duplication, unused private helper concern, and
profile-dependent optimization notes are not correctness bugs and remain out
of scope.

## Planned changes

1. Mark fixed request bodies consumed when their final `next_chunk` is
   returned, and add a regression test.
2. Move the Python callback semaphore permit into the blocking task so it is
   held until callback execution actually ends.
3. Reject caller-supplied `Content-Length` on 205 responses, update the
   body-forbidden documentation, and add a regression test.
4. Reject empty/whitespace-only Python extra and dynamic response header
   values, with focused parity coverage.
5. Clarify that header OWS trimming is intentional RFC-compatible
   normalization and that the second path percent-decode check protects
   against double-encoded dot-segment traversal.
6. Align one stale Python test with the existing constructor-time status
   validation contract exposed by `Response.body_source()`.

## Validation

Validation completed: focused Rust tests; formatting; conformance; workspace
clippy/tests (1,485 passed, 2 ignored); TLS clippy/tests (136 passed); core
doc tests; excluded Python-crate locked check; and the installed Python wheel
suite (750 passed). The stale `Response.body_source()` status test was aligned
with the existing constructor contract. `bugs.md` is removed after closure.
