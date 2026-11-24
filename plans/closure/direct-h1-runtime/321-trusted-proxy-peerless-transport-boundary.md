# Direct H1 Runtime Milestone 321 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/321-trusted-proxy-peerless-transport-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md`

Repository baseline reviewed: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

Implementation state: the change is in the working tree. No commit was created
by this task.

## 1. Executive finding

Forwarded-header trust is now a function of the accepting transport, not of
whether a socket address happens to be present.

- `eggserve-server::connection::PeerTransport` (`Socket` / `Unix` / `Opaque`)
  carries the accepting owner's knowledge. Its `trusts_forwarded` method is the
  single mapping: socket peers use the explicit IP/CIDR policy, runtime-owned
  Unix listeners use `trust_unix`, and opaque caller-owned streams are never
  trusted.
- The direct H1 pipeline calls that mapping; the compatibility H2 pipeline
  calls the same method. Neither re-derives trust from `remote_addr` absence.
- The direct `Server` accept loop passes `Socket`. The compatibility
  `accept_loop_multi` passes `Socket` for TCP/TLS and `Unix` for the
  runtime-owned Unix listener. Every public caller-owned entry point passes
  `Opaque`, including when the caller supplies `ConnectionContext::for_unix()`.
- No `ConnectionContext` field was added. The transport kind travels through
  internal driver/pipeline parameters; the only new public surface is the
  `#[doc(hidden)]` plumbing the compatibility crate needs to name it
  (`PeerTransport`, `serve_http1_connection_with_id_and_transport`).

TCP/TLS/QUIC peer policy, PROXY preambles, raw peers, canonical `Host`, the
request target, and H3 (which does not consume the forwarded policy) are
unchanged.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| Internal transport kind set by the transport owner | met | `connection/peer_transport.rs`; accept loops set it; public drivers pass `Opaque` |
| TCP/TLS/QUIC → `Socket` | met | `eggserve-server/src/lib.rs` accept task; `eggserve-core/src/server/accept.rs` TCP/TLS sites |
| Unix listener → `Unix` (direct and compatibility, including systemd-adopted listeners) | met | compatibility `handle_unix_accept`; systemd adoption feeds the same accept loop |
| Public caller-owned entry points → `Opaque` | met | `serve_http1_connection*` wrappers; compatibility `serve_http_connection*` wrappers |
| One trust mapping, no second boolean implementation | met | `PeerTransport::trusts_forwarded` called by both pipelines |
| TCP/PROXY behavior preserved | met | Existing 35-test trusted-proxy suite stays green |
| Trust-matrix tests, including a real Unix listener and spoofed caller-owned streams | met | 3 new compatibility tests + 2 new direct tests + 3 unit tests |
| Documentation and migration note | met | §7 |
| No public `ConnectionContext` field without a semver decision | met | No field added; doc-hidden plumbing recorded below |
| H3 unchanged | met | H3 adapter does not call the forwarded policy |

## 3. Implementation evidence

### 3.1 Trust mapping

```rust
// crates/eggserve-server/src/connection/peer_transport.rs
match self {
    Self::Socket => remote_addr.is_some_and(|peer| config.is_trusted_peer(&peer)),
    Self::Unix => config.trust_unix,
    Self::Opaque => false,
}
```

### 3.2 Wiring

| Site | Kind |
|---|---|
| `eggserve-server` direct TCP accept task | `Socket` |
| `eggserve-core` `handle_tcp_accept` (plaintext, PROXY-replayed, TLS, TLS-with-preamble) | `Socket` |
| `eggserve-core` `handle_unix_accept` | `Unix` |
| `serve_http1_connection`, `_with_id`, `_with_policy` (server) | `Opaque` |
| `serve_http1_connection`, `_with_id`, `serve_http_connection`, `_with_id` (core) | `Opaque` |
| `serve_connection_with_runtime_state` (TCP-shaped compatibility helper used by tests) | `Socket` |

