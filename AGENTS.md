# Guide for AI coding agents

## Project overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior, intended as a hardened replacement for `python -m http.server`. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. Plans 000–053 are complete. Plan 053 establishes Milestone 3C: Python runtime parity, lifecycle methods (`wait_ready()`, `shutdown()`, `force_shutdown()`, `wait()`, `state`), handler timeout, coroutine rejection, and conformance tests. Plan 047 establishes canonical HTTP request types (`Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo`) in `primitives::`. Plan 048 establishes canonical response types (`StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response`) in `primitives::canonical` and a single normalization path for all response producers. Plan 049 promotes all canonical HTTP types to stable and establishes the conformance corpus for Rust/Python parity testing. Plan 050 closes Milestone 2 by correcting StatusCode validation (100–999), unifying canonical response metadata across all response producers via `normalize_metadata()`, enforcing hop-by-hop header stripping, and completing the response architecture audit. Plan 051 establishes the Milestone 3A runtime service boundary: `server::Server`, `ServerBuilder`, `ServerHandle`, `RuntimeConfig`, `Service` trait, `service_fn`, `StaticService`, and `StaticServiceBuilder` in `eggserve-core::server`. Plan 052 establishes the Milestone 3B lifecycle: lifecycle state machine (Created→Starting→Running→Draining→Stopped/Failed), listener abstraction (bind/from_listener), readiness signaling, graceful/forced shutdown with drain deadline, and connection/task registry in `eggserve-core::server`. The `server` module is experimental and its API is subject to change. Plans 042–045 establish the release evidence infrastructure: a capability matrix (`docs/library-capability-matrix.md`), machine-readable release criteria (`release/criteria.toml`), a criteria validator (`scripts/release_criteria.py`), a unified local validation script (`scripts/release-validate.sh`), and normalized CI gate names with evidence aggregation. Plan 046 closes integration gaps: trigger policy reconciliation, separate package evidence, explicit skip semantics, fail-closed aggregation, and canonical checklist authority.

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. See [docs/security-policy.md](docs/security-policy.md).
- **No serving outside the configured root.** Path traversal and symlink escape must be denied at the library level. On Unix with safe defaults, symlink denial is **descriptor-relative** — each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is refused rather than followed. On non-Unix or follow-symlinks mode, component-wise `symlink_metadata` checks are used. Follow-symlinks is weaker and is explicitly outside the descriptor-relative hardening guarantee. See [docs/threat-model.md](docs/threat-model.md) and [plans/007-filesystem-policy-tightening.md](plans/007-filesystem-policy-tightening.md).
- **No broad dependencies.** Every dependency must have an explicit purpose. See [docs/dependency-policy.md](docs/dependency-policy.md). Current dependencies: `thiserror` (errors), `tokio` (async runtime), `hyper`/`hyper-util`/`http-body-util` (HTTP), `bytes` (buffers), `futures-util` (streaming bodies), `httpdate` (Last-Modified), `phf` (MIME map). Optional: `rustls`/`tokio-rustls`/`webpki-roots` (TLS, behind `client-tls` feature in eggserve-core; `tls` feature in eggserve-bin). Unix-only: `rustix` (descriptor-relative filesystem traversal).
- **Plan-driven development.** Every change must be backed by a plan in `plans/`. No ad-hoc feature additions.

## Layout

