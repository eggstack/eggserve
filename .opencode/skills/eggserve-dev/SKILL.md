---
name: eggserve-dev
description: Use when working on eggserve code, plans, docs, or architecture. Covers Rust workspace conventions, plan-driven development, CI validation, security policy, and the three-crate layout.
---

# eggserve Development Skill

`AGENTS.md` at the repository root is the canonical agent-facing index (CI
commands, quirks, architecture/docs index). This skill adds working detail;
keep the two consistent.

## Project identity

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade. The
CLI is static-only; the Python facade also supports bounded synchronous custom
handlers; `eggserve.lowlevel` exposes a handler-only runtime/service substrate
(`RuntimeConfig`/`Server`, `Response.stream`, `StaticResponder` composition)
for downstream bounded application servers; and `eggserve-core::server` exposes
an experimental, low-level Rust service boundary. EggServe is not an application
framework, ASGI/WSGI runtime, CGI executor, FastCGI gateway, proxy, or
general-purpose `socketserver` replacement. Plan 167 closed as no-go: no
in-tree CGI/FastCGI adapters; downstream gateways implement the canonical
`Service` trait.

**Not** a general web server, framework, ASGI/WSGI runtime, or Granian replacement.

## Workspace layout

Three crates:
- `crates/eggserve-core/` — library: security primitives, path confinement, HTTP serving, response construction
- `crates/eggserve-bin/` — binary: CLI, accept loop, signal handling (depends on eggserve-core)
- `crates/eggserve-python/` — Python wheel packaging (maturin + PyO3, depends on eggserve-core; excluded from workspace; packages the native extension and extension-backed CLI, with no separate bundled executable)

Other directories: `architecture/` (deep-dive docs), `docs/` (reference docs),
`plans/` (historical design/implementation records plus the `ROADMAP.md` and
`RELEASE-READINESS-ROADMAP.md` roadmap files),
`examples/` (canonical CLI/Python examples plus Cargo examples and tiny
fixtures), `fuzz/`, and `scripts/` (small fast/full/deep verification hierarchy
plus package/release checks). The example index is `examples/README.md`.

## Non-negotiables

1. **Safe defaults** — loopback bind, no symlinks, no dotfiles, no directory listing. Every unsafe behavior requires explicit opt-in via CLI flag.
2. **No serving outside root** — path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative traversal via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`.
3. **No broad dependencies** — every dependency must have an explicit purpose. See `docs/dependency-policy.md`.
4. **Plan-driven development** — every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

Keep detailed Python deviations in `docs/python-http-server-compatibility.md`
and detailed Rust ownership in `architecture/runtime.md`; plans record change
history and are not prerequisites for understanding current behavior.

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
# Builds the extension-backed wheel, installs it in a venv, runs smoke + tests
```

Manual platform qualification is separate from routine CI:

```sh
gh workflow run platform-qualification.yml --ref main
```

It exercises the installed wheel on macOS arm64 and the Windows adversarial
filesystem suites. The Windows suite explicitly skips the two cases where
NTFS rejects an external path-based root rename while a descendant handle is
open. Keep Windows support language aligned with the evidence in
`docs/toolchain-support.md` and `docs/security-review.md`.

Or use the local verification script:

```sh
./scripts/verify.sh fast                 # routine dev check (Rust workspace + Python crate check)
./scripts/verify.sh full                 # pre-release validation (examples, Rust + Python wheel)
./scripts/verify.sh deep                 # expensive suites (manual)
```

### Supply-chain and optional package checks

Not run in routine CI. Run manually when preparing a release:

```sh
bash scripts/install-cargo-tools.sh     # deterministic audit/deny installation
cargo audit                             # vulnerability check
cargo deny check                        # license/policy check
bash scripts/verify-cargo-packages.sh --mode all  # package dry-run gates
```

Routine CI runs `cargo audit` and `cargo deny check` in a dedicated
supply-chain job after installing the pinned tools. The package dry-run remains
manual release validation.

## Key conventions

- **Manual argument parsing** in `args.rs` — no clap dependency. The CLI grammar
  is `[OPTIONS] [PORT] [DIRECTORY]`; positional parsing owns those two logical
  slots, treats a directory after an occupied port slot verbatim (including a
  numeric name), and rejects excess positionals. A host-only `--bind` leaves
  the port slot available; `--directory` occupies the directory slot.
