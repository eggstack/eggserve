# Plan 166 — Python Low-Level Runtime and Service Substrate

## Status

**IMPLEMENTED / CLOSED.**

Prerequisites: Plan 161; Plan 162 streaming response contract; Plan 163 canonical connection/runtime ownership; Plan 164 admission/lifecycle controls. Plan 165 should be implemented or API-compatible before exposing advanced response policy.

## Goal

Make `eggserve.lowlevel` sufficient for a separate Python package to build a bounded HTTP application server on top of EggServe without using the `http.server` compatibility facade and without importing private `_native` classes.

This does not turn EggServe itself into an ASGI/WSGI server, process manager, or framework. It exposes the reviewed runtime/service substrate already present in Rust.

## Current baseline

`eggserve.lowlevel` already exports a substantial canonical-primitives surface including `Request`, `RequestBody`, `Response`, `ConnectionInfo`, headers, methods, path/static primitives, and response construction helpers.

The native extension also already contains a server/handler path used behind the Python facade, including:

- Rust-owned networking;
- a synchronous Python `Callable[[Request], Response]` bridge;
- bounded `max_python_callbacks`;
- request body reject/buffer/stream modes;
- lifecycle start/shutdown/wait;
- TLS/static configuration.

However, the native `Server` shape is still primarily static-root/facade-oriented and is not exported as a coherent public low-level application-runtime API. Python responses are currently buffered/file-backed rather than generic streaming responses.

This plan therefore exposes/refactors existing machinery rather than inventing a second Python server.

## Public module boundary

Keep two intentionally different APIs:

```text
eggserve.server
  stdlib-shaped compatibility facade
  HTTPServer / ThreadingHTTPServer / HTTPS variants
  BaseHTTPRequestHandler / SimpleHTTPRequestHandler

eggserve.lowlevel
  canonical Request / RequestBody / Response
  runtime/service configuration and lifecycle
  bounded callback/service adapter
  streaming response primitive
```

Do not make stdlib compatibility classes depend on new low-level public syntax where that would change semantics. Both surfaces may share the same Rust implementation internally.

## Low-level server/service API

Expose a reviewed Python API, exact names provisional, equivalent to:

```python
config = RuntimeConfig(...)
server = Server(config=config, handler=callable)
server.start()
server.wait_ready()
...
server.shutdown()
server.wait()
```

Requirements:

- custom handler/service mode does **not** require a static root;
- optional static service composition remains available as a distinct responder/service, not an implicit root requirement;
- runtime owns sockets, parsing, framing, timeouts, admission, and shutdown;
- Python handler receives only canonical EggServe values;
- no raw socket access;
- no Hyper/Tokio objects exposed to Python;
- lifecycle methods have deterministic error types/state.

Prefer wrapping the same native/Rust server used by the compatibility facade rather than creating another accept loop.

## Runtime configuration exposure

Expose the production-relevant Plan 164 controls with validated types/names:

- bind address/port or equivalent normal Python server construction;
- open connection limit;
- in-flight service limit;
- Python callback limit;
- request-body global ceiling and body mode;
- header/parser limits that operators reasonably need;
- header/body/handler/keep-alive/write-stall/hard-lifetime timeouts as applicable;
- max requests per connection;
- graceful shutdown;
- TLS configuration already supported;
- response privacy policy from Plan 165 where safe.

Do not mirror every Rust internal tuning field if it has no useful Python/operator meaning. The low-level Python API is public, not a dump of `RuntimeConfig` internals.

Use explicit `None`/named modes for disabled controls; do not overload zero as unlimited unless already contractual and documented.

## Python callback execution and GIL rules

Preserve the existing core model: network I/O stays in Rust/Tokio and Python callbacks are bounded separately.

Requirements:

- Rust networking, body reads, file I/O, and socket writes do not hold the GIL unnecessarily;
- at most `max_python_callbacks` Python handlers execute concurrently in one native server instance;
- generic in-flight service admission from Plan 164 is acquired before or consistently with the Python callback permit so limits cannot deadlock or invert ownership;
- no unbounded Python work queue;
- callback timeout behavior remains honest: EggServe can stop waiting/close the HTTP request, but must not claim it can safely kill arbitrary executing Python code;
- callback exceptions/panics become generic client errors and sanitized local diagnostics only;
- all PyObject references release after cancellation/shutdown even if the client disconnects.

Document the distinction between “HTTP request timed out” and “Python thread forcibly terminated” (the latter is not provided).

## Request bodies

Retain current one-shot semantics:

- `.read()` and `.iter_chunks()` are mutually exclusive;
- byte ceilings are enforced by Rust;
- streaming uses bounded cross-thread/channel backpressure;
- incomplete consumption closes the HTTP/1 connection when framing would otherwise be ambiguous;
- disconnect/timeout/cancellation become typed `RequestBodyError` subclasses.

