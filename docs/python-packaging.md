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
│   ├── test_body_smoke.py  # request body support validation
│   ├── test_lifecycle_smoke.py # process lifecycle validation
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

The wheel is platform-specific because it contains a native extension. Release
wheels are built for all 9 Tier 1 targets:

| Platform family | Wheel target |
|---|---|
| Linux x86_64 (glibc) | `manylinux_2_17_x86_64` |
| Linux aarch64 (glibc) | `manylinux_2_17_aarch64` |
| Linux armv7 (glibc) | `manylinux_2_17_armv7l` |
| Linux x86_64 (musl) | `musllinux_1_2_x86_64` |
| Linux aarch64 (musl) | `musllinux_1_2_aarch64` |
| macOS x86_64 | `macosx_11_0_x86_64` |
| macOS arm64 | `macosx_11_0_arm64` |
| Windows x86_64 | `win_amd64` |
| Windows arm64 | `win_arm64` |

Each wheel is an abi3 wheel (`cp311-abi3`), compatible with CPython 3.11+.
One wheel per platform serves all supported CPython minor versions.

Routine CI builds and tests the Linux x86_64 wheel. Full matrix builds and
cross-platform qualification happen at release time via the release workflow.
The wheel smoke suite runs outside the checkout with `PYTHONPATH` unset and
requires the installed extension-backed CLI entry point.

## Versioning

The release version must be identical across three packaging surfaces:

1. `Cargo.toml` (`version = "X.Y.Z"`) — workspace and Python crate
2. `pyproject.toml` (`version = "X.Y.Z"`) — Python distribution metadata
3. `python/eggserve/__init__.py` — derives from installed distribution
   metadata via `importlib.metadata.version("eggserve")`, so it cannot drift
   independently after installation

A preflight check script (`scripts/check-python-release-metadata.py`) validates
that all version surfaces agree before any release build begins. This script
uses only the Python standard library and runs as the first step of the release
workflow.

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
bash run_all.sh ../dist/*.whl python3.11
```

### What the tests validate

| Test file | What it checks |
|-----------|---------------|
| `test_imports.py` | All `__all__` names importable, version valid, native extension loads, no source-tree shadowing |
| `test_server_smoke.py` | Server start/stop, ephemeral port, context manager, callback handler, static fallback, HEAD, range (206), public-bind guard |
| `test_cli_smoke.py` | `python -m eggserve --help` exits 0, installed console script, native entry point, version consistency |
