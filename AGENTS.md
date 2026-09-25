# Guide for AI coding agents

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade.
Static serving is the primary product. CLI is static-only; Python facade adds
bounded synchronous custom handlers; `eggserve.lowlevel` is a handler-only
runtime substrate; `eggserve-core::server` is an experimental low-level Rust
service boundary. Not an app framework, ASGI/WSGI runtime, CGI/FastCGI
executor, proxy, or general `socketserver` replacement. H1 + canonical
`primitives` are supported; `server`/H2/H3/tunnel/trailer/adapter/listener/
proxy/TLS-identity/async-Python remain experimental.

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Loopback bind, no symlinks, no dotfiles, no directory listing unless explicitly opted in. See `docs/security-policy.md`.
- **No serving outside the configured root.** Traversal/symlink escape denied at library level (Unix safe defaults: `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`). See `docs/threat-model.md`.
- **No broad dependencies.** Every dependency needs an explicit purpose. See `docs/dependency-policy.md`.
- **Plan-driven development.** Every change traces to a plan: new work registers in `plans/registry.md` (the next-number/current-work authority) with a subsystem roadmap + bounded `plans/implementation/<subsystem>/` handoff closed by a `plans/closure/<subsystem>/` record (see `plans/README.md` + `plans/003-planning-process.md`). Legacy flat `plans/NNN-*.md` + `release/plan-*.md` are archived in place and immutable. If a change crosses a current non-goal (`docs/non-goals.md`), update `docs/non-goals.md` + `docs/threat-model.md` in the same PR.
- **Unsafe Rust denied by default.** Only the reviewed Windows FFI, systemd descriptor-adoption, and test-fixture boundaries in `docs/unsafe-code-policy.md` are allowed.

## Layout

```
crates/
├── eggnet-tls/          # neutral rustls identity/trust/client-auth/reload substrate (only rustls + rustls-pki-types)
├── eggserve-primitives/ # canonical transport-neutral request/response/body/lifecycle model
├── eggserve-server/     # direct H1 runtime; single `Service` contract; tunnel execution
├── eggserve-static/     # SOLE static/path/filesystem authority (parsing, SecureRoot, confinement, MIME, planning)
├── eggserve-h3/         # experimental H3/QUIC adapter (sole QUIC dependency owner)
├── eggserve-core/       # compatibility/composition umbrella (facades only, no second implementation)
├── eggserve-bin/        # CLI binary; real logic in lib.rs/args.rs (main.rs is a shim)
└── eggserve-python/     # Python wheel (maturin) — EXCLUDED from workspace, own Cargo.lock
architecture/ docs/ plans/ (hierarchy: README + registry + 000-003 + subsystems/implementation/closure/adrs/archive; legacy flat + release/ archived in place, NOT normative) release/
conformance/ benchmarks/ examples/ (index: examples/README.md) fuzz/ scripts/ tests/
```

Authority rules (checked by `scripts/check-crate-topology.py`, run after any
graph/module/feature change): `eggserve-static` owns all static/path/FS
decisions; `eggserve-server` owns the H1 runtime; H1 `Auto` classifies before
any Hyper service exists (core executes H2 only); QUIC deps live only behind
`http3`; `eggserve_bin::run_cli` is plumbing for the wheel's extension CLI,
not a general embedding API.

## Commands

Prefer the wrappers; they encode the required order (cheap topology/metadata
checks before builds). Full sequence lives in `.github/workflows/ci.yml`
(three concurrent jobs: `rust`, `supply-chain`, `python`).

```sh
./scripts/verify.sh fast   # routine dev check (workspace + H2/H3 clippy+tests + Python crate check)
./scripts/verify.sh full   # fast + TLS tests + examples + Python wheel + package dry-run
./scripts/verify.sh deep   # full + fuzz replay, races, proxy interop (manual, expensive)
```

Focused runs: `cargo test -p <name>`; single test: `cargo test -p <name> <test_name>`.
`fast` skips MSRV (`cargo +1.89`) checks and TLS-only `eggserve-bin` tests.

```sh
bash scripts/install-cargo-tools.sh     # required first; pinned audit/deny tools
bash scripts/check-supply-chain.sh      # audits BOTH lockfiles; never substitute root-only cargo audit/deny
bash scripts/verify-cargo-packages.sh --mode all  # release-prep package dry-run
```

Gotchas: `verify.sh full` needs Python 3.14 + maturin (`PYTHON=` overrides,
default `python3.14`); use `fast` for Rust-only work. `cargo test
--workspace` does not cover `eggserve-python` (excluded crate — check via
`cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` or
`test-python-wheel.sh`). `cargo run -p eggserve-bin` serves CWD on
`127.0.0.1:8000`. Platform qualification (macOS arm64 + Windows adversarial
FS) is manual: `gh workflow run platform-qualification.yml --ref main`.
Publishing is manual (crates.io from maintainer env; PyPI via OIDC) — no
push/tag/merge ever publishes.

## Toolchain notes

- Rust edition 2021, resolver `"2"`, MSRV 1.89. No `rustfmt.toml`/`clippy.toml`; CI enforces `cargo fmt --check` + clippy `-D warnings`.
- No pre-build/codegen: `cargo build` / `cargo test` suffice.
- Direct `rustls` constraints carry a `0.23.45` caret floor (RUSTSEC-2026-0285)
  in every constraining manifest including the excluded Python crate — never
  roll it back. `deny.toml` bans wildcards and `native-tls`/`openssl-sys`
  (`aws-lc-rs` is intentionally allowed: dev-only `rcgen` unification,
  production `cargo tree -e no-dev` is ring-only).
