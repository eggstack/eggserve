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

The core library crate. Contains security policy, path confinement, HTTP request handling, response construction, and a public `StartupSummary` helper. This crate must **not** depend on Python packaging concerns.

#### Public API surface (alpha)

The crate divides its modules into three buckets. External callers should only depend on the first two:

| Bucket | Modules | Stability |
|--------|---------|-----------|
| Stable-ish | `config`, `limits`, `policy` | Field shapes may evolve before 1.0; breaking changes bump the major version |
| Experimental | `service` (`handle_request`) | Body type and async surface are not stable; may change in minor releases |
| Internal | `fs`, `path`, `response`, MIME detection, error taxonomy | Not part of the public API; crate-private (`pub(crate)`) |

`service::handle_request` returns `Response<BoxBody<Bytes, Infallible>>`. The body type alias is crate-private; embedding users that need it should reach in through the public `service` module and accept the type as an opaque body type from `hyper::body::Body`. The binary crate owns stdout policy: it imports `ServeConfig::startup_summary()` and prints the banner itself.

Modules:

| Module | Visibility | Responsibility |
|--------|------------|----------------|
| `primitives/` | `pub` | Public facade: re-exports `ConfinedPath`, `PathPolicy`, `PathRejection`, `PathDotfilePolicy` (path-level), `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy` (response-level), plus `SecureRoot`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory`, `ResourceDeniedReason` (secure root and resolution capabilities). Also contains `http.rs` (request validation: `ReadOnlyMethod`, `validate_method/body/target`), `response.rs` (planning types: `BodyPlan`, `HeaderMapPlan`, `StaticResponsePlan`), and `planner.rs` (conditional requests, range requests, ETag generation). Intended boundary for Rust/Python bindings. |
| `config.rs` | `pub` | `ServeConfig` (bind, root, limits, static policy), `ServeState` (runtime state with file-stream semaphore), `StartupSummary` (logging-friendly summary used by the binary to print the startup banner) |
| `policy.rs` | `pub` | Security policy types (`StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy`). `PolicyMode` is crate-private. |
| `limits.rs` | `pub` | Resource limits (`Limits`: connection count, file streams, header/target/body sizes, timeouts, graceful shutdown) |
| `service.rs` | `pub` (experimental) | HTTP request handler: GET/HEAD dispatch, path validation, body-metadata validation, file-stream semaphore, filesystem resolution, index file handling via `RootGuard::resolve_child`, ETag generation, symlink-aware directory listing. Body type is crate-private. |
| `error.rs` | `pub(crate)` | Error taxonomy (`RequestRejected`, `Io`) |
| `path/` | `pub(crate)` | Path confinement: request-target parsing, percent decoding, component validation, rejection types, dotfile/symlink policy, platform-specific checks |
| `path/mod.rs` | `pub(crate)` | `ConfinedPath` entry point — parse, validate, and classify request targets |
| `path/decode.rs` | `pub(crate)` | Single-pass percent decoding (rejects malformed encodings, NUL, invalid UTF-8) |
| `path/request_target.rs` | `pub(crate)` | HTTP origin-form parsing, query string stripping |
| `path/components.rs` | `pub(crate)` | Path normalization, component splitting, per-component validation |
| `path/rejected.rs` | `pub(crate)` | `PathRejection` enum — all path-level rejection reasons (parser and filesystem). `SymlinkDenied` and `RootEscapeDenied` are produced at the `fs/` layer. |
| `path/policy.rs` | `pub(crate)` | `PathPolicy` — dotfile and backslash policies for path validation |
| `path/platform.rs` | `pub(crate)` | Windows-specific checks (reserved names, ADS, drive prefixes) |
| `fs/` | `pub(crate)` | Filesystem confinement: root guard, resolved resource types, platform-specific traversal |
| `fs/mod.rs` | `pub(crate)` | `RootGuard` — canonicalizes and opens the configured root as a directory descriptor on Unix (per request), dispatches to platform-specific resolver, `ResolvedResource` classification (`File`/`Directory`/`NotFound`/`Denied(PathRejection)`). `ResolvedFile` carries pre-opened file handle (no re-open by absolute path). `ResolvedDirectory` carries dir fd on Unix. Each denial carries the specific `PathRejection` reason. |
| `fs/unix.rs` | `pub(crate)` | Unix descriptor-relative traversal: `resolve_fd_relative`, `resolve_child_fd`, `list_directory_fd`. Uses `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)` to deny symlinks at both stat and open time. |
| `response.rs` | `pub(crate)` | Response helpers: file streaming (`StreamBody`), directory listing HTML, error responses (400, 403, 404, 405, 413, 500, 503), MIME-typed headers |
| `mime.rs` | `pub(crate)` | MIME type detection via extension lookup (`phf` map), ~60 common types, `application/octet-stream` fallback |

The core crate exposes a public API for path confinement, policy enforcement, and HTTP serving that can be used independently of the CLI. This is the foundation for safe HTTP/static-serving primitives.

**Note:** `eggserve-core` is published to crates.io but is considered experimental/unstable for the alpha period. The public API surface is intentionally conservative and may change without notice before 1.0.

### `eggserve-bin`

The CLI binary crate. Exposes a library interface (`lib.rs` with `pub fn run()`) and a thin binary entrypoint (`main.rs`). Handles manual argument parsing, configuration loading, TCP listener setup, connection limiting (semaphore), per-connection timeouts (header read, response write), signal handling (Ctrl+C, SIGTERM), and graceful shutdown. Contains the Hyper/Tokio HTTP accept loop. Depends on `eggserve-core` for request handling and response construction.

This crate is the entrypoint for `eggserve` as a command-line tool. It owns the process lifecycle: argument parsing, startup logging, binding, accept loop, and shutdown coordination. The library interface allows the Python package to call `run()` directly.

### `eggserve-python`

Python wheel packaging via maturin. Contains PyO3 native bindings (`src/lib.rs`) that expose `eggserve-core` primitives (path parsing, policy, secure root, resolved resources, response planning) as `eggserve._native`, and Python wrapper code (`python/eggserve/`) that re-exports primitives alongside the subprocess lifecycle API (`ServeConfig`, `ServerProcess`, `serve_directory`).

The Python package provides two API layers:
1. **Native primitives** — Rust-backed path parsing, policy enforcement, secure root resolution, and response planning, available without launching the binary
2. **Subprocess lifecycle** — translates config objects to CLI arguments and manages the binary subprocess for full HTTP serving

The crate depends on `eggserve-core` (for primitives) via path. The `eggserve-bin` dependency was removed when the binary was decoupled from the Python package.

**Important:** The core crate must never depend on Python packaging. The Python package does not own serving logic.

## Design principles

1. **Separation of concerns** — security policy is in the core crate, CLI is in the bin crate, Python packaging is separate
2. **Core-first** — all security-critical logic lives in `eggserve-core` and can be used independently
3. **Core-first serving** — the HTTP substrate lives in `eggserve-core`; the binary crate only owns process concerns
4. **Minimal surface** — each crate exposes only what is necessary for its purpose
5. **Native Python bindings** — the Python package uses PyO3 to expose Rust primitives directly, with the subprocess API as a fallback for full HTTP serving
