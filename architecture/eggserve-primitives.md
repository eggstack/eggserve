# eggserve-primitives

`eggserve-primitives` is the Plan 211 dependency-free canonical leaf. It
contains application-facing HTTP method/version, validated headers, request
targets, owned request/response values, generic limits and error policy, and
trusted-proxy provenance values.

It intentionally has no Cargo dependencies. In particular, it does not pull
Hyper, Hyper-util, Tokio, rustls, Quinn/H3, or filesystem/platform crates.
Transport adapters and richer historical behavior remain in the
`eggserve-core::primitives` compatibility module for the 0.1 line.

Use this crate when an application needs canonical values without selecting a
server runtime or static-file implementation. Its direct dependency status is
checked by `scripts/check-crate-topology.py`.
