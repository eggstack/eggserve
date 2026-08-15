# Guide for AI coding agents

## Project overview

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade. The
CLI is static-only; the Python facade also supports bounded synchronous custom
handlers; and `eggserve-core::server` exposes an experimental, low-level Rust
service boundary. EggServe is not an application framework, ASGI/WSGI runtime,
proxy, or general-purpose `socketserver` replacement. The user-facing Python
compatibility contract lives in [docs/python-http-server-compatibility.md](docs/python-http-server-compatibility.md).

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. See [docs/security-policy.md](docs/security-policy.md).
- **No serving outside the configured root.** Path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative: `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`. See [docs/threat-model.md](docs/threat-model.md).
- **No broad dependencies.** Every dependency must have an explicit purpose. See [docs/dependency-policy.md](docs/dependency-policy.md).
- **Plan-driven development.** Every change must be backed by a plan in `plans/`. No ad-hoc feature additions.

## Layout

```
eggserve/
├── Cargo.toml              # workspace root
├── crates/
│   ├── eggserve-core/      # security policy, path confinement, HTTP serving, response construction
│   ├── eggserve-bin/       # CLI binary, args, signal handling, accept loop
│   └── eggserve-python/    # Python wheel packaging (maturin)
├── architecture/           # deep-dive docs for each subsystem
├── benchmarks/             # benchmark baselines
├── conformance/            # test corpora and conformance matrix
├── docs/                   # project documentation
├── examples/               # canonical CLI/Python examples and tiny fixtures
├── fuzz/                   # fuzz targets, seed corpora, fuzz README
├── plans/                  # design plans and roadmap
├── release/                # release artifacts
├── scripts/                # small fast/full/deep verification hierarchy and package/release checks
└── tests/                  # integration tests (proxy interop, soak, installed-binary qual)
```

## Common commands

Routine CI runs these in two concurrent jobs (`rust` and `python`):

```sh
# Rust job
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --lib --bins --tests -- -D warnings  # lint (warnings are errors)
cargo test --workspace                                     # tests
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                  # TLS tests

# Python job (via scripts/test-python-wheel.sh)
# Builds wheel, installs in venv, runs smoke + tests
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

The `tests/` directory at the repo root holds integration tests (proxy interop, soak, installed-binary qual) that are distinct from in-crate unit/integration tests.

### Local verification script

```sh
./scripts/verify.sh fast                 # routine dev check (Rust only)
./scripts/verify.sh full                 # pre-release validation (examples, Rust + Python wheel)
./scripts/verify.sh deep                 # expensive suites (manual)
```

### Optional manual security/package checks

Not run in routine CI. Run manually when preparing a release:

```sh
bash scripts/install-cargo-tools.sh     # deterministic audit/deny installation
cargo audit                             # vulnerability check
cargo deny check                        # license/policy check
bash scripts/verify-cargo-packages.sh --mode all  # package dry-run gates
```

### Distribution builds

The `dist` profile produces stripped, size-optimized release artifacts:

```sh
cargo build --profile dist --locked -p eggserve-bin              # default CLI
cargo build --profile dist --locked -p eggserve-bin --features tls  # TLS CLI
```

## CI policy

Routine CI is a small regression screen, not release certification:

- Routine CI remains one workflow (`.github/workflows/ci.yml`) with two jobs: `rust` and `python`.
- Platform/product qualification is manual-only in `.github/workflows/platform-qualification.yml`; it exercises the installed wheel on macOS arm64 and the Windows adversarial filesystem suites without expanding routine CI.
- No evidence upload, no gate registry, no generated checklists, no publication.
- Deep verification is local/manual and selected by change risk.
- Crates.io publishing is manual from a maintainer-controlled environment.
- GitHub Actions never publishes.

## Toolchain notes

- Rust edition 2021, workspace `resolver = "2"`.
- No `rustfmt.toml` / `clippy.toml` — defaults apply; CI enforces `-D warnings`.
- `target/` is gitignored; `cargo build` / `cargo test` are sufficient setup (no pre-build step, no codegen).
- `cargo run -p eggserve-bin` starts an HTTP server on `127.0.0.1:8000` serving static files from the current directory. See [crates/eggserve-bin/src/main.rs](crates/eggserve-bin/src/main.rs).

## Important quirks

- **eggserve-python is excluded from the workspace** — it has its own `Cargo.lock` and is built independently via `maturin`. Don't run `cargo test --workspace` expecting to cover Python crate code.
- **Two DotfilePolicy types**: `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served. Don't confuse them.
- **Manual argument parsing** in `args.rs` — no clap dependency.
- **`#[allow(dead_code)]` on public API types** — these are consumed externally (Python bindings), not dead.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability is enforced at both layers.
- **Python wheels**: CPython 3.11+ with abi3 stable ABI. Routine CI builds and tests the Linux wheel; macOS and Windows wheels are built manually. The wheel includes an `eggserve` console script backed by the native extension (no separate bundled binary). `python -m eggserve` uses the same in-process native CLI entry point.
- **Windows**: functionally qualified for the executed handle-relative child-resolution and directory-enumeration classes. Two open-descendant root-rename cases are explicitly skipped because NTFS rejects that external path operation, so Windows remains a trusted/local-content platform. See [docs/toolchain-support.md](docs/toolchain-support.md).
- **RequestBody is one-shot** — `RequestBody` can only be consumed once (via `read_all` or streaming). The `Service::call` method takes `Request` by value, consuming it. Python `RequestBody.read()` and `iter_chunks()` are mutually exclusive; second use raises `RequestBodyConsumedError`.
- **Python server facade** — The supported Python API is `eggserve.server` with `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`, `ThreadingHTTPSServer`, `BaseHTTPRequestHandler`, and `SimpleHTTPRequestHandler`. Native callback and client types are not top-level supported APIs. Advanced primitives are grouped under `eggserve.lowlevel`, CLI subprocess helpers under `eggserve.subprocess`. Stock `SimpleHTTPRequestHandler` with default settings bypasses Python per-request dispatch entirely; subclasses and non-default settings fall back to the Python callback path. The fast path's eligibility contract is exact: the bare class or a `functools.partial` whose `.func` is exactly `SimpleHTTPRequestHandler`, `.args` is empty, and `.keywords` is a subset of `{"directory"}`. The compatibility facade's effective concurrency is enforced through the native connection admission limit when the fast path is active (`HTTPServer`/`HTTPSServer` → 1, `ThreadingHTTPServer(N)`/`ThreadingHTTPSServer(N)` → N).
- **CLI runtime is current-thread** — The standalone CLI uses `Builder::new_current_thread()`. The Python facade uses `rt-multi-thread` with 2 worker threads for GIL scheduling. The library is runtime-agnostic.
- **Structured logging** — `eggserve-core::ops` provides the event model. `Logger::global().emit(Event::new(...))` is the primary API. The CLI initializes the logger with `StderrLogSink`. `--log-format none` disables output; `--quiet` filters to warn/error only. Library code must not use `println!`/`eprintln!`.
- **Canonical examples** — `examples/README.md` is the index. The supported demonstrations are `python_http_server_static.py`, `python_custom_handler.py`, `python_subprocess.py`, `python_safe_download.py`, and the Cargo examples `static_server`, `custom_service`, and `primitives`. Keep examples small, loopback-bound, safe by default, and mechanically checked by `scripts/verify.sh full`.