- Stripped CLI: `cargo build --profile dist --locked -p eggserve-bin [--features tls]`.
- Per-crate versions differ (e.g. `server 0.3.1`, `core/static/h3 0.3.0`); check
  `crates/*/Cargo.toml`, never assume workspace version. Never publish this API
  line as `0.1.x`. Every performance/release claim names a profile
  (`docs/deployment.md`) + evidence.

## Tripwires (mistakes agents actually make)

- **Manual CLI parsing in `args.rs` — no clap.** Grammar `[OPTIONS] [PORT] [DIRECTORY]`; a directory after an occupied port slot is verbatim even if numeric; host-only `--bind` leaves the port slot free; `--directory` occupies the directory slot.
- **Two `DotfilePolicy` types** (parsing in `eggserve_static::path`, serving in `eggserve_primitives::policy` facaded as `eggserve_core::policy`); both must agree for dotfiles to be served.
- **`StaticPolicy` field is `symlinks`, not `follow_symlinks`.** `ResponseStatus` is a struct with constants, not an enum. `FileRange` has private fields — construct via `try_new`/`new`, read via accessors, never a struct literal. `BodyPlan`: `Empty` / `FullBytes` / `FileFull` / `FileRange { start, end_inclusive }`.
- **Five error types, don't conflate:** `PathRejection` (path validation) / `RequestValidationError` (HTTP-level, Python-facing) / `ServerError` (`#[non_exhaustive]`, lifecycle) / `ServiceError` (struct over private `ServiceErrorKind`; inspect via `is_panic`/`is_timeout`) / `RequestBodyError` (`#[non_exhaustive]`, body consumption). `RequestCancellationReason` + `ConnectionOutcome` are also `#[non_exhaustive]` — match with wildcard. Never synthesize a second HTTP error after final commitment.
- **`RequestBody` is one-shot** (`read_all` or streaming, once); `Service::call` takes `Request` by value. Python `read()`/`iter_chunks()` are mutually exclusive. New code prefers `Request::context()` / `new_with_context()` over `connection()`/`lifecycle()`.
- **Response framing belongs to the runtime only.** `StatusCode` allows 100–599; 1xx/204/205/304 are body-forbidden (only 304 keeps representation `Content-Length`); normalize via `primitives::canonical::normalize_metadata()`; EggServe owns Date/Server by default (Hyper `auto_date_header(false)`). Direct H1 may explicitly transfer successful service-response Date/Server metadata; framing, denylist, and runtime-generated error metadata remain runtime-owned. Don't name `BoxBody`/`UnsyncBoxBody` — the adapter returns opaque `http_body::Body`.
- **No `println!`/`eprintln!` in library code** — runtime code emits via `OpsContext` (`ops.emit(...)`); `Logger::global()` is CLI/frontend-init only. `Logger` is `OnceLock`: use `try_init()`, never `init()` twice. Per-runtime contexts via `RuntimeState::with_ops` / `ServerBuilder::ops_context`.
- **Hyper 1.11.1 quirks:** lone `Transfer-Encoding + Content-Length` reaches the service as chunked (200), not 400; `header_read_timeout` also fires on idle keep-alive gaps, so keep `keep_alive_idle_timeout` shorter for distinct idle accounting.
- **Inverted ranges are ignored (full 200), never 416** (RFC 9110 § 14.1.2).
- **Python specifics:** `#[pyclass(frozen)]` / `frozen=True` dataclasses; `#[allow(dead_code)]` on public API types is for external Python bindings, not dead code; stock `SimpleHTTPRequestHandler` fast path requires the exact bare class (or `functools.partial` with `.keywords ⊆ {directory, extra_response_headers}`) — subclasses take the Python callback path; `Response.stream` bridge is 16 chunks, sync iterables only; `telemetry.rs` does not exist, `tracing` was never added — don't create them.
- **H3 stays experimental** (upstream `hyperium/h3#338` + `#262` remainder unresolved; independent-client/adversarial evidence missing). Don't promote tiers; a promotion needs a new scoped plan.
- **Direct `eggserve-server` embedding:** `http-interop`/Tower adapter authority lives in `eggserve-server` (core paths are compatibility re-exports); the direct Tower graph must exclude `eggserve-static`/PHF (topology-guarded). Supervisors use `ServerHandle::into_parts()` + cloneable `ServerControl` with cancellation-safe `ServerCompletion::wait()`; legacy `wait(self)` discards detail. `RuntimeConfig.connection_total_timeout == Duration::ZERO` opts out of only the hard total lifetime (default 60s); independent timeout/admission/shutdown limits remain.

## Reference docs

Load the `eggserve-dev` skill before working on code, plans, docs, or
architecture. The single skill source is `.opencode/skills/eggserve-dev/`
(`.agents/skills/eggserve-dev` is a symlink to it; there is no `.skills/`
directory). Start at `architecture/overview.md` (indexes all subsystem
pages); normative user contracts live in `docs/` (`security-policy`,
`threat-model`, `python-http-server-compatibility`, `cli`, `python-api`,
`http-primitives`, `deployment`, `timeout-reference`, `ops-logging`,
`migration-guide`). `plans/` hierarchy (`README.md` + `registry.md` + `000`-`003` +
subsystems/implementation/closure/adrs/archive; legacy flat files + `plans/ROADMAP.md`
archived in place) are change-trace records, not API
docs — when docs conflict with config/scripts, trust the executable source.
