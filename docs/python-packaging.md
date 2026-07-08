# Python Packaging

eggserve is distributed as a Python wheel containing the pre-built Rust binary. The Python package provides `pip install` and `python -m` entrypoints while the actual serving is performed by the native binary.

## Architecture

```
crates/eggserve-python/
├── Cargo.toml              # depends on eggserve-bin
├── pyproject.toml          # maturin build backend
├── src/main.rs             # Rust binary entrypoint (calls eggserve_bin::run())
├── python/eggserve/
│   ├── __init__.py         # exports version, ServeConfig, StaticPolicy, serve_directory
│   ├── __main__.py         # python -m eggserve entrypoint
│   ├── _bin.py             # locates and executes the packaged binary
│   ├── server.py           # Python API implementation
│   └── test_server.py      # Python API tests
└── README.md
```

### How it works

1. **maturin** builds the Rust binary and packages it into a platform-specific wheel
2. `pip install eggserve` installs the wheel, which places the binary in the package directory
3. `python -m eggserve` invokes `_bin.py`, which locates the binary and executes it via `subprocess.run()`
4. All CLI arguments are forwarded directly to the binary

### Why subprocess?

The binary is a standalone process (Tokio runtime, TCP listener, signal handling). It cannot run inside the Python process. The subprocess approach keeps the Rust binary self-contained and avoids PyO3/GIL complexity.

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
- Python 3.8+
- maturin: `pip install maturin`

### Build a wheel

```sh
cd crates/eggserve-python
maturin build --release
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

CI should build wheels for each target platform. The current alpha targets macOS ARM64 for local development.

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
