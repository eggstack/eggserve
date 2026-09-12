# Dependency Policy

The release smoke fixture uses only Python's standard library, and the existing
Hyper/Tokio transport remains the sole file-stream conversion boundary.

Plan 211 adds a checked crate topology. `eggserve-primitives` is the canonical
leaf and intentionally has no Cargo dependencies. `eggserve-server` owns
Hyper/Tokio transport and depends on primitives, never on static serving or
the compatibility aggregate. `eggserve-static` owns filesystem-specific
behavior and depends on primitives plus server. `eggserve-core` remains a
0.1 compatibility aggregate during migration. See
[`architecture/crate-topology.md`](../architecture/crate-topology.md).

## Rules

Every dependency must have an explicit purpose. The following rules apply to all dependencies:

- **No HTTP client stack without a plan** — HTTP client dependencies require an explicit plan and feature gate
- **No web framework dependency in the initial milestones** — no actix-web, axum, warp, etc.
- **No templating dependency for generated directory listings** — directory listings use static HTML
- **No default TLS dependency** — TLS dependencies are optional, behind the `tls` feature flag, and not included in the default build
- **Feature flags must isolate optional surfaces** — optional dependencies are behind feature flags
- **Security-critical parsing dependencies require review** — any dependency that handles HTTP parsing, path resolution, or encoding must be reviewed before adoption

## Initially allowed categories

The following dependency categories are approved for initial development:

| Category | Dependencies | Purpose |
|----------|-------------|---------|
| Async runtime | `tokio` | Event loop and async primitives |
| HTTP server | `hyper`, `hyper-util`, `http-body`, `http-body-util` | HTTP protocol handling; `http-body` is the `Body` trait for response-completion tracking (Plan 164) |
| Ecosystem interop | `http` (optional, `http-interop` feature) | Direct `http` message types for loss-aware canonical adapters (Plan 200); transitive via Hyper today, direct so public adapters do not rely on re-exports |
| Ecosystem interop | `tower-service`, `tower-layer` (optional, `tower` feature) | Minimal Tower `Service`/`Layer` traits for middleware/app adapters (Plan 200); full `tower` never required, never in default builds |
| Buffer types | `bytes` | Efficient byte buffer management |
| Streaming | `futures-util` | Async stream utilities for file streaming bodies |
| Date formatting | `httpdate` | HTTP date formatting for Last-Modified headers |
| Compile-time map | `phf` | Perfect hash function map for MIME type lookup |
| CLI parsing | manual (no clap) | Manual argument parsing in `eggserve-bin` |
| Error derive | `thiserror` | Derive macro for Error types |
| Python bindings | `pyo3` 0.29.2 (eggserve-python only) | PyO3 bindings for Python wheel; pinned above the current RustSec advisories affecting 0.24.x |
| TLS | `rustls` (optional, feature-gated) | TLS termination |
| TLS | `tokio-rustls` (optional, feature-gated) | Async TLS stream wrapping |
| TLS | `rustls-pki-types` (optional, feature-gated) | PEM certificate and key parsing |
| HTTP/3 transport | `h3`, `h3-quinn`, `quinn` (optional, `http3` feature) | Experimental HTTP/3/QPACK server semantics, Quinn Tokio QUIC transport, and the rustls QUIC crypto adapter; no default/H1/H2 graph impact |
| WebSocket interop fixture (dev-only) | `tokio-tungstenite` (dev-dependency, tests only) | Plan 199 Track I: proves generic tunnel handoff sufficient for downstream WS codec (handshake via EggServe, framing over `TunnelIo`); never enters production `eggserve-core`/`eggserve-bin` graphs |
| Windows filesystem | `windows-sys` (optional, Windows-only, feature-gated) | Handle-relative filesystem operations for Windows hardening |
| Unix syscalls | `rustix` (Unix-only: `fs` + `net`) | Descriptor-relative filesystem confinement plus socket-activation fd validation (`SOCK_STREAM`/`SO_ACCEPTCONN`/family); no service-manager crate in default or minimal builds |

### Tokio feature ownership

| Crate | Tokio features (production) | Notes |
|-------|---------------------------|-------|
| `eggserve-primitives` | none | Dependency-free canonical layer |
| `eggserve-server` | `macros`, `net`, `time`, `io-util`, `sync` | Generic transport runtime; no static edge |
| `eggserve-static` | none | Uses standard-library filesystem APIs through the server/primitives layers |
| `eggserve-core` | `macros`, `net`, `time`, `fs`, `io-util`, `sync` | No `signal`, no `rt-multi-thread` in default |

