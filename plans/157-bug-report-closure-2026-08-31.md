# Plan 157 — Bug Report Closure (2026-08-31)

## Status

**COMPLETE — 2026-09-01.**

## Scope

Fix every actionable item in the temporary `bugs.md` report dated
2026-08-31, with minimal implementation changes and focused regression tests.
Do not implement the report's separate optimization suggestions or add
unrelated features.

## Planned changes

1. Correct HEAD/304 metadata normalization and exact-limit request-body stream
   completion; add focused tests.
2. Make child filesystem validation honor the parsed backslash policy and
   preserve raw Windows handle-clone error codes; add parity tests.
3. Give TLS handshakes their own runtime/limits timeout, wire all bridges, and
   document the timeout catalog change.
4. Bound static extra response header count/size, apply the same validation at
   the Rust and Python boundaries, and add CLI/core regression tests.
5. Document the intentional pre-epoch ETag encoding and divergent DEL handling.
6. Run formatting, lint, tests, documentation checks, remove `bugs.md`, review
   the final diff, commit on `main`, and push to `origin/main`.

## Out of scope

The report's optimization opportunities and informational findings that are
explicitly not bugs remain unchanged.
