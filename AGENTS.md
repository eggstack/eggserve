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
crates/
├── eggserve-core/      # library: security policy, path confinement, HTTP serving, response construction
├── eggserve-bin/       # CLI binary, args, signal handling, accept loop
└── eggserve-python/    # Python wheel packaging (maturin) — EXCLUDED from workspace
architecture/           # deep-dive docs per subsystem (filenames match subsystems)
benchmarks/             # benchmark baselines
conformance/            # test corpora + conformance_matrix.toml
docs/                   # reference documentation
examples/               # canonical CLI/Python/Cargo examples; index: examples/README.md
fuzz/                   # fuzz targets, seed corpora
plans/                  # historical design/change records (not normative)
release/                # release artifacts
scripts/                # verification hierarchy + package/release checks
tests/                  # repo-level integration tests (proxy interop, soak, installed-binary qual)
```

## Common commands

Routine CI (`.github/workflows/ci.yml`) runs two concurrent jobs:

```sh
# rust job
python3 scripts/verify-conformance-matrix.py                # corpus/matrix consistency gate (runs first!)
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings   # warnings are errors
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                   # TLS tests

# python job: bash scripts/test-python-wheel.sh
# builds wheel with maturin, installs in venv, runs smoke + tests
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

### Local verification script

```sh
./scripts/verify.sh fast                 # routine dev check (Rust only)
./scripts/verify.sh full                 # pre-release: fast + TLS + examples + Python wheel
./scripts/verify.sh deep                 # expensive suites (manual): fuzz replay, races, proxy interop
```

Gotcha: `verify.sh full` **dies** without Python 3.14 + maturin installed (it defaults to `python3.14`; override with `PYTHON=`). Use `fast` for Rust-only work.

### Optional manual checks (release prep only)

```sh
bash scripts/install-cargo-tools.sh     # deterministic audit/deny installation (required first)
cargo audit && cargo deny check
bash scripts/verify-cargo-packages.sh --mode all  # package dry-run gates
```

### Distribution builds

The `dist` profile produces stripped, size-optimized artifacts:

```sh
cargo build --profile dist --locked -p eggserve-bin              # default CLI
cargo build --profile dist --locked -p eggserve-bin --features tls  # TLS CLI
```

## CI policy

Routine CI is a small regression screen, not release certification. Platform qualification (macOS arm64 + Windows adversarial FS suites) is manual-only via `.github/workflows/platform-qualification.yml` (`gh workflow run platform-qualification.yml --ref main`). Publishing is manual (crates.io from maintainer env; PyPI via OIDC Trusted Publishing requiring the protected `pypi` GitHub Environment) — no push/tag/merge ever publishes.

## Toolchain notes

- Rust edition 2021, resolver `"2"`. No `rustfmt.toml`/`clippy.toml` — defaults apply; CI enforces `-D warnings`.
- No pre-build/codegen steps: `cargo build` / `cargo test` are sufficient setup.
- `cargo run -p eggserve-bin` serves static files from CWD on `127.0.0.1:8000`.

## Quirks and pitfalls

### Crate boundaries

- **eggserve-python is excluded from the workspace** — own `Cargo.lock`, built independently via maturin. `cargo test --workspace` does not cover it.
- **Package roles**: `eggserve-core::primitives` is the semver-considered facade; `eggserve-core::server` is experimental (API may change). `eggserve-bin::run_cli` is plumbing for the Python wheel's extension-backed CLI, **not** a general embedding API — Rust embedders use `eggserve-core`.
- `crates/eggserve-bin/src/main.rs` is a 2-line shim; real logic is in `lib.rs`/`args.rs`.

### Code shapes agents get wrong

