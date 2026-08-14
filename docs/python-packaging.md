# Python Packaging

eggserve is distributed as a Python wheel containing a PyO3 extension and the
Python façade. The `eggserve` and `python -m eggserve` entry points call the
extension-linked Rust CLI; the wheel does not contain a second standalone
server executable.

## Architecture

```
crates/eggserve-python/
├── Cargo.toml              # depends on eggserve-core + pyo3
├── pyproject.toml          # maturin build backend
├── src/lib.rs              # PyO3 native module (_native)
├── python/eggserve/
│   ├── __init__.py         # exports version, ServeConfig, StaticPolicy, serve_directory
│   ├── __main__.py         # python -m eggserve entrypoint
│   ├── _bin.py             # invokes the extension-backed CLI entry point
│   ├── server.py           # Python API implementation
│   └── test_server.py      # Python API tests
├── packaging-tests/        # standalone installed-wheel validation
│   ├── run_all.sh          # fresh venv + install + run all smoke tests
│   ├── test_imports.py     # import validation, version, native extension
│   ├── test_server_smoke.py # server lifecycle, callback, HEAD, range
│   ├── test_client_smoke.py # HTTP client local request
│   └── test_cli_smoke.py   # CLI help, native entry point
└── README.md
```

### How it works

1. **maturin** builds the Rust lib crate (with PyO3 bindings) and packages it into a platform-specific wheel
2. `pip install eggserve` installs the wheel, which places the native module and Python package in site-packages
3. `python -m eggserve` invokes `_bin.py`, which calls the native `_run_cli` entry point
4. All CLI arguments are forwarded directly to the extension-linked Rust CLI
5. Native primitives (path parsing, resolution, response planning) are available directly via the `_native` PyO3 module without subprocess overhead

### Native CLI entry point

The CLI runs in the Python process through the native extension, sharing the
Rust CLI implementation with the standalone Cargo binary. `ServerProcess` in
`eggserve.subprocess` remains available when an embedding application needs a
separate child process.

## Python API

In addition to CLI usage, eggserve exposes a minimal Python API:

```python
from eggserve.subprocess import ServeConfig, StaticPolicy
from eggserve import serve_directory

# Blocking serve with config
config = ServeConfig(directory="public", port=9000)
serve_directory(config.directory, bind=config.bind, port=config.port)
```

See [docs/python-api.md](python-api.md) for the full API reference.

## Building

### Prerequisites

- Rust toolchain (stable)
- CPython 3.11+ with abi3 stable ABI (`>=3.11`); PyPy and free-threaded builds are not supported
- maturin: `pip install maturin`

### Build a wheel

Build the extension-backed wheel directly; no binary staging step is needed.

```sh
cd crates/eggserve-python
maturin build --profile dist --interpreter python3.11 -o dist
```

This produces a platform-specific wheel in `target/wheels/`.

### Build for development

```sh
maturin develop
```

This installs the package in the current virtualenv in development mode.

## Platform support

The wheel is platform-specific because it contains a native extension. maturin automatically detects:

- **OS**: linux, macos, windows
- **Architecture**: x86_64, aarch64, arm64 (Apple Silicon)

Routine CI builds and tests the Linux wheel with CPython 3.14. macOS and
Windows wheels are built and tested manually. The abi3 wheel is compatible
with CPython 3.11+. The wheel smoke suite runs outside the checkout with
`PYTHONPATH` unset and requires the installed extension-backed CLI entry point.

## Versioning

The version is defined in three places and must be kept in sync:

1. `Cargo.toml` (`version = "0.1.0"`)
2. `pyproject.toml` (`version = "0.1.0"`)
3. `python/eggserve/__init__.py` (`__version__ = "0.1.0"`)

## Entry points

| Command | What runs |
|---------|-----------|
| `eggserve` (from wheel) | `_bin.py` → native `_run_cli` |
| `python -m eggserve` | `_bin.py` → native `_run_cli` |
| `pipx run eggserve` | Installed wheel console script |

## Dependencies

The Python package has **no Python dependencies**. The only requirement is the platform-specific wheel containing the Rust extension.

The native extension depends on: `eggserve-core`, `eggserve-bin`, `tokio`, and `rustls` — all compiled into the wheel. HTTP serving, body handling, and filesystem confinement are provided by `eggserve-core`.

## Packaging Smoke Tests

Standalone tests in `packaging-tests/` validate the wheel works independently of the source checkout. These tests:

- Run from a temporary directory (not the source tree)
- Use `PYTHONPATH` unset to prevent source-tree contamination
- Validate all public imports, version metadata, native extension loading
- Exercise server lifecycle, callback handlers, HEAD/range responses
- Test HTTP client against a local server
- Verify CLI help output and the installed extension-backed entry point

### Running packaging smoke tests

```sh
cd crates/eggserve-python
maturin build --profile dist --interpreter python3.11 -o dist
cd packaging-tests
bash run_all.sh ../dist/*.whl python3.14
```

### What the tests validate

| Test file | What it checks |
|-----------|---------------|
| `test_imports.py` | All `__all__` names importable, version valid, native extension loads, no source-tree shadowing |
| `test_server_smoke.py` | Server start/stop, ephemeral port, context manager, callback handler, static fallback, HEAD, range (206), public-bind guard |
| `test_cli_smoke.py` | `python -m eggserve --help` exits 0, installed console script, native entry point, version consistency |
