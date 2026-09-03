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
./scripts/verify.sh fast                 # routine dev check (Rust workspace + Python crate check)
./scripts/verify.sh full                 # pre-release: fast + TLS + examples + Python wheel
./scripts/verify.sh deep                 # expensive suites (manual): fuzz replay, races, proxy interop
```

Gotcha: `verify.sh full` **dies** without Python 3.14 + maturin installed (it defaults to `python3.14`; override with `PYTHON=`). Use `fast` for Rust-only work.

### Supply-chain and optional package checks

```sh
bash scripts/install-cargo-tools.sh     # deterministic audit/deny installation (required first)
cargo audit && cargo deny check
bash scripts/verify-cargo-packages.sh --mode all  # package dry-run gates
```

Routine CI runs the first two commands in its dedicated supply-chain job;
`verify-cargo-packages.sh` remains a release-preparation check.

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
- **Plan 164 admission/lifecycle fields** — `RuntimeConfig`/`Limits` also own `max_buf_size` (default 65536, Hyper minimum 8192), `max_headers` (default 100, pinned explicitly), `max_header_bytes` (default 32 KiB, 431 pre-service), `max_request_target_bytes` (default 8192, 414 pre-service), `max_in_flight_requests` (default 64, 503 on exhaustion), `keep_alive_idle_timeout` (default 60s, resets on activity), `max_requests_per_connection` (`Option<u64>`, default `None`), and `response_write_timeout` (default 30s, no-progress). `keep_alive_idle_timeout`/`response_write_timeout` are intentionally NOT cross-checked against `connection_total_timeout` (the hard ceiling). CLI exposes all eight (`--max-in-flight-requests`, `--keep-alive-idle-timeout`, `--max-requests-per-connection` with `0` = unlimited, `--response-write-timeout`, `--max-buf-size`, `--max-headers`, `--max-header-bytes`, `--max-request-target-bytes`).
- **Plan 165 response privacy (fingerprint minimization)** — `RuntimeConfig.response_policy: ResponsePolicy` owns `server_identification` (`None` = suppressed default; use `builder.server_header(..)` / `config.server_header_value()`), `date_policy` (`SystemClock` default, `Custom(provider)` trusted time value, `Suppress` explicit RFC tradeoff), `stripped_response_headers` (validated denylist, post-service, no framing/`date`/`content-range`, `minimal_fingerprint()` strips `x-powered-by`), and `error_policy` (`Minimal` default fixed bodies, `Empty` no bytes for runtime errors only; app `Ok` never rewritten). `StaticPolicy.static_metadata: StaticMetadataPolicy` (`standard()` emits `ETag`+`Last-Modified`, `minimal_fingerprint()` suppresses both; planner `plan_file_response_with_preconditions_and_metadata`). `ServeConfig.error_policy` transferred by `try_from_serve_config`. Hyper `auto_date_header(false)` — EggServe is sole `Date` authority (exactly 0/1 `Date` per policy); `Last-Modified <= Date` enforced. CLI/Python keep standards defaults (Rust-only advanced policy).
- **ConnectionOutcome variants** — `Normal`, `ClientError`, `HeaderTimeout`, `IdleTimeout` (clean), `WriteTimeout`, `TotalTimeout`, `Shutdown`, `Internal`. `is_clean()` is true for `Normal`/`Shutdown`/`IdleTimeout`.
- Hyper is 1.11.1 (TE-wins normalization for lone CL+TE; stricter `max_buf_size` enforcement). Lone `Transfer-Encoding + Content-Length` now reaches the service as chunked (200), not 400; only duplicate/conflicting CLs still fail.
- Hyper also applies `header_read_timeout` while a keep-alive connection sits idle: with defaults, idle gaps close as header timeouts. Set the idle timeout shorter than the header timeout for distinct idle accounting; raise both for long-lived keep-alive (see `docs/deployment.md` per-profile defaults).
- **Error taxonomy** — five types: `PathRejection` (17 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-facing), `ServerError` (10 variants, lifecycle), `ServiceErrorKind` (private kind enum behind the public `ServiceError` struct; 4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`), `RequestBodyError` (12 variants, body consumption). See [architecture/error-taxonomy.md](architecture/error-taxonomy.md).
- `telemetry.rs` does not exist — do not create it. `clap` was removed (manual parsing in `args.rs`). `tracing` was never added (custom logging).
- `#[allow(dead_code)]` on public API types — consumed externally by Python bindings, not dead.
- Frozen Python classes — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability enforced at both layers.
- `ResolvedFile::from_parts()/into_std_file()/into_parts()` are `pub` for cross-crate bindings, but the confinement guarantee ends after extraction.

### HTTP semantics

