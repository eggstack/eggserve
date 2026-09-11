# Guide for AI coding agents

## Project overview

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade. Static
serving is the primary product; a separate downstream project may use the
qualified HTTP-only Rust substrate, but EggServe itself is not an application
server. The
CLI is static-only; the Python facade also supports bounded synchronous custom
handlers; `eggserve.lowlevel` exposes a handler-only runtime/service substrate
(`RuntimeConfig`/`Server`, `Response.stream`, `StaticResponder` composition)
for downstream bounded application servers; and `eggserve-core::server` exposes
an experimental, low-level Rust service boundary. EggServe is not an application
framework, ASGI/WSGI runtime, CGI executor, FastCGI gateway, proxy, or
general-purpose `socketserver` replacement. Plan 167 closed as no-go: no
in-tree CGI/FastCGI adapters; downstream gateways implement the canonical
`Service` trait (see `docs/extension-contract.md`, `docs/non-goals.md`). The
`server` module remains experimental even though the HTTP bridge is qualified
by Plan 175. Plan 176 closed as deferred: no generic HTTP upgrade handoff
(`Request` has no upgrade capability, `Service` returns `Response` only, 101 cannot survive normalization; downstream must not bypass via Hyper). The user-facing Python compatibility contract lives in [docs/python-http-server-compatibility.md](docs/python-http-server-compatibility.md).

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
python3 scripts/check-python-release-metadata.py            # version + [profile.dist] sync (cheap, before builds)
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings   # warnings are errors
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                   # TLS tests
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls

# python job: bash scripts/test-python-wheel.sh
# preflight re-runs check-python-release-metadata.py, then
# builds wheel with maturin, installs in venv, runs smoke + tests
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

### Local verification script

