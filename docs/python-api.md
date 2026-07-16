# Python API

eggserve provides a Python API with two layers: native primitives (PyO3-backed Rust bindings) and a subprocess lifecycle wrapper. The native layer exposes hardened path parsing, policy enforcement, resource resolution, and response planning without launching the server binary. Server primitives allow Python code to build HTTP servers while Rust owns socket I/O, HTTP parsing, response serialization, file streaming, and timeout enforcement. The subprocess layer manages the Rust binary for full HTTP serving.

**This is NOT an ASGI/WSGI server, a web framework, or a request callback system.** It is a hardened static-serving primitive.

## Quick start

```python
from eggserve import serve_directory

# Serve current directory on 127.0.0.1:8000 (blocking)
serve_directory(".")
```

For programmatic server control:

```python
from eggserve import Server, ServerSecureRoot

root = ServerSecureRoot(".")
server = Server(root=root)
server.start()
server.wait_ready()
print(f"Serving on {server.addr}, state={server.state}")
# ... do work ...
server.shutdown()
server.wait()
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

Safe metadata wrapper for an opened file. Only obtainable via `ResolvedResource.file`. `ResolvedFile` is a resolver-created capability. While `from_parts()` exists for internal bridging, external consumers should obtain `ResolvedFile` only through `SecureRoot` resolution. Reconstructing from raw parts bypasses the path confinement guarantee.

`ResolvedFile` supports metadata, response planning, and safe body streaming via the resolver-opened file handle. Use `body_for_plan(plan)` to obtain a `BodySource` that carries the opened file forward without path reopening.

**Do not** reconstruct a filesystem path from `safe_relative_components()` and reopen it manually. The file handle from `resource.file` was opened under policy enforcement during resolution; reopening by path bypasses the security guarantees.

- `length` — file size in bytes
- `modified` — modification time as UNIX timestamp (float), or `None`
- `content_type` — MIME type string (e.g. `"text/plain; charset=utf-8"`)
- `safe_relative_components` — path components as list of strings
- `plan_response(method="GET", headers=None)` — returns `ResponsePlan`
- `plan_conditional_response(method="GET", headers=None)` — handles `If-None-Match`, `If-Modified-Since`, `Range`, `If-Range`
- `body_for_plan(plan)` — consumes the resolved file, returns a `BodySource` for the given response plan. The file handle is consumed; this method may only be called once per `ResolvedFile`.

### `ResolvedDirectory`

Directory listing and child resolution. Only obtainable via `ResolvedResource.directory`. Directory primitive listing is policy-filtered filesystem listing; HTTP directory-listing exposure remains separately controlled by `StaticPolicy.directory_listing`.

- `safe_relative_components` — path components as list of strings
- `list()` — returns `[(name: str, is_dir: bool), ...]` filtered by the originating `StaticPolicy`
- `resolve_child(name: str) -> ResolvedResource` — resolve a child entry using the originating `StaticPolicy`

### `ResponsePlan`

A `namedtuple` with fields: `status` (int), `headers` (list of `(name, value)` tuples), `body_kind` (`"empty"`, `"bytes"`, `"file_full"`, `"file_range"`), `range` (tuple or `None`).

### `BodySource`

Opaque Rust-backed body object obtained from `ResolvedFile.body_for_plan(plan)`. Carries the resolver-opened file handle without path reopening.

Properties:
- `kind` — `"empty"`, `"bytes"`, `"file_full"`, or `"file_range"`
- `length` — content length in bytes (`int` or `None`)
- `range` — `(start, end_inclusive)` tuple for range bodies, or `None`

Methods:
- `read_all()` — reads entire body into memory (suitable for small files and tests)
- `read_range(start, end_inclusive)` — reads a byte sub-range

`BodySource` is non-frozen; `read_all()` and `read_range()` take `&mut self` because they advance the file position. The body source is consumed after reading; it cannot be reused.

**Do not** use `read_all()` for large files in production. For production streaming, pass the `BodySource` back to Rust for Hyper response construction.

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
    ├── RequestValidationError # request validation errors
    ├── BodySourceError        # body source conversion errors
    ├── LifecycleError         # lifecycle violations (double start, stop before start)
    ├── ResponseConstructionError # invalid handler Response object
    └── ServerRequestError     # server request handling errors (raises as ValueError)
```

`BodySourceError` covers body-source conversion failures (e.g., invalid range, already consumed). `LifecycleError` is raised on lifecycle violations such as double start or stop before start. `ResponseConstructionError` is raised when a handler returns an invalid `Response` object (e.g., invalid status code, forbidden headers). `ServerRequestError` is a PyO3 enum that raises as `ValueError` with a string message, not a native exception with `(message, code)` tuple args.

## Server primitives

Server primitives allow Python code to build HTTP servers while Rust owns socket I/O, HTTP parsing, response serialization, file streaming, backpressure, concurrency limits, and timeout enforcement. Python receives parsed `Request` objects and returns `Response` values. Rust streams file bodies directly without passing through Python memory.

