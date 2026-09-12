# Crate topology

Plan 211 introduced three dependency layers while preserving the historical
`eggserve-core` 0.x source contract.

```text
eggserve-primitives   (canonical values; no dependencies)
          │
          ▼
eggserve-server       (generic HTTP runtime; Hyper/Tokio)
          │
          ▼
eggserve-static       (filesystem and static specialization)

eggserve-core         (0.1 compatibility aggregate; legacy rich APIs)
   ├── eggserve-primitives
   ├── eggserve-server
   └── eggserve-static
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

## Enforcement

Run `python3 scripts/check-crate-topology.py` to inspect Cargo metadata. The
check rejects forbidden direct dependencies in the primitives leaf, rejects
core/static edges from the generic server, and requires static to consume
primitives plus server. It is part of the Rust CI preflight and
`scripts/verify.sh fast`.

This is a dependency-topology boundary, not a line-count rule. The mature
compatibility implementation will be retired only through a separately
planned semver migration after downstream users have a direct-crate path.
