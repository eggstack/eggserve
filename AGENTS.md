# Guide for AI coding agents

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade.
Static serving is the primary product. The CLI is static-only; the Python
facade adds bounded synchronous custom handlers; `eggserve.lowlevel` exposes a
handler-only runtime/service substrate (plus experimental H1-only async);
`eggserve-core::server` exposes an experimental low-level Rust service
boundary. EggServe is not an app framework, ASGI/WSGI runtime, CGI/FastCGI
executor, proxy, or general `socketserver` replacement. H1 + canonical
`primitives` are supported; `server`/H2/H3/tunnel/trailer/adapter/listener/
proxy/TLS-identity/async-Python remain experimental.

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Loopback bind, no symlinks, no dotfiles, no directory listing unless the user explicitly opts in. See `docs/security-policy.md`.
- **No serving outside the configured root.** Traversal/symlink escape denied at library level (Unix safe defaults: `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`). See `docs/threat-model.md`.
- **No broad dependencies.** Every dependency needs an explicit purpose. See `docs/dependency-policy.md`.
- **Plan-driven development.** Every change must be backed by a plan in `plans/`. No ad-hoc features. If a change crosses a current non-goal (`docs/non-goals.md`), update `docs/non-goals.md` + `docs/threat-model.md` in the same PR.
- **Unsafe Rust denied by default.** Only the reviewed Windows FFI, systemd descriptor-adoption, and test-fixture boundaries in `docs/unsafe-code-policy.md` are allowed.

## Layout

```
crates/
├── eggnet-tls/          # neutral rustls identity/trust/client-auth/reload substrate (only rustls + rustls-pki-types)
├── eggserve-primitives/ # canonical transport-neutral request/response/body/lifecycle model
├── eggserve-server/     # direct H1 connection runtime; single `Service` contract; tunnel execution
├── eggserve-static/     # SOLE static/path/filesystem authority (parsing, SecureRoot, confinement, MIME, planning)
├── eggserve-h3/         # experimental H3/QUIC adapter (sole QUIC dependency owner)
├── eggserve-core/       # compatibility/composition umbrella (facades only, no second implementation)
├── eggserve-bin/        # CLI binary; real logic in lib.rs/args.rs (main.rs is a shim)
└── eggserve-python/     # Python wheel (maturin) — EXCLUDED from workspace, own Cargo.lock
architecture/ docs/ plans/ (change-trace records, NOT normative) release/
conformance/ benchmarks/ examples/ (index: examples/README.md) fuzz/ scripts/ tests/
```

Authority rules (checked by `scripts/check-crate-topology.py`, run after any
graph/module/feature change): `eggserve-static` owns all static/path/FS
decisions (`src/fs`, `src/path`, `src/mime.rs` were deleted from core);
`eggserve-server` owns the H1 runtime; H1 `Auto` classifies before any Hyper
service exists (core executes H2 only); QUIC deps live only behind `http3`;
`eggserve_bin::run_cli` is plumbing for the wheel's extension CLI, not a
general embedding API.

## Common commands

Routine CI (`.github/workflows/ci.yml`) runs three concurrent jobs:

```sh
# rust job (conformance-matrix runs first; topology + metadata checks are cheap, before builds)
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features http-interop
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features tower
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features http-interop
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-server --no-default-features --features tower
cargo test -p eggserve-core --no-default-features --features http-interop --lib
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
# supply-chain job: install-cargo-tools.sh, then check-supply-chain.sh (both lockfiles)
# python job: check-python-release-metadata.py, maturin build, test-python-wheel.sh
```

Note: `./scripts/verify.sh fast` skips the `cargo +1.89` MSRV checks and the
TLS-only `eggserve-bin` tests (those run in `verify.sh full` / CI).

Focused runs: `cargo test -p <name>`; single test: `cargo test -p <name> <test_name>`.

```sh
./scripts/verify.sh fast   # routine dev check (workspace + H2/H3 clippy+tests + Python crate check)
./scripts/verify.sh full   # fast + TLS tests + examples + Python wheel + package dry-run
./scripts/verify.sh deep   # full + fuzz replay, races, proxy interop (manual, expensive)
```

Gotcha: `verify.sh full` dies without Python 3.14 + maturin (`PYTHON=` overrides, default `python3.14`). Use `fast` for Rust-only work.

