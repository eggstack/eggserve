---
name: eggserve-dev
description: Use when working on eggserve code, plans, docs, or architecture. Covers Rust workspace conventions, plan-driven development, CI validation, security policy, and the three-crate layout.
---

# eggserve Development Skill

## Project identity

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction.

**Not** a general web server, framework, ASGI/WSGI runtime, or Granian replacement.

## Workspace layout

Three crates:
- `crates/eggserve-core/` — library: security primitives, path confinement, HTTP serving, response construction
- `crates/eggserve-bin/` — binary: CLI, accept loop, signal handling (depends on eggserve-core)
- `crates/eggserve-python/` — Python wheel packaging (maturin + PyO3, depends on eggserve-core; excluded from workspace)

Other directories: `architecture/` (10 deep-dive docs), `docs/` (21 reference docs), `plans/` (32 plans, 000–031 + ROADMAP), `examples/`, `fuzz/`.

## Non-negotiables

1. **Safe defaults** — loopback bind, no symlinks, no dotfiles, no directory listing. Every unsafe behavior requires explicit opt-in via CLI flag.
2. **No serving outside root** — path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative traversal via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`.
3. **No broad dependencies** — every dependency must have an explicit purpose. See `docs/dependency-policy.md`.
4. **Plan-driven development** — every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

## CI validation sequence

Run before pushing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

## Key conventions

- **Manual argument parsing** in `args.rs` — no clap dependency
- **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing) and `policy::DotfilePolicy` (serving). Both must agree.
- **eggserve-python excluded from workspace** — has its own Cargo.lock, built via maturin. Don't run `cargo test --workspace` for Python crate.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses
- **`#[allow(dead_code)]` on public API types** — consumed externally (Python bindings)
- **Two error types** — `PathRejection` (16 variants, parsing) vs `Error` (top-level taxonomy). `RequestValidationError` for HTTP-level issues.
- **Plan count** — Plans 000–031 complete. Verify against `plans/` directory.

## Architecture docs

The `architecture/` directory contains deep-dive docs for each subsystem:
- `overview.md` — workspace structure, data flow, architectural decisions
- `eggserve-core.md` — core library module map, key types, error taxonomy
- `eggserve-bin.md` — CLI binary, accept loop, signal handling
- `eggserve-python.md` — Python bindings, PyO3, maturin packaging
- `path-confinement.md` — path validation pipeline
- `filesystem-confinement.md` — SecureRoot, symlink-aware resolution
- `policy-system.md` — StaticPolicy, symlink/dotfile/listing policies
- `primitives-api.md` — public API boundary for embedding consumers
- `response-planning.md` — conditional/range/ETag response planning

## Common pitfalls

- `telemetry.rs` is referenced in some older docs but does not exist — do not create it
- Range requests ARE implemented (despite some docs saying otherwise)
- `clap` was removed — manual arg parsing in `args.rs`
- `tracing` was never added — logging is custom
- Error taxonomy: `PathEscape` is a unit variant, `PathNotAccessible(String)` takes a string, `Bind(String)` takes a string
- `BodyPlan` variants: `Empty`, `FullBytes(Vec<u8>)`, `FileFull`, `FileRange { start, end_inclusive }`
- `ResponseStatus` is a struct with associated constants, not an enum
- `FileRange` is a struct `{ start: u64, end_inclusive: u64 }`, not an enum
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`
- **Client is buffered-only** — `HttpClient` buffers full response in memory. Streaming is not yet supported.
- **`ResolvedFile` extraction methods** — `from_parts()`, `into_std_file()`, `into_parts()` are `pub` (for cross-crate Python bindings) but carry security caveats: confinement guarantee ends after extraction. External consumers should use `SecureRoot` resolution.
- **Python Server has runtime hardening** — connection semaphore, header/write timeouts, graceful shutdown, optional handler callback, callback concurrency limit. Parameters: `handler`, `public`, `max_connections`, `max_file_streams`, `max_python_callbacks`, `header_timeout_secs`, `write_timeout_secs`.
- **Python 3.14 build** — requires `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` env var for maturin builds (PyO3 0.24.2 doesn't natively support 3.14).
