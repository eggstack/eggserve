# Plan 213 — HTTP/3 and QUIC Isolation, Qualification, and Promotion Gates

## Result

Implemented on 2026-09-12. The H3/QUIC dependency boundary and qualification
inventory are complete; HTTP/3 remains experimental and was not promoted.

## Delivered

- Added `eggserve-h3` as the only workspace package with direct production
  dependencies on `h3` 0.0.8, `h3-quinn` 0.0.10, and Quinn 0.11.11.
- Made `eggserve-core` and `eggserve-bin` consume that package only through
  their opt-in `http3` features.
- Added topology checks proving that the default graph has no H3/QUIC packages
  and that lower-layer crates do not acquire the transport dependencies.
- Added `conformance/http3_qualification.toml`, separating deterministic,
  manual, and blocked evidence across protocol, lifecycle, adversarial,
  interoperability, configuration, dependency, and promotion categories.
- Updated the README, agent guidance, crate/dependency architecture pages, and
  H3 qualification documentation.

## Compatibility boundary

The mature 0.1 H3 adapter source remains in `eggserve-core` for source and
behavior compatibility, but all raw H3/QUIC imports now resolve through
`eggserve-h3`. A physical source move is intentionally deferred to a separate
semver-scoped change; this plan establishes the real Cargo dependency and
qualification boundary without changing the established compatibility facade.

## Qualification decision

The H3 path remains experimental. Promotion is blocked by the unresolved
upstream stream/error-handling risks tracked in the plan (`hyperium/h3#338`
and `#262`) and by unavailable independent-client, adversarial-wire,
impairment, browser, and cross-platform runtime evidence. The qualification
matrix records those evidence classes explicitly rather than treating local
deterministic tests as promotion evidence.

## Verification

The local gates passed on 2026-09-12:

- conformance and topology validators, formatting, and whitespace checks;
- MSRV workspace checks for default, `http2,tls`, and `http3,tls`;
- default workspace tests (1,891 passed, 3 ignored) and H3-enabled workspace
  tests (1,973 passed, 3 ignored);
- feature-specific clippy/test lanes, including core H2/H3 and bin TLS/H3;
- layered Cargo package dry-run and root/Python supply-chain checks;
- `scripts/qualify-http3.sh` TCP/Alt-Svc/H1 fallback baseline;
- installed Python wheel smoke and test suite (804 tests passed).

The dedicated matrix validator reports 17 Plan 213 scenarios, including
routine deterministic coverage and explicitly blocked manual/promotion items.
The H3 qualification script found no independent H3 client on the host, so
direct H3 wire evidence remains unavailable rather than being counted as a
pass.
