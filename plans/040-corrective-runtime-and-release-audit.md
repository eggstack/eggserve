# Phase 40 — Corrective Runtime, Security, and Release Audit

## Goal

Close verified correctness, resource-lifetime, security-boundary, and release-hygiene defects found during a repository audit without expanding eggserve beyond its static-serving and primitive-library scope.

## Scope

- Keep the HTTP client on origin-form request targets and reject malformed unbracketed IPv6 authorities.
- Apply the existing GET/HEAD request-body framing policy to the Python server runtime.
- Preserve file capabilities through the Python handler boundary so file responses stream instead of being eagerly copied into Python-owned memory.
- Suppress handler bodies for HEAD before acquiring file-stream resources.
- Make public range/body conversions reject invalid or overflowing ranges instead of panicking or narrowing values.
- Propagate file-stream I/O errors to the HTTP body instead of silently presenting a truncated successful response.
- Repair release workflow permissions, dry-run validation, staged artifact publication, and CI feature coverage.
- Reconcile security, release, client, filesystem, and Python architecture documentation with the implementation.

## Non-goals

- No new server protocol, routing, authentication, compression, or proxy behavior.
- No dependency additions.
- No Windows reparse-point hardening or follow-symlink TOCTOU redesign.
- No connection pooling or streaming API for the buffered client.

## Acceptance criteria

- Client wire tests assert origin-form request lines; URL tests reject unbracketed IPv6 authorities.
- Python static and callback paths reject invalid, conflicting, transferred, or non-zero GET/HEAD bodies with the documented status codes.
- Python file-backed handler responses retain streaming semantics and HEAD does not acquire a file-stream permit.
- Invalid range inputs return errors and all range length/conversion arithmetic is checked on supported platforms.
- File read/seek failures become body errors, with no success response claiming bytes that were not delivered.
- CI and release workflows use least-privilege permissions, validate manual dry-runs without pretending they are tags, run the client-TLS/docs checks, and publish the staged artifacts they checksum.
- Documentation no longer claims that ranges are absent, file bodies are eagerly copied through Python, or the client intentionally cannot talk to eggserve.

## Validation

Run formatting, workspace clippy/tests, client and client-TLS tests, raw-wire tests, documentation tests, and relevant Python wheel tests when the local build environment permits.
