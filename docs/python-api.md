# Python API

Custom Python handlers start with runtime-only configuration and do not pin or
otherwise depend on a static responder root. Static handlers construct one
confined static service. Both paths use the running server's single file-stream
admission pool for canonical file bodies.

Stock `SimpleHTTPRequestHandler` with default settings bypasses Python
per-request dispatch entirely. The Rust static service handles requests directly
without GIL acquisition or Python callback overhead.

The supported Python API is the six-class `eggserve.server` façade:

```python
from eggserve.server import (
    HTTPServer, ThreadingHTTPServer, HTTPSServer, ThreadingHTTPSServer,
    BaseHTTPRequestHandler, SimpleHTTPRequestHandler,
)
```

It is a bounded, synchronous subset shaped like `http.server`. Rust owns the
listener, HTTP/1.1 parsing, response framing, timeouts, path confinement, and
static-file streaming. Handlers receive no raw socket. `rfile` and `wfile`
are bounded in-memory adapters, coroutine handlers are rejected, and framing
headers such as `Connection` and `Transfer-Encoding` are runtime-owned.

Compatibility server addresses use `(host, port)` tuples. `""` is normalized
to `"0.0.0.0"` and treated as explicit wildcard intent; literal wildcard
addresses are accepted by this façade, while the CLI still requires
`--public`. `server_address` reports the native bound tuple, including the
actual port selected for port `0`, with unbracketed IPv6 hosts.

## Static files

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

The root is validated and captured at construction. Safe defaults deny
directory listings, dotfiles, and symlinks; `directory_listing`,
`allow_dotfiles`, and `follow_symlinks` are explicit opt-ins. For stock
`SimpleHTTPRequestHandler` with default settings, the entire request path
is native — no Python callback is invoked. Subclasses and non-default
settings fall back to the Python callback path. GET and HEAD retain
Rust-native conditional, range, and streaming behavior.

`extensions_map` values and `guess_type()` results are response metadata, not
filesystem operations. They must be strings without prohibited header
characters; invalid values fail closed with a generic 500. `extensions_map`
also applies to native-selected index files. A subclass `guess_type()` hook is
defined for direct file targets with a suffix, not for index names resolved
only inside Rust.

Unknown suffixes use `SimpleHTTPRequestHandler.default_content_type`, which
defaults to `application/octet-stream`. `extra_response_headers` accepts an
ordered sequence of safe `(name, value)` pairs on the handler class or stock
`functools.partial`; those headers are added only to final `200` static
responses. At most 32 pairs and 8 KiB of combined header names and values are
accepted by default. Runtime-owned and hop-by-hop headers are rejected during
server construction.

## HTTPS

```python
from eggserve.server import HTTPSServer, SimpleHTTPRequestHandler

with HTTPSServer(("127.0.0.1", 8443), SimpleHTTPRequestHandler,
                 certfile="cert.pem", keyfile="key.pem") as server:
    server.serve_forever()
```

`certfile` is required; `keyfile=None` means the key is read from the same PEM
file. `password` is unsupported for encrypted keys. `alpn_protocols` defaults
to `['http/1.1']` and any other protocol is rejected. TLS is rustls-based: no
CPython `SSLContext`, raw wrapped socket, SNI multi-certificate selection,
client certificates, ACME, or certificate reload is provided in Python.
Advanced SNI/mTLS/reload lives in the Rust `TlsServerConfig` substrate
(Plan 203) for downstream embedding first.

## Convenience and advanced namespaces

Module ownership (Plan 182): `eggserve.server` is the six-class
stdlib-shaped compatibility facade; `eggserve.lowlevel` is the bounded
handler/runtime embedding surface; `eggserve.subprocess` is the canonical
owner of the optional subprocess/CLI convenience API (`ServeConfig`,
`ServerProcess`, `StaticPolicy`, `serve_directory`). `eggserve.server`
re-exports the subprocess names for compatibility without expanding its
`__all__`, and top-level `eggserve.serve_directory` re-exports the
subprocess implementation.

