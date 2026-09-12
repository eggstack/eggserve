# Plan 212 — Neutral TLS Crate Extraction and Cross-Repo Reuse Contract

## Status

Planned.

## Purpose

Extract EggServe's mature TLS configuration and identity machinery into a neutral crate suitable for reuse by other Eggstack networking projects without making EggServe depend on Eggress or EggFetch internals.

This is the highest-value cross-repository consolidation identified during the review.

## Rationale

EggServe, Eggress, and EggFetch independently depend on substantially the same Rust TLS stack:

- Rustls 0.23;
- rustls-pki-types;
- Tokio-Rustls;
- Ring-backed providers where configured.

EggServe currently has the strongest inbound/server TLS implementation among these projects, including:

- bounded certificate/key/trust parsing;
- validated identity configuration;
- SNI selection;
- conventional wildcard behavior;
- optional and required mTLS;
- CRL handling;
- explicit early-data disabling;
- atomic reload snapshots.

Eggress already contains a TLS transport crate, but it depends on Eggress-specific core types and tracing conventions, making it unsuitable as a neutral foundation.

The current Eggress optional-client-auth path also appears semantically incorrect: its non-required branch creates a normal `WebPkiClientVerifier` without `allow_unauthenticated()`, which means it does not implement “verify a client certificate when presented, but allow no certificate” semantics as documented.

EggServe should not solve this by depending on Eggress.

Instead, extract the reusable security-sensitive substrate from the stronger implementation.

## Proposed crate

Working name:

`eggnet-tls`

Alternative naming is acceptable if a repository-wide convention already exists.

The crate may live in the EggServe workspace initially and be published independently from that workspace.

A separate repository is not required to establish reuse.

## Dependency rules

The neutral crate must not depend on:

- `eggserve-core`;
- `eggserve-server`;
- `eggress-core`;
- EggFetch internals;
- proxy routing types;
- tracing infrastructure unless behind a narrowly justified optional feature;
- CLI or Python crates.

Core dependencies should remain close to:

- rustls;
- rustls-pki-types;
- optional tokio-rustls;
- minimal parsing/support crates already justified by the implementation.

## Functional scope

### Identity handling

Support:

- bounded PEM parsing;
- certificate chains;
- private keys;
- certificate/key pairing validation;
- multiple identities;
- exact SNI names;
- conventional single-label wildcard SNI;
- optional default identity.

### Client authentication

Provide explicit modes equivalent to:

- disabled;
- optional;
- required.

Optional authentication must use Rustls semantics that genuinely permit an unauthenticated client while validating a presented certificate.

Required authentication must reject clients without an acceptable certificate.

### Trust configuration

Support bounded:

- trust roots;
- CRLs;
- certificate chain limits.

Do not add platform trust discovery unless a concrete consumer requires it.

### Reloadable configuration

Provide a validated immutable snapshot/configuration model that callers can atomically replace.

The reusable crate should not prescribe filesystem watch behavior.

File watching belongs to applications or presentation layers.

### Tokio integration

If needed, provide Tokio-Rustls adapters behind an optional feature.

Keep pure parsing/configuration logic usable independently of Tokio where practical.

## API design requirements

Security policy must be explicit rather than inferred from booleans with ambiguous meaning.

Prefer enums such as `ClientAuthMode` to combinations such as:

`require_client_cert: bool`

Avoid exposing raw Rustls configuration mutation where it would allow consumers to accidentally bypass invariants established by the crate.

Provide escape hatches only where there is a demonstrated use case.

Error types must remain neutral and must not expose EggServe- or Eggress-specific error variants.

## Migration — EggServe

Replace the existing internal TLS implementation with the neutral crate while preserving EggServe's public behavior.

EggServe may retain wrapper/re-export types temporarily so current public paths remain source-compatible.

All existing TLS tests must be ported or retained.

No reduction in:

- parsing bounds;
- identity validation;
- SNI behavior;
- client-auth correctness;
- CRL support;
- reload safety

is acceptable.

## Cross-repository consumer contract — Eggress

Do not modify Eggress as part of the EggServe commit unless the work is intentionally being performed across repositories.

Instead, document the migration contract:

- Eggress should eventually replace its duplicated server TLS configuration with `eggnet-tls`;
- its public transport types may wrap the neutral implementation;
- its optional mTLS behavior must gain a regression test proving anonymous clients are allowed while presented certificates are validated;
- proxy-specific stream boxing remains in Eggress;
- tracing remains in Eggress.

The Eggress migration can be planned and implemented in that repository separately.

## EggFetch

Do not force EggFetch to consume the crate immediately.

EggFetch primarily needs client TLS behavior, while this extraction is motivated by the shared server-side/security-policy implementation.

Client-side components may later be generalized if doing so produces real code reduction and does not force an MSRV increase.

EggFetch currently carries a lower MSRV than EggServe, so cross-project reuse must not silently raise its toolchain floor.

## Test matrix

Add tests for:

- valid single identity;
- identity mismatch;
- malformed PEM;
- configured parsing limits;
- exact SNI;
- wildcard SNI;
- invalid wildcard patterns;
- unknown SNI with/without default;
- disabled client auth;
- optional client auth with no certificate;
- optional client auth with valid certificate;
- optional client auth with invalid certificate;
- required client auth with no certificate;
- required client auth with valid certificate;
- required client auth with invalid certificate;
- trust root bounds;
- CRL loading;
- reload snapshot replacement;
- rollback/no-replacement on invalid reload input.

## MSRV

Choose the crate's MSRV intentionally.

If the immediate consumers are EggServe and Eggress, an MSRV compatible with both is sufficient.

Do not lower or raise it solely to speculate about EggFetch reuse.

If later adopted by EggFetch, either:

- prove compatibility with EggFetch's MSRV; or
- make an explicit EggFetch MSRV decision in that repository.

## Non-goals

- Shared routing.
- Shared HTTP abstractions.
- Shared QUIC.
- ACME.
- Certificate issuance.
- File watchers.
- OpenSSL support.
- Python SSL context interoperability.
- Proxy protocol handling.
- Application-level TLS policy.

## Exit criteria

The plan is complete when:

- TLS identity/client-auth/trust/reload logic resides in a neutral crate;
- EggServe uses that crate without loss of security behavior;
- the crate has no EggServe/Eggress application dependency;
- optional mTLS semantics are explicitly regression-tested;
- an Eggress adoption contract is documented;
- EggFetch remains unaffected unless separately chosen as a consumer.
