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
- `crates/eggserve-python/` — Python wheel packaging (maturin + PyO3, depends on eggserve-core; excluded from workspace; bundles the platform-native CLI binary)

Other directories: `architecture/` (10 deep-dive docs), `docs/` (reference docs), `plans/` (000–041 plus roadmap and closure documents), `examples/`, `fuzz/`.

## Non-negotiables

1. **Safe defaults** — loopback bind, no symlinks, no dotfiles, no directory listing. Every unsafe behavior requires explicit opt-in via CLI flag.
2. **No serving outside root** — path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative traversal via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`.
3. **No broad dependencies** — every dependency must have an explicit purpose. See `docs/dependency-policy.md`.
4. **Plan-driven development** — every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

## CI validation sequence

Run before pushing:

```sh
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --all-targets -- -D warnings      # lint
cargo test --workspace                                     # tests
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                  # TLS tests
cargo test -p eggserve-core --features client              # client feature tests
cargo test -p eggserve-core --test http_wire_correctness   # raw wire tests
cargo test -p eggserve-core --test http_primitives_integration  # HTTP integration
cargo test -p eggserve-bin --test production_path          # production path tests
cargo test -p eggserve-core --test corpus_replay           # fuzz corpus replay
cargo audit                                                # vulnerability check
cargo deny check                                           # license/policy check
```

## Key conventions

- **Manual argument parsing** in `args.rs` — no clap dependency
- **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing) and `policy::DotfilePolicy` (serving). Both must agree.
- **eggserve-python excluded from workspace** — has its own Cargo.lock, built via maturin. Don't run `cargo test --workspace` for Python crate.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses
- **`#[allow(dead_code)]` on public API types** — consumed externally (Python bindings)
- **Two error types** — `PathRejection` (16 variants, parsing) vs `Error` (top-level taxonomy). `RequestValidationError` for HTTP-level issues.
- **Plan status** — Plans 000–040 are complete; Plan 041 is the final release-gate closure pass. Verify release status from `docs/release-checklist.md`, not workflow YAML alone.

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
- **Python wheel support** — CPython 3.14 only (`>=3.14,<3.15`) on the Linux, macOS, and Windows wheel matrix. Builds require `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` with PyO3 0.24.2 and stage `eggserve-bin` under `python/eggserve/bin/` before maturin.
- **Release validation** — run `bash scripts/install-cargo-tools.sh` before `cargo audit`/`cargo deny check`, and `bash scripts/verify-cargo-packages.sh` for both Rust package gates. The release workflow's manual `dry_run=true` path must be executed and recorded before RC approval.
