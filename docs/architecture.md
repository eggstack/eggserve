# Architecture

## Workspace structure

```
eggserve/
├── Cargo.toml              # workspace root
├── crates/
│   ├── eggserve-core/      # library crate: security primitives
│   └── eggserve-bin/       # binary crate: CLI entrypoint
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
└── AGENTS.md
```

`eggserve-python/` is not yet a Cargo crate. Python packaging is deferred to plan 005.

## Crate responsibilities

### `eggserve-core`

The core library crate. Contains security policy, path confinement, static serving primitives, and response construction. This crate must **not** depend on Python packaging concerns.

Placeholder modules:

| Module | Responsibility |
|--------|----------------|
| `config.rs` | Configuration types (bind address, root directory, policy flags) |
| `policy.rs` | Security policy enforcement (method checks, symlink policy, dotfile policy) |
| `limits.rs` | Resource limits (connection count, request size, rate limits) |
| `error.rs` | Error types for path resolution, policy rejection, and serving failures |
| `path.rs` | Path confinement and resolution (root escape prevention, symlink verification) |

The core crate exposes a public API for path confinement and policy enforcement that can be used independently of the CLI. This is the foundation for safe HTTP/static-serving primitives.

### `eggserve-bin`

The CLI binary crate. Handles argument parsing (via `clap`), configuration loading, signal handling, and startup policy display. Depends on `eggserve-core` for policy enforcement and path resolution.

This crate is the entrypoint for `eggserve` as a command-line tool. It does not contain serving logic — that will be introduced in plan 001 via the HTTP substrate.

### `eggserve-python` (deferred)

Python packaging and `python -m` launcher. Not yet a Cargo crate. Packaging and the `python -m` entrypoint are deferred to plan 005. When implemented, this crate will handle:

- Python wheel packaging via maturin/PyO3
- `python -m eggserve` launcher
- Python-side configuration bridging

**Important:** The core crate must never depend on Python packaging. The Python package must initially not own serving logic.

## Design principles

1. **Separation of concerns** — security policy is in the core crate, CLI is in the bin crate, Python packaging is separate
2. **Core-first** — all security-critical logic lives in `eggserve-core` and can be used independently
3. **No premature serving** — the first milestone produces skeletons; plan 001 introduces the HTTP substrate
4. **Minimal surface** — each crate exposes only what is necessary for its purpose
