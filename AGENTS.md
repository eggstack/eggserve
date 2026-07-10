# Guide for AI coding agents

## Project overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior, intended as a hardened replacement for `python -m http.server`. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. Plans 000-031 are complete.

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
│   │       ├── primitives/ # public API facade
│   │       │   ├── mod.rs          # re-exports: ConfinedPath, PathPolicy, StaticPolicy, etc.
│   │       │   ├── secure_root.rs  # SecureRoot, ResolvedResource, ResolvedFile, ResolvedDirectory
│   │       │   ├── http.rs         # request validation: ReadOnlyMethod, validate_method/body/target
│   │       │   ├── response.rs     # response planning types: BodyPlan, HeaderMapPlan, StaticResponsePlan
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
│           ├── test_primitives.py # native primitives tests (142 tests)
│           ├── test_server_primitives.py # server primitives tests (47 tests)
│           └── test_server.py     # subprocess API tests (43 tests)
├── architecture/           # deep-dive docs for each subsystem
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
├── examples/               # usage examples (Python, Rust)
├── fuzz/                   # fuzzing targets
└── AGENTS.md               # this file
```

## Common commands

CI runs these in order; match it locally before pushing:

```sh
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --all-targets -- -D warnings      # lint (warnings are errors)
cargo test --workspace                                     # tests
cargo test -p eggserve-core --features client              # client feature tests
cargo check --workspace --features tls                     # TLS feature build check
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

Full validation sequence (from README):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python packaging smoke test:

```sh
cd crates/eggserve-python
maturin build --release --interpreter 3.14 -o dist
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

Python client primitives tests (requires built wheel):

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_client_primitives -v
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
- **Windows**: functional but reparse-point hardening is deferred. Do not use with untrusted public content on Windows.
- **Two error types for path validation**: `PathRejection` (16 variants for parsing failures) vs `Error` (top-level taxonomy). `RequestValidationError` handles HTTP-level issues.
- **Two BodySource Python types**: `BodySource` (from `lib.rs`, for primitive-level body reading) and `ServerBodySource` (from `server.rs`, for server response streaming). They wrap the same Rust `BodySource` but have different Python names to avoid collision.

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