```
eggserve/
├── Cargo.toml              # workspace root
├── crates/
│   ├── eggserve-core/      # security policy, path confinement, HTTP serving, response construction
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs   # ServeConfig, ServeState (config + file-stream semaphore)
│   │       ├── policy.rs   # StaticPolicy, symlink/dotfile/listing policies
│   │       ├── limits.rs   # Limits: connection count, file streams, header/target/body sizes, timeouts
│   │       ├── error.rs    # error taxonomy (Config, Bind, Runtime, RequestRejected, Io)
│   │       ├── path/       # path confinement engine
│   │       │   ├── mod.rs          # ConfinedPath entry point
│   │       │   ├── decode.rs       # single-pass percent decoding
│   │       │   ├── request_target.rs # HTTP origin-form parsing
│   │       │   ├── components.rs   # normalization, component validation
│   │       │   ├── rejected.rs     # PathRejection enum
│   │       │   ├── policy.rs       # PathPolicy (dotfile, backslash)
│   │       │   └── platform.rs     # Windows reserved names, ADS, drives
│   │       ├── fs/         # filesystem confinement
│   │       │   ├── mod.rs          # RootGuard, ResolvedResource, symlink-aware resolution
│   │       │   └── unix.rs         # descriptor-relative traversal (statat + openat)
│   │       ├── response.rs # file streaming, directory listing HTML, error responses (413, 503)
│   │       ├── mime.rs     # MIME type detection (~60 extensions, octet-stream fallback)
│   │       ├── service.rs  # HTTP handler: GET/HEAD, path validation, body rejection, file-stream semaphore, index, ETag
│   │       ├── server/     # runtime service boundary (experimental)
│   │       │   ├── mod.rs          # Server, ServerBuilder, accept loops, lifecycle integration
│   │       │   ├── lifecycle.rs    # LifecycleState, Lifecycle state machine
│   │       │   ├── service.rs      # Service trait, service_fn, ServiceError
│   │       │   ├── config.rs       # RuntimeConfig, RuntimeConfigBuilder
│   │       │   ├── static_service.rs # StaticService, StaticServiceBuilder
│   │       │   ├── errors.rs       # ServerError
│   │       │   ├── handle.rs       # ServerHandle
│   │       │   └── connection.rs   # serve_connection, serve_connection_with_service
│   │       ├── primitives/ # public API facade
│   │       │   ├── mod.rs          # re-exports: ConfinedPath, PathPolicy, StaticPolicy, etc.
│   │       │   ├── secure_root.rs  # SecureRoot, ResolvedResource, ResolvedFile, ResolvedDirectory
│   │       │   ├── http.rs         # request validation: ReadOnlyMethod, validate_method/body/target
│   │       │   ├── method.rs       # Method: validated HTTP method (standard + extension)
│   │       │   ├── version.rs      # HttpVersion: HTTP/1.0, HTTP/1.1
│   │       │   ├── header_block.rs # HeaderBlock: duplicate-preserving ordered headers
│   │       │   ├── request_target.rs # RequestTarget: validated origin-form target
│   │       │   ├── request_head.rs # RequestHead: canonical request (method, target, version, headers)
│   │       │   ├── connection_info.rs # ConnectionInfo: transport metadata (addrs, scheme, TLS)
│   │       │   ├── response.rs     # response planning types: BodyPlan, HeaderMapPlan, StaticResponsePlan
│   │       │   ├── canonical.rs   # canonical response types: StatusCode, Response, normalize_response
│   │       │   ├── planner.rs      # response planner: conditional requests, range requests, ETag generation
│   │       │   └── client/         # HTTP client primitives (behind `client` feature gate)
│   │       │       ├── mod.rs      # re-exports: HttpClient, ClientConfig, ClientRequest, ClientResponse
│   │       │       ├── error.rs    # ClientError taxonomy (12 variants)
│   │       │       ├── url.rs      # Scheme, ParsedUrl — hand-parsed URL validation
│   │       │       ├── request.rs  # ClientConfig, Method, ClientRequest, ClientRequestBuilder, validate_header
│   │       │       ├── response.rs # ClientResponse — status, headers, body
│   │       │       └── http_client.rs # HttpClient — hyper client, TLS, timeouts
│   ├── eggserve-bin/       # CLI binary, args, signal handling, accept loop
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs     # HTTP accept loop with connection semaphore, timeouts, graceful shutdown
│   │       ├── lib.rs      # pub fn run() entrypoint
│   │       ├── args.rs     # manual argument parsing
│   │       ├── tls.rs      # TLS certificate loading and rustls config (behind tls feature)
│   │       └── shutdown.rs # signal handling (Ctrl+C, SIGTERM)
│   └── eggserve-python/    # Python wheel packaging (maturin)
│       ├── Cargo.toml      # lib crate with PyO3 bindings
│       ├── pyproject.toml  # maturin build backend
│       ├── src/
│       │   ├── lib.rs      # PyO3 native module (_native)
│       │   └── server.rs   # Server primitives: PyRequest, PyResponse, StaticResponder, Server
│       └── python/eggserve/
│           ├── __init__.py # exports version, native primitives, subprocess API
│           ├── __main__.py # python -m eggserve
│           ├── _bin.py     # locates and executes packaged binary
│           ├── server.py   # Python API: ServeConfig, StaticPolicy, serve_directory, ServerProcess
│           ├── test_primitives.py # native primitives tests (143 tests)
│           ├── test_server_primitives.py # server primitives tests (64 tests)
│           ├── test_server_integration.py # live concurrency/timeout/shutdown tests (51 tests)
│           ├── test_parity_matrix.py # real-socket parity matrix tests (28 tests)
│           ├── test_canonical_conformance.py # canonical HTTP type conformance tests
│           ├── test_api_stability.py # API stability/snapshot tests (61 tests)
│           └── test_server.py     # subprocess API tests (43 tests)
├── architecture/           # deep-dive docs for each subsystem
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
├── release/                # machine-readable release criteria (criteria.toml)
├── examples/               # usage examples (Python, Rust)
├── fuzz/                   # fuzzing targets, seed corpora, fuzz README
├── .github/workflows/      # CI workflows (ci.yml, fuzz.yml, fuzz-replay.yml, release.yml)
└── AGENTS.md               # this file
```