```sh
./scripts/verify.sh fast                 # routine dev check (Rust workspace + Python crate check)
./scripts/verify.sh full                 # pre-release: fast + TLS + examples + Python wheel
./scripts/verify.sh deep                 # expensive suites (manual): fuzz replay, races, proxy interop
bash scripts/qualify-http2.sh             # manual Linux H2 wire/ALPN qualification
bash scripts/qualify-http3.sh             # manual H3/QUIC qualification; direct clients required for wire evidence
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
- **Package roles**: `eggserve-core::primitives` is the semver-considered facade; `eggserve-core::server` is experimental (API may change). `eggserve-bin::run_cli` is plumbing for the Python wheel's extension-backed CLI, **not** a general embedding API — Rust embedders use `eggserve-core`. Canonical types, `Service`, and the caller-owned connection API are Hyper-free; `RequestHead::try_from_hyper()` and `primitives::to_hyper_response()` are the two intentional public conversion adapters.
- `crates/eggserve-bin/src/main.rs` is a 2-line shim; real logic is in `lib.rs`/`args.rs`.

### Code shapes agents get wrong

- **Two DotfilePolicy types**: `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served.
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`.
- `ResponseStatus` is a struct with associated constants, not an enum. `FileRange` is a struct `{ start, end_inclusive }`, not an enum. `BodyPlan` variants: `Empty`, `FullBytes(Vec<u8>)`, `FileFull`, `FileRange { start, end_inclusive }`.
- **Plan 164 admission/lifecycle fields** — `RuntimeConfig`/`Limits` also own `max_buf_size` (default 65536, Hyper minimum 8192), `max_headers` (default 100, pinned explicitly), `max_header_bytes` (default 32 KiB, 431 pre-service), `max_request_target_bytes` (default 8192, 414 pre-service), `max_in_flight_requests` (default 64, 503 on exhaustion), `keep_alive_idle_timeout` (default 60s, resets on activity), `max_requests_per_connection` (`Option<u64>`, default `None`), and `response_write_timeout` (default 30s, no-progress). `keep_alive_idle_timeout`/`response_write_timeout` are intentionally NOT cross-checked against `connection_total_timeout` (the hard ceiling). CLI exposes all eight (`--max-in-flight-requests`, `--keep-alive-idle-timeout`, `--max-requests-per-connection` with `0` = unlimited, `--response-write-timeout`, `--max-buf-size`, `--max-headers`, `--max-header-bytes`, `--max-request-target-bytes`).
- **Plan 179 canonical runtime authority** — shared runtime defaults/validation live once in `eggserve-core::runtime_limits` (`SharedRuntimeValues` + `Violation`, crate-private). `Limits::validate()` delegates shared checks + static listing budgets; `RuntimeConfigBuilder::build()` validates the shared group + `ResponsePolicy`; `try_from_serve_config()` projects via `RuntimeConfig::from_shared_runtime`. Static listing/extra-header budgets stay service-owned; frontend-only controls stay in their surfaces. Services may lower `max_request_body_bytes` but never raise the hard ceiling.
- **Plan 165 response privacy (fingerprint minimization)** — `RuntimeConfig.response_policy: ResponsePolicy` owns `server_identification` (`None` = suppressed default; use `builder.server_header(..)` / `config.server_header_value()`), `date_policy` (`SystemClock` default, `Custom(provider)` trusted time value, `Suppress` explicit RFC tradeoff), `stripped_response_headers` (validated denylist, post-service, no framing/`date`/`content-range`, `minimal_fingerprint()` strips `x-powered-by`), and `error_policy` (`Minimal` default fixed bodies, `Empty` no bytes for runtime errors only; app `Ok` never rewritten). `StaticPolicy.static_metadata: StaticMetadataPolicy` (`standard()` emits `ETag`+`Last-Modified`, `minimal_fingerprint()` suppresses both; planner `plan_file_response_with_preconditions_and_metadata`). `ServeConfig.error_policy` transferred by `try_from_serve_config`. Hyper `auto_date_header(false)` — EggServe is sole `Date` authority (exactly 0/1 `Date` per policy); `Last-Modified <= Date` enforced. CLI keeps standards defaults; Python `lowlevel` exposes the safe subset (`server_header`, `system`/`suppress`, denylist, `minimal`/`empty`) with `Custom` clocks Rust-only.
- **ConnectionOutcome variants** — `Normal`, `ClientError`, `HeaderTimeout`, `IdleTimeout` (clean), `WriteTimeout`, `TotalTimeout`, `Shutdown`, `Internal`. `is_clean()` is true for `Normal`/`Shutdown`/`IdleTimeout`.
- Hyper is 1.11.1 (TE-wins normalization for lone CL+TE; stricter `max_buf_size` enforcement). Lone `Transfer-Encoding + Content-Length` now reaches the service as chunked (200), not 400; only duplicate/conflicting CLs still fail.
- Hyper also applies `header_read_timeout` while a keep-alive connection sits idle: with defaults, idle gaps close as header timeouts. Set the idle timeout shorter than the header timeout for distinct idle accounting; raise both for long-lived keep-alive (see `docs/deployment.md` per-profile defaults).
- **Error taxonomy** — five types: `PathRejection` (17 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-facing), `ServerError` (10 variants, lifecycle, `#[non_exhaustive]`), `ServiceErrorKind` (private kind enum behind the public `ServiceError` struct; 4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`; struct stays future-proof, inspect via `is_panic`/`is_timeout`), `RequestBodyError` (14 variants, body consumption incl. `InvalidTrailers`/`TrailersNotReady`, `#[non_exhaustive]`). `RequestCancellationReason` + `ConnectionOutcome` are also `#[non_exhaustive]` (match with wildcard). `ServiceError::rejected` preserves `200..=599` (`1xx`/out-of-range → 500) with a truthful central body (`runtime_error_with_policy`, no detail leak; `HEAD`/`Empty`/body-forbidden empty). Never synthesize a second HTTP error after final commitment. See [architecture/error-taxonomy.md](architecture/error-taxonomy.md).
- `telemetry.rs` does not exist — do not create it. `clap` was removed (manual parsing in `args.rs`). `tracing` was never added (custom logging).
- `#[allow(dead_code)]` on public API types — consumed externally by Python bindings, not dead.
- Frozen Python classes — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability enforced at both layers.
- `ResolvedFile::from_parts()/into_std_file()/into_parts()` are `pub` behind the `python-bindings-internal` feature for cross-crate bindings, but the confinement guarantee ends after extraction.

