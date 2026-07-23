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

Other directories: `architecture/` (14 deep-dive docs), `docs/` (reference docs), `plans/` (000–059 plus roadmap and closure documents), `release/` (criteria.toml), `conformance/` (shared corpora), `examples/`, `fuzz/`.

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
cargo test -p eggserve-core --test canonical_conformance  # canonical HTTP type conformance
cargo test -p eggserve-core --test canonical_wire_interop  # canonical wire interop
cargo test -p eggserve-core --test request_body_integration  # request body ingestion integration
cargo test -p eggserve-core --test request_body_wire  # request body wire tests
cargo audit                                                # vulnerability check
cargo deny check                                           # license/policy check
bash scripts/verify-cargo-packages.sh                      # package and publish dry-run gates
python3 scripts/check-contract-consistency.py              # contract consistency validation
python3 -m unittest scripts.test_corrective_tooling -v     # corrective baseline/finding tests
```

## Key conventions

- **Manual argument parsing** in `args.rs` — no clap dependency
- **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing) and `policy::DotfilePolicy` (serving). Both must agree.
- **eggserve-python excluded from workspace** — has its own Cargo.lock, built via maturin. Don't run `cargo test --workspace` for Python crate.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses
- **`#[allow(dead_code)]` on public API types** — consumed externally (Python bindings)
- **Two error types** — `PathRejection` (16 variants, parsing) vs `Error` (top-level taxonomy). `RequestValidationError` for HTTP-level issues.
- **Plan status** — Plans 000–089 are complete. Plan 055 verifies Milestone 3 final state. Plan 059 closes Milestone 4. Plans 075–083 establish the corrective roadmap (runtime timeout semantics, custom-service ownership, request-body rejection, configuration authority, directory-index conditional headers, HEAD/error-response correctness, HTTP conformance closure). Plan 078 corrects custom-service ownership and connection metadata. ADR-003 documents the ownership model decision. Plan 085 implements Windows handle-relative directory enumeration via `NtQueryDirectoryFile`, replacing the path-based fallback. Plan 089 closes the production-readiness roadmap. Production profiles are machine-readable in `release/support-profiles.toml`.
- **Canonical HTTP types (stable)** — Plan 049 promotes all canonical HTTP types to stable. `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo` (request types) and `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response()` (response types) are all stable.
- **Canonical response normalization** — All response producers converge on `primitives::canonical::normalize_metadata()` for response metadata and framing.
- **`server` module types** — `eggserve-core::server` provides the runtime service boundary for embedding: `Server`, `ServerBuilder`, `ServerHandle`, `RuntimeConfig`, `Service` trait, `service_fn`, `StaticService`, `StaticServiceBuilder`. Lifecycle types: `LifecycleState` (Created, Starting, Running, Draining, Stopped, Failed). Custom services receive real connection metadata (local/remote addresses, scheme, TLS state) via the `Request` envelope. The module is experimental; API may change.
- **RequestBody is one-shot** — `RequestBody` can only be consumed once. The `Service` trait's `call` method takes `Request` by value. Body policy defaults to `Reject`.
- **Python RequestBody** — `RequestBody.read()` and `RequestBody.iter_chunks()` are mutually exclusive. `iter_chunks()` bridges async Rust body to synchronous Python via bounded channel with backpressure.
- **Structured logging** — `eggserve-core::ops` provides the event model (`Event`, `EventKind`, `Severity`, `Logger`, `LogSink`, `OpsCounters`). The CLI initializes with `StderrLogSink`. Python server can add `PyLogObserver`. Library crates must not use `println!`/`eprintln!` — use `Logger::global().emit()` instead.
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
- `client.md` — HTTP client primitives, feature-gated substrate
- `security-model.md` — trust boundaries, defensive layers, attacker model
- `release-infrastructure.md` — release criteria, evidence aggregation, CI gates
- `testing-and-conformance.md` — test layers, conformance corpora, fuzzing

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
- **`ResolvedFile` extraction methods** — `from_parts()`, `into_std_file()`, `into_parts()` are `pub` (for cross-crate Python bindings) but carry security caveats: confinement guarantee ends after extraction.
- **Python Server has runtime hardening** — connection semaphore, header/write timeouts, graceful shutdown, optional handler callback, callback concurrency limit.
- **Python wheel support** — CPython 3.14 only (`>=3.14,<3.15`) on the Linux, macOS, and Windows wheel matrix.
- **Release validation** — run `bash scripts/install-cargo-tools.sh` before `cargo audit`/`cargo deny check`.
- **Release criteria** — `release/criteria.toml` is the single source of truth for release gates.
- **Corrective baseline** — `release/corrective-baseline.toml` records the pinned SHA/toolchain. `release/corrective-findings.toml` has 17 findings (all closed). Gate IDs in findings must match `release/criteria.toml`. Run `python3 -m unittest scripts.test_corrective_tooling -v` to validate.
- **Canonical HTTP types (stable)** — `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo`, `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response()` are all stable.
- **`server` module is experimental** — `eggserve-core::server` provides the runtime service boundary. Its API is subject to change without notice.
- **Production profiles** — `release/support-profiles.toml` defines 7 production profiles. Every production claim must name a profile. Hardened profiles must not allow symlink following. Windows is functional-only until reparse hardening evidence passes.
- **`ops` module** — `Logger` uses `OnceLock` for global initialization. `try_init()` is for Python bindings that may coexist with CLI initialization. Do not call `Logger::init()` twice.
- **No println/eprintln in library code** — The core library must use `Logger::global().emit()` for all operational output. The two `eprintln!` calls in `response.rs` have been replaced with structured logging.
