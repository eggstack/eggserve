# TLS Identity Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#2` (architectural ownership: `eggnet-tls` is the neutral rustls identity substrate)
- `plans/000-long-term-specification.md#3` (invariants)
- `plans/000-long-term-specification.md#4` (TLS identity remains experimental)
- `plans/001-terminology-and-domain-model.md#subsystem-names-stable` (subsystem vocabulary: `tls-identity`)
- `plans/002-long-term-roadmap.md` (dependency notes)

Related ADRs:

- none (the substrate boundary is defined by `plans/000` section 2 and `architecture/eggnet-tls.md`)

## 1. Purpose and ownership boundary

Owns the server-side TLS identity substrate: bounded PEM loading, certificate/key pairing validation, SNI exact/wildcard/default resolution, WebPKI client authentication with explicit trust roots and CRLs, ALPN validation, immutable `TlsServerConfig` snapshots, atomic reload handles, and the QUIC identity assembly `eggserve-h3` derives from that substrate.

Owns: `crates/eggnet-tls/` (identity, trust, client auth, reload, ALPN) and the TLS-specific assembly in `crates/eggserve-h3/src/quic.rs`.

Consumes: `eggserve-core` for the compatibility facade (`eggserve_core::tls` re-export), accept-loop integration, runtime configuration validation, and the CLI/Python frontends. Never owns HTTP framing, listener lifecycle, proxy routing, or application TLS policy.

## 2. Work classification

### Invariants

- Certificate/key pairing and all bounded inputs fail before readiness, never at the first hostile handshake.
- TLS configuration construction never depends on the process-default rustls `CryptoProvider`; the ring provider is explicit.
- A TLS configuration must not advertise an ALPN protocol the runtime cannot serve in its current configuration.
- Conservative defaults remain: no application 0-RTT, no stateful tickets, no implicit revocation without CRLs.
- Accepted ALPN and verified metadata remain bounded and sanitized; no key material is logged.

### Capabilities

- Single-identity TLS for the CLI and Python `HTTPSServer`/`ThreadingHTTPSServer`.
- Rust SNI multi-identity, optional/required WebPKI client authentication, and atomic reload for new handshakes.
- Experimental H2 ALPN on the compatibility server and experimental H3/QUIC identity assembly.

### Infrastructure

- Bounded PEM/trust/CRL parsing and identity bounds.
- `TlsServerConfigBuilder`, `TlsReloadHandle`, and the `eggserve_core::tls` facade.
- Runtime ALPN/runtime-flag coherence validation.

### Polish

- Typed, non-misleading error categories for QUIC transport bounds.
- Documentation that matches the executable CLI and runtime behavior.

## 3. Non-goals

- No ACME, certificate discovery/watching, OCSP stapling, KMS signing, or secret storage.
- No Python certificate-verification callback and no client certificates on the Python facade.
- No tier promotion for SNI/mTLS/reload or H2/H3; `server`/TLS-identity remain experimental.
- No second TLS implementation outside `eggnet-tls`/`eggserve-h3`.
- No CLI multi-identity or mTLS flags.

## 4. Current state

The substrate is implemented and tested: bounded identity/trust/CRL parsing, exact and single-level wildcard SNI with an optional default, `ClientAuthMode::{Disabled, Optional, Required}`, atomic reload, neutral ALPN construction, and a separate TLS 1.3-only `h3` QUIC config. The CLI remains single-identity; Python remains single-identity HTTP/1.1-only.

A capability review at baseline `26649bd` found five defects at the seams:

1. `eggnet-tls` and `eggserve-h3` construct rustls configurations through the process-default provider, which panics when the dependency graph enables both `ring` and `aws-lc-rs` and no default has been installed.
2. The compatibility server does not validate that a TLS configuration's ALPN advertisement agrees with `RuntimeConfig.http2.enabled`; an h2 advertisement on an H2-disabled runtime completes the handshake and then drops the connection.
3. `--header-timeout` is documented as the TLS handshake bound but only sets the header-read timeout.
4. The SNI identity cap counts the optional default identity and is enforced in one builder method only, making the documented ceiling order-dependent.
5. H3 QUIC window/idle-timeout bounds are not validated to the QUIC VarInt range and are reported through `TlsError::InvalidKey`, misdiagnosing transport configuration as certificate material.

The follow-up security review of 327 found one more seam: a running server's reload swap accepted an h2 advertisement while HTTP/2 was disabled, and the accept loop then completed every h2 client's handshake before dropping it. Milestone 328 enforces the coherence check at the swap.

## 5. Target architecture

Every rustls server configuration is built with an explicit `Arc<CryptoProvider>` (ring), matching the provider already used for key/cert pairing. The compatibility runtime validates the effective TLS ALPN set (static or reload snapshot) against its HTTP/2 capability before serving, and the server handle rejects a contradictory snapshot at the reload swap. The CLI's documented header timeout governs both header reads and TLS handshakes. The identity builder enforces one order-independent ceiling for named SNI identities, with the optional default separate. H3 validates transport bounds in `Http3Config::validate` and reports transport-specific failures through a dedicated typed error.