### HTTP semantics

- **RequestBody is one-shot** — consumable once via `read_all` or streaming (`read_all_with_trailers` for trailers). `Service::call` takes `Request` by value. Python `read()`/`iter_chunks()` are mutually exclusive; second use raises `RequestBodyConsumedError`. Plan 174: Stream bodies share a lifecycle (Active→Complete/Abandoned/Failed, Drop-derived for network bodies; in-memory copies never force close); service may return response-start with Active body delegated to a task, reuse waits for Complete, Abandoned/Failed forces close (Hyper-pinned). `Request::lifecycle()`/`into_parts_with_lifecycle()` expose transport-neutral `RequestLifecycle` (`cancelled()`, `is_cancelled()`, `cancellation_reason()`; PeerDisconnected/ServerShutdown/ConnectionTimeout/TransportFailure, first wins, `#[non_exhaustive]` — match with wildcard). Plan 197: `RequestContext` (`connection()` + `lifecycle()`) is the single attachment point for transport metadata + future opaque capabilities (no type map, no raw handles); prefer `Request::context()`/`new_with_context()`/`into_parts_with_context()` for new code — `connection()`/`lifecycle()`/`into_parts*` forward to the context and preserve the Plan 175 path. Plan 198: `RequestContext::interim()` is the bounded 1xx sender (only 1xx no 101, no body/trailers, no post-commit, HTTP/1.0 suppressed, single 100); `RequestBody::trailers()`/`read_all_with_trailers()` expose terminal trailers only after completion (separate bounds, `InvalidTrailers` on failure, H1 without valid framing cannot inject). Stream `Service::call` stays collapsed as `min(body, handler)` for compat (disambiguated via lifecycle); remaining body timeout continues after return via watchdog. `max_in_flight_requests` bounds pre-response `Service::call` only; downstream app admission is separate.
- Canonical response semantics: `StatusCode` accepts 100–599 only; 1xx/204/205/304 are body-forbidden (only 304 may retain a matching representation `Content-Length`); weak metadata ETags satisfy `If-None-Match` but never `If-Range`; exactly 0/1 authoritative `Date` per `DatePolicy` at final construction (`SystemClock` default = one `Date`; `Suppress` = zero; Hyper auto-`Date` disabled). `normalize_response` maps every body-forbidden status except 304 to `BodyLength::Known(0)` and drops HEAD/body-forbidden streams without polling (prompt producer release). `BodyLength::Unknown` (streaming) never becomes `Content-Length: 0`; unknown HEAD omits the header. Normalization is idempotent (`Response::is_normalized`, mutation via `head_mut`/`take_body` clears it) so the static service can normalize eagerly while the connection pipeline normalizes every service response. The runtime is the only framing authority for `Content-Length`/`Transfer-Encoding`/reuse. All producers converge on `primitives::canonical::normalize_metadata()`.
- Stable canonical types: `Method`, non-exhaustive `HttpVersion` (`Http10`, `Http11`, `Http2`, `Http3`), `Authority`, `HeaderBlock`, `HeaderValue` (octet-preserving; `from_bytes`/`as_bytes()`, fallible `to_str()`; `OWS` stripped), `HeaderValueTextError`, `RequestTarget` (`raw_bytes()`/`path_bytes()`/`query_bytes()`; empty query → `None`), `RequestHead` (including validated effective authority), `RequestLifecycle`/`RequestCancellationReason` (`#[non_exhaustive]`), `RequestContext` (single attachment point: `connection()` + `lifecycle()` + bounded `interim()`), `ConnectionInfo` (`local_addr`/`remote_addr` are `Option<SocketAddr>`; non-socket transports expose `None` via `SocketEndpoints`/`without_socket_addrs`), `StatusCode`, `ResponseHead`, `ResponseBody` (`Empty`/`Bytes`/`File`/`Stream`/`EmptyWithLength`), `Response` (`strip_response_trailers`/`has_response_trailers`), `BodyLength` (`Known`/`Unknown`), `ResponseStream`/`ResponseStreamError` (`with_trailers`/`with_known_length_and_trailers`), `Trailers`/`TrailerLimits`, `InterimSender`/`InterimLimits`, `normalize_response()`.
- **Protocol-neutral request preparation (Plans 184–187)** — `RequestTarget::parse()` is the sole HTTP target-form classifier. Static/runtime code passes its validated `path()` to `ConfinedPath::from_path_component()`; the raw `ConfinedPath::parse()` API remains a compatibility adapter. `Authority` maps validated HTTP/1 `Host` or HTTP/2/3 authority metadata without exposing pseudo-header names. Default builds remain HTTP/1-only; `http2` adds bounded H1/H2 Rust serving and `http3` adds the experimental H3/QUIC adapter, while Python remains HTTP/1.1-shaped. H3 binds UDP to the resolved TCP port, uses a separate TLS 1.3/`h3` configuration, disables 0-RTT, and shares canonical service/admission/response policy.
- **Plans 189–190 multiprotocol correctness/qualification** — H1 Reject remains framing based; H2 Reject uses Hyper's public `Incoming::is_end_stream()` and H3 performs one bounded receive probe when `Content-Length` is absent/zero, so DATA cannot reach a Reject service. Focused H2/H3 tests cover DATA without `Content-Length`, bodyless dispatch, H3 zero-length-plus-DATA, bounded probe timeout, sibling isolation, detached lifecycle wake-up, (Plan 192) early-error stream scoping plus close-race survival, and (Plan 194) stalled H3 producer timeout with sibling survival. Shared `RequestBody` consumption checks both under- and over-declared lengths. H3 reuses the connection lifecycle registry for peer/shutdown cancellation and stream-local failure handling. Runtime error status/reason/body construction is canonical across H1/H2/H3. H2 response timeout accounting observes application-body producer/poll progress only (no guaranteed wire progress, no public Hyper stream reset; stall uses connection fallback). H3 observes per-stream producer + send progress with stream-scoped reset (siblings survive). Both remain experimental. See `release/plan-190-multiprotocol-corrective-qualification.md`.
- **Shared service kernel and lifecycle** — body-policy branches prepare `Request` values, then one internal invocation helper owns service admission, panic containment, timeout, error conversion, normalization, and conversion. It returns typed `LifecycleDisposition` state; only the HTTP/1 adapter maps close requirements to `Connection: close`. Request/response activity identities are distinct from aggregate connection activity.
- **Protocol-owned configuration** — Plan 179 shared defaults remain in `runtime_limits`; HTTP/1 parser knobs are projected through internal `Http1Config` without a second defaults table. H2/H3 controls must use protocol-owned projections. Hyper upgrade machinery is not enabled. The workspace MSRV is Rust 1.88; keep dependency updates compatible with the pinned MSRV and supply-chain checks.
- **ResponseStream producer bound** — `ResponseStream::new`/`with_known_length` accept `Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static`; `Sync` is intentionally not required. The stream is one-shot and exclusively polled by its owning connection task. `Response` and the internal transport body remain `Send`, while concurrent body polling is unsupported. Plan 198 adds `with_trailers`/`with_known_length_and_trailers` for one terminal trailer block (no data after, `HEAD`/body-forbidden never poll, known length counts data only, adapters map without buffering).
- **Outbound response conversion boundary** — `primitives::to_hyper_response()` is an explicit low-level adapter with an opaque `http_body::Body<Data = Bytes, Error = io::Error>` body; downstream code must not name `BoxBody`/`UnsyncBoxBody`. The semaphore-aware helper is runtime-internal. The current `main` contract is the intentional `0.1.x` → `0.2.0` pre-1.0 transition documented in `docs/migration-guide.md`.
- Listener accept errors are classified by `io::ErrorKind` (transient/resource-exhaustion/persistent) with bounded exponential backoff — use `classify_accept_error()`.
- Transport-neutral driver: `server::connection::serve_http1_connection` remains strict HTTP/1, while feature-gated `serve_http_connection` accepts HTTP/1 or bounded HTTP/2 prior knowledge over any `AsyncRead + AsyncWrite` stream. Both drive a canonical `Service` with explicit `ConnectionContext` (no fabricated addresses, no Hyper types, scheme/TLS asserted by caller), shared `Arc<RuntimeState>` admission (constructed via `RuntimeState::try_new(&config)` preferred, `new(&config)` validates + panics), and per-connection `ConnectionShutdown` returning `ConnectionOutcome`. Invalid hand-constructed `RuntimeConfig` is rejected at `ServerBuilder::build()`, `RuntimeConfig::validate()`, `RuntimeState::try_new()`, and the caller-owned entry (logs + `Internal`) before semaphore/Hyper use. `ConnectionShutdown` is level-triggered and idempotent (pre-signaled shutdown observed promptly, no polling). TCP/TLS `Server` selects H1/H2 through ALPN or the bounded prior-knowledge classifier; raw Hyper helpers are crate-private.
- **HTTP/3 boundary (Plans 187–190, 192–195)** — `RuntimeConfig::http3` owns explicit QUIC/H3 windows, stream/handshake/field-section/send-buffer/idle/retry limits. `ServerBuilder::http3_identity` supplies PEM paths without exposing Quinn/rustls types; the runtime creates a separate TLS 1.3 QUIC config and starts UDP atomically after TCP bind. H3 request streams map to the canonical `Request`/`Service`/`Response` path, including body limits, strict content length, per-stream reset, GOAWAY drain, and runtime-owned `Alt-Svc`. Plan 190 adds in-process DATA/bodyless/timeout/lifecycle qualification; Plan 192 adds early-error receive aborts plus early-error scoping and close-race regressions, and closes `BLOCKED` on the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11): upstream `hyperium/h3#338` has no released fix and the `#262` stream-drop remainder is unresolved. Plan 193 closed at preflight on 2026-09-10 without entering promotion qualification (unmet Plan 192 prerequisite; unchanged candidate; `#338`/`#262` still open; two-family, browser, adversarial, impairment, and platform evidence inventoried as unavailable). Plan 194 bounds the H3 `ResponseStream` producer poll with `response_write_timeout` no-progress semantics (absolute deadline, empty chunks are not progress, per-stream reset, `WriteStallTimeout` observed, siblings survive) and corrects the promotion-trace docs; tier unchanged. Plan 195 correctively qualifies that bound (H3 suite 14 → 16: shutdown-race drain plus write-stall observability/permit-release regressions) with no source change; tier unchanged. H3 remains experimental; a future promotion requires a new scoped plan. See `release/plan-193-http3-supported-tier-qualification.md`, `release/plan-194-http3-producer-timeout-correction.md`, and `release/plan-195-http3-response-timeout-corrective-qualification.md`.
- **Downstream app-server consumer (Plan 175) + application-service contract (Plan 197)** — `crates/eggserve-core/tests/app_server_consumer.rs` is the external-consumer qualification: bounded full-duplex bridge (cap-2 channels, no `read_all`, no Hyper/private imports; fixture-local event names only), deferred ownership, lifecycle cancellation, handler/body timeout split, downstream admission split, TCP/TLS/caller-owned parity, non-gating perf sanity. `crates/eggserve-core/tests/application_service_contract.rs` is the Hyper-free stabilized-contract fixture (`RequestContext`, buffered/streamed/lifecycle, `#[non_exhaustive]` wildcards, runtime admission 503); `crates/eggserve-core/examples/application_service.rs` is the minimal native demo (no static FS). Builder-facing rules + normative 7-stage commitment/cancellation + `Send + Sync` (no `poll_ready`) + error-taxonomy rules live in `docs/downstream-app-server.md`; EggServe itself is not an app server/ASGI runtime. `Service::call` stays `Response`-only (no `ServiceOutcome`; Track C decision).
- **Downstream substrate closure (Plans 172/177)** — Plans 172–175 close the qualified HTTP-only downstream-substrate line; Plan 176 remains conditional on a concrete upgrade consumer. Keep separate application-server work in its own project and preserve the Plan 175 public-API/bounded-coordination boundary.
- **Response-planning edge semantics (Plan 168)** — inverted ranges (`start > end`) are invalid: the Range header is ignored (full 200), never 416 (RFC 9110 § 14.1.2); `evaluate_if_match("*", None)` is `false`; HEAD normalization retains known lengths via `ResponseBody::EmptyWithLength` (zero wire bytes); a literal `#` with no `?` is an ordinary path character.

