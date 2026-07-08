# Dependency Policy

## Rules

Every dependency must have an explicit purpose. The following rules apply to all dependencies:

- **No HTTP client stack for a server-only feature** — eggserve is a server, not a client
- **No web framework dependency in the initial milestones** — no actix-web, axum, warp, etc.
- **No templating dependency for generated directory listings** — directory listings use static HTML
- **No default TLS dependency before TLS milestone** — TLS is deferred; dependencies for it are deferred
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
| CLI parsing | `clap` | Command-line argument parsing |
| Logging | `tracing` (optional) | Structured logging |
| TLS | `rustls` (optional, deferred) | TLS termination |

## Notes

- The first milestone (plan 000) added only `clap` for CLI parsing in `eggserve-bin`
- Plan 001 adds HTTP substrate dependencies (`tokio`, `hyper`, `hyper-util`, `http-body-util`, `bytes`) to both crates. `clap` was removed in favor of manual argument parsing.
- Plan 003 adds streaming/date/compile-time-map dependencies (`futures-util`, `httpdate`, `phf`) for file serving, Last-Modified headers, and MIME type detection.
- No dependency is added without updating this document
- `cargo audit` and `cargo deny` are run as part of the beta release gate (see [release-criteria.md](release-criteria.md))