## Common commands

CI runs these in order; match it locally before pushing:

```sh
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --all-targets -- -D warnings      # lint (warnings are errors)
cargo test --workspace                                     # tests
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                  # TLS tests
cargo test -p eggserve-core --features client              # client feature tests
cargo test -p eggserve-core --test http_wire_correctness   # raw wire tests
cargo test -p eggserve-core --test http_primitives_integration  # HTTP integration
cargo test -p eggserve-bin --test production_path          # production path tests
cargo test -p eggserve-core --test corpus_replay           # fuzz corpus replay
cargo test -p eggserve-core --test canonical_conformance  # canonical HTTP type conformance
bash scripts/install-cargo-tools.sh                        # deterministic audit/deny installation
cargo audit                                                # vulnerability check
cargo deny check                                           # license/policy check
bash scripts/verify-cargo-packages.sh                      # package and publish dry-run gates
python3 scripts/check-contract-consistency.py              # contract consistency validation
# Canonical HTTP type conformance:
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_canonical_conformance -v
# Unified local validation:
./scripts/release-validate.sh fast                 # routine dev check
./scripts/release-validate.sh full                 # pre-release validation
./scripts/release-validate.sh gate <gate-id>       # run a single gate
./scripts/release-validate.sh metadata             # metadata/contract consistency check
./scripts/release-validate.sh evidence --output <path>  # copy evidence to output path
./scripts/release_criteria.py validate release/criteria.toml  # validate criteria
./scripts/release_criteria.py list                 # list all gates
# Aggregate evidence bundle (fail-closed validation):
python3 scripts/release_criteria.py aggregate --criteria release/criteria.toml --evidence <evidence-dir> --sha <commit-sha>
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

Full validation sequence (from README):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --features client
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test http_primitives_integration
cargo test -p eggserve-bin --test production_path
cargo test -p eggserve-core --test corpus_replay
cargo test -p eggserve-core --test canonical_conformance
bash scripts/install-cargo-tools.sh
cargo audit
cargo deny check
```

Python packaging smoke test:

```sh
cargo build --release --locked -p eggserve-bin
mkdir -p crates/eggserve-python/python/eggserve/bin
cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
chmod +x crates/eggserve-python/python/eggserve/bin/eggserve
cd crates/eggserve-python
maturin build --release --interpreter python3.14 -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

Python native primitives tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
```

Python server primitives tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v
```

Python subprocess API tests:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server -v
```

Python boundary hardening tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_boundary_hardening -v
```

Python client primitives tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_client_primitives -v
```

Python server integration tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server_integration -v
```

Python canonical HTTP type conformance tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_canonical_conformance -v
```

Release infrastructure tests:

```sh
python3 -m unittest scripts.test_release_criteria -v      # criteria validator unit tests
python3 -m unittest scripts.test_check_contract_consistency -v  # contract consistency tests
python3 -m unittest scripts.test_release_safety -v        # release safety tests
```

Packaging smoke tests (installed-wheel validation, no source-tree imports):

```sh
cargo build --release --locked -p eggserve-bin
mkdir -p crates/eggserve-python/python/eggserve/bin
cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
chmod +x crates/eggserve-python/python/eggserve/bin/eggserve
cd crates/eggserve-python
maturin build --release --interpreter python3.14 -o dist
cd packaging-tests
bash run_all.sh ../dist/*.whl python3.14
```

## Toolchain notes

- Rust edition 2021, workspace `resolver = "2"`.
- No `rustfmt.toml` / `clippy.toml` — defaults apply; CI enforces `-D warnings`.
- `target/` is gitignored; `cargo build` / `cargo test` are sufficient setup (no pre-build step, no codegen).
- `cargo run -p eggserve-bin` starts an HTTP server on `127.0.0.1:8000` serving static files from the current directory. See [crates/eggserve-bin/src/main.rs](crates/eggserve-bin/src/main.rs).

## Important quirks

