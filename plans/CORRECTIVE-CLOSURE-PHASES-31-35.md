# Corrective Closure Pass — Phases 31–35

## Purpose

Close the remaining verification and repository-hygiene gaps discovered after implementing phases 31–35. This pass is intentionally narrow. It must not add new client features, framework behavior, ASGI/WSGI support, HTTP/2, HTTP/3, routing, middleware, retries, redirects, cookies, pooling, or proxy behavior.

## Blocking findings

1. A built wheel is committed under `dist/`.
2. Several HTTP wire tests accept multiple outcomes instead of enforcing one normative contract.
3. Some raw-wire tests bypass the production accept-loop configuration.
4. Mid-stream file I/O failures are silently converted to end-of-stream, weakening truncation/connection-close guarantees.
5. API stability documentation is not yet mechanically enforced.
6. Fuzz corpus inputs are not clearly replayed in ordinary CI.
7. Filesystem tests described as races include sequential mutation tests and should distinguish their actual guarantees.

## Track A — Repository artifact hygiene

- Remove committed wheel artifacts from source control.
- Add `dist/`, wheel files, and other release-build outputs to `.gitignore` unless a documented exception exists.
- Search history-adjacent documentation and scripts for assumptions that artifacts are checked in.
- Ensure release workflows upload artifacts through GitHub Actions rather than committing them.
- Verify `git status` remains clean after wheel and binary builds.

Acceptance:
- No wheel or native release binary remains tracked.
- Local build outputs are ignored.
- Release documentation points to generated artifacts, not repository copies.

## Track B — Normative HTTP wire expectations

Review every permissive assertion in `http_wire_correctness.rs`, especially tests accepting combinations such as `400 or close`, `405 or close`, or HTTP/1.0 either accepted or rejected.

For each case:
- define one expected behavior in `docs/release-contract.md`;
- adjust implementation if needed;
- tighten the test to the normative result;
- document deliberate connection-close behavior where no response is sent.

At minimum settle:
- HTTP/1.0 handling;
- absolute-form, authority-form, and asterisk-form requests;
- lowercase/unknown methods;
- malformed request lines;
- invalid headers and framing ambiguity;
- unsupported transfer codings;
- pipelining and keep-alive after malformed input.

Acceptance:
- No security-relevant raw-wire test accepts unrelated outcomes.
- Contract and tests agree exactly.
- Unsupported syntax has one documented failure mode.

## Track C — Production-path wire coverage

Keep the direct Hyper service harness for focused parser/service tests, but add a smaller production-path suite against either:
- the real binary; or
- the same accept-loop/server-builder path used in production.

Cover:
- header timeout timer configuration;
- connection limit integration;
- graceful shutdown behavior;
- malformed request closure;
- keep-alive/connection-close behavior;
- static full and range responses.

Acceptance:
- Critical wire guarantees are exercised through the production server path.
- The focused service harness and production harness have clearly separated responsibilities.

## Track D — Stream error propagation and connection safety

Replace silent `Err(_) => None` stream termination with an error-preserving body path where supported by Hyper/body types.

Required behavior:
- stream I/O error reaches the HTTP body machinery;
- response terminates as incomplete;
- the underlying connection is not reused after truncation;
- the file-stream semaphore permit is released;
- the error is observable through existing logging or a minimal internal diagnostic path;
- no local path is exposed to the client.

If Hyper cannot surface a body error while preserving current abstractions, document the exact limitation and force connection closure through the response/connection driver.

Tests:
- injected full-file read error;
- injected range read error;
- client detects premature EOF;
- next request is not served on the same connection;
- permit release after failure.

Acceptance:
- Mid-stream failures are not silently indistinguishable from clean EOF.
- Truncated responses cannot leave a reusable connection.

## Track E — API stability enforcement

Add lightweight mechanical checks for the phase 31 inventory:

Rust:
- public API snapshot or compile-sample checks for stable modules;
- default-feature build proving `python-bindings-internal` APIs are unavailable;
- client-feature and client-tls compile samples.

Python:
- installed-wheel test of `eggserve.__all__`;
- public names import successfully;
- internal names are absent;
- experimental client names remain explicitly classified.

Headers:
- duplicate request/response headers survive the declared raw representation;
- `Set-Cookie`-style duplicates are not collapsed.

Acceptance:
- API inventory drift fails CI or produces an explicit reviewed snapshot change.
- Internal bridge APIs cannot leak through ordinary builds.

## Track F — Fuzz corpus replay in normal CI

Add a fast deterministic corpus-regression job or test harness that runs every committed corpus input through its target logic without live mutation fuzzing.

Requirements:
- runs on normal pull-request CI;
- uses stable Rust where practical;
- fails on panic or invariant violation;
- retains weekly/manual `cargo fuzz` campaigns separately;
- documents mapping from corpus directories to target functions.

Acceptance:
- Every committed corpus input is replayed on normal CI.
- Weekly fuzz workflow remains supplemental, not the only regression path.

## Track G — Filesystem test taxonomy

Rename and document tests according to what they prove:
- sequential post-mutation regression;
- descriptor-relative traversal invariant;
- concurrent race stress;
- kernel-enforced `O_NOFOLLOW` behavior.

Add at least one bounded concurrent swap stress test on Unix for symlink/directory replacement during repeated resolution. The test need not prove absence of all races by itself; it should complement the structural `openat`/`O_NOFOLLOW` argument.

Acceptance:
- Test names do not overstate concurrency guarantees.
- Architecture docs clearly separate proof by design from stress evidence.

## Validation matrix

Run:
- `cargo fmt --check`
- clippy with default, TLS, client, and client-tls features
- full Rust tests
- full Python unit/integration tests
- production-path raw-wire tests
- installed-wheel export tests
- corpus replay
- `cargo audit`
- `cargo deny check`

## Completion gate

This pass is complete when:
- repository artifacts are clean;
- wire behavior is normative;
- production-path coverage exists;
- stream errors force safe connection termination;
- API stability is mechanically checked;
- corpus regression runs in ordinary CI;
- filesystem race claims are precise.