`serve_directory()` is a blocking convenience at `eggserve.serve_directory`.
`ServeConfig`, `ServerProcess`, and `StaticPolicy` live in
`eggserve.subprocess`. Security-sensitive embedding primitives such as
`SecureRoot`, `StaticPolicy`, `RequestTarget`, and canonical HTTP types live
in `eggserve.lowlevel`.

## Low-level runtime/service substrate (Plan 166)

`eggserve.lowlevel` is the public substrate for a downstream bounded HTTP
application server. It reuses the same native runtime as the facade (no
second accept loop) without the `http.server` compatibility shapes:

```python
from eggserve import lowlevel

config = lowlevel.RuntimeConfig(bind="127.0.0.1", port=0)
server = lowlevel.Server(config=config, handler=my_handler)
server.start()
server.wait_ready()
...
server.shutdown()
server.wait()
```

- Handler-only: no static root is required or validated. Optional static
  composition uses a distinct `StaticResponder` owned by the caller; EggServe
  adds no routing.
- `RuntimeConfig` is frozen and validated: bind/port, connection/in-flight/
  callback limits, body ceiling/mode, parser ceilings, all timeouts,
  `max_requests_per_connection` (`None` disables, `0` rejected), TLS files,
  the safe privacy subset (`server_header`, `date_policy`
  `system`/`suppress`, `stripped_response_headers`, `error_policy`
  `minimal`/`empty`), and the Plan 202 trusted-proxy subset
  (`trusted_proxies` IP/CIDR list with no implicit loopback trust,
  `trust_unix_local`, `proxy_protocol` preamble mode, `forwarded_standard` /
  `forwarded_legacy` header switches; all default to nothing trusted).
  Custom Rust clocks stay Rust-only. Projection to the
  native constructor flows through the single `_native_kwargs()` helper;
  only Python-domain enum/`None` checks live in Python and Rust remains
  the final limit authority. `Request` exposes `remote_addr` unchanged for
  compatibility plus provenance-tagged `effective_addr`/`effective_address` /
  `effective_scheme` / `effective_authority` / `proxy_provenance` /
  `forwarded_provenance` (all `None`/absent without explicit trust).
- `Response.stream(status, iterable, headers, content_length)` consumes a
  synchronous bytes iterable incrementally through a bounded 16-chunk bridge:
  backpressure stalls the iterator, `content_length` selects known-length
  validation (mismatch truncates), omission selects chunked framing, HEAD and
  1xx/204/205/304 never advance the iterator, non-bytes items and iterator
  exceptions truncate with sanitized type-only diagnostics, and
  `Transfer-Encoding` cannot be set by services. Async producers are rejected.
- GIL/networking split: Rust owns I/O; at most `max_python_callbacks`
  handlers run concurrently; in-flight admission is held before the callback
  permit. Timeouts close the HTTP request but cannot kill Python code.

The six-class `eggserve.server` façade is the primary stdlib-shaped surface;
`eggserve.lowlevel` is the public runtime/service substrate for downstream
bounded application servers. The runnable demonstration is
`examples/python_lowlevel_service.py` (buffered plus bounded streamed
responses, `create_server()` with ephemeral-port support). The experimental
Python HTTP client is not part of the supported package surface. Subprocess
lifecycle helpers are under `eggserve.subprocess` (a process-management
convenience, not the preferred in-process embedding API); `_native` remains private
implementation detail.

## Async low-level substrate (Plan 204, experimental, H1-only)

`eggserve.lowlevel.AsyncServer(config, handler, max_async_tasks=...)` is the
async bridge for downstream ASGI-class servers (EggServe itself remains a
static server/library, not a maintained ASGI server). Handlers are
`async def handler(request: AsyncRequest) -> Response`:

