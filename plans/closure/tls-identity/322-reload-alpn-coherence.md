# TLS Identity Milestone 322 - Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tls-identity/322-reload-alpn-coherence.md`

Source subsystem roadmap:

- `plans/subsystems/tls-identity-roadmap.md#7` (Milestone 322)

Repository baseline reviewed: `26649bd` plus the milestone 321 working-tree changes (nothing committed by the agent)

Implementation commits or pull requests:

- not yet committed (working tree; commit is a user-owned action)

## 1. Executive finding

Milestone 321's ALPN/runtime guarantee is now enforced at the server-owned reload swap. A running server whose HTTP/2 capability is disabled rejects an h2-advertising TLS snapshot before it becomes live, so the accept loop no longer completes a handshake only to drop the connection for every h2-capable client. The regression test fails on the 321 behavior because `replace_tls_server_config` returned `Ok`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The reload swap rejects a contradictory snapshot | `tls_identity::reload_rejects_h2_alpn_when_http2_disabled` | pass | Pre-fix the swap returned `Ok` and the next h2 handshake completed before being dropped. |
| A rejected swap leaves the live identity in service | same test: HTTP/1.1 request after the rejected swap returns 200 | pass | The guard runs before `TlsReloadHandle::replace`. |
| A coherent reload is still accepted | same test: final `replace_tls_server_config(&http1)` | pass | Only the h2 advertisement is rejected. |
| H2-enabled servers still accept h2 reloads | existing `alpn_parity_survives_reload` | pass | Runtime capability is captured at start. |

## 3. Production implementation evidence

- `crates/eggserve-core/src/server/handle.rs`: `ServerHandle` captures `tls_h2_enabled`; `replace_tls_config` rejects an h2 advertisement before the swap with `ServerError::Config`; the `tls_reload_handle` doc records the raw-handle bypass.
- `crates/eggserve-core/src/server/mod.rs`: the capability is projected from `runtime_config.http2.enabled` at start (false when the `http2` feature is absent).
- `crates/eggserve-core/src/server/config/runtime.rs`: `tls_advertises_h2` is `pub(crate)` and shared by build/start validation and the swap check.
- `crates/eggserve-core/tests/tls_identity.rs`: `reload_rejects_h2_alpn_when_http2_disabled`.
- Documentation: `docs/tls.md`, `CHANGELOG.md`, `architecture/eggnet-tls.md`.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p eggserve-core --features http2,tls --test tls_identity
cargo test -p eggserve-core --features tls --test tls_identity
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features tls --lib --tests -- -D warnings
cargo check -p eggserve-core
cargo check -p eggserve-bin --features http2,tls
git diff --check
python3 scripts/check-crate-topology.py
```

### Results

- Core TLS identity with `http2,tls`: 19 passed, including `reload_rejects_h2_alpn_when_http2_disabled`.
- Core TLS identity with `tls` only: 16 passed; the h2-only regression is correctly not compiled.
- Full `eggserve-core` suite with `http2,tls`: exit 0 across every target.
- Clippy for `http2,tls` and for `tls` only: exit 0.
- Default `eggserve-core` check and `eggserve-bin` `http2,tls` check: exit 0 (the default check still prints the pre-existing Cargo `unused_dependencies` warning for `thiserror`).
- `cargo doc -D warnings` is not a repository gate; it reports pre-existing unresolved private-item links in untouched modules and none in this change.

## 5. Invariant review

- Reload remains all-or-nothing: the check precedes the swap, and the live `Arc<ServerConfig>` is untouched on rejection.
- Established connections keep their session; the accept-loop h2 rejection remains as defense in depth.
- The error is a fixed configuration category naming the contradiction; it carries no key or certificate bytes.
- The neutral `eggnet-tls` surface, TLS defaults, and capability tiers are unchanged.

## 6. Failure and recovery review

- A contradictory server-owned reload now fails at the call site and leaves the previous identity serving.
- A contradictory snapshot installed through the raw `TlsReloadHandle` is still dropped by `accept_tls` after the handshake; no request is parsed and the connection permit is released.
- The guard reads only captured state and the incoming snapshot; it adds no lock or shared mutable state.

## 7. Migration and compatibility review

- `replace_tls_config` can now return a configuration error for an input the 321 behavior accepted. The handle is experimental; this is a correctness fix with no signature change.
- No public item is added or removed. The neutral `TlsReloadHandle` API is unchanged.
- Operators replacing through `ServerHandle` get the guarantee; the raw-handle bypass is documented rather than removed.

## 8. Security review

- Closes the gap where a reload could make every h2-capable client complete a handshake (including client-certificate verification under mTLS) and then be dropped.
- The swap is checked before mutation, so a rejected snapshot cannot degrade the live listener.
- The remaining bypass requires the embedding application to ignore the documented accessor guidance; it stays fail-closed at the accept loop and cannot cause request smuggling or protocol confusion.

## 9. Documentation and operations

- `docs/tls.md` states the build/start and swap-time behavior and the raw-handle bypass.
- `architecture/eggnet-tls.md` includes the swap in the compatibility-runtime validation.
- `CHANGELOG.md` Unreleased ALPN bullet covers the swap.
- The 321 closure and implementation records carry a correction note pointing here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `ServerHandle::tls_reload_handle` still returns the neutral handle, whose `replace` bypasses the capability check | An embedding application that ignores the doc can install a contradictory snapshot | Accepted and documented; making the neutral handle runtime-aware is deferred to keep `eggnet-tls` protocol-neutral. |

## 11. Roadmap disposition

Milestone 322 is closed. The TLS identity roadmap's completion definition now includes the reload swap. No hard dependency blocks the next TLS identity or H2/H3 milestone.

## 12. Registry updates

- `plans/registry.md`: milestone 322 marked closed with this record; next milestone number 323.
- `plans/subsystems/tls-identity-roadmap.md`: milestone 322 marked closed.
- `plans/implementation/tls-identity/322-reload-alpn-coherence.md`: status implemented.
