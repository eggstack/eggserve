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
    ├── bin/            # staged platform-native eggserve binary included in wheels
    ├── server.py       # Python API: ServeConfig, ServerProcess, serve_directory
    ├── test_primitives.py
    ├── test_server_primitives.py
    ├── test_server_integration.py
    ├── test_boundary_hardening.py
    ├── test_server.py
    └── test_api_stability.py
└── packaging-tests/
    ├── run_all.sh              # installs wheel in fresh venv, runs all smoke tests
    ├── test_imports.py         # import validation, version metadata, no source-tree shadowing
    ├── test_server_smoke.py    # server lifecycle, callback, HEAD, range, public-bind
    ├── test_client_smoke.py    # HTTP client local request
    └── test_cli_smoke.py       # CLI help, binary discovery, version consistency
```

**Important:** `eggserve-python` is excluded from the workspace and has its own `Cargo.lock`. It is built independently via `maturin`. Release wheels support CPython 3.14 only (`>=3.14,<3.15`) and bundle the platform-native CLI binary.

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

Exceptions: `EggserveError` (base), `PathPolicyError`, `RequestTargetError`, `SecureRootError`, `RequestValidationError`, `BodySourceError`, `ResponseConstructionError`, `LifecycleError`.

## Server Primitives (`src/server.rs`)

PyO3 bindings for building HTTP servers with Rust-owned I/O. Uses `tokio` for the async runtime, `hyper` for HTTP/1.1, and `futures-util` for streaming.

| Python Class | Wraps | Key Methods |
|---|---|---|
| `Request` | parsed HTTP request | `method`, `path`, `query`, `headers`, `remote_addr`, `http_version`, `has_body` |
| `Response` | response builder | `empty(status)`, `bytes(status, data, headers=None)`, `text(status, text, headers=None)`, `body_source(status, source, headers=None)` |
| `StaticResponder` | `SecureRoot` + `resolve_and_plan` | `respond(method, target, headers=None)` → `Response` |
| `StaticPolicyWrapper` | `policy::StaticPolicy` | `new(directory_listing, follow_symlinks, allow_dotfiles)`, getters |
| `ServerSecureRoot` | `primitives::SecureRoot` | `new(path, policy)`, `root_path` getter |
| `ServerBodySource` | `primitives::BodySource` | `kind`, `length`, `range`, `read_all()`, `read_range()`, `to_response(status=200)` |
| `Server` | tokio runtime + TcpListener | `start()`, `stop()`, `addr`, context manager, optional `handler` callback, `max_python_callbacks` concurrency limit |

Exceptions: `ServerRequestError` (method not allowed, target invalid, body not allowed).

### Lifecycle methods (Plan 053)

| Method | Behavior |
|--------|----------|
| `start()` | Creates tokio runtime, binds TcpListener, spawns accept loop. Blocks until ready. |
| `stop()` | Sends shutdown signal, joins thread (blocking). Idempotent. |
| `wait_ready()` | Returns `Ok(())` if Running; raises `LifecycleError` otherwise. |
| `shutdown()` | Sends shutdown signal, returns immediately (non-blocking). |
| `force_shutdown(timeout_secs)` | Graceful shutdown with deadline. Returns `"clean"` or `"timeout"`. |
| `wait()` | Blocks until thread joins. Returns `"stopped"`. |
| `state` (property) | Returns lifecycle state: `"created"`, `"running"`, `"stopped"`, `"failed"`. |

### Callback model (Plan 053)

- Handler timeout (`handler_timeout_secs`, default 30s): best-effort in Python; enforced at transport level by Rust server.
- Coroutine rejection: handlers returning coroutine objects are rejected with a 500 response.
- GIL released during network/file I/O via `py.allow_threads`.
- Callback concurrency bounded by `max_python_callbacks` semaphore.

### Response Validation

Every Python-produced `Response` passes through `validate_handler_response()` in Rust before being sent to the client:

- Status must be 200–999 (1xx informational responses rejected)
- Header values must not contain NUL, CR, or LF
- Hop-by-hop headers (connection, transfer-encoding, te, etc.) are blocked — Hyper manages these
- 204 and 304 responses must have empty bodies (body is stripped regardless of handler return)
- HEAD responses have body suppressed automatically
- Invalid responses fall back to 500 Internal Server Error

Handler exceptions produce a generic 500 with no traceback, filesystem path, or Python repr leakage.

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

- **GIL management:** `tokio::task::spawn_blocking` + `Python::with_gil` ensures tokio is never blocked by Python. Callback concurrency is bounded by a semaphore (`max_python_callbacks`), preventing handler overload.
- **File streaming:** File bodies retain their Rust-owned `BodySource` capability and stream directly to the socket without an eager Python-memory copy.
- **Error handling:** Handler exceptions → 500 Internal Server Error without leaking tracebacks. Python-produced responses are validated in Rust via `validate_handler_response()` (plan 037) — hop-by-hop headers, 204/304 body prohibition, status range checks.
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
3. `PATH` fallback, including `eggserve.exe` on Windows

`python -m eggserve` forwards all args to the located binary.

## Key Design Decisions

1. **No serving logic in Python** — The Python layer is purely a lifecycle manager and config translator. All serving happens in the Rust binary or via Rust-owned I/O in the server primitives.

2. **Subprocess, not FFI** — The binary is spawned as a subprocess rather than linked as a shared library. This isolates the Python process from the server's memory and lifecycle.

3. **Server primitives use Rust-owned I/O** — The `Server` type runs a tokio runtime in a background thread. When a handler callback is provided, Python code receives parsed `Request` objects and returns `Response` values. When no handler is provided, the server serves static files. Socket I/O, HTTP parsing, and file streaming are handled by Rust. Connection limits, header timeouts, write timeouts, and callback concurrency are enforced. The GIL is released during I/O. Lifecycle methods (`wait_ready()`, `shutdown()`, `force_shutdown()`, `wait()`, `state`) provide parity with the Rust `ServerHandle` API. Coroutine handlers are rejected with a 500 response.

4. **Frozen immutability** — All PyO3 classes use `#[pyclass(frozen)]`. All Python dataclasses use `frozen=True`. This prevents mutation at both layers.