| `eggserve-bin` | `macros`, `net`, `signal`, `time`, `sync` | Signal handling for graceful shutdown |
| `eggserve-python` | `rt-multi-thread`, `net`, `io-util`, `sync`, `time` | Python GIL scheduling requires multi-thread |

## Notes

- The dependency graph is intentionally layered: `eggserve-primitives` owns
  canonical values, `eggserve-server` owns generic HTTP transport,
  `eggserve-static` owns filesystem/MIME behavior, and `eggserve-core` keeps
  the mature aggregate for 0.1 compatibility. The CLI and Python crates add
  only their frontend/runtime requirements.
- `tokio`, `hyper`, `hyper-util`, `http-body`, `http-body-util`, and `bytes` provide the
  HTTP/1 transport and body pipeline. Manual CLI parsing avoids a broad CLI
  framework dependency.
- `futures-util`, `httpdate`, and `phf` support streaming, HTTP dates, and the
  compile-time MIME map.
- TLS dependencies are optional and feature-gated. Windows filesystem support
  is likewise target-gated; platform-only dependencies do not enter the
  default Unix graph.
- H3 dependencies are optional and feature-gated behind `http3` (which also
  enables `tls`). The adapter keeps Quinn/h3 types internal, pins the selected
  versions in `Cargo.lock`, and is covered by the same `cargo audit`/`cargo deny`
  gates as the default graph. Plans 188, 190, 192, 193, 194, and 195 close with H3
  experimental because
  independent-client and adversarial QUIC evidence was unavailable on the
  qualification host; the manual script remains the release path. Plan 192
  freezes the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn
  0.11.11 / rustls 0.23.x) and closes `BLOCKED`: `hyperium/h3#338` has no
  released fix and the `hyperium/h3#262` stream-drop remainder is unresolved,
  so H3 stays experimental until a later readiness update (see
  `release/plan-192-http3-dependency-readiness.md`). Plan 193 retains the
  tier on the unchanged candidate; Plan 194 adds the per-stream
  producer-timeout correction without changing the tier, and Plan 195
  correctively qualifies that bound without changing the tier. No fork or vendored H3
  patch is permitted to force a supported label.
- Tokio features are owned narrowly: the core library does not enable signal
  handling or a multi-thread runtime; the CLI owns signals and uses a
  current-thread runtime, while Python enables a bounded multi-thread runtime
  for GIL scheduling.
- The default product is a hardened static server with reusable HTTP/security
  primitives; unused client and application-framework dependencies are not
  part of the default graph.
- No dependency is added without updating this document
- `cargo audit` and `cargo deny` run in the routine CI supply-chain job using
  the pinned versions from `scripts/install-cargo-tools.sh`. Because
  `eggserve-python` is excluded from the root workspace, CI invokes
  `scripts/check-supply-chain.sh`, which checks both `Cargo.lock` files and
  applies this same `deny.toml` to both manifests.

## Release validation tool versions

CI and release validation install these cargo subcommands from the checked-in
`scripts/install-cargo-tools.sh` script before invoking them. The versions are
deliberately pinned and the script fails if the installed executable reports a
different version.

| Tool | Version | Install command |
|------|---------|-----------------|
| `cargo-audit` | `0.22.2` | `cargo install cargo-audit --version 0.22.2 --locked --force` |
| `cargo-deny` | `0.19.0` | `cargo install cargo-deny --version 0.19.0 --locked --force` |

Run the shared installer locally with:

```bash
bash scripts/install-cargo-tools.sh
```

## Automated enforcement

`cargo-deny` is configured via `deny.toml` at the workspace root. It checks:

- **Advisories** — known vulnerabilities in dependencies
- **Licenses** — only permissive licenses allowed (MIT, Apache-2.0, BSD, ISC, Unicode-DFS-2016, Zlib)
- **Bans** — multiple versions of the same crate produce warnings
- **Sources** — only crates.io registry allowed; no git dependencies

To run locally:
```bash
bash scripts/install-cargo-tools.sh
cargo audit --version
cargo deny --version
bash scripts/check-supply-chain.sh
```

Routine CI runs the shared root/Python audit and deny checks in a dedicated,
self-contained supply-chain job. Maintainers can reproduce that job locally
with `scripts/install-cargo-tools.sh` followed by
`scripts/check-supply-chain.sh`. The release preflight repeats the same
closure check before building wheels.

The `audit.toml` at the workspace root configures `cargo audit` defaults. The `deny.toml` configures `cargo deny`.
