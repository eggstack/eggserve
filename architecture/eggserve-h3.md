# `eggserve-h3` transport adapter

`eggserve-h3` is the implementation home of EggServe's experimental
HTTP/3 and QUIC transport adapter plus the coordinated direct production
dependencies:

| Package | Version |
|---|---:|
| `h3` | 0.0.8 |
| `h3-quinn` | 0.0.10 |
| `quinn` | 0.11.11 |

Plan 220 moves the adapter implementation here from the compatibility core.
The crate owns endpoint lifecycle, request conversion, response
streaming/trailers, Extended CONNECT tunnel bridging, shutdown/drain, QUIC
close classification, H3-only transport configuration (`Http3Config`), and
QUIC TLS/endpoint assembly. Canonical service semantics stay in
`eggserve-primitives` / `eggserve-server`; this crate depends downward on
those layers (`primitives <- server <- h3`) and never upward on
core/static. The compatibility core consumes the adapter behind its
optional `http3` feature as a thin facade, so default, HTTP/1, and HTTP/2
graphs do not compile the QUIC stack.

## Boundary rules

- H3/QUIC versions are updated as one reviewed compatibility set.
- Downward-only: `eggserve-h3` may depend on `eggserve-primitives`,
  `eggserve-server`, and `eggnet-tls`; those crates never depend upward on H3.
  No dependency on `eggserve-core`/`eggserve-static` (no cycle, no second
  static implementation).
- Narrow adapter API expressed in canonical `Service`, server `RuntimeConfig`,
  `OpsContext`, semaphores, and `ShutdownResult` plus H3-owned `Http3Config`;
  Quinn/H3 transport types stay crate-internal or doc-hidden.
- Generic runtime limits stay in `eggserve-server::runtime_limits`;
  H3-only transport policy stays here; `eggserve-server::RuntimeConfig`
  never gains Quinn types. QUIC TLS assembly stays here; reusable identity
  parsing stays in `eggnet-tls`.
- Canonical request, response, policy, timeout, and lifecycle behavior remains
  owned by the shared runtime; this package adds no second service semantics
  (generic finalization via `server::connection`, H3 `Alt-Svc` here).
- H3 stays opt-in and experimental. The package does not imply support for
  WebTransport or generic WebSocket-over-H3 when the selected `h3` release
  rejects that protocol before EggServe receives the request.

## Qualification

The machine-readable inventory is
[`conformance/http3_qualification.toml`](../conformance/http3_qualification.toml).
Routine checks include the no-feature dependency graph, the coordinated
version set, canonical H3 runtime fixtures, and sibling/lifecycle regressions.
Independent-client, adversarial-wire, impairment, browser, and cross-platform
evidence remains manual or blocked; see the
[Plan 213 release record](../release/plan-213-http3-quic-isolation-qualification.md).

## Qualification

The machine-readable inventory is
[`conformance/http3_qualification.toml`](../conformance/http3_qualification.toml).
The verifier distinguishes deterministic local checks from manual evidence and
known promotion blockers. Run the routine package/topology checks with:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo test -p eggserve-h3
cargo test -p eggserve-core --features http3,tls
bash scripts/qualify-http3.sh
```

Known upstream risks remain recorded in [`http3.md`](http3.md):
`hyperium/h3#338` (buffered data around connection failure) and the unresolved
`hyperium/h3#262` stream-drop/reset remainder. They continue to block a
hardened promotion claim.
