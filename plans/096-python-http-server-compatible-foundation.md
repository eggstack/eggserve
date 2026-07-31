# Plan 096 — Python `http.server`-Compatible Foundation

## Goal

Implement the base Python server and request-handler facade expected by users of the standard-library `http.server` module while continuing to use EggServe's existing Rust-owned runtime, parser, response validation, and shutdown machinery.

This phase provides:

- `HTTPServer`
- `ThreadingHTTPServer`
- `BaseHTTPRequestHandler`

It does not yet implement `SimpleHTTPRequestHandler` or the TLS server classes. Those complete in Plans 097 and 098.

## Required outcome

A user can write an ordinary subclass-based handler:

```python
from eggserve.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"ok\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
    print(server.server_address)
    server.serve_forever()
```

The request is still parsed by Hyper. Rust still owns the accepted connection and response framing. Python sees a documented handler facade and produces a validated response description.

## Design principle

Do not port CPython's `http.server` implementation. Reproduce only the documented public programming model that maps cleanly to EggServe.

The compatibility facade must be an adapter over the current native runtime:

```text
Rust accept loop
  -> Hyper request parsing
  -> canonical request envelope
  -> Python handler adapter
  -> BaseHTTPRequestHandler instance
  -> staged validated response
  -> canonical Rust response normalization
  -> Hyper serialization
```

There must not be:

- a Python accept loop;
- a Python HTTP parser;
- a second socketserver-style runtime;
- a raw Python socket writer;
- manual response framing in Python;
- path-based file reopening;
- an ASGI/WSGI adapter.

## Scope firewall

This plan must not add:

- `SimpleHTTPRequestHandler` static filesystem behavior;
- TLS classes;
- CGI;
- routing or middleware;
- async Python handlers;
- WebSockets or upgrade handling;
- raw accepted-socket access;
- request trailers;
- chunked response authoring by handlers;
- sendfile APIs;
- compression;
- multipart parsing;
- form or JSON helpers;
- cookie/session/auth helpers;
- client functionality;
- a new public Rust server abstraction;
- a new routine CI job or workflow;
- a compatibility corpus framework.

## Required file inspection

Before editing, inspect at least:

- `crates/eggserve-python/python/eggserve/__init__.py`
- `crates/eggserve-python/python/eggserve/server.py`
- `crates/eggserve-python/src/lib.rs`
- `crates/eggserve-python/src/server.rs`
- `crates/eggserve-core/src/server/mod.rs`
- `crates/eggserve-core/src/server/service.rs`
- `crates/eggserve-core/src/server/connection.rs`
- `crates/eggserve-core/src/server/config.rs`
- `crates/eggserve-core/src/primitives/request.rs`
- `crates/eggserve-core/src/primitives/request_head.rs`
- `crates/eggserve-core/src/primitives/request_body.rs`
- `crates/eggserve-core/src/primitives/header_block.rs`
- `crates/eggserve-core/src/primitives/canonical.rs`
- current Python server tests and API stability tests
- Plan 095 implementation and final response behavior

Search for all names that will conflict with the new facade:

```sh
rg -n "class Server|Server\(|PyServer|ServerSecureRoot|StaticResponder" crates/eggserve-python
rg -n "from eggserve import Server|eggserve\.Server|Server\(" examples docs README.md crates/eggserve-python/tests
rg -n "HashMap<String, String>|headers:" crates/eggserve-python/src/server.rs
```

## Public module placement

The canonical supported module after this plan is:

```python
from eggserve.server import HTTPServer, ThreadingHTTPServer, BaseHTTPRequestHandler
```

The existing `eggserve.server` subprocess helpers may remain in the same module temporarily, but the module must be organized so that the server classes are the primary documented API.

Do not rename or remove subprocess helpers in this phase unless a direct name conflict makes it unavoidable. Public API reconciliation is completed in Plan 098.

The native PyO3 server type may remain exported internally by `_native`, but Python implementation code should import it under an explicitly internal name such as `_NativeServer`. Do not expose the native type as the recommended `Server` API.

