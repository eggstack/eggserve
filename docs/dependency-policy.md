# Dependency Policy

The release smoke fixture uses only Python's standard library, and the existing
Hyper/Tokio transport remains the sole file-stream conversion boundary.

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
| HTTP server | `hyper`, `hyper-util`, `http-body-util` | HTTP protocol handling |
| Buffer types | `bytes` | Efficient byte buffer management |
| Streaming | `futures-util` | Async stream utilities for file streaming bodies |
| Date formatting | `httpdate` | HTTP date formatting for Last-Modified headers |
| Compile-time map | `phf` | Perfect hash function map for MIME type lookup |
| CLI parsing | manual (no clap) | Manual argument parsing in `eggserve-bin` |
| Error derive | `thiserror` | Derive macro for Error types |
| Python bindings | `pyo3` (optional, eggserve-python only) | PyO3 bindings for Python wheel |
| TLS | `rustls` (optional, feature-gated) | TLS termination |
| TLS | `tokio-rustls` (optional, feature-gated) | Async TLS stream wrapping |
| TLS | `rustls-pki-types` (optional, feature-gated) | PEM certificate and key parsing |
| Windows filesystem | `windows-sys` (optional, Windows-only, feature-gated) | Handle-relative filesystem operations for Windows hardening |

### Tokio feature ownership

| Crate | Tokio features (production) | Notes |
|-------|---------------------------|-------|
| `eggserve-core` | `macros`, `net`, `time`, `fs`, `io-util`, `sync` | No `signal`, no `rt-multi-thread` in default |

| `eggserve-bin` | `macros`, `net`, `signal`, `time`, `sync` | Signal handling for graceful shutdown |
| `eggserve-python` | `rt-multi-thread`, `net`, `io-util`, `sync`, `time` | Python GIL scheduling requires multi-thread |

## Notes

- The dependency graph is intentionally small: `eggserve-core` owns the HTTP,
  runtime, filesystem, and MIME capabilities; the CLI and Python crates add
  only their frontend/runtime requirements.
- `tokio`, `hyper`, `hyper-util`, `http-body-util`, and `bytes` provide the
  HTTP/1 transport and body pipeline. Manual CLI parsing avoids a broad CLI
  framework dependency.
- `futures-util`, `httpdate`, and `phf` support streaming, HTTP dates, and the
  compile-time MIME map.
- TLS dependencies are optional and feature-gated. Windows filesystem support
  is likewise target-gated; platform-only dependencies do not enter the
  default Unix graph.
- Tokio features are owned narrowly: the core library does not enable signal
  handling or a multi-thread runtime; the CLI owns signals and uses a
  current-thread runtime, while Python enables a bounded multi-thread runtime
  for GIL scheduling.
- The default product is a hardened static server with reusable HTTP/security
  primitives; unused client and application-framework dependencies are not
  part of the default graph.
- No dependency is added without updating this document
- `cargo audit` and `cargo deny` are run manually during release preparation (see `scripts/install-cargo-tools.sh`); they are not part of routine CI

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
cargo audit
cargo deny check
```

Routine CI intentionally does not run `cargo audit` or `cargo deny check`.
Maintainers install the pinned tools and run both checks during manual release
preparation (see `scripts/install-cargo-tools.sh`). The manually dispatched
release workflow builds, qualifies, and optionally publishes wheel artifacts;
it does not run the supply-chain audit tools.

The `audit.toml` at the workspace root configures `cargo audit` defaults. The `deny.toml` configures `cargo deny`.
