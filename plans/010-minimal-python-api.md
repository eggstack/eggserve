# Plan 010: minimal Python API

## Goal

Expose a small, stable Python API for eggserve after the CLI and Rust core have proven safe defaults. The API should make eggserve usable as a hardened static-serving primitive from Python code while avoiding application-server semantics. This phase must not become ASGI, WSGI, callback routing, middleware, request parsing, or dynamic Python handler support.

The intended Python API is a programmatic equivalent of the safe CLI:

```python
from eggserve import serve_directory, ServeConfig, StaticPolicy

serve_directory("public", bind="127.0.0.1", port=8000)
```

## Scope

In scope:

```text
Python config dataclasses or typed classes
serve_directory convenience function
safe defaults matching Rust/CLI defaults
subprocess-based implementation initially, unless PyO3 embedding is explicitly chosen
clear lifecycle semantics
unit tests for Python argument/config mapping
wheel smoke tests for Python API
Python docs and examples
```

Out of scope:

```text
ASGI
WSGI
Python request callbacks
routing
middleware
template engines
upload handlers
response construction APIs
async Python server lifecycle unless added deliberately later
embedding arbitrary Python code in request path
```

## Design choice: subprocess first

The safest initial Python API is a thin wrapper around the packaged `eggserve` binary. This avoids embedding the Tokio runtime into Python and avoids premature PyO3 API stability commitments.

Recommended implementation:

```python
@dataclass(frozen=True)
class StaticPolicy:
    directory_listing: bool = False
    follow_symlinks: bool = False
    allow_dotfiles: bool = False

@dataclass(frozen=True)
class ServeConfig:
    directory: str | Path = "."
    bind: str = "127.0.0.1"
    port: int = 8000
    public: bool = False
    policy: StaticPolicy = StaticPolicy()
    log_format: Literal["text", "json", "none"] = "text"


def serve_directory(directory: str | Path = ".", *, bind="127.0.0.1", port=8000, ...):
    ...
```

The wrapper should translate config to CLI args and execute the binary. It may block until the server exits, matching the CLI mental model.

Do not add a Python callback API.

## Lifecycle API

Two levels are acceptable:

```python
def serve_directory(...):
    """Blocking call; runs until interrupted."""
```

and optionally:

```python
class ServerProcess:
    def start(self) -> None: ...
    def stop(self, timeout: float | None = None) -> None: ...
    def wait(self) -> int: ...
```

If `ServerProcess` is added, keep it a subprocess lifecycle wrapper, not a Python server object. It should be useful for tests and simple embedding.

## API defaults

The Python defaults must match CLI/Rust defaults:

```text
directory: current directory
bind: 127.0.0.1
port: 8000
public: false
directory_listing: false
follow_symlinks: false
allow_dotfiles: false
log_format: text
```

If `bind="0.0.0.0"` is passed without `public=True`, raise `ValueError` before spawning the binary. Keep the public exposure guard at both Python and Rust layers.

## Error handling

Python wrapper errors should be clear:

```text
ValueError for invalid config before process start
FileNotFoundError or RuntimeError if packaged binary is missing
subprocess return code propagated by blocking API
KeyboardInterrupt maps to normal interrupt behavior
```

Do not parse or reinterpret all Rust errors. The Rust binary remains the source of truth.

## Tests

Add Python tests without heavy dependencies where possible. If `pytest` is introduced, justify it. A stdlib `unittest` suite is enough.

Required tests:

```text
ServeConfig defaults match documented defaults
StaticPolicy defaults are safe
serve_directory translates directory/bind/port to expected CLI argv via test seam
public bind without public=True raises ValueError
unsafe policy flags map to CLI flags
log_format validation works
ServerProcess start/stop smoke test if implemented
```

Wheel smoke test:

```text
pip install built wheel
python -c "import eggserve; print(eggserve.__version__)"
python - <<'PY'
from eggserve import ServeConfig, StaticPolicy
print(ServeConfig())
PY
```

Optional integration smoke:

```text
create temp dir with hello.txt
start ServerProcess on a chosen local port
fetch /hello.txt using urllib.request
stop process
assert body matches
```

## Packaging changes

Update `crates/eggserve-python/python/eggserve/`:

```text
__init__.py       exports version, ServeConfig, StaticPolicy, serve_directory
server.py         API implementation
_bin.py           binary discovery/execution helpers
__main__.py       keeps python -m eggserve CLI behavior
```

Keep the binary-in-wheel strategy. Do not introduce PyO3 unless there is a specific reason.

## Documentation

Add:

```text
docs/python-api.md
README Python API section
examples/python_basic.py
```

Docs should emphasize:

```text
this is a static-serving API
this is not an ASGI/WSGI server
serve_directory is blocking by default
public exposure requires explicit public=True
unsafe filesystem options require explicit flags/policy fields
```

## Acceptance criteria

```text
Python API imports from installed wheel.
serve_directory exposes safe static-serving defaults.
Python public-bind guard works.
Policy flags map to Rust CLI flags.
python -m eggserve still works unchanged.
No request callback/app-server API is introduced.
Python docs include at least one minimal example.
CI runs a Python API smoke test.
```

## Suggested commit sequence

```text
feat(python): add StaticPolicy and ServeConfig wrappers
feat(python): add serve_directory blocking API
feat(python): add optional ServerProcess helper
test(python): add config mapping and wheel API smoke tests
docs: document minimal Python API and non-goals
ci: extend python-wheel job with API import smoke
```

## Risks

The main risk is API scope creep. Python users may ask for request callbacks, routing, or ASGI compatibility. Reject those in this phase. eggserve should provide hardened static-serving primitives, not a Python app framework.
