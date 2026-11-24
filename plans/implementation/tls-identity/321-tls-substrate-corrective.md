# TLS Identity Milestone 321 - TLS substrate corrective

Status: implemented

Repository baseline: `26649bd` (branch `plan-320-public-bind-acknowledgment-hardening`, clean tree)

Source roadmap:

- `plans/subsystems/tls-identity-roadmap.md#7` (Milestone 321)

Long-term requirements:

- `plans/000-long-term-specification.md#2` (neutral `eggnet-tls` ownership)
- `plans/000-long-term-specification.md#3` (invariants)
- `plans/000-long-term-specification.md#4` (TLS identity remains experimental)
- `plans/001-terminology-and-domain-model.md#subsystem-names-stable` (subsystem vocabulary)

Applicable ADRs:

- none

Primary class: invariant

## 1. Objective

Remove five defects found by a capability review of the TLS surface at the baseline, without changing TLS tiers, safe defaults, or the single-identity CLI/Python surface.

## 2. Why this milestone is ready

There is no hard dependency: all five defects are in already-landed infrastructure. Each defect has a concrete code path and a deterministic test that fails before the fix (provider selection, ALPN/runtime disagreement, CLI limit projection, identity cap, QUIC VarInt bounds).

## 3. Current implementation evidence

- `crates/eggnet-tls/src/lib.rs` builds rustls configurations with `ServerConfig::builder()` (lines 274 and 730) and `WebPkiClientVerifier::builder()` (line 765). `validate_key_pair` already uses the ring provider explicitly (line 326).
- `crates/eggserve-h3/src/quic.rs` uses `builder_with_protocol_versions` (line 28) and maps VarInt/IdleTimeout range failures to `TlsError::InvalidKey` (lines 41-53).
- `crates/eggserve-core/src/server/config/runtime.rs` states that TLS has no extra scalar invariants (line 397); `crates/eggserve-core/src/server/accept.rs` drops an h2-negotiated connection when `http2.enabled` is false (lines 846-861).
- `crates/eggserve-bin/src/args.rs` maps `--header-timeout` only to `header_read_timeout` (lines 838-840).
- `crates/eggnet-tls/src/lib.rs` counts the optional default identity in the `add_identity` cap (line 575) but `default_identity` has no cap check (line 597).
- `crates/eggserve-h3/src/config.rs` validates QUIC windows only for non-zero (lines 71-115).
- Documentation conflicts: `docs/tls.md#137` claims ALPN derives from the runtime flag; `docs/tls.md#188` and `docs/security-policy.md#159` claim `--header-timeout` bounds the TLS handshake while `docs/deployment.md#116` says there is no CLI flag.

## 4. Invariants that must not regress

- Identity/trust/CRL parsing remains bounded and fails before readiness.
- No application 0-RTT; TLS 1.3-only QUIC; default rustls ticket policy.
- The CLI and Python remain single-identity; Python remains HTTP/1.1-only.
- No key/certificate bytes in errors, Debug, events, or logs.
- Accepted metadata remains bounded (SNI 253, chain 8 x 64 KiB).
- H1 is unaffected; H2/H3 remain experimental.

## 5. Scope

### In scope

- Explicit ring-provider construction in `eggnet-tls` and `eggserve-h3`; removal of the Python process-global workaround.
- Runtime validation that the effective TLS ALPN set agrees with HTTP/2 availability.
- `--header-timeout` also setting `tls_handshake_timeout` (and matching docs).
- Order-independent named-SNI identity cap with the optional default separate.
- H3 VarInt/IdleTimeout upper-bound validation and a typed transport error.
- Focused tests and documentation updates.

### Explicitly out of scope

- New CLI flags (for example a dedicated `--tls-handshake-timeout`).
- Tier promotion, ACME/OCSP/watchers, mTLS on the CLI/Python, H3 SNI metadata.
- Any change to TLS defaults, cipher policy, or the supported version range.

## 6. Required production changes

### Crates and ownership

`eggnet-tls` owns provider selection; `eggserve-h3` uses an explicit ring provider for its TLS 1.3 QUIC config; `eggserve-core` owns the ALPN/runtime coherence check; `eggserve-bin` owns the CLI projection.

### Config and policy

Add a private `crypto_provider()` helper in `eggnet-tls` returning `Arc<rustls::crypto::CryptoProvider>` and use it in both server builders and the WebPKI verifier builder. Add `TlsError::{InvalidCryptoProvider, InvalidTransportConfig}` for non-key failures.

### Protocol and compatibility

In `RuntimeConfig::validate`, reject a TLS configuration (static or current reload snapshot) whose ALPN contains `h2` when the build has no `http2` feature or when `http2.enabled` is false.

### Runtime and concurrency

Validation is repeated on the connection path (existing `config.validate()` call) so a reload snapshot is checked against the live runtime flag; existing connections are untouched.

### Frontend or operator surface (CLI / Python)

`Args::limits()` sets both `header_read_timeout` and `tls_handshake_timeout` from `--header-timeout`. Remove the Python `install_default()` side effect because every construction path is now provider-explicit.

### Security and confinement

No new exposure; the changes remove a panic, a protocol mismatch, a resource-bound inconsistency, and a misleading error.

### Documentation and static guards

Update `docs/tls.md`, `docs/security-policy.md`, `docs/deployment.md`, `docs/cli.md`, `docs/timeout-reference.md`, `architecture/eggnet-tls.md`, and `CHANGELOG.md`.

## 7. Ordered work packages

### Work package A - Provider determinism