## Track A — Native adapter contract

### Objective

Define the narrow data exchange between Rust and the Python handler facade.

### Request input

The existing native callback adapter already provides a request envelope. Correct it where necessary so the compatibility layer receives:

- method string;
- full origin-form path including query information through distinct path/query accessors or one raw request-target accessor;
- HTTP version;
- ordered duplicate-preserving headers;
- remote address;
- local address;
- scheme;
- bounded request body object when permitted.

The compatibility layer must not use a `dict` as the canonical header representation because dictionaries collapse repeated fields.

Preferred Python-facing representation:

- an immutable ordered sequence of `(name, value)` pairs at the FFI boundary;
- wrapped in a small `HTTPMessage`-like Python object for handler use.

Do not add the entire `email.message` policy system unless the standard library's `HTTPMessage` can be reused directly without losing duplicates or introducing parser ambiguity. A small purpose-built wrapper is acceptable and likely simpler.

### Response output

The Python handler adapter must return:

- validated status code;
- ordered response header fields preserving duplicates;
- empty or bounded in-memory body for this phase;
- an internal marker indicating whether the original method was HEAD, if not already available at normalization time.

All output must pass through canonical Rust validation and normalization.

Handlers may not control:

- `Transfer-Encoding`;
- connection persistence;
- `Connection`;
- `Keep-Alive`;
- `Upgrade`;
- framing derived from a supplied `Content-Length` that disagrees with the staged body.

The runtime should either reject these fields or strip them according to one documented policy. Prefer rejecting handler-supplied framing and connection-specific fields with a controlled 500 response and a sanitized log event, because silent acceptance creates confusing behavior.

### File-backed body scope

This phase does not require general handler-returned file bodies. `SimpleHTTPRequestHandler` will use an internal static-response fast path in Plan 097.

However, correct any existing conversion bug that causes a file-backed body already produced by the native layer to become an empty body if that bug blocks the adapter architecture. Do not expose a new public file-body factory in this phase.

## Track B — Server classes

### `HTTPServer`

Required constructor shape:

```python
HTTPServer(server_address, RequestHandlerClass, bind_and_activate=True)
```

Required behavior:

- `server_address` accepts a two-item `(host, port)` tuple;
- port 0 requests an ephemeral port;
- host may be IPv4 or IPv6 where supported by the native runtime;
- wildcard addresses supplied explicitly through this constructor are accepted without a second `public=True` flag;
- `bind_and_activate=False` may defer startup only if the existing runtime can support this cleanly;
- if deferred bind semantics require substantial new native state, support the argument for signature compatibility but clearly restrict manual `server_bind()`/`server_activate()` behavior rather than building socketserver internals;
- the handler class is stored as `RequestHandlerClass`;
- handler callback concurrency is one for `HTTPServer`, approximating stdlib's serial request-handler execution;
- the Rust runtime may still accept and hold multiple TCP connections subject to existing connection limits.

Required attributes:

- `server_address`
- `server_name`
- `server_port`
- `RequestHandlerClass`
- `allow_reuse_address` only as a documented compatibility attribute if it maps to actual behavior; do not claim mutable pre-bind semantics that do not exist.

Required lifecycle methods:

- `serve_forever(poll_interval=0.5)`
- `shutdown()`
- `server_close()`
- `handle_request()`
- `fileno()` only if the native runtime can expose a safe duplicated listener descriptor/handle without broad platform work; otherwise omit and document the incompatibility rather than returning a fake value.
- `__enter__()` / `__exit__()`

### Lifecycle mapping

Map methods to the native runtime directly:

- construction creates configuration but need not start if compatible with current design;
- `serve_forever()` starts once, blocks until shutdown, then returns;
- `shutdown()` is safe from another Python thread and returns after initiating or completing shutdown according to documented stdlib-like behavior;
- `server_close()` is idempotent and releases the runtime/listener;
- context exit closes the server;
- double start or reuse after close raises a clear `RuntimeError` or `OSError`-compatible exception.