### `StaticResponder`

Resolves request paths under a secure root and produces responses. Wraps `SecureRoot` and `resolve_and_plan()` from the core crate.

```python
from eggserve import StaticResponder, ServerSecureRoot

root = ServerSecureRoot("/var/www")
responder = StaticResponder(root)
response = responder.respond("GET", "/index.html")
print(response.status)   # 200
print(response.headers)  # {"content-length": "1234", ...}
```

Methods:
- `respond(method, target, headers=None)` — returns a `Response` object

The responder derives `PathPolicy` from the `SecureRoot`'s `StaticPolicy`. If the policy denies dotfiles, dotfile requests are rejected at the path-parsing level (403 Forbidden).

### `Response`

Response object returned by `StaticResponder` or constructed manually for dynamic endpoints.

```python
from eggserve import Response

# Empty response
r = Response.empty(204)

# Bytes response
r = Response.bytes(200, b"Hello, world!")

# Text response
r = Response.text(200, "Hello, world!")

# Body source response (for file streaming)
r = Response.body_source(200, body_source)
```

Properties: `status` (int), `headers` (dict of string → string).

Factory methods:
- `Response.empty(status)` — zero-length body
- `Response.bytes(status, data, headers=None)` — in-memory bytes body
- `Response.text(status, text, headers=None)` — text body with default content type `text/plain; charset=utf-8`
- `Response.body_source(status, source, headers=None)` — file-backed body from `ServerBodySource`

### `Server`

TCP server that accepts connections, parses HTTP requests, and dispatches to a responder or handler callback. The server uses the actual Rust runtime (`Server`/`ServerHandle` from `eggserve-core::server`) rather than implementing its own accept loop. Rust owns the accept loop, connection parsing, response serialization, and timeout enforcement.

```python
from eggserve import Server, ServerSecureRoot

root = ServerSecureRoot(".")
with Server(root=root) as server:
    print(f"Serving on {server.addr}")
```

With handler callback:

```python
from eggserve import Server, ServerSecureRoot, Request, Response

root = ServerSecureRoot(".")

def handler(request: Request) -> Response:
    if request.path == "/health":
        return Response.text(200, "ok")
    return Response.empty(404)

with Server(root=root, handler=handler) as server:
    print(f"Serving on {server.addr}")
```

Constructor: `Server(root, bind="127.0.0.1", port=8000, policy=None, handler=None, public=False, max_connections=100, max_file_streams=64, max_python_callbacks=8, header_timeout_secs=10, write_timeout_secs=30, handler_timeout_secs=30, graceful_shutdown_timeout_secs=10)`

**Default parity with Rust/CLI:** Python defaults intentionally differ from Rust `RuntimeConfig` defaults for Python-specific workloads:

| Parameter | Python default | Rust/CLI default | Reason |
|-----------|---------------|------------------|--------|
| `max_connections` | 100 | 64 | Higher concurrency for callback-heavy workloads |
| `max_file_streams` | 64 | 32 | Higher file-stream concurrency for mixed static/callback serving |
| `write_timeout_secs` | 30 | 60 | Lower write timeout for more responsive Python callbacks |
| `header_timeout_secs` | 10 | 10 | Same |
| `handler_timeout_secs` | 30 | 30 | Same |
| `graceful_shutdown_timeout_secs` | 10 | 10 | Same |

Parameters:
- `root` — server root directory path (string)
- `bind` — bind address (default: "127.0.0.1")
- `port` — listen port (default: 8000)
- `policy` — optional `StaticPolicyWrapper` for filesystem policy
- `handler` — optional Python callable `(Request) -> Response` for dynamic responses
- `public` — must be `True` to bind to 0.0.0.0 or ::
- `max_connections` — maximum concurrent connections (default: 100)
- `max_file_streams` — maximum concurrent file streams (default: 64)
- `max_python_callbacks` — maximum concurrent handler callbacks (default: 8)
- `header_timeout_secs` — header read timeout in seconds (default: 10)
- `write_timeout_secs` — response write timeout in seconds (default: 30)
- `handler_timeout_secs` — handler callback timeout in seconds (default: 30); uses the actual Rust runtime's handler timeout mechanism, enforced at transport level by the Rust server
- `graceful_shutdown_timeout_secs` — graceful shutdown drain deadline in seconds (default: 10)

Properties:
- `addr` — bound address string (e.g. "127.0.0.1:8000"), or `None` when stopped
- `state` — lifecycle state string: `"created"`, `"running"`, `"stopped"`, or `"failed"`

Methods:
- `start()` — start the server in a background thread; blocks until the listener is ready
- `stop()` — shut down the server and join the background thread (idempotent)
- `wait_ready()` — returns `Ok(())` if Running; raises `LifecycleError` otherwise
- `shutdown()` — sends shutdown signal, returns immediately (non-blocking)
- `force_shutdown(timeout_secs)` — graceful shutdown with deadline; returns `"clean"` or `"timeout"`
- `wait()` — blocks until thread joins; returns `"stopped"`
- `__enter__` / `__exit__` — context manager support

