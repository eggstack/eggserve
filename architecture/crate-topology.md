# Crate topology

Plans 211–213 introduced dependency layers while preserving the historical
`eggserve-core` 0.x source contract.

```text
eggnet-tls             (neutral rustls identity/trust/reload substrate)

eggserve-primitives   (canonical values; no dependencies)
          │
          ▼
eggserve-server       (generic HTTP runtime; Hyper/Tokio)
          │
          ▼
eggserve-static       (filesystem and static specialization)

eggserve-h3           (experimental Quinn/H3/H3-Quinn dependency boundary)

eggserve-core         (0.1 compatibility aggregate; legacy rich APIs)
   ├── eggserve-primitives
   ├── eggserve-server
   ├── eggserve-static
   ├── eggnet-tls (optional `tls` feature)
   └── eggserve-h3 (optional `http3` feature)
```

## Ownership

`eggserve-primitives` is the leaf crate. It owns dependency-free canonical
request, response, header, method/version, generic policy, limits, and proxy
provenance values. It must not acquire Hyper, Hyper-util, Tokio, TLS, QUIC, or
filesystem dependencies.

`eggserve-server` owns the generic transport boundary and application
`Service` contract. It may depend on the primitives crate and transport
dependencies, but never on `eggserve-core` or `eggserve-static`.

`eggserve-static` owns the filesystem specialization and depends on the two
lower layers. Static policy, root confinement, MIME selection, and static
service composition do not belong in the generic server crate.

`eggserve-core` remains an aggregate during the 0.1 compatibility window. Its
existing `eggserve_core::primitives` and `eggserve_core::server` paths retain
the mature rich implementation and behavior. The `eggserve_core::layers`
module exposes the new crates for migration experiments; new dependency-
sensitive consumers should name the direct leaf crate they need.

`eggnet-tls` is the neutral TLS security substrate. It depends only on
`rustls` and `rustls-pki-types` at runtime and owns bounded PEM parsing, SNI
identity selection, explicit WebPKI client authentication, trust/CRL bounds,
and atomic reload snapshots. It must not acquire EggServe, Eggress, EggFetch,
HTTP, proxy, tracing, Tokio, or QUIC dependencies. EggServe keeps only the
HTTP/3-specific QUIC configuration assembly in its compatibility facade and
re-exports the neutral API from `eggserve_core::tls`.

`eggserve-h3` owns the coordinated direct production dependencies on `h3`,
`h3-quinn`, and Quinn. Its public surface is deliberately limited to the
transport re-exports and version record needed by the experimental compatibility
adapter; it does not depend on the generic server, static, or primitives
crates. `eggserve-core` consumes it only behind `http3`, so the default and
HTTP/1/H2 core graphs do not compile the QUIC stack. The mature 0.1 adapter
remains in core while a future semver cleanup can move its source ownership
without changing canonical service semantics.

## Enforcement

Run `python3 scripts/check-crate-topology.py` to inspect Cargo metadata. The
check rejects forbidden direct dependencies in the primitives leaf and neutral
TLS crate, rejects
core/static edges from the generic server, and requires static to consume
primitives plus server. It is part of the Rust CI preflight and
`scripts/verify.sh fast`.

This is a dependency-topology boundary, not a line-count rule. The mature
compatibility implementation will be retired only through a separately
planned semver migration after downstream users have a direct-crate path.