## 6. Dependency graph

    Milestone 327 TLS substrate corrective (closed)
        |
        +--> Milestone 328 reload ALPN coherence (closed)
        |
        +--> future SNI/mTLS capability milestones (soft)
        `--> H2/H3 transport milestones (soft; ALPN coherence shared)

Milestone 327 has no hard dependency; milestone 328 takes 327's shared ALPN predicate as an interface dependency and is closed.

## 7. Milestones

### Milestone 327 - TLS substrate corrective

Class: invariant

Objective: remove the five seam defects above without changing TLS capability tiers, defaults, or the single-identity CLI/Python surface.

Dependencies: none (hard); `eggserve-core` compatibility facade is the interface.

Deliverable boundary: `eggnet-tls`, the core runtime validator, the CLI limit projection, the H3 QUIC assembler, focused tests, and the matching docs.

User or operator value: no process abort on valid credentials in multi-provider builds; no silent h2 connection drops from contradictory configuration; the documented handshake timeout actually applies; documented identity limits are order-independent; H3 transport misconfiguration is diagnosed correctly.

Exit conditions: all acceptance criteria in `plans/implementation/tls-identity/327-tls-substrate-corrective.md` pass and the closure record is written.

Deferred work: H3 SNI metadata exposure; mTLS/SNI CLI flags; any tier promotion.

### Milestone 328 - Reload ALPN coherence

Class: invariant

Objective: enforce 327's ALPN/runtime coherence at the server handle's reload swap, so a running server never accepts an h2 advertisement it cannot serve.

Dependencies: 327 (interface: the shared ALPN predicate); the `eggserve-core` compatibility facade.

Deliverable boundary: `ServerHandle` capability capture and swap guard, one regression test, docs, and the 327 correction note.

User or operator value: a contradictory certificate rotation is rejected at the call site with the previous identity still live, instead of silently failing every h2 client after a completed handshake.

Exit conditions: the acceptance criteria in `plans/implementation/tls-identity/328-reload-alpn-coherence.md` pass and this closure record is written.

Deferred work: making the neutral `TlsReloadHandle` runtime-aware; the raw accessor remains a documented bypass.

## 8. Cross-cutting requirements

### Protocol and compatibility

Static TLS configuration and reload snapshots are both validated against the runtime's HTTP/2 capability. A configuration that advertises `h2` while H2 is disabled or unavailable is rejected rather than accepted and dropped after the handshake.

### Security and authorization

Provider determinism removes a startup abort; ALPN validation prevents protocol confusion; the identity-cap correction keeps the documented certificate/key resource ceiling; no key material or rustls internals are exposed.

### Concurrency, cancellation, and recovery

Reload remains all-or-nothing for new handshakes; per-connection validation reads the current snapshot atomically. Existing connections keep their session.

### Observability and audit

TLS failures remain fixed sanitized categories. The new validation errors name the configuration field, never certificate bytes.

### Documentation and operations

`docs/tls.md`, `docs/security-policy.md`, `docs/deployment.md`, `docs/cli.md`, `docs/timeout-reference.md`, and `architecture/eggnet-tls.md` must match the executable behavior.

## 9. Verification strategy

Focused crate tests for each fix, the existing TLS identity/abuse/neutral suites, `cargo fmt --check`, clippy `-D warnings` for the changed crates, and `scripts/check-crate-topology.py`. `./scripts/verify.sh fast` is the broad routine gate; the TLS-only bin tests and H3 suites run where CI would run them.

## 10. Risks and decision points

- Adding a `TlsError` variant is a public-enum change in a 0.x crate; it is intentional so transport configuration is not reported as a private-key failure. No ADR is required for this 0.x diagnostic correction.
- Coupling `--header-timeout` to `tls_handshake_timeout` preserves the documented security policy; a future dedicated flag would supersede it through a new milestone.
- The raw `TlsReloadHandle` accessor remains a documented bypass of the swap check; enforcing it there would couple the neutral substrate to EggServe's HTTP/2 policy.
- Downstream graphs that enable `aws-lc-rs` in production remain supported because the substrate no longer consults the process default.

## 11. Completion definition

The subsystem is complete when configuration construction is provider-deterministic, ALPN/runtime coherence is enforced at validation and at the server reload swap, the documented CLI bound is real, identity limits are order-independent, H3 transport bounds are validated and typed, and the closure record contains the exact commands and outcomes.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 327 | closed | `plans/implementation/tls-identity/327-tls-substrate-corrective.md` | `plans/closure/tls-identity/327-tls-substrate-corrective.md` | none |
| 328 | closed | `plans/implementation/tls-identity/328-reload-alpn-coherence.md` | `plans/closure/tls-identity/328-reload-alpn-coherence.md` | none |
