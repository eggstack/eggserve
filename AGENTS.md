# Guide for AI coding agents

## Project overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior, intended as a hardened replacement for `python -m http.server`. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. Plans 000–108 are historical implementation records; Plan 108 is the completed corrective closure pass. Plan 091 defines current CI/release policy.

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. See [docs/security-policy.md](docs/security-policy.md).
- **No serving outside the configured root.** Path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative: `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`. See [docs/threat-model.md](docs/threat-model.md).
- **No broad dependencies.** Every dependency must have an explicit purpose. See [docs/dependency-policy.md](docs/dependency-policy.md). Current deps: `thiserror`, `tokio`, `hyper`/`hyper-util`/`http-body-util`, `bytes`, `futures-util`, `httpdate`, `phf`. Optional: `rustls`/`tokio-rustls`/`webpki-roots` (TLS). Unix-only: `rustix` (descriptor-relative traversal).
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
├── benchmarks/             # benchmark baselines (Plan 088)
├── conformance/            # test corpora and conformance matrix
├── docs/                   # project documentation
├── examples/               # usage examples (Python, Rust)
├── fuzz/                   # fuzz targets, seed corpora, fuzz README
├── plans/                  # design plans and roadmap
├── release/                # release artifacts
├── scripts/                # verify.sh, test-python-wheel.sh, install-cargo-tools.sh
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
# Builds CLI, stages binary, builds wheel, installs in venv, runs smoke + tests
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

The `tests/` directory at the repo root holds integration tests (proxy interop, soak, installed-binary qual) that are distinct from in-crate unit/integration tests.

### Local verification script

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

### Distribution builds (Plan 105)

The `dist` profile produces stripped, size-optimized release artifacts:

```sh
cargo build --profile dist --locked -p eggserve-bin              # default CLI
cargo build --profile dist --locked -p eggserve-bin --features tls  # TLS CLI
```

## CI policy (Plan 091)

Routine CI is a small regression screen, not release certification:

- One workflow (`.github/workflows/ci.yml`) with two jobs: `rust` and `python`.
- No evidence upload, no gate registry, no generated checklists, no publication.
- Deep verification is local/manual and selected by change risk.
- Crates.io publishing is manual from a maintainer-controlled environment.
- GitHub Actions never publishes.
- Historical plans (039, 044–046, 086, 089, 090) defined the prior evidence/qualification framework; Plan 091 supersedes their CI and release requirements while preserving their product implementation and test coverage.

## Toolchain notes

- Rust edition 2021, workspace `resolver = "2"`.
- No `rustfmt.toml` / `clippy.toml` — defaults apply; CI enforces `-D warnings`.
- `target/` is gitignored; `cargo build` / `cargo test` are sufficient setup (no pre-build step, no codegen).
- `cargo run -p eggserve-bin` starts an HTTP server on `127.0.0.1:8000` serving static files from the current directory. See [crates/eggserve-bin/src/main.rs](crates/eggserve-bin/src/main.rs).

## Important quirks

