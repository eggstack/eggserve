# Python Packaging

eggserve is distributed as a Python wheel containing the pre-built Rust binary. The Python package provides `pip install` and `python -m` entrypoints while the actual serving is performed by the native binary.

## Architecture

```
crates/eggserve-python/
├── Cargo.toml              # depends on eggserve-core + pyo3
├── pyproject.toml          # maturin build backend
├── src/lib.rs              # PyO3 native module (_native)
├── python/eggserve/
│   ├── __init__.py         # exports version, ServeConfig, StaticPolicy, serve_directory
│   ├── __main__.py         # python -m eggserve entrypoint
│   ├── _bin.py             # locates and executes the packaged binary
│   ├── server.py           # Python API implementation
│   └── test_server.py      # Python API tests
├── packaging-tests/        # standalone installed-wheel validation
│   ├── run_all.sh          # fresh venv + install + run all smoke tests
│   ├── test_imports.py     # import validation, version, native extension
│   ├── test_server_smoke.py # server lifecycle, callback, HEAD, range
│   ├── test_client_smoke.py # HTTP client local request
│   └── test_cli_smoke.py   # CLI help, binary discovery
└── README.md
```

### How it works

1. **maturin** builds the Rust lib crate (with PyO3 bindings) and packages it into a platform-specific wheel
2. `pip install eggserve` installs the wheel, which places the native module and Python package in site-packages
3. `python -m eggserve` invokes `_bin.py`, which locates the bundled binary and executes it via `subprocess.run()`
4. All CLI arguments are forwarded directly to the binary
5. Native primitives (path parsing, resolution, response planning) are available directly via the `_native` PyO3 module without subprocess overhead

### Why subprocess for CLI?

The binary is a standalone process (Tokio runtime, TCP listener, signal handling). It cannot run inside the Python process for full HTTP serving. The subprocess approach keeps the Rust binary self-contained. For primitive operations (path parsing, resolution, response planning), the native `_native` PyO3 module provides direct in-process access without subprocess overhead.

## Python API

In addition to CLI usage, eggserve exposes a minimal Python API:

```python
from eggserve import ServeConfig, StaticPolicy, serve_directory

# Blocking serve with config
config = ServeConfig(directory="public", port=9000)
serve_directory(config.directory, bind=config.bind, port=config.port)
```

See [docs/python-api.md](python-api.md) for the full API reference.

## Building

### Prerequisites

- Rust toolchain (stable)
- CPython 3.14 only (`>=3.14,<3.15`); PyPy and free-threaded builds are not supported
- maturin: `pip install maturin`

### Build a wheel

The wheel bundles the platform-native `eggserve` CLI. Stage the binary before
calling maturin; the release and CI workflows do this on each OS runner.

```sh
cargo build --release --locked -p eggserve-bin
mkdir -p crates/eggserve-python/python/eggserve/bin
cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
chmod +x crates/eggserve-python/python/eggserve/bin/eggserve
cd crates/eggserve-python
maturin build --release --interpreter python3.14 -o dist
```

This produces a platform-specific wheel in `target/wheels/`.

### Build for development

```sh
maturin develop
```

This installs the package in the current virtualenv in development mode.

## Platform support

The wheel is platform-specific because it contains a native binary. maturin automatically detects:

- **OS**: linux, macos, windows
- **Architecture**: x86_64, aarch64, arm64 (Apple Silicon)

CI and release validation build and install wheels on Linux, macOS, and
Windows runners with CPython 3.14. The wheel smoke suite runs outside the
checkout with `PYTHONPATH` unset and requires the bundled CLI to be found.

## Versioning

The version is defined in three places and must be kept in sync:

1. `Cargo.toml` (`version = "0.1.0"`)
2. `pyproject.toml` (`version = "0.1.0"`)
3. `python/eggserve/__init__.py` (`__version__ = "0.1.0"`)

## Entry points

| Command | What runs |
|---------|-----------|
| `eggserve` (from wheel) | Native binary directly |
| `python -m eggserve` | `_bin.py` → subprocess → native binary |
| `pipx run eggserve` | Native binary directly |

## Dependencies

The Python package has **no Python dependencies**. The only requirement is the platform-specific wheel containing the Rust binary.

The Rust binary depends on: `tokio`, `hyper`, `hyper-util`, `http-body-util`, `bytes`, `futures-util`, `httpdate`, `phf`, `thiserror` — all compiled in.

## Packaging Smoke Tests

Standalone tests in `packaging-tests/` validate the wheel works independently of the source checkout. These tests:

- Run from a temporary directory (not the source tree)
- Use `PYTHONPATH` unset to prevent source-tree contamination
- Validate all public imports, version metadata, native extension loading
- Exercise server lifecycle, callback handlers, HEAD/range responses
- Test HTTP client against a local server
- Verify CLI help output and binary discovery

### Running packaging smoke tests

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

### What the tests validate

| Test file | What it checks |
|-----------|---------------|
| `test_imports.py` | All `__all__` names importable, version valid, native extension loads, no source-tree shadowing |
| `test_server_smoke.py` | Server start/stop, ephemeral port, context manager, callback handler, static fallback, HEAD, range (206), public-bind guard |
| `test_client_smoke.py` | HttpClient construction, GET/HEAD against local server, 404, response headers, error handling |
| `test_cli_smoke.py` | `python -m eggserve --help` exits 0, binary discovery, binary executable, version consistency |
