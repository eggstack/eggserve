# Guide for AI coding agents

## Project overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior, intended as a hardened replacement for `python -m http.server`. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. Plans 000-013 are complete.

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. See [docs/security-policy.md](docs/security-policy.md).
- **No serving outside the configured root.** Path traversal and symlink escape must be denied at the library level. On Unix with safe defaults, symlink denial is **descriptor-relative** — each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is refused rather than followed. On non-Unix or follow-symlinks mode, component-wise `symlink_metadata` checks are used. Follow-symlinks is weaker and is explicitly outside the descriptor-relative hardening guarantee. See [docs/threat-model.md](docs/threat-model.md) and [plans/007-filesystem-policy-tightening.md](plans/007-filesystem-policy-tightening.md).
- **No broad dependencies.** Every dependency must have an explicit purpose. See [docs/dependency-policy.md](docs/dependency-policy.md). Current dependencies: `thiserror` (errors), `tokio` (async runtime), `hyper`/`hyper-util`/`http-body-util` (HTTP), `bytes` (buffers), `futures-util` (streaming bodies), `httpdate` (Last-Modified), `phf` (MIME map). Optional: `rustls`/`tokio-rustls`/`rustls-pemfile` (TLS, behind `tls` feature flag). Unix-only: `rustix` (descriptor-relative filesystem traversal).
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
│   │       └── telemetry.rs # startup logging
│   ├── eggserve-bin/       # CLI binary, args, signal handling, accept loop
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs     # HTTP accept loop with connection semaphore, timeouts, graceful shutdown
│   │       ├── lib.rs      # pub fn run() entrypoint
│   │       ├── args.rs     # manual argument parsing
│   │       ├── tls.rs      # TLS certificate loading and rustls config (behind tls feature)
│   │       └── shutdown.rs # signal handling (Ctrl+C, SIGTERM)
│   └── eggserve-python/    # Python wheel packaging (maturin)
│       ├── Cargo.toml      # depends on eggserve-bin
│       ├── pyproject.toml  # maturin build backend
│       ├── src/main.rs     # Rust binary entrypoint
│       └── python/eggserve/
│           ├── __init__.py # exports version, ServeConfig, StaticPolicy, serve_directory
│           ├── __main__.py # python -m eggserve
│           ├── _bin.py     # locates and executes packaged binary
│           ├── server.py   # Python API: ServeConfig, StaticPolicy, serve_directory, ServerProcess
│           └── test_server.py # Python API tests
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
└── AGENTS.md               # this file
```

## Common commands

CI runs these in order; match it locally before pushing:

```sh
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --all-targets -- -D warnings      # lint (warnings are errors)
cargo test --workspace                                     # tests
cargo check --workspace --features tls                     # TLS feature build check
```

Run a single crate with `-p <name>` (e.g. `cargo test -p eggserve-core`).

Python packaging smoke test:

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

## Toolchain notes

- Rust edition 2021, workspace `resolver = "2"`.
- No `rustfmt.toml` / `clippy.toml` — defaults apply; CI enforces `-D warnings`.
- `target/` is gitignored; `cargo build` / `cargo test` are sufficient setup (no pre-build step, no codegen).
- `cargo run -p eggserve-bin` starts an HTTP server on `127.0.0.1:8000` serving static files from the current directory. See [crates/eggserve-bin/src/main.rs](crates/eggserve-bin/src/main.rs).

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
- [docs/deployment.md](docs/deployment.md) — deployment patterns (local, reverse proxy, native TLS)