Intent: configuration construction never panics because of an unrelated provider in the graph.

Required changes: `eggnet-tls` helper plus three builder call sites; H3 QUIC builder; remove the Python global install.

Acceptance evidence: a unit test builds an identity and a client-auth verifier without installing a process default, in a test binary where both providers are enabled.

### Work package B - ALPN/runtime coherence

Intent: contradictory configuration is rejected before serving.

Required changes: `RuntimeConfig::validate` checks the effective ALPN set for h2 against `http2.enabled` and the compiled feature.

Acceptance evidence: builder tests for static and reload configurations; the error names the contradiction.

### Work package C - CLI timeout truth

Intent: the documented security bound is real.

Required changes: `Args::limits()` sets both timeouts; usage text and docs updated.

Acceptance evidence: an args test asserts both limits equal the flag value.

### Work package D - Identity cap

Intent: the documented named-identity ceiling is order-independent.

Required changes: `add_identity` no longer counts the optional default; constant docs state the default is separate.

Acceptance evidence: 64 named identities build with a default configured first; a 65th add fails with `TooManyIdentities`.

### Work package E - H3 transport bounds and typing

Intent: out-of-range QUIC settings fail with a field-named transport error.

Required changes: `Http3Config::validate` bounds windows and idle timeout to the QUIC VarInt range; `quic.rs` uses `InvalidTransportConfig`.

Acceptance evidence: unit tests for each field; the error message does not say private key.

## 8. Failure, cancellation, restart, and contention semantics

Static validation fails `Server::build`/`start`. A reload that introduces an h2 advertisement on an H2-disabled runtime is rejected on the next connection validation (fail closed); established connections are unaffected. **Correction (milestone 322):** the TLS accept path rejects the h2 negotiation before the per-connection validator runs, so 322 enforces the check at `ServerHandle`'s reload swap. Provider selection is deterministic and stateless, so concurrent construction and reload are unchanged. The identity-cap change does not alter handshake behavior.

## 9. Compatibility and migration

- `TlsError` gains two variants. The crate is 0.x and this is a diagnostic correction; no version bump is part of this milestone.
- `--header-timeout` now also bounds TLS handshakes, matching the already-published security policy. Operators who relied on the 10-second handshake budget while lowering the header timeout will get the configured (smaller) value.
- Python's process-global provider installation is removed; the observable TLS behavior is unchanged because the substrate uses ring explicitly.
- No configuration field is added or removed.

## 10. Required tests

### Focused unit tests

- `eggnet-tls`: provider-explicit build with client auth and no installed default; identity-cap order independence.
- `eggserve-bin`: `--header-timeout` sets both limits.
- `eggserve-h3`: window and idle-timeout upper bounds (and zero/unbounded messages).

### Integration tests

- `eggserve-core/tests/tls_identity.rs`: h2 ALPN with `http2.enabled=false` rejected for both `tls_config` and a reload handle.

### Contention and cancellation tests

Not applicable; no new concurrency primitive. Reload all-or-nothing semantics stay covered by the existing tests.

### Security and negative tests

Provider test runs with both rustls providers enabled in the test graph; ALPN negative tests fail closed; H3 bounds reject `u64::MAX`.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p eggnet-tls
cargo test -p eggserve-h3
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --features tls --test tls_identity
cargo test -p eggserve-core --features http2,tls --test tls_identity
cargo clippy -p eggnet-tls -p eggserve-h3 -p eggserve-core -p eggserve-bin --all-targets --features http2,tls -- -D warnings
python3 scripts/check-crate-topology.py
./scripts/verify.sh fast
```

Python crate: `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` where the environment allows it.

## 12. Documentation updates

- `docs/tls.md`: ALPN/runtime validation; handshake timeout surface; provider determinism.
- `docs/security-policy.md`: keep the header-timeout statement now that it is true.
- `docs/deployment.md`: direct-TLS timeout note.
- `docs/cli.md`, `docs/timeout-reference.md`: flag semantics.
- `architecture/eggnet-tls.md`: explicit provider and runtime ALPN validation.
- `CHANGELOG.md`: Unreleased entry.

## 13. Acceptance criteria

1. `load_tls_config`, `TlsServerConfigBuilder::build`, and `load_quic_server_config` succeed with both rustls providers compiled and no process default installed.
2. A configuration advertising `h2` while HTTP/2 is disabled or unavailable is rejected by `RuntimeConfig::validate`/`Server::build`, for both `tls_config` and a reload handle, with a message naming the contradiction.
3. `--header-timeout N` produces `header_read_timeout == tls_handshake_timeout == N`, and the docs name that surface.
4. With a default identity configured first, 64 named identities are accepted and a 65th is rejected; the reverse order behaves identically.
5. Out-of-range QUIC windows/idle timeout are rejected by `Http3Config::validate` with the field named, and no transport-bound failure is reported as `invalid private key`.

## 14. Stop conditions

Stop and report if: a fix requires changing TLS defaults or capability tiers; the provider change requires a new dependency; the Python crate cannot be checked in this environment; topology ownership would move; or a test cannot be made deterministic without weakening an invariant.

## 15. Closure evidence required

Exact commands and outcomes for the focused and broad gates, the before/after regression evidence for each finding, docs diff, and a statement of anything not run (for example the Python crate check).

## 16. Handoff notes

The checkout has no `target/` directory, so the first build is cold. `eggserve-python` is excluded from the workspace and has its own lockfile. Tests must not rely on a previously installed process default: the provider regression test deliberately does not install one. Preserve unrelated user changes; the worktree is clean at the baseline.
