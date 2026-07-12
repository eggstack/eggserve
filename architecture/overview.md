# Architecture Overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction.

## Workspace Structure

```
eggserve/
├── Cargo.toml              # workspace root (resolver = "2", edition 2021)
├── crates/
│   ├── eggserve-core/      # library: security primitives, HTTP serving
│   ├── eggserve-bin/       # binary: CLI, accept loop, signal handling
│   └── eggserve-python/    # Python wheel (maturin + PyO3, independent build)
├── docs/                   # project documentation (21 files)
├── plans/                  # design plans (000–039 complete)
├── fuzz/                   # fuzzing targets
└── examples/
```

## Crate Dependency Graph

```
eggserve-core          ← eggserve-bin (path dep)
eggserve-core          ← eggserve-python (path dep)
eggserve-bin           → (standalone, owns process lifecycle)
eggserve-python        → (standalone, owns Python packaging)
```

`eggserve-core` has no workspace dependencies. `eggserve-bin` and `eggserve-python` each depend on `eggserve-core` via path. The Python subprocess layer communicates with the binary via CLI arguments (no shared memory, no FFI to the bin crate).

## Components

| Component | Crate | Deep Dive |
|-----------|-------|-----------|
| Core library | `eggserve-core` | [eggserve-core.md](eggserve-core.md) |
| CLI binary | `eggserve-bin` | [eggserve-bin.md](eggserve-bin.md) |
| Python bindings | `eggserve-python` | [eggserve-python.md](eggserve-python.md) |
| Path confinement | `eggserve-core::path` | [path-confinement.md](path-confinement.md) |
| Filesystem confinement | `eggserve-core::fs` | [filesystem-confinement.md](filesystem-confinement.md) |
| Policy system | `eggserve-core::policy` | [policy-system.md](policy-system.md) |
| HTTP response planning | `eggserve-core::primitives::planner` | [response-planning.md](response-planning.md) |
| Public API boundary | `eggserve-core::primitives` | [primitives-api.md](primitives-api.md) |

## Key Architectural Decisions

1. **Safe by default** — Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. Binding to `0.0.0.0` requires `--public` to acknowledge intent.

2. **No serving outside the configured root** — Path traversal and symlink escape are denied at the library level. On Unix with safe defaults, symlink denial is *descriptor-relative* — each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is refused rather than followed.

3. **No broad dependencies** — Every dependency has an explicit purpose. `phf` for compile-time MIME map, `rustix` for Unix syscalls, `httpdate` for Last-Modified. No framework dependencies beyond Hyper. Manual argument parsing (no clap).

4. **Plan-driven development** — Every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

5. **Framework-independent response planning** — `StaticResponsePlan`, `BodyPlan`, `HeaderMapPlan` are pure value objects with no Hyper dependency. The Python bindings consume these directly.

6. **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing level, controls whether dotfile paths are accepted) and `policy::DotfilePolicy` (serving level, controls whether dotfiles are served). Both must agree for dotfiles to be served.

7. **File-stream semaphore** — A bounded semaphore limits concurrent file streams (default 32). When exhausted, the handler returns 503 Service Unavailable.

8. **Python immutability** — All PyO3 classes are `#[pyclass(frozen)]` and Python dataclasses use `frozen=True`. Immutability enforced at both layers.

## Data Flow

```
HTTP Request
    │
    ▼
┌─────────────────────────────────────────────┐
│ eggserve-bin: accept loop, connection mgmt  │
│  • Semaphore for connection limiting        │
│  • Per-connection timeouts                  │
│  • TLS termination (optional)               │
└─────────────────┬───────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────┐
│ eggserve-core::service::handle_request()    │
│  1. Validate method (GET/HEAD only)         │
│  2. Reject body (metadata only)             │
│  3. Parse request target → ConfinedPath     │
│  4. Resolve via SecureRoot → ResolvedResource│
│  5. Plan response (conditional, range, ETag)│
│  6. Stream file / list directory / error    │
└─────────────────────────────────────────────┘
                  │
                  ▼
         HTTP Response
```

## Module Visibility Model

| Tier | Modules | Stability |
|------|---------|-----------|
| Stable | `primitives` (facade), `primitives::http`, `primitives::planner`, `primitives::response` | Intended public boundary for embedding consumers |
| Stable-ish | `config`, `limits`, `policy` | Field shapes may evolve before 1.0 |
| Experimental | `service` (`handle_request`) | Body type and async surface may change |
| Internal | `fs`, `path`, `response`, `mime`, `error` | `pub(crate)` — not part of public API |

## Non-Goals

eggserve explicitly does not aim to be: an ASGI/WSGI server, a CGI executor, a file upload handler, a reverse proxy, an ACME client, a plugin host, a template engine, or an auth system. It competes with `python -m http.server` for local development use cases, not with nginx, Caddy, or Uvicorn.
