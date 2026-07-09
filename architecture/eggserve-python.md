# eggserve-python — Deep Dive

Python wheel packaging via maturin. Provides two API layers: native Rust primitives via PyO3, and subprocess lifecycle management for full HTTP serving.

## Structure

```
crates/eggserve-python/
├── Cargo.toml          # cdylib, depends on eggserve-core + pyo3
├── pyproject.toml      # maturin build backend, module-name = "eggserve._native"
├── src/lib.rs          # PyO3 native module (_native)
└── python/eggserve/
    ├── __init__.py     # exports all public symbols
    ├── __main__.py     # python -m eggserve
    ├── _bin.py         # locates packaged binary or PATH fallback
    ├── server.py       # Python API: ServeConfig, ServerProcess, serve_directory
    ├── test_primitives.py
    └── test_server.py
```

**Important:** `eggserve-python` is excluded from the workspace and has its own `Cargo.lock`. It is built independently via `maturin`.

## Native Primitives (`src/lib.rs`)

PyO3 bindings wrapping `eggserve-core` types. All classes are **frozen** (`#[pyclass(frozen)]`).

| Python Class | Wraps | Key Methods |
|---|---|---|
| `PathPolicy` | `path::PathPolicy` | `__init__(allow_dotfiles, reject_backslash)` |
| `StaticPolicy` | `policy::StaticPolicy` | `__init__(directory_listing, follow_symlinks, allow_dotfiles)` |
| `RequestTarget` | `ConfinedPath` | `parse(raw, policy=None)` → `decoded_path`, `components` |
| `SecureRoot` | `primitives::SecureRoot` | `__init__(path, policy)`, `resolve(target)`, `resolve_path(raw_path)` |
| `ResolvedResource` | `primitives::ResolvedResource` | `kind` getter, `file`, `directory`, `denied_reason` |
| `ResolvedFile` | `primitives::ResolvedFile` | `length`, `modified`, `content_type`, `plan_response()`, `plan_conditional_response()`, `body_for_plan(plan)` |
| `ResolvedDirectory` | `primitives::ResolvedDirectory` | `list()`, `resolve_child(child)` |
| `BodySource` | `primitives::BodySource` | `kind`, `length`, `range`, `read_all()`, `read_range(start, end)` |
| `ResponsePlan` | `primitives::StaticResponsePlan` | `status`, `headers`, `body_kind`, `range` |

Functions: `validate_method()`, `validate_request_body()`, `validate_request_target()`, `generate_etag()`.

Exceptions: `EggserveError` (base), `PathPolicyError`, `RequestTargetError`, `SecureRootError`, `RequestValidationError`.

## Python Wrapper Layer (`server.py`)

High-level Python API that translates config to CLI arguments and manages the binary subprocess.

### `StaticPolicy` (frozen dataclass)

Mirrors the Rust policy. All fields default to `False` (most restrictive):
- `directory_listing: bool`
- `follow_symlinks: bool`
- `allow_dotfiles: bool`

### `ServeConfig` (frozen dataclass)

Configuration for the server. Includes validation in `__post_init__`:
- Port range check (1–65535)
- Public-bind guard (requires `public_bind=True` for `0.0.0.0`)

### `ServerProcess`

Subprocess lifecycle manager:
- `start()` — Spawns the `eggserve` binary with CLI arguments derived from `ServeConfig`
- `stop()` — Sends SIGTERM, waits for graceful shutdown
- `wait()` — Blocks until process exits
- `is_running` — Property checking process status
- `pid` — Property returning process ID

### `serve_directory()`

Blocking convenience function: starts a server and waits.

## Binary Location (`_bin.py`)

Searches for the `eggserve` binary in:
1. Package `bin/` directory
2. Parent `bin/` directory
3. `PATH` fallback

`python -m eggserve` forwards all args to the located binary.

## Key Design Decisions

1. **No serving logic in Python** — The Python layer is purely a lifecycle manager and config translator. All serving happens in the Rust binary.

2. **Subprocess, not FFI** — The binary is spawned as a subprocess rather than linked as a shared library. This isolates the Python process from the server's memory and lifecycle.

3. **Frozen immutability** — All PyO3 classes use `#[pyclass(frozen)]`. All Python dataclasses use `frozen=True`. This prevents mutation at both layers.

4. **Independent build** — The Python crate has its own `Cargo.lock` and is built via `maturin`, not the workspace. This avoids pulling workspace dependencies into the Python wheel.

## Build & Test

```sh
# Build wheel
cd crates/eggserve-python
maturin build --release -o dist

# Install
python -m pip install --force-reinstall dist/*.whl

# Run native primitives tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_primitives -v

# Run subprocess API tests (no wheel needed, uses mocks)
PYTHONPATH=python python -m unittest eggserve.test_server -v
```

## See Also

- [eggserve-core.md](eggserve-core.md) — Core library (native types)
- [primitives-api.md](primitives-api.md) — Public API boundary