Do not create a polling loop merely to honor `poll_interval`; accept the argument and document that the Rust runtime uses event-driven shutdown. The argument may be ignored if needed for source compatibility.

### `handle_request()`

Support one-request operation only if the current runtime can stop after one dispatched request with a small bounded adapter.

Preferred implementation:

- an internal one-request counter/event in the Python adapter;
- start the runtime;
- wait for one completed handler invocation or server timeout;
- initiate shutdown;
- return.

Do not add a separate one-shot accept implementation.

If exact one-request semantics would require invasive runtime changes, implement a narrower documented behavior and mark the specific divergence. Do not fake success.

### `ThreadingHTTPServer`

`ThreadingHTTPServer` should be a thin subclass or configuration variant of `HTTPServer`.

It uses the existing Rust concurrent runtime and sets callback concurrency above one, bounded by an explicit class or constructor setting.

Do not create one Python thread per connection. The compatibility meaning is concurrent handler execution, not CPython's internal thread model.

Expose one simple extension:

```python
ThreadingHTTPServer(..., max_workers=8)
```

or use an equivalent documented name only if needed. Keep the standard positional constructor valid. Internally map this to `max_python_callbacks`.

Avoid adding an executor abstraction. The existing `spawn_blocking` plus semaphore model is sufficient.

## Track C — `BaseHTTPRequestHandler`

### Construction and dispatch

The facade must instantiate one handler object per request with the familiar shape:

```python
RequestHandlerClass(request, client_address, server)
```

The `request` object is an internal adapter, not a raw socket. The handler constructor should perform one request dispatch, matching the practical behavior expected by standard subclasses.

Required dispatch:

1. Populate request attributes.
2. Resolve a method name `do_<METHOD>` for syntactically valid token methods.
3. If absent, send 501 Not Implemented.
4. Invoke the method synchronously.
5. Finalize the staged response.
6. Return it to the native adapter.

Do not allow coroutine handlers. If a `do_METHOD` returns an awaitable, produce a controlled 500 response.

### Required attributes

At minimum:

- `client_address`
- `server`
- `command`
- `path`
- `request_version`
- `headers`
- `rfile`
- `wfile`
- `close_connection` as a compatibility attribute controlled by the runtime; user mutation must not bypass runtime framing or lifecycle policy.
- `requestline` if it can be reconstructed accurately from canonical request metadata; otherwise document a normalized form.

Class attributes:

- `server_version`
- `sys_version`
- `protocol_version`
- `error_message_format`
- `error_content_type`
- `responses`

Keep these minimal and source-compatible where practical. Do not copy the entire CPython response table if a small mapping through `http.HTTPStatus` is sufficient.

### Header representation

`self.headers` must provide common mapping-like and duplicate-aware operations:

- `get(name, default=None)`
- `get_all(name)`
- `items()` preserving field order and duplicates
- membership tests
- iteration

Header names are case-insensitive. Original values remain validated strings.

Do not expose a mutable object that can rewrite the canonical incoming request.

### `rfile`

Provide a bounded file-like reader over the native one-shot request body.

Required common methods:

- `read(size=-1)`
- `readinto(buffer)` if simple to support
- `readline(limit=-1)`
- iteration by lines
- `readable()`

The reader may buffer the body once under the configured request-body ceiling. For this compatibility phase, prefer buffer mode over exposing a complex synchronous streaming bridge unless current code already provides a reliable stream bridge.

Default request-body policy for `HTTPServer` handlers:

- allow bounded bodies for custom methods;
- configure one explicit ceiling;
- reject declared or actual bodies above the ceiling;
- preserve existing timeout behavior.

Use the existing request-body limit machinery. Do not introduce multipart or form decoding.

A reasonable facade default may be selected from an existing project limit. If none exists, add one Python server constructor keyword such as `max_request_body_bytes` with a conservative documented default. Do not add multiple overlapping body-limit options.

### `wfile`

Provide a bounded binary writer that stages the handler response body.

Required common methods:

- `write(bytes_like)` returning the number of bytes accepted;
- `writelines(iterable)` if trivial;
- `flush()` as a no-op or validation boundary;
- `writable()`.

