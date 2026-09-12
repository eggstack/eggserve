# `eggserve-h3` transport boundary

`eggserve-h3` is the dedicated Cargo boundary for EggServe's experimental
HTTP/3 and QUIC stack. It owns the coordinated direct production dependencies:

| Package | Version |
|---|---:|
| `h3` | 0.0.8 |
| `h3-quinn` | 0.0.10 |
| `quinn` | 0.11.11 |

The package intentionally has no dependency on `eggserve-primitives`,
`eggserve-server`, or `eggserve-static`. The 0.1 `eggserve-core` compatibility
facade consumes it only through the opt-in `http3` feature. As a result,
default, HTTP/1, and HTTP/2 package graphs do not compile the QUIC stack.

The current compatibility adapter remains in `eggserve-core::server::http3`
so the mature 0.1 API and its existing qualification suite stay unchanged.
Its raw H3/QUIC imports resolve through this package. A future semver cleanup
may move the adapter source into this crate after the small set of experimental
runtime seams it uses can be made public without duplicating canonical service
semantics.

## Boundary rules

- H3/QUIC versions are updated as one reviewed compatibility set.
- No H3 dependency is added to the canonical primitives or generic server
  packages.
- Canonical request, response, policy, timeout, and lifecycle behavior remains
  owned by EggServe's shared runtime; this package does not define application
  semantics.
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
