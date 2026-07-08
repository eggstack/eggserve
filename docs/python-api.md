# Python API

eggserve provides a minimal Python API for programmatic static file serving. The API is a thin wrapper around the packaged Rust binary — it translates Python config objects to CLI arguments and manages the subprocess lifecycle.

**This is NOT an ASGI/WSGI server, a web framework, or a request callback system.** It is a hardened static-serving primitive.

## Quick start

```python
from eggserve import serve_directory

# Serve current directory on 127.0.0.1:8000 (blocking)
serve_directory(".")
```

## Configuration

### `StaticPolicy`

Controls filesystem access. All defaults are safe.

```python
from eggserve import StaticPolicy

policy = StaticPolicy(
    directory_listing=False,  # no directory listing
    follow_symlinks=False,   # deny symlinks
    allow_dotfiles=False,    # deny dotfiles
)
```

### `ServeConfig`

Full server configuration with safe defaults matching the CLI.

```python
from eggserve import ServeConfig, StaticPolicy

config = ServeConfig(
    directory="public",       # root directory to serve
    bind="127.0.0.1",         # bind address
    port=8000,                # listen port
    public=False,             # require True for 0.0.0.0
    policy=StaticPolicy(),    # filesystem policy
    log_format="text",        # "text", "json", or "none"
)
```

## Serving

### `serve_directory()` — blocking

Runs the server until interrupted. This is the simplest API.

```python
from eggserve import serve_directory

serve_directory("public", bind="127.0.0.1", port=9000)
```

### `ServerProcess` — lifecycle control

For tests and embedding where you need start/stop control.

```python
from eggserve import ServeConfig, ServerProcess

config = ServeConfig(directory="public", port=9000)
proc = ServerProcess(config)
proc.start()
print(f"PID: {proc.pid}")
# ... do work ...
proc.stop()
```

Methods:
- `start()` — spawn the server subprocess
- `stop(timeout=None)` — terminate (with optional graceful timeout)
- `wait()` — block until exit, returns exit code
- `is_running` — property, True if subprocess is alive
- `pid` — property, subprocess PID or None

## Error handling

| Condition | Exception |
|-----------|-----------|
| Bind to 0.0.0.0 without `public=True` | `ValueError` |
| Binary not found | `FileNotFoundError` |
| Server already running | `RuntimeError` |
| Interrupted | `KeyboardInterrupt` (normal) |

## Defaults

All Python defaults match CLI/Rust safe defaults:

| Setting | Default |
|---------|---------|
| directory | `.` (current) |
| bind | `127.0.0.1` |
| port | `8000` |
| public | `False` |
| directory_listing | `False` |
| follow_symlinks | `False` |
| allow_dotfiles | `False` |
| log_format | `text` |

## Non-goals

The Python API deliberately does **not** provide:

- ASGI or WSGI compatibility
- Request callbacks or middleware
- Routing or template engines
- Response construction APIs
- Async server lifecycle
- Dynamic Python code execution in request paths

For those use cases, consider Uvicorn, Granian, or similar application servers.

## Installation

```sh
pip install eggserve
```

## Examples

See [examples/python_basic.py](../examples/python_basic.py).