- **Two DotfilePolicy types**: `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served.
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`.
- `ResponseStatus` is a struct with associated constants, not an enum. `FileRange` is a struct `{ start, end_inclusive }`, not an enum. `BodyPlan` variants: `Empty`, `FullBytes(Vec<u8>)`, `FileFull`, `FileRange { start, end_inclusive }`.
- **Error taxonomy** — five types: `PathRejection` (16 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-facing), `ServerError` (10 variants, lifecycle), `ServiceErrorKind` (4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`), `RequestBodyError` (12 variants, body consumption). See [architecture/error-taxonomy.md](architecture/error-taxonomy.md).
- Range requests ARE implemented (some older docs claim otherwise).
- `telemetry.rs` does not exist — do not create it. `clap` was removed (manual parsing in `args.rs`). `tracing` was never added (custom logging).
- `#[allow(dead_code)]` on public API types — consumed externally by Python bindings, not dead.
- Frozen Python classes — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability enforced at both layers.
- `ResolvedFile::from_parts()/into_std_file()/into_parts()` are `pub` for cross-crate bindings, but the confinement guarantee ends after extraction.

### HTTP semantics

- **RequestBody is one-shot** — consumable once via `read_all` or streaming. `Service::call` takes `Request` by value. Python `read()`/`iter_chunks()` are mutually exclusive; second use raises `RequestBodyConsumedError`.
- Canonical response semantics: `StatusCode` accepts 100–599 only; 205 responses are body-forbidden; weak metadata ETags satisfy `If-None-Match` but never `If-Range`; exactly one authoritative `Date` header added at final construction. All producers converge on `primitives::canonical::normalize_metadata()`.
- Stable canonical types: `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo`, `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response()`.
- Listener accept errors are classified by `io::ErrorKind` (transient/resource-exhaustion/persistent) with bounded exponential backoff — use `classify_accept_error()`.

### CLI

- **Manual argument parsing** in `args.rs` — no clap. Grammar `[OPTIONS] [PORT] [DIRECTORY]`; positionals own those two slots; a directory after an occupied port slot is taken verbatim even if numeric; excess positionals rejected. Host-only `--bind` leaves the port slot free; `--directory` occupies the directory slot. `--bind` accepts hostnames resolved once before listener startup; `--tls-cert` may name a combined cert/key PEM when `--tls-key` is omitted.
- **CLI runtime is current-thread**; the Python facade uses `rt-multi-thread` with 2 worker threads (GIL scheduling). The library itself is runtime-agnostic.
- Logging flags: `--log-format none` → `NopLogSink`; `--quiet` → `FilteredLogSink` (warn/error). Argument-validation errors printed before logger init may still hit stderr.

### Structured logging

- Library code must not use `println!`/`eprintln!` — use `eggserve-core::ops`: `Logger::global().emit(Event::new(...))`.
- `Logger` uses `OnceLock`; `try_init()` exists for Python bindings coexisting with CLI init. Never call `Logger::init()` twice.
- `max_connections`/`max_file_streams` are validated against `tokio::sync::Semaphore::MAX_PERMITS`; larger values are rejected.

### Python facade

- Supported API is `eggserve.server`: `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`, `ThreadingHTTPSServer`, `BaseHTTPRequestHandler`, `SimpleHTTPRequestHandler`. Advanced primitives live in `eggserve.lowlevel`; CLI subprocess helpers in `eggserve.subprocess`. Native callback/client types are not top-level supported APIs.
- Stock `SimpleHTTPRequestHandler` with default settings bypasses Python dispatch entirely (native fast path). Eligibility is exact: bare class, or a `functools.partial` whose `.func` is exactly `SimpleHTTPRequestHandler`, `.args` empty, `.keywords` ⊆ `{directory, extra_response_headers}`. Subclasses and other settings fall back to the Python callback path.
- `default_content_type` and ordered `extra_response_headers` are native static metadata; extras apply only to final 200 responses. Fast-path concurrency is enforced natively (non-threading classes → 1 connection, `Threading*(N)` → N). Handler `protocol_version` is constrained to HTTP/1.1.
- Wheels: CPython 3.11+ (abi3). Routine CI tests Linux only; release wheels target 9 platforms (manylinux_2_17 x86_64/aarch64/armv7, musllinux_1_2 x86_64/aarch64, macOS x86_64/arm64, Windows x86_64/arm64). Wheel ships the `eggserve` console script and `python -m eggserve` backed by the native extension — no separate bundled binary.
- Windows is functionally qualified for handle-relative child resolution and directory enumeration, but remains **trusted/local-content only** (NTFS rejects the two open-descendant root-rename cases). See [docs/toolchain-support.md](docs/toolchain-support.md).

### Examples

`examples/README.md` is the mechanically checked index (`scripts/test-examples.sh` in `verify.sh full`). Supported demos: Python `python_http_server_static.py`, `python_custom_handler.py`, `python_subprocess.py`, `python_safe_download.py`, `python_https_server.py`, `python_custom_headers.py`; Cargo examples `static_server`, `custom_service`, `custom_headers`, `https_server` (`--features tls`), `primitives`. Keep examples small, loopback-bound, safe by default; do not turn them into a framework or second policy reference.

### Verification beyond routine CI

- For library/CLI usability work also run: `cargo test --doc -p eggserve-core`, `cargo check -p eggserve-core --examples`, both dist builds, and `bash scripts/verify-cargo-packages.sh --mode all`.
- For native bind/TLS changes, run the manual platform qualification workflow after pushing.
- Production profiles: every production claim must name a profile ([docs/deployment.md](docs/deployment.md)). Hardened profiles must not allow symlink following.

## Reference docs

`docs/` holds reference pages (security-policy, threat-model, non-goals, dependency-policy, deployment, release-process, cli, python-api, http-primitives, public-api-boundary, etc.). `architecture/` holds deep-dives named after their subsystem — most useful entry points: `overview.md`, `error-taxonomy.md`, `runtime.md`, `filesystem-confinement.md`, `testing-and-conformance.md`.

`plans/` records design history through Plan 144 plus roadmap files. Plans are change-trace records, **not** normative API documentation; treat README.md, `docs/`, and `architecture/` as owning current invariants.
