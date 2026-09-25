# `eggserve-h3` — experimental H3/QUIC transport adapter

`eggserve-h3` is the sole home of EggServe's HTTP/3 and QUIC transport:
the coordinated production dependency set **and** the adapter
implementation (Plan 220 extraction from the compatibility core, Plan 213
dependency isolation). It remains experimental with the Plan 192–195
blockers intact; see [the H3 boundary](http3.md) for qualification detail.

## Dependency isolation

| Package | Version | Authority const |
|---|---:|---|
| `h3` | 0.0.8 | `H3_VERSION` |
| `h3-quinn` | 0.0.10 | `H3_QUINN_VERSION` |
| `quinn` | 0.11.11 | `QUINN_VERSION` |

Rules:

- The three QUIC versions move as one reviewed compatibility set; the
  triple is also asserted by `dependency_versions()` (`src/lib.rs`) and
  the `coordinated_dependency_versions_are_explicit` unit test.
- QUIC dependencies live **only** here. `eggserve-core` consumes them
  exclusively via `eggserve-h3` behind its optional `http3` feature
  (`http3 = ["tls", "dep:eggserve-h3"]`), so default, H1, and H2 graphs
  never compile h3/h3-quinn/Quinn (checked by `scripts/check-crate-topology.py`
  and the no-feature `cargo tree` probe in `scripts/qualify-http3.sh`).
- Downward-only layering: `eggserve-primitives <- eggserve-server <-
  eggserve-h3`. This crate may depend on `eggserve-primitives`,
  `eggserve-server`, and `eggnet-tls`; those crates never depend upward
  on H3. No dependency on `eggserve-core` / `eggserve-static` (no cycle,
  no second static implementation).
- `rustls` carries the workspace `0.23.45` caret floor (RUSTSEC-2026-0285)
  here as in every constraining manifest. Reusable PEM parsing stays in
  `eggnet-tls`; QUIC TLS assembly (TLS 1.3, `h3` ALPN, zero 0-RTT) stays
  here in `quic.rs`.

## Module inventory

| Module | Owned symbols / responsibility |
|---|---|
| `lib.rs` | Crate surface: `accept_loop` + `apply_alt_svc` re-exports, `Http3Config` re-export, `H3_VERSION` consts + `dependency_versions()`; `h3` / `h3_quinn` / `quinn` re-exported `#[doc(hidden)]` only (not stable app APIs) |
| `config.rs` | `Http3Config` authority: fields, defaults, `validate()` |
| `quic.rs` | `load_quic_server_config`, `server_endpoint`, `endpoint_from_socket`, `validate_same_port_udp`: QUIC TLS + endpoint construction, same-port validation |
| `endpoint.rs` | `ActiveConnectionGuard`, `h3_connection_close_reason`: endpoint lifecycle, close classification, peer/shutdown cancellation |
| `request.rs` | `convert_request_head`, `declared_content_length`, `h3_trailers_to_block`, `invoke_service`: H3→canonical conversion, length checks, trailer receive |
| `response.rs` | `runtime_error_response`, `spawn_body_timeout_watchdog`, `send_*` trio: error construction, `response_write_timeout` no-progress watchdog, data/trailer/known-length sends |
| `tunnel.rs` | `kind_string`, `H3ActiveTunnelGuard`, `send_h3_tunnel_handshake`: Extended CONNECT bridging |
| `adapter.rs` | `accept_loop`, `apply_alt_svc`: endpoint accept, connection/request dispatch, shutdown/drain over the shared kernel |

Generic invocation, panic containment, body policy, canonical
normalization, privacy finalization, and timeout accounting stay in the
shared kernel (`eggserve-server::connection`); this crate adds no second
service semantics. Generic runtime limits stay in
`eggserve-server::runtime_limits`; H3-only transport policy stays in
`Http3Config`; `RuntimeConfig` never gains Quinn types. H3 `Alt-Svc`
advertisement stays here (post-pass after generic finalization); the core
keeps only a feature-gated composition hook (Plan 253 ledger in
[crate-topology.md](crate-topology.md), no tier change).

## Adapter authority vs core facade

- Authority: `eggserve-h3::accept_loop` takes canonical types only
  (`RuntimeConfig`, `Http3Config`, `OpsContext`, admission semaphores,
  `ShutdownResult`, `Service`). Quinn/H3 transport types stay
  crate-internal or doc-hidden.
- Facade: `eggserve_core::server::http3::accept_loop` (`crates/eggserve-core/src/server/http3.rs`,
  `pub(crate)`, feature-gated) projects core `RuntimeConfig`/`RuntimeState`
  into that narrow API and shares the TCP admission pools (no second budget).
  `Http3Config` is a compatibility re-export;
  `eggserve_core::tls::load_quic_server_config` delegates to
  `eggserve-h3::load_quic_server_config` (no second QUIC/TLS impl).
- Startup (owned by core `Server`, executed by this adapter): TCP bind →
  QUIC config from `ServerBuilder::http3_identity` PEM paths → same-port
  UDP bind (or `http3_socket` prebound socket, same-port validated) →
  one supervisor over both accept loops. Port `0` resolves to one origin
  port; UDP failure returns before the server task starts.
- H3 requires its accompanying TCP listener so
  `ServerHandle::local_addr()` stays a truthful origin address; H3 is
  unavailable over Unix sockets. TCP TLS reload does not atomically
  rotate the QUIC endpoint (replacement/drain required).

## Qualification boundary

Machine-readable inventory:
[`conformance/http3_qualification.toml`](../conformance/http3_qualification.toml)
(`routine` = deterministic local; `manual` = tooling/environment-gated;
`blocked` = promotion blocker, never reported as pass).

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo test -p eggserve-h3
cargo test -p eggserve-core --features http3,tls
bash scripts/qualify-http3.sh
```

Deterministic: no-feature graph (no h3/h3-quinn/Quinn), coordinated
version set, canonical H3 runtime fixtures (bodyless dispatch, declared-
length edge cases, sibling survival, shutdown-race drain, write-stall
observability/permit-release), lifecycle regressions. Manual or blocked:
independent-client(s), adversarial wire, impairment, browser, and
cross-platform evidence; upstream `hyperium/h3#338` plus the `#262`
stream-drop remainder (see [http3.md](http3.md) and the Plan 192–195 /
213 release records). A promotion claim needs a new scoped plan.

## Links

- Boundary + evidence trail: [http3.md](http3.md)
- TLS substrate: [eggnet-tls.md](eggnet-tls.md), [tls.md](tls.md)
- H2 counterpart (core glue, no extraction): [http2.md](http2.md)
- Topology ledger: [crate-topology.md](crate-topology.md)
- Normative TLS operator contract: [../docs/tls.md](../docs/tls.md)
- Release records: `release/plan-192-http3-dependency-readiness.md`,
  `release/plan-193-http3-supported-tier-qualification.md`,
  `release/plan-194-http3-producer-timeout-correction.md`,
  `release/plan-195-http3-response-timeout-corrective-qualification.md`,
  `release/plan-213-http3-quic-isolation-qualification.md`,
  `plans/220-http3-adapter-extraction.md`