- **Two DotfilePolicy types**: `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served. Don't confuse them.
- **eggserve-python is excluded from the workspace** — it has its own `Cargo.lock` and is built independently via `maturin`. Don't run `cargo test --workspace` expecting to cover Python crate code.
- **test_primitives.py requires a built wheel** (imports `_native`). test_server.py does not (uses mocks).
- **Manual argument parsing** in `args.rs` — no clap dependency.
- **`#[allow(dead_code)]` on public API types** — these are consumed externally (Python bindings), not dead.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses; immutability is enforced at both layers.
- **Python wheels**: CPython 3.14 only (`>=3.14,<3.15`) on the Linux, macOS, and Windows wheel matrix. The wheel bundles the platform-native CLI binary.
- **Windows**: functional but reparse-point hardening is deferred. Do not use with untrusted public content on Windows.
- **Two error types for path validation**: `PathRejection` (16 variants for parsing failures) vs `Error` (top-level taxonomy). `RequestValidationError` handles HTTP-level issues.
- **Two BodySource Python types**: `BodySource` (from `lib.rs`, for primitive-level body reading) and `ServerBodySource` (from `server.rs`, for server response streaming). They wrap the same Rust `BodySource` but have different Python names to avoid collision.
- **Two Method types**: `ReadOnlyMethod` (GET/HEAD only, stable) and `Method` (standard + extension, experimental). `ReadOnlyMethod` is used by the response planner. `Method` is the canonical type for new code.
- **HeaderBlock is a list, not a map**: `HeaderBlock` stores headers as an ordered `Vec<HeaderField>`, preserving duplicates. `get_unique()` returns `DuplicateHeaderError` on duplicates. Python `HeaderBlock` is frozen/immutable.
- **Response validation boundary**: Python handler-returned `Response` objects are validated in Rust via `validate_handler_response()` — status 200–999, no hop-by-hop headers, 204/304 empty bodies, no NUL/CR/LF in header values. Invalid responses fall back to 500.
- **Typed lifecycle/response exceptions**: `LifecycleError` (double start, stop before start) and `ResponseConstructionError` (response validation failure) are typed exceptions, not generic `PyValueError`.
- **Release criteria** — `release/criteria.toml` is the single source of truth for release gates. Each gate declares a `triggers` field specifying which CI triggers (pull_request, push, manual_dispatch, tagged_push) apply. `scripts/release_criteria.py` validates the criteria file and generates the release checklist. `scripts/release-validate.sh` provides unified local validation. Dirty-tree runs are refused (cannot serve as release evidence).
- **Generated release checklist** — `docs/release-checklist.md` is the single canonical checklist file, generated from `release/criteria.toml`. Do not edit by hand; regenerate with `python scripts/release_criteria.py generate-checklist --criteria release/criteria.toml`.
- **Contract consistency** — `scripts/check-contract-consistency.py` validates that documentation claims are consistent (TLS, Python version, package versions, platform classifications, stable API inventory, README links). Run via `./scripts/release-validate.sh metadata` or directly.
- **Fail-closed aggregation** — `scripts/release_criteria.py aggregate` validates an evidence bundle against all criteria gates and fails closed: MALFORMED > CONFLICTING > INVALIDATED > STALE > FAILED > MISSING. Waivers cannot hide malformed or conflicting evidence.
- **Canonical HTTP types (stable)** — Plan 049 promotes all canonical HTTP types to stable after conformance completion. `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `ConnectionInfo` (request types) and `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, `normalize_response()` (response types) are all stable. `ReadOnlyMethod` (GET/HEAD only) remains stable for existing consumers.
- **Canonical response normalization** — All response producers converge on `primitives::canonical::normalize_metadata()` for response metadata and framing. `normalize_response()` applies HEAD suppression, body-forbidden enforcement, and hop-by-hop stripping for in-memory bodies. `normalize_metadata()` applies the same framing rules (Transfer-Encoding stripping, Content-Length computation) for file-backed bodies without consuming the body. `to_hyper_response()` converts to Hyper after normalization.
- **Two status code types**: `ResponseStatus` (stable, existing) and `StatusCode` (stable, canonical). `ResponseStatus` is a simple u16 newtype used by the planner. `StatusCode` has range validation (100–999, three-digit only) and classification helpers (is_informational, permits_payload_body). New code should prefer `StatusCode`.
- **Two header map types**: `HeaderMapPlan` (stable, existing) and `HeaderBlock` (stable, canonical). `HeaderMapPlan` stores `ResponseHeader { name: String, value: String }`. `HeaderBlock` stores `HeaderField { name: HeaderName, value: HeaderValue }` with validation. The canonical response types use `HeaderBlock`.
- **Python Server lifecycle parity** — Plan 053 adds lifecycle methods to Python `Server`: `wait_ready()`, `shutdown()`, `force_shutdown(timeout_secs)`, `wait()`, and `state` property. Constructor accepts `handler_timeout_secs` and `graceful_shutdown_timeout_secs`. Coroutine handlers are rejected with a 500 response. Handler timeout is best-effort in Python (enforced at transport level by the Rust server). The `server` module remains experimental.
- **`server` module is experimental** — `eggserve-core::server` provides the runtime service boundary (`Server`, `Service` trait, `StaticService`, etc.) for embedding. Includes lifecycle state machine (`LifecycleState`), listener abstraction, readiness signaling, and graceful/forced shutdown with drain deadline. Its API is subject to change without notice. Do not depend on it for stable integrations.

## Plan-driven development

All implementation work must be traced to a plan in `plans/`. Plans define scope, acceptance criteria, and boundaries. Do not implement features that are not covered by an existing plan. If a change requires expanding scope, update the relevant plan first.

## Scope discipline

Before implementing any feature, check:

1. Does the feature appear in a plan in `plans/`?
2. Is it listed as a non-goal in `docs/non-goals.md`? If so, the non-goal must be updated first.
3. Does it affect the threat model? If so, update `docs/threat-model.md`.

## Don'ts

- Do not add broad dependencies without justification (see [docs/dependency-policy.md](docs/dependency-policy.md))
- Do not add comments to code unless explicitly asked
- Do not make broad PRs that touch multiple unrelated files
- Do not create files outside the directories specified by plans

