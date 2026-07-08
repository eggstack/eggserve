# Architecture

## Workspace structure

```
eggserve/
├── Cargo.toml              # workspace root
├── crates/
│   ├── eggserve-core/      # library crate: security primitives
│   └── eggserve-bin/       # binary crate: CLI entrypoint
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
└── AGENTS.md
```

`eggserve-python/` is not yet a Cargo crate. Python packaging is deferred to plan 005.

## Crate responsibilities

### `eggserve-core`

The core library crate. Contains security policy, path confinement, HTTP request handling, response construction, and telemetry. This crate must **not** depend on Python packaging concerns.

Modules:

| Module | Responsibility |
|--------|----------------|
| `config.rs` | Configuration types (`ServeConfig` with bind address, root directory, limits, policy) |
| `policy.rs` | Security policy types (`StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy`) |
| `limits.rs` | Resource limits (connection count, header size, request target size, timeouts) |
| `error.rs` | Error taxonomy (`Config`, `Bind`, `Runtime`, `RequestRejected`, `PathEscape`, `Io`) |
| `path/` | Path confinement: request-target parsing, percent decoding, component validation, rejection types, dotfile/symlink policy, platform-specific checks |
| `path/mod.rs` | `ConfinedPath` entry point — parse, validate, and classify request targets |
| `path/decode.rs` | Single-pass percent decoding (rejects malformed encodings, NUL, invalid UTF-8) |
| `path/request_target.rs` | HTTP origin-form parsing, query string stripping |
| `path/components.rs` | Path normalization, component splitting, per-component validation |
| `path/rejected.rs` | `PathRejection` enum — all path-level rejection reasons |
| `path/policy.rs` | `PathPolicy` — dotfile and backslash policies for path validation |
| `path/platform.rs` | Windows-specific checks (reserved names, ADS, drive prefixes) |
| `fs/` | Filesystem confinement: root guard, resolved resource types |
| `fs/mod.rs` | `RootGuard` — canonical-root enforcement, symlink/dotfile checks, `ResolvedResource` classification |
| `response.rs` | Response helpers: file streaming (`StreamBody`), directory listing HTML, error responses, MIME-typed headers |
| `mime.rs` | MIME type detection via extension lookup (`phf` map), ~60 common types, `application/octet-stream` fallback |
| `service.rs` | HTTP request handler: GET/HEAD dispatch, path validation, filesystem resolution, index file handling, ETag generation |
| `telemetry.rs` | Startup logging and policy display |

The core crate exposes a public API for path confinement, policy enforcement, and HTTP serving that can be used independently of the CLI. This is the foundation for safe HTTP/static-serving primitives.

### `eggserve-bin`

The CLI binary crate. Handles manual argument parsing, configuration loading, TCP listener setup, signal handling (Ctrl+C, SIGTERM), and graceful shutdown. Contains the Hyper/Tokio HTTP accept loop. Depends on `eggserve-core` for request handling and response construction.

This crate is the entrypoint for `eggserve` as a command-line tool. It owns the process lifecycle: argument parsing, startup logging, binding, accept loop, and shutdown coordination.

### `eggserve-python` (deferred)

Python packaging and `python -m` launcher. Not yet a Cargo crate. Packaging and the `python -m` entrypoint are deferred to plan 005. When implemented, this crate will handle:

- Python wheel packaging via maturin/PyO3
- `python -m eggserve` launcher
- Python-side configuration bridging

**Important:** The core crate must never depend on Python packaging. The Python package must initially not own serving logic.

## Design principles

1. **Separation of concerns** — security policy is in the core crate, CLI is in the bin crate, Python packaging is separate
2. **Core-first** — all security-critical logic lives in `eggserve-core` and can be used independently
3. **Core-first serving** — the HTTP substrate lives in `eggserve-core`; the binary crate only owns process concerns
4. **Minimal surface** — each crate exposes only what is necessary for its purpose