```sh
bash scripts/install-cargo-tools.sh     # required first; pinned audit/deny tools
bash scripts/check-supply-chain.sh      # audits BOTH lockfiles; never substitute root-only cargo audit/deny
bash scripts/verify-cargo-packages.sh --mode all  # release-prep package dry-run
```

Direct `rustls` constraints carry a `0.23.45` caret floor (RUSTSEC-2026-0285)
in every constraining manifest including the excluded Python crate — never
roll it back. `deny.toml` bans wildcards and `native-tls`/`openssl-sys`
(`aws-lc-rs` is intentionally allowed: dev-only `rcgen` unification, production `cargo tree -e no-dev` is ring-only).

```sh
cargo build --profile dist --locked -p eggserve-bin                 # stripped CLI
cargo build --profile dist --locked -p eggserve-bin --features tls  # TLS CLI
```

Routine CI is a regression screen, not certification. Platform qualification
(macOS arm64 + Windows adversarial FS) is manual:
`gh workflow run platform-qualification.yml --ref main`. Publishing is manual
(crates.io from maintainer env; PyPI via OIDC, protected `pypi` environment) —
no push/tag/merge ever publishes.

## Toolchain notes

- Rust edition 2021, resolver `"2"`, MSRV 1.89. No `rustfmt.toml`/`clippy.toml`; CI enforces `-D warnings`.
- No pre-build/codegen: `cargo build` / `cargo test` suffice.
- `cargo run -p eggserve-bin` serves CWD on `127.0.0.1:8000`.
- `eggserve-python` is workspace-excluded: `cargo test --workspace` does not cover it; check it via the `cargo check --manifest-path ...` line or `test-python-wheel.sh`.
- Wheels: GIL-enabled CPython 3.11–3.15 via one `cp311-abi3` wheel per platform (build-once/test-many ABI proof on 3.11–3.15; no per-minor wheels; free-threaded owned by Plan 267); routine CI tests Linux only; release targets 10 required platforms from `release/wheel-matrix.toml` (manylinux x86_64/aarch64/armv7, musllinux x86_64/aarch64/armv7, macOS x86_64/arm64, Windows x86_64/arm64) + declared i686/win32 candidates. Plan 268: `manylinux` baseline vs `--compatibility pypi` are separate controls; cross-built AArch64 glibc/musl + Windows ARM64 are deferred to native qualifiers gating aggregation (AArch64 musl via Alpine container); ARMv7 via matching QEMU userspace with `sh`; 3.15 lanes use prerelease until final.

## Tripwires (mistakes agents actually make)

