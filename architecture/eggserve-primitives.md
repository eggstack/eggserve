# eggserve-primitives

`eggserve-primitives` is the Plan 214 canonical leaf. It contains the extracted
application-facing HTTP method/version, validated headers, request targets,
owned request/response values, one-shot body/lifecycle types, generic limits
and error policy, and trusted-proxy provenance values.

Its only production dependencies are transport-neutral `bytes` and
`futures-util`; it does not pull Hyper, Hyper-util, Tokio, rustls, Quinn/H3, or
filesystem/platform crates. Hyper adapters remain compatibility/runtime glue;
the canonical model itself is owned here.

Use this crate when an application needs canonical values without selecting a
server runtime or static-file implementation. Its direct dependency status is
checked by `scripts/check-crate-topology.py`.