- **Two DotfilePolicy types**: `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served. Don't confuse them.
- **eggserve-python is excluded from the workspace** — it has its own `Cargo.lock` and is built independently via `maturin`. Don't run `cargo test --workspace` expecting to cover Python crate code.
- **Manual argument parsing** in `args.rs` — no clap dependency.
- **`#[allow(dead_code)]` on public API types** — these are consumed externally (Python bindings), not dead.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability is enforced at both layers.
- **Python wheels**: CPython 3.14 only (`>=3.14,<3.15`). Routine CI builds and tests the Linux wheel; macOS and Windows wheels are built manually. The wheel bundles the platform-native CLI binary.
- **Windows**: functional with handle-relative child resolution (Plan 084) and handle-relative directory enumeration (Plan 085). `OwnedHandle::try_clone()` is fallible (not `Clone`), so `ResolvedDirectory` on Windows retains an owned `dir_handle` for handle-relative child resolution. Adversarial qualification test scaffold established (Plan 086, 114 tests). Independent adversarial review is incomplete. Do not use with untrusted public content on Windows until that review is completed.
- **Two error types for path validation**: `PathRejection` (16 variants for parsing failures) vs `Error` (top-level taxonomy). `RequestValidationError` handles HTTP-level issues.
- **Plan 109 active corrective pass** — Plan 108’s implementation and hosted-CI record are historical. Plan 109 is the active bounded correction for final admission ownership, build-time static-service consumption, exact Stream wire proof, and truthful release evidence. Production `Server` paths own one shared `RuntimeState` file-stream pool. The pre-runtime `eggserve_core::service` adapter is deprecated, requires an explicit caller-owned runtime context, and must not be treated as a production server path.
- **Two BodySource Python types**: `BodySource` (from `lib.rs`, for primitive-level body reading) and `ServerBodySource` (from `server.rs`, for server response streaming). They wrap the same Rust `BodySource` but have different Python names to avoid collision.
- **Two Method types**: `ReadOnlyMethod` (GET/HEAD only, stable) and `Method` (standard + extension, experimental). `ReadOnlyMethod` is used by the response planner. `Method` is the canonical type for new code.
- **Client vs Method**: Rust client method types are feature-gated and Rust-only; they are not part of the shipped Python surface. `Method` (from `primitives::method`) is the canonical HTTP method type supporting standard + extension methods.
- **HeaderBlock is a list, not a map**: `HeaderBlock` stores headers as an ordered `Vec<HeaderField>`, preserving duplicates. `get_unique()` returns `DuplicateHeaderError` on duplicates. Python `HeaderBlock` is frozen/immutable.
- **Response validation boundary**: Python handler-returned responses are staged and validated atomically in Rust — status 100–599, no hop-by-hop headers, explicit one-shot body ownership, body-forbidden statuses (including 205) are normalized empty, exact `Content-Length`, and no NUL/CR/LF in header values. Unknown or malformed bodies never silently become empty; invalid responses produce a generic 500.
- **Typed lifecycle/response exceptions**: `LifecycleError` (double start, stop before start) and `ResponseConstructionError` (response validation failure) are typed exceptions, not generic `PyValueError`.
- **Canonical HTTP types (stable)** — `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo` (request types) and `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response()` (response types) are all stable. `ReadOnlyMethod` (GET/HEAD only) remains stable for existing consumers.
- **Canonical response normalization** — All response producers converge on `primitives::canonical::normalize_metadata()` for response metadata and framing. `normalize_response()` applies HEAD suppression, body-forbidden enforcement, and hop-by-hop stripping for in-memory bodies. `normalize_metadata()` applies the same framing rules (Transfer-Encoding stripping, Content-Length computation) for file-backed bodies without consuming the body. `to_hyper_response()` converts to Hyper after normalization.
- **Two status code types**: `ResponseStatus` (stable, existing) and `StatusCode` (stable, canonical). `ResponseStatus` is a simple u16 newtype used by the planner. `StatusCode` has range validation (100–599, three-digit only) and classification helpers (is_informational, permits_payload_body). 205 Reset Content is body-forbidden. New code should prefer `StatusCode`.
- **Two header map types**: `HeaderMapPlan` (stable, existing) and `HeaderBlock` (stable, canonical). `HeaderMapPlan` stores `ResponseHeader { name: String, value: String }`. `HeaderBlock` stores `HeaderField { name: HeaderName, value: HeaderValue }` with validation. The canonical response types use `HeaderBlock`.
- **Python server facade** — The supported Python API is `eggserve.server` with `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`, `ThreadingHTTPSServer`, `BaseHTTPRequestHandler`, and `SimpleHTTPRequestHandler`. It uses the actual Rust runtime internally; native callback and client types are not top-level supported APIs. HTTPS reuses the core rustls PEM loader and supports HTTP/1.1 ALPN only.
- **SimpleHTTPRequestHandler static facade** — `partial(SimpleHTTPRequestHandler, directory=...)` pins and validates one root at server construction. `index_pages` defaults to `("index.html", "index.htm")`; listing, dotfiles, and symlink following are opt-in class policies captured at startup. Redirects, index resolution, listing enumeration, conditional requests, ranges, and file streaming use the Rust resolver/planner. `extensions_map` applies to direct files and native-selected indexes; subclass `guess_type()` is bounded to direct file targets with suffixes. Invalid MIME values fail closed. `translate_path()` is not an authoritative host-path API, and `list_directory()` never receives a raw path.
- **RequestBody is one-shot** — `RequestBody` can only be consumed once (via `read_all` or streaming). The `Service::call` method takes `Request` by value, consuming it. Static service rejects bodies for unsupported methods via method-aware body policy. Body policy defaults to `Reject`. Body ingestion plumbing (Hyper Incoming → RequestBody) is in the connection pipeline with `Service::request_body_policy(&RequestHead)` selecting the effective policy (method-aware).
- **Service trait takes Request** — The `Service` trait's `call` method now accepts a `Request` envelope (containing `RequestHead`, `RequestBody`, `ConnectionInfo`) instead of `RequestHead` directly. `service_fn` updated accordingly. All existing implementations (StaticService, PythonCallbackService) updated.
- **RuntimeConfig body fields** — `RuntimeConfig` has `max_request_body_bytes` (default 0, hard ceiling). Body policy is service-declared via `Service::request_body_policy(&RequestHead)`, not a runtime field. The runtime enforces the ceiling. `incomplete_body_policy` is always `Close` (hardcoded, not configurable).
- **Service body policy** — `Service::request_body_policy(&RequestHead)` declares the preferred body policy (Reject/Buffer/Stream) for the actual request. The runtime enforces the global ceiling (`max_request_body_bytes`) and service-specific limits may only lower it. GET/HEAD/DELETE are not globally body-forbidden; TRACE content remains rejected. StaticService declares `Reject`.
- **Body read timeout** — `RuntimeConfig::body_read_timeout` (default 30s) is a total deadline for body consumption in Buffer mode. Stream mode passes through without pre-buffering.
- **Incomplete body handling** — When a service returns without fully consuming a Stream body, the connection closes. Active drain is not safely implementable because the body stream is consumed into the `Request` envelope by value and is no longer accessible from the connection pipeline after service invocation.
- **Server without ServeConfig** — `Server::builder().runtime(config).build()` creates a runtime-only server. `Server::start()` requires `serve_config` (constructs `StaticService` internally). `Server::start_with_service()` works without serve config — custom services have no implicit filesystem root. One accept loop (`accept_loop_generic`) serves both static and custom services.
- **Semaphore bounds** — `max_connections` and `max_file_streams` are validated against `tokio::sync::Semaphore::MAX_PERMITS` in both `Limits::validate()` and `RuntimeConfigBuilder::build()`. Values above this bound are rejected with a controlled error, not clamped or panicked.
- **Body error mapping** — `RequestBodyError` maps to HTTP status codes: 400 (malformed), 408 (timeout), 413 (too large), 500 (transport error). Terminal errors include `Connection: close`.
- **Python RequestBody is one-shot** — `RequestBody.read()` and `RequestBody.iter_chunks()` are mutually exclusive and consume the body. Second use raises `RequestBodyConsumedError`. `iter_chunks()` bridges async Rust body to synchronous Python via a bounded channel with backpressure. Body objects are only present when `has_body` is True (non-empty bodies with allowed policy). Empty bodies and rejected bodies produce `body=None`.
- **Advanced runtime remains internal** — `eggserve-core::server` remains the Rust embedding boundary. Python users should use the six-class facade; advanced primitives are grouped under `eggserve.lowlevel`, and CLI subprocess helpers under `eggserve.subprocess`.
- **Production profiles** — Production deployment profiles are documented in README.md and `docs/deployment.md`. Profiles are: unix-reverse-proxy (candidate), unix-direct-https (candidate), windows-reverse-proxy (candidate), windows-direct-https (functional), local-development (supported-hardened), windows-functional (functional), link-following-compat (functional).
- **CLI runtime is current-thread** — The standalone CLI uses `Builder::new_current_thread()` (Plan 105). The Python facade uses `rt-multi-thread` for GIL scheduling. The library is runtime-agnostic.
- **Structured logging** — `eggserve-core::ops` provides the event model. `Logger::global().emit(Event::new(...))` is the primary API. The CLI initializes the logger with `StderrLogSink`. `--log-format none` uses `NopLogSink` (no output). `--quiet` wraps the format-specific sink with `FilteredLogSink` (warn/error only). The Python `Server` delegates logging to the Rust runtime's stderr sink. Python handler failures use fixed categories and never interpolate exception text, response reprs, or raw response headers. Library code must not use `println!`/`eprintln!`.
- **Runtime `Server` header** — `RuntimeConfig.server_header` is validated at build time and applied exactly once at final response construction, replacing service-provided values. `None` removes the header.

## Reference docs

`docs/` has reference docs (security-policy, threat-model, non-goals, dependency-policy, compatibility, release-process, deployment, http-primitives, python-api, etc.). `architecture/` has deep-dive docs per subsystem (core, bin, python, path-confinement, policy-system, runtime, etc.). `plans/` has design plans 000–106 (historical/implementation records; Plan 091 defines current CI/release policy; Plan 105 defines product-surface freeze and binary-size reduction; Plan 106 closes the roadmap).