Do not expose the network socket. The writer stores bytes in memory until the handler returns.

Add one bounded response ceiling, preferably a single `max_handler_response_bytes` server option with a conservative default. Use an internal bytearray or `BytesIO`-like implementation that checks the bound before growth.

Do not add temporary-file spooling in this phase. Static files use the Rust streaming path in Plan 097.

### Response methods

Implement:

- `send_response(code, message=None)`
- `send_response_only(code, message=None)`
- `send_header(keyword, value)`
- `end_headers()`
- `flush_headers()` if required by ordinary subclasses and simple to map
- `send_error(code, message=None, explain=None)`

Required rules:

- status range is 100–599;
- reason phrases are cosmetic and not authoritative;
- header names and values are validated immediately;
- duplicate response fields are preserved;
- CR, LF, and NUL injection is rejected;
- runtime-owned framing and connection fields are rejected;
- `end_headers()` seals the response head;
- writing a body before headers are ended raises a clear exception or is handled consistently;
- a handler that returns without sending a response produces a controlled 500;
- a handler exception produces a generic 500 without traceback leakage to the client;
- 1xx, 204, 205, and 304 bodies are discarded by canonical normalization;
- HEAD bodies are suppressed while metadata describes the equivalent GET where the handler supplied or the runtime can compute it.

For handler-generated HEAD responses, if the handler writes a body, retain the staged body length for `Content-Length` computation and suppress transmission. This mirrors the equivalent GET representation model without exposing body bytes.

### Logging hooks

Implement override points:

- `log_request(code='-', size='-')`
- `log_error(format, *args)`
- `log_message(format, *args)`

Default logging should bridge to EggServe's sanitized structured logging or a concise stderr message. It must not allow control-character injection from request data.

Do not add a new Python logging framework or dependency.

### Helper methods

Implement narrowly:

- `version_string()`
- `date_time_string(timestamp=None)`
- `address_string()` returning the numeric peer address by default; do not add reverse DNS lookups.

## Track D — Exception and timeout behavior

### Handler exceptions

- Catch exceptions at the adapter boundary.
- Emit a sanitized operational event.
- Return generic 500.
- Do not include exception messages or traceback text in the wire response.
- Preserve the exception in test-only observation only through existing test hooks, not a new public observer system.

### Handler timeouts

Use the existing Rust handler timeout.

A timed-out Python callback cannot be safely cancelled. Preserve the existing bounded callback semaphore behavior and document that the Python callable may continue running in the blocking worker after the client receives a timeout response or the connection closes.

Do not add unsafe thread cancellation.

### Shutdown

Tests must verify:

- shutdown from another thread unblocks `serve_forever()`;
- close is idempotent;
- an in-flight bounded handler follows existing graceful-shutdown policy;
- forced shutdown remains available internally but does not need to be a standard public method on `HTTPServer`.

## Track E — Tests

Create a focused installed-wheel test module, for example:

```text
crates/eggserve-python/tests/test_http_server_compat.py
```

Use standard `unittest` and real loopback sockets. Do not add pytest or another runner.

Required compatibility tests:

### Construction and lifecycle

- imports from `eggserve.server`;
- ephemeral port construction;
- `server_address`, `server_name`, `server_port`;
- context management;
- `serve_forever()` plus cross-thread `shutdown()`;
- idempotent `server_close()`;
- invalid address and invalid handler class failures;
- explicit wildcard address accepted through this constructor.

### Dispatch

- GET calls `do_GET`;
- POST calls `do_POST` with a bounded body;
- missing method handler returns 501;
- handler exception returns generic 500;
- awaitable return is rejected;
- `HTTPServer` serializes callback execution;
- `ThreadingHTTPServer` allows bounded concurrent callback execution.

Use events and barriers, not sleeps, for concurrency assertions.

### Request facade

- path includes query in the stdlib-compatible `self.path` form;
- method and version exposed;
- peer and local server metadata exposed;
- duplicate request headers available through `get_all()`;
- bounded body reads;
- second full-body consumption behavior is clear and tested;
- oversized body rejected before or during handler dispatch according to documented policy.

