# Plan 290 — Direct H1 boundary-ownership qualification

Status: **local qualification complete; hosted CI pending**.

## Candidate and release decision

- Proof-bearing implementation commit: `dc39fef20dd658755ef268c4cd82916448fa3da1`.
- Hosted CI: run `36102738591` (pending when this record was prepared).
- Rust MSRV used by the local matrix: `rustc 1.89.0 (29483883e 2025-08-04)`.
- Resolved parser/runtime dependencies: `hyper 1.11.1`, `hyper-util 0.1.20`.
- Registry state before publication: server/core `0.3.0`, primitives `0.2.1`.
- Candidate package: `eggserve-server 0.3.1` only.

The public API change is additive: it adds
`ResponseMetadataOwnership` and ownership builder methods on
`H1ConnectionPolicy`, whose fields are private. Existing public runtime-limit
constants remain, while their former maxima are documented as conservative
guidance. Validation now preserves the Hyper buffer minimum and positive
header-count requirement without an EggServe upper ceiling. No public field
type or existing method signature changed. A compatible `0.3.x` patch is
therefore appropriate.

The existing `eggserve-core 0.3.0` requirement is `eggserve-server = "^0.3.0"`;
it can resolve server patch `0.3.1` without republishing core. The topology
gate now checks that compatibility relationship rather than requiring
synchronized package versions.

## Qualification evidence

The following passed locally before push:

- `./scripts/verify.sh fast`, including workspace tests, direct server/core
  tests, H2+TLS and H3+TLS matrices, clippy gates, feature checks, formatting,
  and the locked excluded-Python-crate check.
- `python3 scripts/verify-conformance-matrix.py`.
- `python3 scripts/check-crate-topology.py` and its self-tests.
- `python3 scripts/check-python-release-metadata.py`.
- `bash scripts/check-supply-chain.sh` against both lockfiles.
- `bash scripts/verify-cargo-packages.sh --mode all`, including the local
  registry compatibility consumer resolving core `0.3.0` with server `0.3.1`.
- `cargo fmt --all -- --check` and `git diff --check`.

Focused tests exercise actual H1 requests whose parsed footprint exceeds the
former 4 MiB bound and whose header count exceeds 10,000; external aggregate
header ownership with parser limits still enforced; and preservation of
service Date/Server values and absence over sequential requests. They also
cover invalid/duplicate Date fallback, denylist precedence,
Last-Modified/Date ordering, runtime error ownership, caller-owned TLS-H1, and
Tower/Axum streaming responses with duplicate application headers.

The direct dependency graph is owned by `eggserve-server`; the direct profile
does not introduce `eggserve-core`, `eggserve-static`, or PHF. H2's independent
`Http2Config.max_header_list_size` remains unchanged. Defaults remain the
existing 64 KiB parser buffer, 100 headers, 32 KiB aggregate header policy,
and EggServe-owned response metadata.

## Publication gate

This record does not claim publication or downstream unblock. Those claims
belong to Plan 291 and require green hosted CI for the proof-bearing SHA,
crates.io publication, and registry-only consumer evidence.
