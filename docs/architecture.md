# Architecture

## Workspace structure

```
eggserve/
├── Cargo.toml              # workspace root
├── crates/
│   ├── eggserve-core/      # library crate: security primitives
│   ├── eggserve-bin/       # binary crate: CLI entrypoint
│   └── eggserve-python/    # Python wheel packaging (maturin)
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
└── AGENTS.md
```

## Crate responsibilities

### `eggserve-core`

The core library crate. Contains security policy, path confinement, HTTP request handling, response construction, and telemetry. This crate must **not** depend on Python packaging concerns.

Modules:

| Module | Responsibility |
|--------|----------------|
| `config.rs` | Configuration types (`ServeConfig` with bind address, root directory, limits, policy) and `ServeState` (runtime state with file-stream semaphore) |
| `policy.rs` | Security policy types (`StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy`) |
| `limits.rs` | Resource limits (`Limits`: connection count, file streams, header/target/body sizes, timeouts, graceful shutdown) |
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
| `fs/mod.rs` | `RootGuard` — component-wise path resolution, canonical-root enforcement, per-component symlink/dotfile checks, `ResolvedResource` classification (`File`/`Directory`/`NotFound`/`Denied` with `SymlinkDenied`/`RootEscapeDenied` variants). Directory results carry the original safe components so index lookup can use the same resolver without ad hoc path joins. |
| `response.rs` | Response helpers: file streaming (`StreamBody`), directory listing HTML, error responses (400, 403, 404, 405, 413, 500, 503), MIME-typed headers |
| `mime.rs` | MIME type detection via extension lookup (`phf` map), ~60 common types, `application/octet-stream` fallback |
| `service.rs` | HTTP request handler: GET/HEAD dispatch, path validation, body-metadata validation (`Content-Length`/`Transfer-Encoding`), file-stream semaphore, filesystem resolution, index file handling via `RootGuard::resolve_child`, ETag generation, symlink-aware directory listing |
| `telemetry.rs` | Startup logging: bind address, root, methods, policies, enforced limits |

The core crate exposes a public API for path confinement, policy enforcement, and HTTP serving that can be used independently of the CLI. This is the foundation for safe HTTP/static-serving primitives.

### `eggserve-bin`

The CLI binary crate. Exposes a library interface (`lib.rs` with `pub fn run()`) and a thin binary entrypoint (`main.rs`). Handles manual argument parsing, configuration loading, TCP listener setup, connection limiting (semaphore), per-connection timeouts (header read, response write), signal handling (Ctrl+C, SIGTERM), and graceful shutdown. Contains the Hyper/Tokio HTTP accept loop. Depends on `eggserve-core` for request handling and response construction.

This crate is the entrypoint for `eggserve` as a command-line tool. It owns the process lifecycle: argument parsing, startup logging, binding, accept loop, and shutdown coordination. The library interface allows the Python package to call `run()` directly.

### `eggserve-python`

Python wheel packaging via maturin. Contains the Rust binary entrypoint (`src/main.rs` calls `eggserve_bin::run()`) and Python launcher code (`python/eggserve/_bin.py` locates and executes the binary via subprocess). The crate depends on `eggserve-bin` via path.

The Python package provides `pip install eggserve` and `python -m eggserve` entrypoints. All arguments are forwarded directly to the bundled Rust binary.

**Important:** The core crate must never depend on Python packaging. The Python package does not own serving logic.

## Design principles

1. **Separation of concerns** — security policy is in the core crate, CLI is in the bin crate, Python packaging is separate
2. **Core-first** — all security-critical logic lives in `eggserve-core` and can be used independently
3. **Core-first serving** — the HTTP substrate lives in `eggserve-core`; the binary crate only owns process concerns
4. **Minimal surface** — each crate exposes only what is necessary for its purpose
5. **Binary-not-library Python packaging** — the Python wheel contains the compiled binary, not a PyO3 binding