### CLI

- **Manual argument parsing** in `args.rs` — no clap. Grammar `[OPTIONS] [PORT] [DIRECTORY]`; positionals own those two slots; a directory after an occupied port slot is taken verbatim even if numeric; excess positionals rejected. Host-only `--bind` leaves the port slot free; `--directory` occupies the directory slot. `--bind` accepts hostnames resolved once before listener startup; `--tls-cert` may name a combined cert/key PEM when `--tls-key` is omitted.
- **CLI runtime is current-thread**; the Python facade uses `rt-multi-thread` with 2 worker threads (GIL scheduling). The library itself is runtime-agnostic.
- Logging flags: `--log-format none` → `NopLogSink`; `--quiet` → `FilteredLogSink` (warn/error). Argument-validation errors printed before logger init may still hit stderr.

### Structured logging

- Library code must not use `println!`/`eprintln!` — runtime code uses the explicit `OpsContext` (`ops.emit(Event::new(...))`); CLI/frontend init keeps the `Logger::global()` compat path.
- **Plan 181 per-runtime observability** — `RuntimeState` owns an `OpsContext` (sink + counters + correlation IDs). `RuntimeState::with_ops` / `ServerBuilder::ops_context` attach explicit contexts; `new`/`try_new` clone the global default. `ConnectionActivity` carries the connection's context; connection modules take `ops` params or read `activity.ops()`. Standalone canonical conversions (`to_hyper_response`) keep the documented process-global fallback via `resolve_ops(None)`.
- Connection IDs start at 1 per context (`OpsContext::next_connection_id`); the old static `NEXT_CONN_ID` is gone. Explicit caller IDs via `serve_http1_connection_with_id` still win.
- Sink-failure accounting is context-local: `CompositeLogSink::with_failure_counters` / `OpsContext::with_sinks`; plain `new()` keeps global accounting. Never re-emit failures through a logger (no recursive sink-graph traversal; the counter is the signal).
- Bounded snapshots: `RuntimeState::ops_snapshot()` / `ServerHandle::ops_snapshot()` / `OpsContext::snapshot()` (never reset-on-read, no exporter). `ops` event/sink/counter vocabulary is semver-considered pre-1.0; runtime attachment is experimental with `server`.
- `Logger` uses `OnceLock`; `try_init()` exists for Python bindings coexisting with CLI init and adopts the sink into the global default. Never call `Logger::init()` twice.
- `max_connections`/`max_file_streams`/`max_in_flight_requests` are validated once in the Plan 179 kernel against `tokio::sync::Semaphore::MAX_PERMITS`; larger values are rejected.

