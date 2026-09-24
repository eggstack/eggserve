# Plan 277 — Direct Tower adapter publication closure

**Disposition: PUBLICATION PENDING.** This record covers the release candidate
and local package evidence. The direct adapter is not yet available from
crates.io; publication and registry-only post-publication checks remain open.

## Candidate and registry baseline

- Planning baseline: `5c41141`.
- Registry queried with `cargo info --registry crates-io` on 2026-09-24:
  `eggserve-server 0.2.1`, `eggserve-core 0.2.2`.
- Selected synchronized candidate: `0.2.3`.
- Source/API publish set: `eggserve-server` (new opt-in adapter authority) and
  `eggserve-core` (feature forwarding and compatibility re-exports). Other
  workspace package implementations are unchanged. Source metadata remains
  synchronized across the workspace and excluded Python crate; no wheel
  release is included.
- Core now requires `eggserve-server = "0.2.3"`, so its published `tower`
  feature cannot resolve to a server release that lacks the forwarded feature.

## Plan 276 implementation

`eggserve-server` owns `interop` and `tower` behind opt-in features. Its
default graph remains Tower-trait-free, and the Tower graph has no core,
static, or PHF ancestry. `RequestBodyPolicy` is available from the server root.
Core's prior source paths and root Tower re-exports remain facades. The
behavioral suites now live under `crates/eggserve-server/tests/`; the core
feature checks include a small compatibility import smoke test.

The standalone fixtures are under
`release/fixtures/plan-276-direct-axum-consumer/` and
`release/fixtures/plan-276-core-axum-consumer/`. Both exercise the same Axum
request/response streaming, duplicate headers, middleware, disconnect
cancellation, prebound listener, and typed shutdown behavior. The direct
fixture has no direct primitives dependency.

## Local qualification

The following passed before push:

- Direct and core feature checks on Rust 1.89 for `http-interop` and `tower`.
- Direct server Tower clippy and tests; core Tower clippy and tests; core
  `http-interop` library tests.
- Standalone direct and core-shaped Axum fixtures from local path dependencies.
- `scripts/check-crate-topology.py` and its mutation self-tests.
- `bash scripts/install-cargo-tools.sh` and
  `bash scripts/check-supply-chain.sh` (both lockfiles audited; advisory,
  ban, license, and source checks passed; cargo-deny reported informational
  duplicate-version warnings).
- `ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all` staged
  all candidate packages in a temporary local registry, checked the required
  adapter source and test files in the archives, then built and tested both
  consumers against the staged artifacts.
- `cargo publish -p eggserve-server --locked --allow-dirty --dry-run` passed.
  The equivalent crates.io dry-run for core cannot resolve `eggserve-server
  ^0.2.3` until the server is published. The staged local-registry consumer
  check validates the exact core archive and feature forwarding in the
  required dependency order without publishing either package.

Local-registry comparison, same host/toolchain and release profile:

| Consumer | Lockfile packages | No-dev dependency nodes | Release executable bytes | Graph result |
|----------|------------------:|------------------------:|-------------------------:|--------------|
| Direct `eggserve-server/tower` | 45 | 111 | 579,584 | No core, static, or PHF family |
| Core compatibility `tower` | 62 | 155 | 647,112 | Static and PHF closure retained |

These figures describe only the two qualification fixtures and are not a
general binary-size or performance claim.

## Remaining closure gates

- Record the pushed candidate SHA and successful hosted CI run ID.
- Publish `eggserve-server 0.2.3` manually, wait for registry visibility, then
  publish `eggserve-core 0.2.3` manually.
- Record publication timestamps and crates.io checksums.
- Re-run both fixtures with exact crates.io-only dependencies (no path, git,
  or patch overrides). The direct graph must exclude core/static/PHF; the core
  fixture must compile the historical imports.
- Re-record registry graph and package/binary measurements from those exact
  published archives.

The requested repository commit and push do not constitute crates.io
publication. Until the remaining gates are recorded, Plan 277 remains
`PUBLICATION PENDING`.
