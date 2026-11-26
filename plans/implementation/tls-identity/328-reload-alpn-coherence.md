# TLS Identity Milestone 328 - Reload ALPN coherence

Status: implemented

Repository baseline: `26649bd` plus the milestone 327 working-tree changes (branch `plan-320-public-bind-acknowledgment-hardening`; nothing committed by the agent)

Source roadmap:

- `plans/subsystems/tls-identity-roadmap.md#7` (Milestone 328)

Long-term requirements:

- `plans/000-long-term-specification.md#2` (neutral `eggnet-tls` ownership)
- `plans/000-long-term-specification.md#3` (invariants)
- `plans/000-long-term-specification.md#4` (TLS identity remains experimental)
- `plans/001-terminology-and-domain-model.md#subsystem-names-stable` (subsystem vocabulary)

Applicable ADRs:

- none

Primary class: invariant

## 1. Objective

Make milestone 327's ALPN/runtime guarantee true at the server-owned reload swap: a running server must not accept a TLS snapshot that advertises `h2` when its runtime cannot serve HTTP/2.

## 2. Why this milestone is ready

327 already added the shared h2 advertisement test and the build/start validation. A security review of that change found the remaining path: `ServerHandle::replace_tls_config` wrote the snapshot into the reload handle without any runtime-coherence check, and the per-connection validator the 327 records relied on is not reached for an h2-negotiating client. The fix is local to `eggserve-core` and needs no new dependency.

## 3. Current implementation evidence

- `crates/eggserve-core/src/server/handle.rs`: `replace_tls_config` wrote the snapshot through `TlsReloadHandle::replace` with no capability check.
- `crates/eggserve-core/src/server/accept.rs`: `accept_tls` completes the handshake and returns `None` when the negotiated ALPN is `h2` and `h2_enabled` is false, before the HTTP driver runs.
- `crates/eggserve-core/src/server/connection/mod.rs`: the per-connection `config.validate()` lives inside `serve_http_connection_with_id_and_protocol`, so an h2-negotiating client never reaches it.
- `plans/closure/tls-identity/327-tls-substrate-corrective.md`: section 6 claimed the per-connection validator caught a contradictory reload; that holds only for clients that negotiate `http/1.1`.

## 4. Invariants that must not regress

- Reload stays all-or-nothing: a rejected swap leaves the live snapshot unchanged.
- Established sessions keep their session; new handshakes read the current snapshot atomically.
- The accept-loop ALPN rejection stays as defense in depth.
- The error names the contradiction and never includes key or certificate bytes.
- The neutral `eggnet-tls` surface is unchanged.

## 5. Scope

### In scope

- Capture the runtime's HTTP/2 capability on `ServerHandle` at start.
- Reject an h2 advertisement at the server-owned reload swap.
- Share the ALPN predicate between build/start validation and the swap.
- Regression test, user docs, and planning records.

### Explicitly out of scope

- Making the neutral `TlsReloadHandle` runtime-aware.
- Removing or changing `ServerHandle::tls_reload_handle`.
- New CLI/Python flags or changed reload semantics.

## 6. Required production changes

- `RuntimeConfig::tls_advertises_h2` becomes `pub(crate)` and serves both validation points.
- `ServerHandle` stores `tls_h2_enabled` (TLS builds), set from `runtime_config.http2.enabled` at start and false when the `http2` feature is absent.
- `replace_tls_config` returns `ServerError::Config` for an h2-advertising snapshot while HTTP/2 is disabled; the swap does not happen.
- The `tls_reload_handle` doc states that replacing through the raw handle bypasses the server check.

## 7. Ordered work packages

### Work package A - Capture and guard

Intent: the swap cannot install a protocol advertisement the accept loop cannot serve.

Required changes: handle field and constructor parameter, swap-time check, module wiring in `Server::start`.

Acceptance evidence: an h2 snapshot is rejected before the live snapshot changes.

### Work package B - Regression test

Intent: the pre-fix behavior fails deterministically.

Required changes: `tls_identity::reload_rejects_h2_alpn_when_http2_disabled`.

Acceptance evidence: the test fails because `replace_tls_server_config` returns `Ok` before the fix.

### Work package C - Documentation and records

Intent: the documented guarantee matches the enforced one.

Required changes: `docs/tls.md`, `CHANGELOG.md`, `architecture/eggnet-tls.md`, roadmap, registry, and a correction note in the 327 closure.

## 8. Failure, cancellation, restart, and contention semantics

A rejected swap returns `Err` and mutates nothing; the next handshake still sees the previous identity. A contradictory snapshot installed through the raw handle is still rejected after the handshake by the accept loop (fail closed, no request parsed). Existing connections are untouched, and the reload lock is unaffected by a rejection.

## 9. Compatibility and migration

- `replace_tls_config` now returns a configuration error for an input it previously accepted. The handle type is experimental and this is a correctness fix.
- No method is added or removed; `ServerHandle` is not constructible outside the crate.
- The neutral `eggnet-tls` API and the `TlsReloadHandle` semantics are unchanged.

## 10. Required tests

### Integration tests

- Start a server with `http2.enabled = false`, reject an h2 snapshot, prove the live HTTP/1.1 identity still serves, then accept a coherent reload.
- The existing `reload_changes_new_handshakes_not_established`, `alpn_parity_survives_reload`, `concurrent_reload_handshake_race_safe`, and `failed_reload_preserves_old` stay green.

### Feature-shape tests

`tls` without `http2` and `http2,tls` both compile and pass, because the conflict message is selected by feature.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p eggserve-core --features http2,tls --test tls_identity
cargo test -p eggserve-core --features tls --test tls_identity
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features tls --lib --tests -- -D warnings
cargo check -p eggserve-bin --features http2,tls
git diff --check
python3 scripts/check-crate-topology.py
```

## 12. Documentation updates

- `docs/tls.md`: ALPN guarantee at build/start and at the reload swap; the raw-handle bypass.
- `CHANGELOG.md`: the Unreleased ALPN bullet names the swap.
- `architecture/eggnet-tls.md`: compatibility-runtime validation includes the swap.
- `plans/subsystems/tls-identity-roadmap.md` and `plans/registry.md`: milestone 328.
- `plans/closure/tls-identity/327-tls-substrate-corrective.md`: correction note for the per-connection claim.

## 13. Acceptance criteria

1. A server with HTTP/2 disabled rejects an h2 snapshot at `replace_tls_config` / `replace_tls_server_config`, and the error names `http2.enabled`.
2. The live snapshot is unchanged after a rejected swap and HTTP/1.1 still serves.
3. A coherent reload is still accepted.
4. An HTTP/2-enabled server still accepts an h2 reload (existing parity test).
5. The `tls` and `http2,tls` shapes both compile and pass.

## 14. Stop conditions

Stop and report if the guard needs a new dependency or public type, if it cannot leave the live snapshot unchanged, or if enforcing it would require changing the neutral `eggnet-tls` reload API.

## 15. Closure evidence required

Exact commands and outcomes, the regression test name, the protected behavior (live snapshot, coherent reload), and the disclosed raw-handle bypass.

## 16. Handoff notes

The raw `TlsReloadHandle` remains a documented bypass; making the neutral handle capability-aware is deferred because `eggnet-tls` must stay protocol-neutral. The broad `./scripts/verify.sh fast` wrapper still stops at the pre-existing macOS `eggserve-server` failure recorded in the 327 closure.
