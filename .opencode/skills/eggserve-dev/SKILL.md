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

Other directories: `architecture/` (deep-dive docs), `docs/` (reference docs), `plans/` (000–125 historical/implementation records; Plans 112–118 form the consolidation roadmap; Plan 125 closes Windows qualification, support truthfulness, and final closure), `examples/`, `fuzz/`.

## Non-negotiables

1. **Safe defaults** — loopback bind, no symlinks, no dotfiles, no directory listing. Every unsafe behavior requires explicit opt-in via CLI flag.
2. **No serving outside root** — path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative traversal via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`.
3. **No broad dependencies** — every dependency must have an explicit purpose. See `docs/dependency-policy.md`.
4. **Plan-driven development** — every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

## CI validation sequence

Routine CI runs these in two concurrent jobs (`rust` and `python`):

```sh
# Rust job
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --lib --bins --tests -- -D warnings  # lint (warnings are errors)
cargo test --workspace                                     # tests
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                  # TLS tests

# Python job (via scripts/test-python-wheel.sh)
# Builds CLI, stages binary, builds wheel, installs in venv, runs smoke + tests
```

Or use the local verification script:

```sh
./scripts/verify.sh fast                 # routine dev check (Rust only)
./scripts/verify.sh full                 # pre-release validation (Rust + Python wheel)
./scripts/verify.sh deep                 # expensive suites (manual)
```

### Optional manual security/package checks

Not run in routine CI. Run manually when preparing a release:

```sh
bash scripts/install-cargo-tools.sh     # deterministic audit/deny installation
cargo audit                             # vulnerability check
cargo deny check                        # license/policy check
bash scripts/verify-cargo-packages.sh   # package dry-run gates
```

## Key conventions

- **Manual argument parsing** in `args.rs` — no clap dependency
- **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing) and `policy::DotfilePolicy` (serving). Both must agree.
- **eggserve-python excluded from workspace** — has its own Cargo.lock, built via maturin. Don't run `cargo test --workspace` for Python crate.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses
- **`#[allow(dead_code)]` on public API types** — consumed externally (Python bindings)
- **Error taxonomy** — Five distinct error types: `PathRejection` (16 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-only), `ServerError` (10 variants, server lifecycle), `ServiceError` (4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`), `RequestBodyError` (12 variants, body consumption). See `architecture/error-taxonomy.md`.
- **Plan status** — Plans 112–118 formed the consolidation roadmap (product surface simplification, dependency slimming, CI consolidation, timeout/taxonomy cleanup, Python distribution cleanup, documentation consolidation and roadmap closure). Plan 125 closes Windows qualification, support truthfulness, and final closure. Production servers use the shared `RuntimeState` admission pool.
- **Canonical HTTP types (stable)** — `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo`, `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response()` are all stable.
- **Canonical response semantics** — `StatusCode` accepts 100–599 only; 205 responses are body-forbidden; weak metadata ETags may satisfy `If-None-Match` but never `If-Range`; and the runtime adds exactly one authoritative `Date` header at final response construction. Python callback conversion stages headers and body ownership atomically; malformed body state never falls back to an empty response.
- **Canonical response normalization** — All response producers converge on `primitives::canonical::normalize_metadata()`.
- **`server` module types** — `eggserve-core::server` provides the runtime service boundary for embedding. The module is experimental; API may change.
- **RequestBody is one-shot** — `RequestBody` can only be consumed once. The `Service` trait's `call` method takes `Request` by value. Body policy defaults to `Reject`.
- **Body policy** — The policy is evaluated for the actual method; GET/HEAD/DELETE/OPTIONS/extension bodies are not globally rejected. TRACE content remains rejected. `StaticService` declares `Reject`; bodyless unsupported static methods receive 405, while body-bearing requests may be rejected by policy first.
- **Python RequestBody** — `RequestBody.read()` and `RequestBody.iter_chunks()` are mutually exclusive. `iter_chunks()` bridges async Rust body to synchronous Python via bounded channel with backpressure.
- **Structured logging** — `eggserve-core::ops` provides the event model (`Event`, `EventKind`, `Severity`, `Logger`, `LogSink`, `OpsCounters`). The CLI initializes with `StderrLogSink`. The Python `Server` delegates logging to the Rust runtime's stderr sink. Library crates must not use `println!`/`eprintln!` — use `Logger::global().emit()` instead.
- **Listener error classification** — Accept errors are classified by `io::ErrorKind` into transient/resource-exhaustion/persistent categories with bounded exponential backoff. Use `classify_accept_error()` helper.

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
- `runtime.md` — runtime service boundary, Server, Service trait, StaticService
- `security-model.md` — trust boundaries, defensive layers, attacker model
- `testing-and-conformance.md` — test layers, conformance corpora, fuzzing
- `configuration.md` — configuration inventory, ownership model, field inventory
- `structured-logging.md` — event model, event kinds, operational counters, log sinks
- `error-taxonomy.md` — five error layers, variant inventory, conversion flow
- `tls.md` — TLS support, feature gates, PEM loading
- `adr-002-windows-handle-relative-filesystem.md` — Windows handle-relative confinement design
- `adr-003-custom-service-ownership.md` — custom service ownership model

## Common pitfalls

- `telemetry.rs` is referenced in some older docs but does not exist — do not create it
- Range requests ARE implemented (despite some docs saying otherwise)
- `clap` was removed — manual arg parsing in `args.rs`
- `tracing` was never added — logging is custom
- Error taxonomy: `PathRejection` (16 variants, path validation), `RequestValidationError` (6 variants, HTTP-level), `ServerError` (10 variants, lifecycle), `ServiceError` (4 kinds), `RequestBodyError` (12 variants, body consumption)
- `BodyPlan` variants: `Empty`, `FullBytes(Vec<u8>)`, `FileFull`, `FileRange { start, end_inclusive }`
- `ResponseStatus` is a struct with associated constants, not an enum
- `FileRange` is a struct `{ start: u64, end_inclusive: u64 }`, not an enum
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`
- **`ResolvedFile` extraction methods** — `from_parts()`, `into_std_file()`, `into_parts()` are `pub` (for cross-crate Python bindings) but carry security caveats: confinement guarantee ends after extraction.
- **Python server façade** — `eggserve.server` is the supported six-class API, including rustls-backed `HTTPSServer` and `ThreadingHTTPSServer` with HTTP/1.1 ALPN only. Compatibility `""` binds normalize to `0.0.0.0`, literal wildcard binds are explicit façade opt-ins, and native activation publishes structured actual addresses. `eggserve.lowlevel` contains advanced primitives and `eggserve.subprocess` contains optional CLI lifecycle helpers. Stock `SimpleHTTPRequestHandler` with default settings bypasses Python per-request dispatch entirely; subclasses and non-default settings fall back to the Python callback path.
- **Python wheel support** — CPython 3.11+ with abi3 stable ABI. Routine CI builds and tests the Linux wheel; macOS and Windows wheels are built manually.
- **Semaphore bounds** — `max_connections` and `max_file_streams` are validated against `tokio::sync::Semaphore::MAX_PERMITS` in both `Limits::validate()` and `RuntimeConfigBuilder::build()`. Values above this bound are rejected with a controlled error.
- **Logging modes** — `--log-format none` uses `NopLogSink` (no output). `--quiet` wraps the format-specific sink with `FilteredLogSink` (warn/error only). Direct argument-validation errors printed before logger initialization may remain on stderr.
- **Release validation** — run `bash scripts/install-cargo-tools.sh` before `cargo audit`/`cargo deny check`.
- **`server` module is experimental** — `eggserve-core::server` provides the runtime service boundary. Its API is subject to change without notice.
- **Production profiles** — Production profiles are documented in README.md and `docs/deployment.md`. Every production claim must name a profile. Hardened profiles must not allow symlink following. Windows is functional-only until reparse hardening evidence passes.
- **`ops` module** — `Logger` uses `OnceLock` for global initialization. `try_init()` is for Python bindings that may coexist with CLI initialization. Do not call `Logger::init()` twice.
- **No println/eprintln in library code** — The core library must use `Logger::global().emit()` for all operational output.