- **Manual CLI parsing in `args.rs` — no clap.** Grammar `[OPTIONS] [PORT] [DIRECTORY]`; a directory after an occupied port slot is verbatim even if numeric; host-only `--bind` leaves the port slot free; `--directory` occupies the directory slot.
- **Two `DotfilePolicy` types** (parsing in `eggserve_static::path`, serving in `eggserve_primitives::policy` facaded as `eggserve_core::policy`); both must agree for dotfiles to be served.
- **`StaticPolicy` field is `symlinks`, not `follow_symlinks`.** `ResponseStatus` is a struct with constants, not an enum. `FileRange` has private fields — construct via `try_new`/`new`, read via accessors, never a struct literal. `BodyPlan`: `Empty` / `FullBytes` / `FileFull` / `FileRange { start, end_inclusive }`.
- **Five error types, don't conflate:** `PathRejection` (path validation) / `RequestValidationError` (HTTP-level, Python-facing) / `ServerError` (`#[non_exhaustive]`, lifecycle) / `ServiceError` (struct over private `ServiceErrorKind`; inspect via `is_panic`/`is_timeout`) / `RequestBodyError` (`#[non_exhaustive]`, body consumption). `RequestCancellationReason` + `ConnectionOutcome` are also `#[non_exhaustive]` — match with wildcard. Never synthesize a second HTTP error after final commitment.
- **`RequestBody` is one-shot** (`read_all` or streaming, once); `Service::call` takes `Request` by value. Python `read()`/`iter_chunks()` are mutually exclusive. New code prefers `Request::context()` / `new_with_context()` over `connection()`/`lifecycle()`.
- **Response framing belongs to the runtime only.** `StatusCode` allows 100–599; 1xx/204/205/304 are body-forbidden (only 304 keeps representation `Content-Length`); normalize via `primitives::canonical::normalize_metadata()`; EggServe is sole `Date` authority (Hyper `auto_date_header(false)`). Don't name `BoxBody`/`UnsyncBoxBody` — the adapter returns opaque `http_body::Body`.
- **No `println!`/`eprintln!` in library code** — runtime code emits via `OpsContext` (`ops.emit(...)`); `Logger::global()` is CLI/frontend-init only. `Logger` is `OnceLock`: use `try_init()`, never `init()` twice. Per-runtime contexts via `RuntimeState::with_ops` / `ServerBuilder::ops_context`.
- **Hyper 1.11.1 quirks:** lone `Transfer-Encoding + Content-Length` reaches the service as chunked (200), not 400; `header_read_timeout` also fires on idle keep-alive gaps, so keep `keep_alive_idle_timeout` shorter for distinct idle accounting.
- **Inverted ranges are ignored (full 200), never 416** (RFC 9110 § 14.1.2).
- **Python specifics:** `#[pyclass(frozen)]` / `frozen=True` dataclasses; `#[allow(dead_code)]` on public API types is for external Python bindings, not dead code; stock `SimpleHTTPRequestHandler` fast path requires the exact bare class (or `functools.partial` with `.keywords ⊆ {directory, extra_response_headers}`) — subclasses take the Python callback path; `Response.stream` bridge is 16 chunks, sync iterables only; `telemetry.rs` does not exist, `tracing` was never added — don't create them.
- **H3 stays experimental** (upstream `hyperium/h3#338` + `#262` remainder unresolved; independent-client/adversarial evidence missing). Don't promote tiers; a promotion needs a new scoped plan.
- `0.2.0` is the historical initial pre-1.0 release; `0.2.1` added the direct-server API and `0.2.2` repaired and registry-qualified the experimental HTTP/Tower adapter (Plans 270–275; evidence: `release/plan-272-downstream-embedding-qualification-closure.md` and `release/plan-275-http-tower-adapter-patch-publication-closure.md`). The standalone `0.2.3` candidate was folded into Plan 286 and never published alone; the current line is `eggserve-server`/`eggserve-static`/`eggserve-h3`/`eggserve-core` at `0.3.0`, `eggserve-primitives` at `0.2.1`, `eggserve-bin` at `0.2.1`, Python wheel at `0.2.3` (evidence: `release/plan-286-embedding-contract-publication-closure.md`). Never publish this API line as `0.1.x`. Every performance/release claim names a profile (`docs/deployment.md`) + evidence.
- Plans 276–286 separate direct H1 application-server consumption from the compatibility/static umbrella: optional `http-interop`/Tower adapter authority lives in `eggserve-server` (published in `0.3.0`), and existing `eggserve-core` paths are compatibility re-exports. Plans 278–286 add the opt-in absolute-form seam plus external policy/admission ownership, narrow `H1ConnectionPolicy` projection, typed rejection presentation, and the TunnelIo KEEP decision (see `architecture/overview.md` plan context + `architecture/runtime.md`). Preserve resolved graph guards proving the direct Tower profile excludes `eggserve-static`/PHF and keep adapter checks for both direct authority and core compatibility forwarding. Do not make static optional inside core or add an adapter micro-crate without concrete blocker evidence. Plan 286 owns the `0.3.0` publication and registry-only closure.
- Direct `eggserve-server` supervisors use `ServerHandle::into_parts()` and retain cloneable `ServerControl` while selecting on the cancellation-safe typed `ServerCompletion::wait()`; legacy `ServerHandle::wait(self) -> ()` remains source-compatible and discards terminal detail. `RuntimeConfig.connection_total_timeout == Duration::ZERO` opts out of only the hard total lifetime; the default stays 60s and independent timeout/admission/shutdown limits remain.

## Reference docs

Load the `eggserve-dev` skill before working on code, plans, docs, or
architecture. The single skill source is `.opencode/skills/eggserve-dev/`
(`.agents/skills/eggserve-dev` is a symlink to it; there is no `.skills/`
directory). Start at `architecture/overview.md` (indexes all subsystem
pages); normative user contracts live in `docs/` (`security-policy`,
`threat-model`, `python-http-server-compatibility`, `cli`, `python-api`,
`http-primitives`, `deployment`, `timeout-reference`, `ops-logging`,
`migration-guide`). `plans/` + `plans/ROADMAP.md` are change-trace records, not API
docs — when docs conflict with config/scripts, trust the executable source.