Do not add an unbounded `bytes` compatibility escape hatch for application servers.

## Streaming Python responses

Map Plan 162's canonical `ResponseStream` into Python without buffering the entire producer output.

First supported producer should be a **synchronous iterator of bytes-like chunks**, because it fits the existing bounded callback/thread model and can support WSGI/Gunicorn-style server construction without introducing a Python asyncio runtime into EggServe.

Provide an API equivalent to:

```python
Response.stream(status, iterable, headers=None, content_length=None)
```

Requirements:

- iterable is consumed incrementally;
- bridge/channel between Python producer and Rust body stream is bounded;
- client backpressure eventually stops iterator advancement;
- optional `content_length` maps to Plan 162 known-length validation;
- no content length means runtime-owned HTTP/1 streaming framing;
- non-bytes items fail deterministically;
- iterator exceptions before commitment may become a generic 500; after commitment close the connection and log sanitized type/category only;
- iterator cancellation/drop releases Python references promptly;
- HEAD/body-forbidden responses must not advance the iterator;
- no service can set raw `Transfer-Encoding`.

### Async Python producers

Do **not** implement async-generator/awaitable handlers opportunistically in this plan.

Perform a documented design gate after the synchronous low-level API works:

- if an external app-server consumer requires direct asyncio/ASGI-style integration, write a separate follow-up plan for an event-loop bridge;
- otherwise keep asyncio ownership in the downstream app server.

This prevents EggServe from quietly becoming an ASGI runtime while still supporting a Python-built application server through the synchronous service contract.

## Static responder composition

Retain/export `StaticResponder`/secure-root primitives so a downstream Python server can choose to delegate selected requests to hardened static serving.

EggServe must not add routing rules. Composition belongs to the caller:

```python
def handler(request):
    if application_decides_static(request):
        return static.respond(...)
    return app(request)
```

The documentation example should be minimal and explicitly non-framework.

## Response privacy policy

Expose a safe subset of Plan 165 through `eggserve.lowlevel`, ideally as immutable validated config:

- Server suppression/fixed value;
- Date mode: standards clock / explicit suppression; caller-supplied custom Rust clock may remain Rust-only unless a Python provider can be called without adding a per-response GIL hotspot;
- response header denylist;
- canonical error mode;
- static metadata policy.

Do not call arbitrary Python clock functions on every response by default. For Python embedding, a preconfigured standards clock or fixed native policy is preferable.

## Typing and packaging

Update:

- `lowlevel.py` exports;
- `_native.pyi` and any public `.pyi` files;
- wheel packaging tests on all supported CPython/platform targets;
- API snapshot/import tests;
- README and Python compatibility docs.

Keep `_native` private implementation detail even if its classes back `lowlevel`.

## Tests

Add Python integration tests for:

- handler-only server with no static root;
- buffered GET/POST-style service requests according to body policy;
- streaming request consumption;
- synchronous streaming response known/unknown length;
- HEAD without advancing response iterator;
- iterator exception before/after commitment;
- slow client backpressure and bounded Python producer advancement;
- Python callback saturation separate from open connections;
- generic service admission saturation;
- handler timeout + continued process health;
- shutdown with active callback/request/response stream;
- TLS low-level service;
- response privacy options;
- stdlib facade behavior unchanged in the same wheel.

Run repeated start/stop tests to catch runtime/thread/PyObject leaks.

## Non-goals

Do not add:

- ASGI/WSGI protocol implementations;
- Gunicorn master/worker process management;
- Python routing/middleware;
- raw sockets or `socketserver` compatibility;
- arbitrary Python async transport objects;
- unbounded response generators;
- framework-specific adapters.

A downstream package should be able to implement those policies on the low-level service API.

## Acceptance criteria

- [ ] `eggserve.lowlevel` exposes a handler-only server/runtime that requires no static root.
- [ ] The API reuses the canonical Rust runtime rather than adding another accept loop.
- [ ] Network I/O remains Rust-owned and Python callbacks are bounded independently.
- [ ] Python can produce known/unknown-length streaming responses with real backpressure and no whole-body buffering.
- [ ] Runtime/resource/privacy configuration is available without exposing Hyper/Tokio internals.
- [ ] The stdlib-shaped `eggserve.server` API and native static fast path remain behaviorally unchanged unless explicitly documented.
- [ ] Type stubs, wheels, packaging tests, and public documentation match the implementation.

## Handoff

Plan 167 may implement legacy CGI/FastCGI as service adapters without modifying this public runtime. Plan 168 benchmarks the low-level callback and streaming paths separately from the stdlib facade.

## Closure record

Implemented in commit `fcb39f12abcf37298442013d5c96e58ae4f37120`.

No material deviation from the handler-only runtime/service substrate. The
Rust response-stream bound polish in Plan 169 remains compatible with the
Python bounded channel adapter.
