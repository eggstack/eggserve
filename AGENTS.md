# Guide for AI coding agents

## Project overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior, intended as a hardened replacement for `python -m http.server`. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. Plans 000–108 are historical implementation records; Plan 109 is the verified final admission and wire-verification corrective pass; Plan 110 is documentation polish only. Plan 091 defines current CI/release policy.

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

- **eggserve-python is excluded from the workspace** — it has its own `Cargo.lock` and is built independently via `maturin`. Don't run `cargo test --workspace` expecting to cover Python crate code.
- **Two DotfilePolicy types**: `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served. Don't confuse them.
- **Manual argument parsing** in `args.rs` — no clap dependency.
- **`#[allow(dead_code)]` on public API types** — these are consumed externally (Python bindings), not dead.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability is enforced at both layers.
- **Python wheels**: CPython 3.14 only (`>=3.14,<3.15`). Routine CI builds and tests the Linux wheel; macOS and Windows wheels are built manually. The wheel bundles the platform-native CLI binary.
- **Windows**: functional with handle-relative child resolution (Plan 084) and handle-relative directory enumeration (Plan 085). Independent adversarial review is incomplete. Do not use with untrusted public content on Windows until that review is completed.
- **Two error types for path validation**: `PathRejection` (16 variants for parsing failures) vs `Error` (top-level taxonomy). `RequestValidationError` handles HTTP-level issues.
- **Two BodySource Python types**: `BodySource` (from `lib.rs`, for primitive-level body reading) and `ServerBodySource` (from `server.rs`, for server response streaming). They wrap the same Rust `BodySource` but have different Python names to avoid collision.
- **Two Method types**: `ReadOnlyMethod` (GET/HEAD only, stable) and `Method` (standard + extension, experimental). `ReadOnlyMethod` is used by the response planner. `Method` is the canonical type for new code. Client method types are feature-gated and Rust-only, not part of the Python surface.
- **HeaderBlock is a list, not a map**: `HeaderBlock` stores headers as an ordered `Vec<HeaderField>`, preserving duplicates. `get_unique()` returns `DuplicateHeaderError` on duplicates. Python `HeaderBlock` is frozen/immutable.
- **Two status code types**: `ResponseStatus` (stable, used by the planner) and `StatusCode` (stable, canonical with range validation). New code should prefer `StatusCode`. Two header map types: `HeaderMapPlan` (stable, existing) and `HeaderBlock` (stable, canonical). The canonical response types use `HeaderBlock`.
- **RequestBody is one-shot** — `RequestBody` can only be consumed once (via `read_all` or streaming). The `Service::call` method takes `Request` by value, consuming it. Python `RequestBody.read()` and `iter_chunks()` are mutually exclusive; second use raises `RequestBodyConsumedError`.
- **Python server facade** — The supported Python API is `eggserve.server` with `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`, `ThreadingHTTPSServer`, `BaseHTTPRequestHandler`, and `SimpleHTTPRequestHandler`. Native callback and client types are not top-level supported APIs. Advanced primitives are grouped under `eggserve.lowlevel`, CLI subprocess helpers under `eggserve.subprocess`.
- **Server without ServeConfig** — `Server::builder().runtime(config).build()` creates a runtime-only server. `Server::start()` requires `serve_config`. `Server::start_with_service()` works without serve config — custom services have no implicit filesystem root.
- **CLI runtime is current-thread** — The standalone CLI uses `Builder::new_current_thread()` (Plan 105). The Python facade uses `rt-multi-thread` for GIL scheduling. The library is runtime-agnostic.
- **Structured logging** — `eggserve-core::ops` provides the event model. `Logger::global().emit(Event::new(...))` is the primary API. The CLI initializes the logger with `StderrLogSink`. `--log-format none` disables output; `--quiet` filters to warn/error only. Library code must not use `println!`/`eprintln!`.

## Reference docs

`docs/` has reference docs (security-policy, threat-model, non-goals, dependency-policy, compatibility, release-process, deployment, http-primitives, python-api, etc.). `architecture/` has deep-dive docs per subsystem (core, bin, python, path-confinement, policy-system, runtime, etc.). `plans/` has design plans 000–110 (historical/implementation records; Plan 091 defines current CI/release policy; Plan 105 defines product-surface freeze and binary-size reduction; Plan 109 is verified complete; Plan 110 is documentation polish only).
