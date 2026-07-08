# Guide for AI coding agents

## Project overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior, intended as a hardened replacement for `python -m http.server`. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. The project is currently in early planning; the workspace skeleton exists but no HTTP serving logic is implemented yet.

## Non-negotiables

- **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. See [docs/security-policy.md](docs/security-policy.md).
- **No serving outside the configured root.** Path traversal and symlink escape must be denied at the library level. See [docs/threat-model.md](docs/threat-model.md).
- **No broad dependencies.** Every dependency must have an explicit purpose. See [docs/dependency-policy.md](docs/dependency-policy.md).
- **Plan-driven development.** Every change must be backed by a plan in `plans/`. No ad-hoc feature additions.

## Layout

```
eggserve/
├── Cargo.toml              # workspace root
├── crates/
│   ├── eggserve-core/      # security policy, path confinement, static serving, response construction
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs   # configuration types
│   │       ├── policy.rs   # security policy enforcement
│   │       ├── limits.rs   # resource limits
│   │       ├── error.rs    # error types
│   │       └── path.rs     # path confinement and resolution
│   └── eggserve-bin/       # CLI binary, config loading, signal handling
│       └── Cargo.toml
├── docs/                   # project documentation
├── plans/                  # design plans and roadmap
└── AGENTS.md               # this file
```

`eggserve-python/` is not yet a Cargo crate; Python packaging is deferred to plan 005.

## Common commands

```sh
cargo fmt --all                    # format all crates
cargo clippy --workspace --all-targets -- -D warnings  # lint
cargo test --workspace             # run tests
```

## Plan-driven development

All implementation work must be traced to a plan in `plans/`. Plans define scope, acceptance criteria, and boundaries. Do not implement features that are not covered by an existing plan. If a change requires expanding scope, update the relevant plan first.

## Scope discipline

Before implementing any feature, check:

1. Does the feature appear in a plan in `plans/`?
2. Is it listed as a non-goal in `docs/non-goals.md`? If so, the non-goal must be updated first.
3. Does it affect the threat model? If so, update `docs/threat-model.md`.

## Don'ts

- Do not add broad dependencies without justification (see [docs/dependency-policy.md](docs/dependency-policy.md))
- Do not write HTTP server code yet — plan 001 introduces the HTTP substrate
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
