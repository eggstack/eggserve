# Python API

eggserve provides a Python API with two layers: native primitives (PyO3-backed Rust bindings) and a subprocess lifecycle wrapper. The native layer exposes hardened path parsing, policy enforcement, resource resolution, and response planning without launching the server binary. The subprocess layer manages the Rust binary for full HTTP serving.

**This is NOT an ASGI/WSGI server, a web framework, or a request callback system.** It is a hardened static-serving primitive.

## Quick start

```python
from eggserve import serve_directory

# Serve current directory on 127.0.0.1:8000 (blocking)
serve_directory(".")
```

## Native primitives

When available (`eggserve.NATIVE_AVAILABLE is True`), the package exposes Rust-backed primitives directly. These provide path confinement, policy enforcement, secure root resolution, and response planning without a subprocess.

### `PathPolicy`

Path-level policy for request-target parsing.

```python
from eggserve import PathPolicy

policy = PathPolicy(allow_dotfiles=False, reject_backslash=True)
```

Defaults: `allow_dotfiles=False`, `reject_backslash=True`. Frozen after construction.

### `StaticPolicy`

Filesystem access policy. All defaults are safe.

```python
from eggserve import StaticPolicy

policy = StaticPolicy(
    directory_listing=False,  # no directory listing
    follow_symlinks=False,   # deny symlinks
    allow_dotfiles=False,    # deny dotfiles
)
```

Frozen after construction. Identical defaults to the CLI.

### `RequestTarget`

Parses and validates HTTP request targets.

```python
from eggserve import RequestTarget, PathPolicy

# Parse with default policy
target = RequestTarget.parse("/assets/app.css")
print(target.decoded_path)  # "/assets/app.css"
print(target.components)    # ["assets", "app.css"]

# Parse with custom policy
target = RequestTarget.parse("/.hidden", PathPolicy(allow_dotfiles=True))
```

Raises `PathPolicyError` for traversal, dotfiles, empty paths, NUL bytes, backslash (default). Raises `RequestTargetError` for unsupported URI forms.

### `SecureRoot`

Resolves request-derived paths under a canonical root directory.

```python
from eggserve import SecureRoot, StaticPolicy

root = SecureRoot("public", policy=StaticPolicy())
resource = root.resolve_path("/assets/app.css")
```

Methods:
- `resolve(target: RequestTarget) -> ResolvedResource`
- `resolve_path(raw_path: str, path_policy: PathPolicy | None = None) -> ResolvedResource`
- `policy` — read-only `StaticPolicy` property

Raises `SecureRootError` for missing/non-directory root.

### `ResolvedResource`

The result of resolving a path under a secure root.

```python
resource = root.resolve_path("/file.txt")

if resource.is_file:
    f = resource.file         # ResolvedFile
    plan = f.plan_response("GET")
    print(plan.status)        # 200
    print(plan.headers)       # [("content-length", "11"), ...]
    print(plan.body_kind)     # "file_full"

elif resource.is_directory:
    d = resource.directory    # ResolvedDirectory
    entries = d.list()        # [("file.txt", False), ...]

elif resource.is_denied:
    msg, code = resource.denied_reason  # ("symlink denied", "symlink_denied")

elif resource.is_not_found:
    print("not found")
```

Kind values: `"file"`, `"directory"`, `"not_found"`, `"denied"`.

### `ResolvedFile`

Safe metadata wrapper for an opened file. Only obtainable via `ResolvedResource.file`. `ResolvedFile` is a resolver-created capability — there is no public constructor; it can only be obtained through `SecureRoot` resolution.

Python `ResolvedFile` currently supports metadata and response planning. It does not yet expose the resolver-opened file handle for streaming; production byte-serving should wait for a dedicated file-streaming primitive.

**Do not** reconstruct a filesystem path from `safe_relative_components()` and reopen it manually. The file handle from `resource.file` was opened under policy enforcement during resolution; reopening by path bypasses the security guarantees.

- `length` — file size in bytes
- `modified` — modification time as UNIX timestamp (float), or `None`
- `content_type` — MIME type string (e.g. `"text/plain; charset=utf-8"`)
- `safe_relative_components` — path components as list of strings
- `plan_response(method="GET", headers=None)` — returns `ResponsePlan`
- `plan_conditional_response(method="GET", headers=None)` — handles `If-None-Match`, `If-Modified-Since`, `Range`, `If-Range`

### `ResolvedDirectory`

Directory listing and child resolution. Only obtainable via `ResolvedResource.directory`. Directory primitive listing is policy-filtered filesystem listing; HTTP directory-listing exposure remains separately controlled by `StaticPolicy.directory_listing`.

- `safe_relative_components` — path components as list of strings
- `list()` — returns `[(name: str, is_dir: bool), ...]` filtered by the originating `StaticPolicy`
- `resolve_child(name: str) -> ResolvedResource` — resolve a child entry using the originating `StaticPolicy`

### `ResponsePlan`

A `namedtuple` with fields: `status` (int), `headers` (list of `(name, value)` tuples), `body_kind` (`"empty"`, `"bytes"`, `"file_full"`, `"file_range"`), `range` (tuple or `None`).

### Validation functions

```python
from eggserve import validate_method, validate_request_body, validate_request_target

validate_method("GET")                              # returns "GET"
validate_request_body()                             # OK for no body
validate_request_body(content_length="100")         # raises RequestValidationError
validate_request_target("/valid/path")              # OK
validate_request_target("http://example.com/path")  # raises RequestValidationError
```

**Note:** `validate_request_target()` performs a coarse origin-form syntax check (starts with `/`, no `*` or authority form). It is not a replacement for `RequestTarget.parse()` / `ConfinedPath` validation, which performs full path confinement, dotfile, traversal, and backslash checks.

### `generate_etag`

```python
from eggserve import generate_etag

etag = generate_etag(resolved_file)  # e.g. 'W/"11-1783605793"'
```

Returns a weak ETag string or `None`.

### Exception hierarchy

```
Exception
└── EggserveError
    ├── PathPolicyError        # path validation/confinement errors
    ├── RequestTargetError     # malformed/unsupported request targets
    ├── SecureRootError        # root initialization/resolution errors
    └── RequestValidationError # request validation errors
```

All exceptions carry `(message, code)` args.

## Configuration (subprocess API)

The subprocess API wraps the Rust binary for full HTTP serving.

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

## Serving (subprocess API)

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
| Invalid `port` (non-int, out of range, bool) | `ValueError` (at config construction) |
| Invalid `log_format` (not in `text`/`json`/`none`) | `ValueError` (at config construction) |
| Bind to `0.0.0.0` or `::` without `public=True` | `ValueError` (at config construction) |
| Binary not found | `FileNotFoundError` |
| Server already running | `RuntimeError` |
| Interrupted | `KeyboardInterrupt` (normal) |

`ServeConfig.__post_init__` validates port, log format, and public-bind combinations so invalid configurations fail before the subprocess is spawned. The Rust CLI performs the same checks independently as defense in depth.

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
- Async server lifecycle
- Dynamic Python code execution in request paths

The native primitives provide response **planning**, not response **writing**. The caller maps plans to sockets. For those use cases, consider Uvicorn, Granian, or similar application servers.

## Installation

```sh
pip install eggserve
```

The wheel includes the native extension (PyO3) and the Rust binary. If the native extension cannot be loaded (e.g. platform mismatch), the subprocess API remains available.

## Examples

See [examples/python_basic.py](../examples/python_basic.py).

## Testing

```sh
# Native primitives tests
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_primitives -v

# Subprocess API tests
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_server -v
```