### Python facade

- Supported API is `eggserve.server`: `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`, `ThreadingHTTPSServer`, `BaseHTTPRequestHandler`, `SimpleHTTPRequestHandler`. Advanced primitives live in `eggserve.lowlevel`; CLI subprocess helpers are canonically owned by `eggserve.subprocess` (`eggserve.server` keeps compatibility re-exports without expanding `__all__`; top-level `serve_directory` re-exports the subprocess implementation). Native callback/client types are not top-level supported APIs.
- `eggserve.lowlevel` is the public runtime/service substrate: handler-only `Server(config, handler)` requiring no static root (same native runtime as the facade, no second accept loop), frozen `RuntimeConfig` (Plan 164 controls + safe privacy subset, `None` disables, `0` never means unlimited; projected via the single `_native_kwargs()` helper with Rust as final limit authority), bounded `Response.stream(status, iterable, headers, content_length)` over a 16-chunk backpressured bridge (HEAD/body-forbidden never advance the iterator; async producers rejected), and `StaticResponder` composition owned by the caller (no routing in EggServe).
- Stock `SimpleHTTPRequestHandler` with default settings bypasses Python dispatch entirely (native fast path). Eligibility is exact: bare class, or a `functools.partial` whose `.func` is exactly `SimpleHTTPRequestHandler`, `.args` empty, `.keywords` ⊆ `{directory, extra_response_headers}`. Subclasses and other settings fall back to the Python callback path.
- `default_content_type` and ordered `extra_response_headers` are native static metadata; extras apply only to final 200 responses. Fast-path concurrency is enforced natively (non-threading classes → 1 connection, `Threading*(N)` → N). Handler `protocol_version` is constrained to HTTP/1.1.
- Wheels: CPython 3.11+ (abi3). Routine CI tests Linux only; release wheels target 9 platforms (manylinux_2_17 x86_64/aarch64/armv7, musllinux_1_2 x86_64/aarch64, macOS x86_64/arm64, Windows x86_64/arm64). Wheel ships the `eggserve` console script and `python -m eggserve` backed by the native extension — no separate bundled binary.
- Windows is functionally qualified for handle-relative child resolution and directory enumeration, but remains **trusted/local-content only** (NTFS rejects the two open-descendant root-rename cases). See [docs/toolchain-support.md](docs/toolchain-support.md).