### Response facade

- `send_response` + headers + `wfile.write` produces correct body;
- duplicate response headers survive to the wire;
- invalid status rejected;
- CRLF header injection rejected;
- handler-supplied Transfer-Encoding/Connection rejected;
- mismatched supplied Content-Length cannot create incorrect wire framing;
- HEAD suppresses bytes;
- 204, 205, and 304 suppress bytes;
- Date is present from Plan 095;
- handler return without a response becomes 500;
- response buffer limit enforced.

### Compatibility comparison

Add a small behavior table in the test module or docs mapping each tested public behavior to `http.server`. Do not create a generated compatibility registry.

Do not duplicate planner, path confinement, or file streaming tests here.

## Documentation

Add or update:

- `docs/python-http-server-compatibility.md` as the focused public compatibility contract;
- `docs/python-api.md` to identify the new primary server API;
- README minimal example;
- `docs/compatibility.md` to distinguish CLI compatibility from library compatibility;
- `docs/non-goals.md` to reinforce private socketserver incompatibilities.

Document all material differences:

- Rust event-driven runtime;
- no raw socket access;
- bounded `rfile`/`wfile`;
- duplicate-preserving validated headers;
- runtime-owned framing;
- no coroutine handlers;
- no unsafe default directory listing, symlink, or dotfile behavior;
- no guaranteed thread-per-request implementation;
- `poll_interval` accepted but not used as a polling mechanism.

Do not document `SimpleHTTPRequestHandler` or TLS classes as complete until their plans land.

## Suggested commit sequence

1. `refactor: expose duplicate-preserving native handler boundary`
2. `feat: add HTTPServer lifecycle facade`
3. `feat: add BaseHTTPRequestHandler dispatch and response staging`
4. `feat: add bounded request and response file facades`
5. `test/docs: establish base http.server compatibility contract`

Keep the native boundary and Python facade commits separable where practical. Do not mix static filesystem handler work into this plan.

## Verification

Run targeted Rust and Python tests while implementing:

```sh
cargo test -p eggserve-core --test server_integration
cargo test -p eggserve-core --test canonical_wire_interop
bash scripts/test-python-wheel.sh
```

Run the focused Python module repeatedly for lifecycle/concurrency reliability using the installed-wheel harness rather than `PYTHONPATH` source imports.

Then run:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Routine CI remains unchanged.

## Acceptance criteria

Plan 096 is complete only when all of the following are true on the same final commit:

- `HTTPServer`, `ThreadingHTTPServer`, and `BaseHTTPRequestHandler` import from `eggserve.server` in an installed wheel.
- A standard subclass with `do_GET()` can send status, duplicate headers, and a body.
- A custom `do_POST()` can read a bounded request body from `rfile`.
- Hyper remains the only HTTP parser.
- The Rust runtime remains the only accept loop.
- Python never writes raw response bytes to a socket.
- Handler responses pass through canonical normalization.
- Invalid status, header injection, framing fields, and oversized bodies fail closed.
- Request and response headers preserve duplicates.
- `HTTPServer` has serial handler execution.
- `ThreadingHTTPServer` has bounded concurrent handler execution without thread-per-connection architecture.
- `serve_forever()`, `shutdown()`, `server_close()`, and context management work reliably.
- Handler exceptions and timeouts do not leak tracebacks.
- No new framework, parser, runtime, or CI architecture was added.
- Documentation clearly states the compatibility boundary.
- `verify.sh fast` passes.
- `verify.sh full` passes.
- Both current routine CI jobs pass on the final commit.

## Completion handoff

The implementation handoff must report:

- final public constructor signatures;
- the exact native request and response boundary types;
- request and response buffer ceilings;
- documented compatibility differences;
- focused test commands and results;
- any stdlib public behavior deliberately deferred to Plans 097 or 098;
- final commit SHA with both existing CI jobs green.

Do not mark `SimpleHTTPRequestHandler` or TLS compatibility complete under this plan.