## Common pitfalls

- Range requests ARE implemented (despite some docs saying otherwise).
- `clap` was removed — manual arg parsing in `args.rs`.
- `tracing` was never added — logging is custom.
- `BodyPlan` variants: `Empty`, `FullBytes(Vec<u8>)`, `FileFull`, `FileRange { start, end_inclusive }`.
- `ResponseStatus` is a struct with associated constants, not an enum.
- `FileRange` is a struct `{ start: u64, end_inclusive: u64 }`, not an enum.
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`.
- **`ResolvedFile` extraction methods** — `from_parts()`, `into_std_file()`, `into_parts()` are `pub` (for cross-crate Python bindings) but carry security caveats: confinement guarantee ends after extraction.
- **`server` module is experimental** — `eggserve-core::server` provides the runtime service boundary. Its API is subject to change without notice.
- **`ops` module** — `Logger` uses `OnceLock` for global initialization. `try_init()` is for Python bindings that may coexist with CLI initialization. Do not call `Logger::init()` twice.
- **Error taxonomy** — Five distinct error types: `PathRejection` (16 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-only), `ServerError` (10 variants, server lifecycle), `ServiceError` (4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`), `RequestBodyError` (12 variants, body consumption). See [architecture/error-taxonomy.md](architecture/error-taxonomy.md).
- **No println/eprintln in library code** — The core library must use `Logger::global().emit()` for all operational output.
- **Semaphore bounds** — `max_connections` and `max_file_streams` are validated against `tokio::sync::Semaphore::MAX_PERMITS`. Values above this bound are rejected.
- **Logging modes** — `--log-format none` uses `NopLogSink` (no output). `--quiet` wraps the format-specific sink with `FilteredLogSink` (warn/error only). Direct argument-validation errors printed before logger initialization may remain on stderr.

## Reference docs

`docs/` has reference docs (security-policy, threat-model, non-goals, dependency-policy, compatibility, release-process, deployment, http-primitives, python-api, cli, etc.). `architecture/` has deep-dive docs per subsystem:

- `architecture/overview.md` — workspace structure, data flow, architectural decisions
- `architecture/eggserve-core.md` — core library module map, key types, error taxonomy
- `architecture/eggserve-bin.md` — CLI binary, accept loop, signal handling
- `architecture/eggserve-python.md` — Python bindings, PyO3, maturin packaging
- `architecture/path-confinement.md` — path validation pipeline
- `architecture/filesystem-confinement.md` — SecureRoot, symlink-aware resolution
- `architecture/policy-system.md` — StaticPolicy, symlink/dotfile/listing policies
- `architecture/primitives-api.md` — public API boundary for embedding consumers
- `architecture/response-planning.md` — conditional/range/ETag response planning
- `architecture/runtime.md` — runtime service boundary, Server, Service trait, StaticService
- `architecture/security-model.md` — trust boundaries, defensive layers, attacker model
- `architecture/testing-and-conformance.md` — test layers, conformance corpora, fuzzing
- `architecture/configuration.md` — configuration inventory, ownership model, field inventory
- `architecture/structured-logging.md` — event model, event kinds, operational counters, log sinks
- `architecture/error-taxonomy.md` — five error layers, variant inventory, conversion flow
- `architecture/tls.md` — TLS support, feature gates, PEM loading
- `architecture/adr-002-windows-handle-relative-filesystem.md` — Windows handle-relative confinement design
- `architecture/adr-003-custom-service-ownership.md` — custom service ownership model

`plans/` has historical design and implementation records through Plan 133;
the current documentation/compatibility contract is recorded in Plan 131.
Plans are repository navigation and change-trace records, not prerequisites
for understanding runtime behavior; use the owning docs and architecture
pages for current invariants.