### Examples

`examples/README.md` is the mechanically checked index (`scripts/test-examples.sh` in `verify.sh full`). Supported demos: Python `python_http_server_static.py`, `python_custom_handler.py`, `python_lowlevel_service.py`, `python_subprocess.py`, `python_safe_download.py`, `python_https_server.py`, `python_custom_headers.py`; Cargo examples `static_server`, `custom_service`, `streaming_service`, `application_service` (Plan 197 native contract demo, no static FS), `caller_owned_stream`, `custom_headers`, `https_server` (`--features tls`), `primitives`. Keep examples small, loopback-bound, safe by default; do not turn them into a framework or second policy reference. Listener-based server examples support port `0`, wait for readiness, and shut down on Ctrl+C; `caller_owned_stream` binds nothing by design. Python examples expose `create_server()` for smoke tests.

### Verification beyond routine CI

- For library/CLI usability work also run: `cargo test --doc -p eggserve-core`, `cargo check -p eggserve-core --examples`, both dist builds, and `bash scripts/verify-cargo-packages.sh --mode all`.
- Qualification evidence (Plans 168/170): deterministic track suites and the manual performance-evidence matrix are mapped in `architecture/testing-and-conformance.md`; methodology, regression policy, and claims policy live in `benchmarks/README.md`, with machine-readable results under `benchmarks/168-qualification/` and `benchmarks/170-closure/`. No absolute RPS/latency gates in CI; every performance/release claim names a profile + evidence.
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
| HTTP/2 ownership, limits, and qualification boundary | [http2.md](architecture/http2.md) |
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