- Same native runtime as the sync facade (no second accept loop, H1-only;
  H2/H3 remain Rust-only experimental). `eggserve.server` stays synchronous.
- Manual asyncio bridge (no new PyO3 async dependency, abi3 compatible):
  Rust releases the GIL during all network/body waits; Python never blocks
  the loop on Rust (blocking native calls go via `asyncio.to_thread`).
- Byte-fidelity metadata: `header_items_bytes`, `raw_target_bytes`,
  `path_bytes`/`query_bytes`, ordered duplicate-preserving headers,
  canonical `authority`/`scheme`, provenance-tagged effective/proxy fields,
  and verified TLS fields (`tls_server_name`, `tls_alpn`,
  `client_authenticated`).
- Bounded incremental bodies: `await body.aread()` (buffered, ceiling
  enforced) vs `async for chunk in body.aiter_chunks()` (streaming via
  `read_chunk`, no hidden `read_all`); `await body.trailers()` after
  terminal state (`None` when absent).
- Incremental responses: `AsyncResponse.stream(status, async_iterable,
  headers, content_length, trailers)` bridges async iterables through a
  bounded 16-queue (backpressure, HEAD/body-forbidden never advance,
  unknown length allowed, trailers validated before commitment via
  `stream_with_trailers`, no second response after commitment).
- Interim 1xx: `await request.send_interim(status, headers)` (native
  1xx-only/no-101/ordering/bounds enforcement; Python cannot bypass).
- Generic tunnels: `request.take_tunnel()` (one-shot) then
  `handshake, tunnel = cap.accept(headers)` (validated H1 Upgrade/CONNECT
  and Extended CONNECT where the runtime attaches a capability; runtime owns
  101/200 framing, no raw socket) plus bounded `await tunnel.recv()` /
  `send()` / `close()` (16-chunk backpressure, lifecycle-aware). Denial
  stays ordinary HTTP; WebSocket framing stays downstream (fixture-only).
- Admission: `max_async_tasks` (default `config.max_python_callbacks`)
  bounds app-task lifetime separately from pre-response
  `max_in_flight_requests`; overload fails fast with `503`; streaming
  producers and tunnel drivers hold the permit until completion;
  disconnect/cancel/shutdown releases exactly once and wakes blocked sends.
- Ownership: constructed outside a loop, `await start()` captures the
  running loop (wrong-loop/closed-loop misuse raises `RuntimeError`),
  `await shutdown()` cancels tracked tasks deterministically (graceful
  timeout from config) and joins the native runtime. No destructor network
  cleanup (explicit shutdown required). Long-lived tunnel/SSE drivers should
  be registered via `server.track(task)`.
- ASGI sufficiency is demonstrated by the test/example fixture
  (`crates/eggserve-python/tests/asgi_fixture.py`: HTTP + WebSocket echo,
  no lifespan/workers/reload/router), not a maintained server.

The runnable async demonstration is `examples/python_async_server.py`
(`create_server(port)` with port `0` support).

## Compatibility boundary

The façade is not ASGI, WSGI, CGI, FastCGI, a routing framework, middleware, a proxy, or a
general-purpose HTTP server. It intentionally does not expose socketserver
internals, `fileno()`, authoritative `translate_path()`, or one-request
`handle_request()` mode. Platform support and the Windows trusted-content
boundary are maintained in [`toolchain-support.md`](toolchain-support.md),
[`security-review.md`](security-review.md), and the [security policy](security-policy.md).

Python handler responses are validated in one Rust-owned conversion boundary.
Malformed structural bodies, failed body reads, consumed one-shot bodies,
invalid headers, and length mismatches produce a generic 500; malformed state
is never treated as an empty successful response. Operational diagnostics use
bounded categories and do not include handler exception text or response data.

The complete source-compatibility matrix and intentional incompatibility list
are maintained in
[`python-http-server-compatibility.md`](python-http-server-compatibility.md).