- **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing) and `policy::DotfilePolicy` (serving). Both must agree.
- **eggserve-python excluded from workspace** — has its own Cargo.lock, built via maturin. Don't run `cargo test --workspace` for Python crate.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses
- **`#[allow(dead_code)]` on public API types** — consumed externally (Python bindings)
- **Error taxonomy** — Five distinct error types: `PathRejection` (17 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-facing), `ServerError` (10 variants, server lifecycle), `ServiceErrorKind` (private kind enum behind the public `ServiceError` struct; 4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`), `RequestBodyError` (12 variants, body consumption). See `architecture/error-taxonomy.md`.
- **Plan status** — Plans are historical change-trace records. The current product and compatibility contract is owned by `README.md`, `docs/python-http-server-compatibility.md`, and the relevant architecture pages. Production servers use the shared `RuntimeState` admission pool.
- **Canonical HTTP types (stable)** — `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo` (`local_addr`/`remote_addr` are `Option<SocketAddr>`; non-socket transports expose `None` via `SocketEndpoints`/`without_socket_addrs`), `StatusCode`, `ResponseHead`, `ResponseBody` (`Empty`/`Bytes`/`File`/`Stream`/`EmptyWithLength`), `Response`, `BodyLength` (`Known`/`Unknown`), `ResponseStream`/`ResponseStreamError`, `normalize_response()` are all stable.
- **ResponseStream producer bound** — `ResponseStream::new`/`with_known_length` accept `Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static`; `Sync` is intentionally not required. The stream is one-shot and exclusively polled by its owning connection task. `Response` and the internal transport body remain `Send`, while concurrent body polling is unsupported.
- **Canonical response semantics** — `StatusCode` accepts 100–599 only; 1xx/204/205/304 are body-forbidden (only 304 may retain a matching representation `Content-Length`); weak metadata ETags may satisfy `If-None-Match` but never `If-Range`; exactly 0/1 authoritative `Date` per `DatePolicy` at final construction (`SystemClock` default = one `Date`; `Suppress` = zero; Hyper `auto_date_header(false)`). `normalize_response` maps every body-forbidden status except 304 to `BodyLength::Known(0)` and drops HEAD/body-forbidden streams without polling (prompt producer release). `BodyLength::Unknown` never becomes `Content-Length: 0`; unknown HEAD omits the header. Normalization is idempotent (`Response::is_normalized`, mutation via `head_mut`/`take_body` clears it) so the static service can normalize eagerly while the connection pipeline normalizes every service response. The runtime is the only framing authority for `Content-Length`/`Transfer-Encoding`/reuse. Python callback conversion stages headers and body ownership atomically; malformed body state never falls back to an empty response.
- **Canonical response normalization** — All response producers converge on `primitives::canonical::normalize_metadata()`.
- **Plan 165 response privacy** — `RuntimeConfig.response_policy: ResponsePolicy` owns `server_identification` (`None` suppressed default; `builder.server_header(..)` / `config.server_header_value()`), `date_policy` (`SystemClock` default, `Custom(provider)` trusted time, `Suppress` RFC tradeoff), `stripped_response_headers` (validated denylist, post-service, no framing/`date`/`content-range`, `minimal_fingerprint()` strips `x-powered-by`), `error_policy` (`Minimal` fixed bodies default, `Empty` runtime-errors-only; app `Ok` never rewritten). `StaticPolicy.static_metadata` (`standard()` vs `minimal_fingerprint()`; planner `plan_file_response_with_preconditions_and_metadata`). `ServeConfig.error_policy` transferred by `try_from_serve_config`. CLI keeps standards defaults; Python `lowlevel` exposes the safe subset (`server_header`, `system`/`suppress`, denylist, `minimal`/`empty`) with `Custom` clocks Rust-only.
- **`server` module types** — `eggserve-core::server` provides the runtime service boundary for embedding. The module is experimental; API may change.
- **Transport-neutral driver** — `server::connection::serve_http1_connection` drives a canonical `Service` over any `AsyncRead + AsyncWrite` stream with explicit `ConnectionContext`, shared `Arc<RuntimeState>` (`RuntimeState::new(&config)`), and per-connection `ConnectionShutdown` returning `ConnectionOutcome`. TCP/TLS `Server` shares the same pipeline via `serve_http1_connection_with_id`; raw Hyper helpers are crate-private. No fabricated socket addresses, no Hyper types in the driver signature.
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
- `policy-system.md` — StaticPolicy, symlink/dotfile/listing/static-metadata policies plus `ResponsePolicy`/`DatePolicy`/denylist/error profile
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
- Error taxonomy: `PathRejection` (17 variants, path validation), `RequestValidationError` (6 variants, HTTP-level), `ServerError` (10 variants, lifecycle), `ServiceErrorKind` (private, 4 kinds behind the public `ServiceError` struct), `RequestBodyError` (12 variants, body consumption)
- `BodyPlan` variants: `Empty`, `FullBytes(Vec<u8>)`, `FileFull`, `FileRange { start, end_inclusive }`
- `ResponseStatus` is a struct with associated constants, not an enum
- `FileRange` is a struct `{ start: u64, end_inclusive: u64 }`, not an enum
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`; it also owns `static_metadata: StaticMetadataPolicy` (use `..Default` in literals)
- **`ResolvedFile` extraction methods** — `from_parts()`, `into_std_file()`, `into_parts()` are `pub` behind the `python-bindings-internal` feature (for cross-crate Python bindings) but carry security caveats: confinement guarantee ends after extraction.
- **Python server façade** — `eggserve.server` is the supported six-class API, including rustls-backed `HTTPSServer` and `ThreadingHTTPSServer` with HTTP/1.1 ALPN only. The exact fast-path eligibility and intentional incompatibility contract is maintained in `docs/python-http-server-compatibility.md`. Stock static handlers also support `default_content_type` and ordered safe `extra_response_headers`; those headers are limited to final 200 responses. Handler `protocol_version` must remain HTTP/1.1.
- **Python lowlevel substrate (Plan 166)** — `eggserve.lowlevel` exposes handler-only `Server(config, handler)` (no static root, same native runtime, no second accept loop), frozen `RuntimeConfig` (Plan 164 controls + safe privacy subset: `server_header`/`date_policy` system|suppress/`stripped_response_headers`/`error_policy` minimal|empty; `None` disables, `0` never unlimited), bounded `Response.stream(status, iterable, headers, content_length)` over a 16-chunk bridge (HEAD/body-forbidden never advance the iterator; async rejected; non-bytes/iterator errors truncate with sanitized type-only logs; no `Transfer-Encoding` from services), and caller-owned `StaticResponder` composition (no routing in EggServe).
- **CLI compatibility polish** — Manual parsing accepts hostname `--bind` values, repeatable `-H/--header` and `--content-type` static metadata, and a combined certificate/key PEM when `--tls-key` is omitted. Header metadata is validated against runtime-owned and hop-by-hop fields. Production admission/lifecycle CLI flags: `--max-in-flight-requests`, `--keep-alive-idle-timeout`, `--max-requests-per-connection` (`0` = unlimited), `--response-write-timeout`, `--max-buf-size`, `--max-headers`, `--max-header-bytes`, `--max-request-target-bytes`.
- **Python wheel support** — CPython 3.11+ with abi3 stable ABI. Routine CI builds and tests the Linux wheel; macOS and Windows wheels are built manually. Release wheels target 9 platforms: manylinux_2_17 (x86_64, aarch64, armv7l), musllinux_1_2 (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64, arm64).
- **Semaphore bounds** — `max_connections`, `max_file_streams`, and `max_in_flight_requests` are validated against `tokio::sync::Semaphore::MAX_PERMITS` in both `Limits::validate()` and `RuntimeConfigBuilder::build()`. Values above this bound are rejected with a controlled error.
- **Plan 164 admission/lifecycle fields** — `RuntimeConfig`/`Limits` own `max_buf_size` (65536, Hyper min 8192), `max_headers` (100, pinned explicitly; Hyper answers excess with 431), `max_header_bytes` (32 KiB, 431 pre-service), `max_request_target_bytes` (8192, 414 pre-service), `max_in_flight_requests` (64, 503 on exhaustion, held across `Service::call`), `keep_alive_idle_timeout` (60s, resets on activity), `max_requests_per_connection` (`Option<u64>`, `None` = unlimited; CLI `0` = unlimited), `response_write_timeout` (30s, no-progress via `ProgressIo` + `TrackedBody`). Idle/write timeouts are NOT cross-checked against `connection_total_timeout`. `ConnectionOutcome` adds `IdleTimeout` (clean) and `WriteTimeout`. Hyper is 1.11.1: lone TE+CL normalizes to TE-wins (200), only duplicate/conflicting CLs still fail; Hyper also applies `header_read_timeout` while keep-alive idle, so set idle shorter for distinct accounting or raise both for long-lived keep-alive. Per-profile defaults live in `docs/deployment.md`; full semantics in `docs/timeout-reference.md`.
- **Logging modes** — `--log-format none` uses `NopLogSink` (no output). `--quiet` wraps the format-specific sink with `FilteredLogSink` (warn/error only). Direct argument-validation errors printed before logger initialization may remain on stderr.
- **Release validation** — run `bash scripts/install-cargo-tools.sh` before `cargo audit`/`cargo deny check`.
- **`server` module is experimental** — `eggserve-core::server` provides the runtime service boundary. Its API is subject to change without notice.
- **Production profiles** — Production profiles are documented in README.md and `docs/deployment.md`. Every production claim must name a profile. Hardened profiles must not allow symlink following. Windows is functionally qualified, but remains trusted/local-content only because two open-descendant root-rename cases are rejected by NTFS path-rename semantics; see `docs/toolchain-support.md`.
- **`ops` module** — `Logger` uses `OnceLock` for global initialization. `try_init()` is for Python bindings that may coexist with CLI initialization. Do not call `Logger::init()` twice.
- **No println/eprintln in library code** — The core library must use `Logger::global().emit()` for all operational output.
- **Examples are product demonstrations** — Use the canonical examples in `examples/README.md` when documenting CLI, Python, or Rust usage. Rust examples must use public APIs only; listener-based server examples bind loopback, support port `0` for smoke tests, wait for readiness, and cleanly shut down on Ctrl+C (`caller_owned_stream.rs` binds nothing by design; `primitives.rs` opens no socket). Python examples expose `create_server()` for smoke tests. Do not turn examples into a framework, router, or alternate policy reference.
- **Qualification evidence (Plans 168/170)** — Plans 168/170 are qualification phases: deterministic suites per track plus a manual, same-machine performance matrix (see `architecture/testing-and-conformance.md` mapping), never absolute-timing CI gates. Benchmark methodology, regression policy, and claims policy live in `benchmarks/README.md`; machine-readable results are in `benchmarks/088-baseline/results.json`, `benchmarks/168-qualification/results.json`, and `benchmarks/170-closure/results.json`. Every performance/release claim must name a profile + workload + evidence; forbidden claims (edge-proxy parity, DDoS resistance, un-fingerprintability, ASGI/WSGI parity, HTTP/2/3, universal superiority headlines) stay forbidden.
- **Response-planning edge semantics** — Inverted ranges (`start > end`, e.g. `bytes=50-10`) are invalid specifiers and the Range header is ignored (full 200), never 416 (RFC 9110 § 14.1.2); 416 is only for unsatisfiable-but-valid ranges (start beyond EOF, empty file). `evaluate_if_match("*", None)` is `false` (no representation, nothing to match). HEAD normalization retains a known representation length via `ResponseBody::EmptyWithLength` (zero wire bytes; unknown lengths stay `Empty`); body-forbidden statuses still normalize to `Empty`. A literal `#` with no `?` is an ordinary path character (documented, tested), not a fragment delimiter.
- **Rust package boundary** — `eggserve-core` is the intended 0.x Rust library crate. `eggserve-core::primitives` is the semver-considered facade; `eggserve-core::server` is experimental. `eggserve-bin::run_cli` exists for the Python wheel's extension-backed CLI and is not a general Rust embedding API. Do not add a facade crate for naming convenience.
- **Rust closure verification** — For library/CLI usability work, run `cargo test --doc -p eggserve-core`, `cargo check -p eggserve-core --examples`, both dist builds, and `bash scripts/verify-cargo-packages.sh --mode all`; use a temporary clean external consumer for static and custom-service TCP smokes when the plan requires it. For native bind/TLS changes, run the manual platform qualification workflow after pushing the final SHA.