5. **Independent build** — The Python crate has its own `Cargo.lock` and is built via `maturin`, not the workspace. This avoids pulling workspace dependencies into the Python wheel.

6. **File streaming bypasses Python** — File bodies are streamed directly from Rust to the socket. The handler boundary clones the file capability when necessary but does not read the file into Python memory, keeping memory usage low and avoiding GIL contention.

## Build & Test

```sh
# Build wheel after staging target/release/eggserve under python/eggserve/bin/
cd crates/eggserve-python
maturin build --release --interpreter python3.14 --target x86_64-apple-darwin -o dist

# Install
python -m pip install --force-reinstall dist/*.whl

# Run native primitives tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_primitives -v

# Run server primitives tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v

# Run subprocess API tests (no wheel needed, uses mocks)
PYTHONPATH=python python -m unittest eggserve.test_server -v

# Run boundary hardening tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_boundary_hardening -v

# Run server integration tests (requires built wheel)
PYTHONPATH=python python -m unittest eggserve.test_server_integration -v

# Run packaging smoke tests (installed-wheel validation, no source-tree imports)
cd packaging-tests
bash run_all.sh ../dist/*.whl python3.14
```

## Packaging Smoke Tests

Standalone tests in `packaging-tests/` validate the wheel works independently of the source checkout:

- `test_imports.py` — all `__all__` names importable, version metadata valid, native extension loads, no source-tree shadowing
- `test_server_smoke.py` — server lifecycle, callback handler, HEAD/range responses, public-bind guard
- `test_client_smoke.py` — HTTP client local request against a running server
- `test_cli_smoke.py` — `python -m eggserve --help`, binary discovery, version consistency
- `run_all.sh` — creates fresh venv, installs wheel, copies scripts to temp dir, runs all tests

These tests run from a temporary directory with `PYTHONPATH` unset to ensure no source-tree contamination.

## See Also

- [eggserve-core.md](eggserve-core.md) — Core library (native types)
- [primitives-api.md](primitives-api.md) — Public API boundary

## API Stability

Python API stability follows the same tiers as the Rust core. See [api-stability.md](../docs/api-stability.md) for the full classification and [release-contract.md](../docs/release-contract.md) for the product surface.

Key points:
- `ServeConfig`, `ServerProcess`, `serve_directory`, and all native primitives are **stable**.
- `Server`, `Request`, `Response`, `StaticResponder`, and server primitives are **stable**.
- `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse`, `ClientError`, and `Method` are **experimental**.
- Internal names (`_bin.py`, `_parse_bind`, `_config_to_argv`) are **internal** and not exported.