- **RequestBody is one-shot** — consumable once via `read_all` or streaming. `Service::call` takes `Request` by value. Python `read()`/`iter_chunks()` are mutually exclusive; second use raises `RequestBodyConsumedError`.
- Canonical response semantics: `StatusCode` accepts 100–599 only; 1xx/204/205/304 are body-forbidden (only 304 may retain a matching representation `Content-Length`); weak metadata ETags satisfy `If-None-Match` but never `If-Range`; exactly 0/1 authoritative `Date` per `DatePolicy` at final construction (`SystemClock` default = one `Date`; `Suppress` = zero; Hyper auto-`Date` disabled). `normalize_response` maps every body-forbidden status except 304 to `BodyLength::Known(0)` and drops HEAD/body-forbidden streams without polling (prompt producer release). `BodyLength::Unknown` (streaming) never becomes `Content-Length: 0`; unknown HEAD omits the header. Normalization is idempotent (`Response::is_normalized`, mutation via `head_mut`/`take_body` clears it) so the static service can normalize eagerly while the connection pipeline normalizes every service response. The runtime is the only framing authority for `Content-Length`/`Transfer-Encoding`/reuse. All producers converge on `primitives::canonical::normalize_metadata()`.
- Stable canonical types: `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo` (`local_addr`/`remote_addr` are `Option<SocketAddr>`; non-socket transports expose `None` via `SocketEndpoints`/`without_socket_addrs`), `StatusCode`, `ResponseHead`, `ResponseBody` (`Empty`/`Bytes`/`File`/`Stream`/`EmptyWithLength`), `Response`, `BodyLength` (`Known`/`Unknown`), `ResponseStream`/`ResponseStreamError`, `normalize_response()`.
- Listener accept errors are classified by `io::ErrorKind` (transient/resource-exhaustion/persistent) with bounded exponential backoff — use `classify_accept_error()`.
- Transport-neutral driver: `server::connection::serve_http1_connection` drives a canonical `Service` over any `AsyncRead + AsyncWrite` stream with an explicit `ConnectionContext` (no fabricated addresses, no Hyper types, scheme/TLS asserted by caller), shared `Arc<RuntimeState>` admission (constructed via `RuntimeState::new(&config)`), and per-connection `ConnectionShutdown` returning `ConnectionOutcome`. TCP/TLS `Server` shares the same pipeline via `serve_http1_connection_with_id`; raw Hyper helpers are crate-private.

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

### Agent assets

The project skill lives at `.opencode/skills/eggserve-dev/SKILL.md`
(symlinked from `.agents/skills/eggserve-dev`) — load it before working on
code, plans, docs, or architecture. Keep it and this file consistent; both are
maintained against the codebase.

### Architecture index (`architecture/`)

Deep-dive pages, named after their subsystems. Start at `overview.md`; it
indexes every page below.

| Subsystem | Page |
|-----------|------|
| Workspace structure, data flow, decisions | [overview.md](architecture/overview.md) |
| Core library module map | [eggserve-core.md](architecture/eggserve-core.md) |
| CLI binary, accept loop, signals | [eggserve-bin.md](architecture/eggserve-bin.md) |
| Python bindings, PyO3/maturin packaging | [eggserve-python.md](architecture/eggserve-python.md) |
| Path validation pipeline | [path-confinement.md](architecture/path-confinement.md) |
| SecureRoot, symlink-aware resolution | [filesystem-confinement.md](architecture/filesystem-confinement.md) |
| StaticPolicy / policy flags | [policy-system.md](architecture/policy-system.md) |
| Public primitives facade for embedders | [primitives-api.md](architecture/primitives-api.md) |
| Conditional/range/ETag planning | [response-planning.md](architecture/response-planning.md) |
| Server, Service trait, StaticService | [runtime.md](architecture/runtime.md) |
| Trust boundaries, defensive layers | [security-model.md](architecture/security-model.md) |
| Test layers, corpora, fuzzing | [testing-and-conformance.md](architecture/testing-and-conformance.md) |
| Configuration field inventory | [configuration.md](architecture/configuration.md) |
| Event model, sinks, counters | [structured-logging.md](architecture/structured-logging.md) |
| Five error layers, variant inventory | [error-taxonomy.md](architecture/error-taxonomy.md) |
| TLS feature gates, PEM loading | [tls.md](architecture/tls.md) |
| ADR: Windows handle-relative FS | [adr-002-windows-handle-relative-filesystem.md](architecture/adr-002-windows-handle-relative-filesystem.md) |
| ADR: custom service ownership | [adr-003-custom-service-ownership.md](architecture/adr-003-custom-service-ownership.md) |

### Reference pages (`docs/`)

Normative user-facing contracts: [security-policy](docs/security-policy.md),
[threat-model](docs/threat-model.md),
[python-http-server-compatibility](docs/python-http-server-compatibility.md)
(the Python compatibility contract), [cli](docs/cli.md),
[python-api](docs/python-api.md), [http-primitives](docs/http-primitives.md),
[public-api-boundary](docs/public-api-boundary.md),
[deployment](docs/deployment.md) (production profiles),
[timeout-reference](docs/timeout-reference.md) (runtime timeout catalog),
[ops-logging](docs/ops-logging.md) (log schema/event reference),
[migration-guide](docs/migration-guide.md) (legacy → canonical mappings),
[action-pinning](docs/action-pinning.md) (CI supply-chain policy),
plus non-goals, dependency-policy, toolchain-support, release-process,
release-contract, python-packaging, secure-root, api-stability, fuzzing,
invariants, compatibility, body-migration, extension-contract.

`plans/` records historical design/change records plus roadmap files
(`ROADMAP.md`, `RELEASE-READINESS-ROADMAP.md`). Plans are change-trace
records, **not** normative API documentation; treat README.md, `docs/`, and
`architecture/` as owning current invariants.
