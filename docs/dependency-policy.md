# Dependency Policy

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
| TLS | `rustls-pemfile` (optional, feature-gated) | PEM certificate and key parsing |
| HTTP client TLS | `webpki-roots` (optional, behind `client-tls`) | Mozilla CA root certificates for TLS verification |

## Notes

- The first milestone (plan 000) added initial dependencies
- Plan 001 adds HTTP substrate dependencies (`tokio`, `hyper`, `hyper-util`, `http-body-util`, `bytes`) to both crates. Manual argument parsing was adopted instead of `clap`.
- Plan 003 adds streaming/date/compile-time-map dependencies (`futures-util`, `httpdate`, `phf`) for file serving, Last-Modified headers, and MIME type detection.
- Plan 009 adds optional TLS dependencies (`rustls`, `tokio-rustls`, `rustls-pemfile`) behind the `tls` feature flag in `eggserve-bin`. The default build remains TLS-free.
- Plan 028 adds optional HTTP client dependencies behind the `client` feature flag in `eggserve-core`. Reuses `hyper` and `hyper-util` (already non-optional) with `client`/`client-legacy` features. Adds `rustls`, `tokio-rustls`, `webpki-roots` behind the `client-tls` feature. Default build remains server-only.
- No dependency is added without updating this document
- `cargo audit` and `cargo deny` are run as part of the beta release gate (see [release-criteria.md](release-criteria.md))

## Accepted maintenance-risk dependencies

- `rustls-pemfile` (optional, behind the `tls` feature) is flagged as
  unmaintained by `cargo audit` under `RUSTSEC-2025-0134`. The crate is
  still in the official `rustls` GitHub organization and is the supported
  PEM parser consumed by `rustls` consumers; there is no published
  drop-in replacement that integrates with the in-tree `rustls 0.23`
  version pinned by `eggserve-bin`. We accept the risk because:
  - It is only pulled in when the `tls` feature is enabled
  - It is a small parser with a narrow surface (PEM → DER)
  - It is not on a network or authentication code path
  - The risk is tracked and re-evaluated each release via `cargo audit`

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

CI enforces dependency policy on every push and pull request:

- **`cargo audit`** — checks for known vulnerabilities in dependencies.
- **`cargo deny check`** — checks licenses, bans, sources, and advisory databases.

Both run in the `supply-chain` CI job (see `.github/workflows/ci.yml`) after
the pinned installer. The release workflow (`.github/workflows/release.yml`)
uses the same installer and runs both as a gate before artifact staging and
publication.

The `audit.toml` at the workspace root configures `cargo audit` defaults. The `deny.toml` configures `cargo deny`.