When `handler` is provided, the server calls `handler(request)` for each request and streams the returned `Response` back to the client. When `handler` is `None`, the server serves static files from the root directory. Handler exceptions map to generic 500 Internal Server Error responses without traceback leakage. Coroutine handlers (functions returning a coroutine object) are rejected with a 500 response.

Handler timeout (`handler_timeout_secs`) uses the actual Rust runtime's handler timeout mechanism — enforced at the transport level by the Rust server, not by a Python-side timer. If the handler does not return within the deadline, the connection is closed.

The server enforces connection limits, header read timeouts, and response write timeouts. Binding to 0.0.0.0 or :: requires `public=True`.

**Framing strictness:** The server enforces hardened HTTP/1 framing before any handler invocation. Requests containing both `Transfer-Encoding` and `Content-Length` are rejected with 400. Duplicate `Content-Length` fields are rejected with 400, even when values are identical. Malformed `Content-Length` values (non-numeric, negative, overflowing) are rejected at the HTTP/1 wire level by Hyper. These checks prevent HTTP request smuggling attacks where front-end and back-end servers disagree on message boundaries.

**Observability hooks:** The `Server` provides minimal observability via `state` and `addr` properties. Active connection/stream counters are not exposed as public API — they are internal to the Rust runtime and may be added as test-only instrumentation in a future milestone if needed for lifecycle verification.

### `ServerSecureRoot`

Root directory for the server. Wraps `SecureRoot` from the core crate.

```python
from eggserve import ServerSecureRoot, StaticPolicyWrapper

policy = StaticPolicyWrapper(allow_dotfiles=True)
root = ServerSecureRoot("/var/www", policy=policy)
print(root.root_path)  # "/var/www"
```

Constructor: `ServerSecureRoot(path, policy=None)` — defaults to safe policy if omitted.

### `StaticPolicyWrapper`

Policy wrapper for use with `ServerSecureRoot`. Maps Python booleans to the Rust policy enums.

```python
from eggserve import StaticPolicyWrapper

policy = StaticPolicyWrapper(
    directory_listing=False,
    follow_symlinks=False,
    allow_dotfiles=False,
)
```

Constructor: `StaticPolicyWrapper(directory_listing=False, follow_symlinks=False, allow_dotfiles=False)`

### `ServerBodySource`

File-backed body source for the server API. Wraps `BodySource` from the core crate. Obtained from `ResolvedFile.body_for_plan(plan)` via the native primitives.

Properties:
- `kind` — `"empty"`, `"bytes"`, `"file_full"`, or `"file_range"`
- `length` — content length in bytes (`int` or `None`)
- `range` — `(start, end_inclusive)` tuple for range bodies, or `None`

Methods:
- `read_all()` — reads entire body into memory
- `read_range(start, end_inclusive)` — reads a byte sub-range
- `to_response(status=200)` — creates a `Response` from this body source

### `ServerRequestError`

Exception raised for invalid server requests.

```python
try:
    responder.respond("POST", "/file.txt")
except ServerRequestError as e:
    print(e)  # "Method not allowed; supported: GET, HEAD"
```

Variants:
- `MethodNotAllowed(allowed)` — non-GET/HEAD method received
- `TargetInvalid(reason)` — malformed request target
- `PathRejected(reason)` — path failed policy validation
- `BodyNotAllowed()` — request body on GET/HEAD

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
- Session, cookie, or auth framework
- Reverse proxying
- Generic plugin host
- Dynamic Python code execution in request paths

The native primitives provide response **planning**, not response **writing**. The caller maps plans to sockets. For those use cases, consider Uvicorn, Granian, or similar application servers.

## Adapter-building posture

Python primitives allow downstream projects to build app servers and adapters. The safe design is to let Rust own socket I/O and let Python return explicit response values.

The `StaticResponder` and `Server` primitives provide the building blocks: Python handles request logic, Rust handles connection management, file streaming, and timeouts. For production file serving, prefer `StaticResponder` which streams file bodies directly through Rust without passing through Python memory.

**Security note:** Reopening paths in Python is outside the security guarantee. A resolved resource's file handle was opened under policy enforcement during resolution; reconstructing a path and reopening it bypasses symlink and confinement checks.

## Installation

```sh
pip install eggserve
```

The wheel includes the native extension (PyO3) and the Rust binary. If the native extension cannot be loaded (e.g. platform mismatch), the subprocess API remains available.

## Examples

See [examples/python_basic.py](../examples/python_basic.py), [examples/python_dynamic_static.py](../examples/python_dynamic_static.py), and [examples/python_safe_download.py](../examples/python_safe_download.py).

## Testing

```sh
# Native primitives tests
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_primitives -v

# Server primitives tests
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_server_primitives -v

# Subprocess API tests
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_server -v
```