## Pointers to docs/

- [docs/security-policy.md](docs/security-policy.md) — safe defaults and opt-in behaviors
- [docs/threat-model.md](docs/threat-model.md) — assets, trust boundaries, attacker model
- [docs/non-goals.md](docs/non-goals.md) — explicit scope boundaries
- [docs/architecture.md](docs/architecture.md) — workspace and module responsibilities
- [docs/dependency-policy.md](docs/dependency-policy.md) — dependency rules and allowed categories
- [docs/compatibility.md](docs/compatibility.md) — compatibility with `python -m http.server`
- [docs/release-criteria.md](docs/release-criteria.md) — alpha, beta, 1.0 gates
- [docs/release-checklist.md](docs/release-checklist.md) — pre-release checklist and release blockers
- [docs/security-review.md](docs/security-review.md) — alpha security posture and known limitations
- [docs/tls.md](docs/tls.md) — optional TLS feature, certificate requirements, limitations
- [docs/secure-root.md](docs/secure-root.md) — SecureRoot public API, resolved-resource capabilities, platform guarantees
- [docs/deployment.md](docs/deployment.md) — deployment patterns (local, reverse proxy, native TLS)
- [docs/extension-contract.md](docs/extension-contract.md) — how downstream projects may build on eggserve
- [docs/invariants.md](docs/invariants.md) — invariant test matrix across Rust and Python APIs
- [docs/http-primitives.md](docs/http-primitives.md) — HTTP/1.1 primitive contract, supported subset, and behavior guarantees
- [docs/http-client-primitives.md](docs/http-client-primitives.md) — HTTP client primitive contract, feature gates, and usage
- [docs/release-contract.md](docs/release-contract.md) — product surface and compatibility commitments
- [docs/api-stability.md](docs/api-stability.md) — API classification by stability tier
- [docs/fuzzing.md](docs/fuzzing.md) — fuzz targets, property tests, seed corpora, CI integration
- [docs/action-pinning.md](docs/action-pinning.md) — GitHub Action SHA pinning policy and update procedure
- [docs/release-process.md](docs/release-process.md) — release operator guide, evidence philosophy, and failure handling
- [docs/library-capability-matrix.md](docs/library-capability-matrix.md) — Rust/Python/CLI capability parity matrix
- [docs/toolchain-support.md](docs/toolchain-support.md) — language, toolchain, and platform support policy
- [release/criteria.toml](release/criteria.toml) — machine-readable release gate definitions (source of truth)
- [docs/ci-gate-inventory.md](docs/ci-gate-inventory.md) — CI job-to-gate mapping, execution policy, evidence classes

## Architecture deep dives

- [architecture/overview.md](architecture/overview.md) — workspace structure, data flow, architectural decisions
- [architecture/eggserve-core.md](architecture/eggserve-core.md) — core library module map, key types, error taxonomy
- [architecture/eggserve-bin.md](architecture/eggserve-bin.md) — CLI binary, accept loop, signal handling
- [architecture/eggserve-python.md](architecture/eggserve-python.md) — Python bindings, PyO3, maturin packaging
- [architecture/path-confinement.md](architecture/path-confinement.md) — path validation pipeline
- [architecture/filesystem-confinement.md](architecture/filesystem-confinement.md) — SecureRoot, symlink-aware resolution
- [architecture/policy-system.md](architecture/policy-system.md) — StaticPolicy, symlink/dotfile/listing policies
- [architecture/primitives-api.md](architecture/primitives-api.md) — public API boundary for embedding consumers
- [architecture/response-planning.md](architecture/response-planning.md) — conditional/range/ETag response planning
- [architecture/client.md](architecture/client.md) — HTTP client primitives, feature-gated substrate
- [architecture/runtime.md](architecture/runtime.md) — runtime service boundary, Server, Service trait, StaticService