No public `ConnectionContext` field was added. `PeerTransport` and
`serve_http1_connection_with_id_and_transport` are public only because the
compatibility crate lives across the crate boundary; both are
`#[doc(hidden)]`, documented as internal plumbing, and are additive. The
semver decision is therefore recorded: no breaking change, no external struct
literal is affected, and no stable API is implied.

### 3.3 Behavior before/after on a caller-owned stream

With `trust_unix = true`, `Forwarded: for=203.0.113.9`:

- Before: `effective_client` became `203.0.113.9`.
- After: `effective_client` stays absent, `forwarded_provenance` is `None`,
  `forwarded_rejected` increments, and raw peer/canonical `Host` are unchanged.

The direct `forwarded_transport_boundary` test asserts exactly that for both
`for_non_socket` and `for_unix` contexts. A runtime-owned Unix listener with
the same header and `trust_unix = true` still adopts `203.0.113.9`
(compatibility test), and the same listener with `trust_unix = false` ignores
it.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p eggserve-server --lib` | PASS — 92 passed, 1 ignored |
| `cargo test -p eggserve-server --test forwarded_transport_boundary` | PASS — 2 |
| `cargo test -p eggserve-core --test trusted_proxy` | PASS — 38 (35 existing + 3 new) |
| `cargo test -p eggserve-core --lib` | PASS — 161 |
| `cargo test -p eggserve-core --test cross_protocol_conformance` | PASS — 17 |
| `cargo test -p eggserve-core --test direct_h1_parity` | PASS — 16 |
| `cargo clippy --workspace --lib --bins --tests -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

## 5. Invariant review

| Invariant | Result |
|---|---|
| No implicit trust | Held; absence of a socket address is never trust |
| Canonical request data never rewritten | Held; raw peer, `Host`, target unchanged in every test |
| PROXY trust separate and unchanged | Held; no PROXY code path touched |
| One trust decision | Held; both pipelines call one method |
| H3 stays out | Held |
| No public field without a semver decision | Held; decision recorded in §3.2 |

## 6. Failure and recovery review

- An unknown/missing transport kind is `Opaque` at every construction site; the
  default direction is closed.
- A Unix accept path that could not identify its listener would fail closed;
  no such path exists at this baseline (the accept loop owns the kind).
- Concurrent requests on one connection share the connection's kind; no
  per-request mutation.
- Shutdown/cancellation semantics are untouched.

## 7. Documentation and migration

| Surface | Change |
|---|---|
| `crates/eggserve-server/src/config.rs` | `trust_unix_local` scope: runtime-owned Unix listeners only |
| `crates/eggserve-primitives/src/primitives/proxy.rs` | `trust_unix` field scope |
| `docs/threat-model.md` | Forwarding-spoof paragraph names the transport-kind boundary |
| `docs/python-api.md` | `trust_unix_local` never applies to caller-owned streams |
| `docs/migration-guide.md` | Plan 321 section: behavior change and migration |
| `CHANGELOG.md` | Unreleased entry |

## 8. Compatibility and residual risk

Embedders that relied on `trust_unix` to adopt forwarding metadata on a
caller-owned stream lose that implicit trust. This is the intended correction:
the runtime did not accept that peer. No replacement implicit opt-in was added;
an explicit caller-owned trust surface would need its own reviewed plan.

The Python facade passes the same native flag through; runtimes with a Unix
listener are unaffected and no Python signature changed.

## 9. Unresolved findings

- The compatibility H2 branch over a Unix listener is wired (`Unix` reaches the
  H2 pipeline) but not exercised by a live H2-over-Unix test; the shared
  mapping and the H1 wire tests cover the decision. This is reported, not
  fixed; an H2-over-Unix client fixture would be a separate test investment.

## 10. Roadmap disposition

`plans/subsystems/direct-h1-runtime-roadmap.md` Milestone 16 (plan 321) is
closed. No publication is implied; the additive doc-hidden surface is not a
stable contract.

## 11. Registry updates

`plans/registry.md` row 321 moves to `closed` with a pointer to this record;
the direct-H1 roadmap milestone table and status rows are updated to match.
