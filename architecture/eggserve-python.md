# eggserve-python — Deep Dive

Python wheel packaging via maturin. Provides three API layers: native Rust primitives via PyO3, server primitives for building HTTP servers with Rust-owned I/O, and subprocess lifecycle management for full HTTP serving.

## Structure

```
crates/eggserve-python/
├── Cargo.toml          # cdylib, depends on eggserve-core + pyo3 + tokio + hyper
├── pyproject.toml      # maturin build backend, module-name = "eggserve._native"
├── src/
│   ├── lib.rs          # PyO3 native module (_native): primitives bindings
│   └── server.rs       # Server primitives: PyRequest, PyResponse, StaticResponder, Server
└── python/eggserve/
    ├── __init__.py     # exports all public symbols
    ├── __main__.py     # python -m eggserve
    ├── _bin.py         # locates packaged binary or PATH fallback
    ├── server.py       # Python API: ServeConfig, ServerProcess, serve_directory
    ├── test_primitives.py
    ├── test_server_primitives.py
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

## Server Primitives (`src/server.rs`)

PyO3 bindings for building HTTP servers with Rust-owned I/O. Uses `tokio` for the async runtime, `hyper` for HTTP/1.1, and `futures-util` for streaming.

| Python Class | Wraps | Key Methods |
|---|---|---|
| `Request` | parsed HTTP request | `method`, `path`, `query`, `headers`, `remote_addr`, `http_version`, `has_body` |
| `Response` | response builder | `empty(status)`, `bytes(status, data)`, `text(status, text)`, `body_source(status, source)` |
| `StaticResponder` | `SecureRoot` + `resolve_and_plan` | `respond(method, target, headers=None)` → `Response` |
| `StaticPolicyWrapper` | `policy::StaticPolicy` | `new(...)`, `deny_all()`, getters |
| `ServerSecureRoot` | `primitives::SecureRoot` | `new(path, policy)`, `root_path` getter |
| `ServerBodySource` | `primitives::BodySource` | `kind`, `length`, `range`, `read_all()`, `read_range()`, `to_response()` |
| `Server` | tokio runtime + TcpListener | `start()`, `stop()`, `addr`, context manager, optional `handler` callback |

Functions: `parse_request(target, headers)` → `Request`.

Exceptions: `ServerRequestError` (method not allowed, target invalid, body not allowed).

### Architecture

```
Python handler code
    ↓ respond(method, target, headers)
StaticResponder
    ↓ resolve_and_plan() [eggserve-core]
    ↓ returns (StaticResponsePlan, BodySource)
PyResponse constructed
    ↓ returned to ServerService
convert_to_hyper_response()
    ↓ streams file body via futures_util::stream::unfold
Hyper Response sent to client
```

- **GIL management:** `tokio::task::spawn_blocking` + `Python::with_gil` ensures tokio is never blocked by Python.
- **File streaming:** File bodies bypass Python entirely — Rust streams `BodySource` directly to the socket.
- **Error handling:** Handler exceptions → 500 Internal Server Error without leaking tracebacks.
- **Readiness signal:** `start()` blocks until the listener is bound and ready, using `std::sync::mpsc`.

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

1. **No serving logic in Python** — The Python layer is purely a lifecycle manager and config translator. All serving happens in the Rust binary or via Rust-owned I/O in the server primitives.

2. **Subprocess, not FFI** — The binary is spawned as a subprocess rather than linked as a shared library. This isolates the Python process from the server's memory and lifecycle.

3. **Server primitives use Rust-owned I/O** — The `Server` type runs a tokio runtime in a background thread. When a handler callback is provided, Python code receives parsed `Request` objects and returns `Response` values. When no handler is provided, the server serves static files. Socket I/O, HTTP parsing, and file streaming are handled by Rust. Connection limits, header timeouts, and write timeouts are enforced. The GIL is released during I/O.

4. **Frozen immutability** — All PyO3 classes use `#[pyclass(frozen)]`. All Python dataclasses use `frozen=True`. This prevents mutation at both layers.

5. **Independent build** — The Python crate has its own `Cargo.lock` and is built via `maturin`, not the workspace. This avoids pulling workspace dependencies into the Python wheel.

6. **File streaming bypasses Python** — File bodies are streamed directly from Rust to the socket. Python never sees file contents in the server path, keeping memory usage low and avoiding GIL contention.

## Build & Test

```sh
# Build wheel
cd crates/eggserve-python
maturin build --release --interpreter 3.14 --target x86_64-apple-darwin -o dist

# Install
python3.14 -m pip install --force-reinstall dist/*.whl

# Run native primitives tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_primitives -v

# Run server primitives tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v

# Run subprocess API tests (no wheel needed, uses mocks)
PYTHONPATH=python python -m unittest eggserve.test_server -v
```

## See Also

- [eggserve-core.md](eggserve-core.md) — Core library (native types)
- [primitives-api.md](primitives-api.md) — Public API boundary
