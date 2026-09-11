# eggserve-python — Deep Dive

The Python wheel is built by maturin and contains a PyO3 extension plus the
Python façade. The supported programming surface is `eggserve.server`; advanced
primitives and subprocess lifecycle helpers are kept in separate namespaces.
The wheel includes an `eggserve` console script backed by the native extension
(no separate bundled binary).

Custom-handler startup is runtime-only: it constructs a Python callback service
and `RuntimeConfig` without a `ServeConfig`, responder root, or pinned
filesystem state. Its compatibility `root` argument is inactive and is not
opened or validated. `SimpleHTTPRequestHandler` is the separate static branch and
constructs one confined `StaticService`. Both branches use the server-wide
runtime file-stream semaphore for canonical file responses.

Stock `SimpleHTTPRequestHandler` (or a `functools.partial` wrapping it with
only `directory=` and/or `extra_response_headers=` keywords) with all default
settings bypasses Python entirely. The
Rust `Server::start()` static path owns request handling directly — no
`PythonCallbackService`, no GIL acquisition, and no Python-side
`StaticResponder` construction. Subclasses and non-default settings fall back to
the Python callback path.

The fast-path eligibility contract is exact: the bare
`SimpleHTTPRequestHandler` class, or a `functools.partial` whose `.func` is
exactly `SimpleHTTPRequestHandler`, whose `.args` is empty, and whose keyword
names are a subset of `{"directory", "extra_response_headers"}`. Bound positional args, arbitrary extra
keywords, subclasses, and mutated stock class attributes (custom
`index_pages`, non-default `extensions_map`, opt-in `directory_listing`,
`follow_symlinks`, `allow_dotfiles`) all fall back to Python.

`default_content_type` and `extra_response_headers` are the exception: they are
validated static metadata settings and remain on the native fast path. Extra
headers preserve input order, are limited to final status-200 responses, and
cannot replace runtime-owned or hop-by-hop metadata. The same metadata is
passed to the callback-backed static responder.

When the fast path is active, the compatibility facade's effective concurrency
is enforced through the native connection admission limit. `HTTPServer` /
`HTTPSServer` (compat `max_workers=1`) bound native `max_connections` to `1`;
`ThreadingHTTPServer(N)` / `ThreadingHTTPSServer(N)` bound it to `N`. Callback
paths keep the existing `max_python_callbacks` semaphore behavior. No new
scheduler/semaphore abstraction is introduced for this compatibility fix.

The callback contract is covered at the public compatibility boundary: a
custom `BaseHTTPRequestHandler` that holds two requests in Python with
`ThreadingHTTPServer(max_workers=2)` does not admit a third callback until one
held request releases its permit. This is distinct from fast-path selection;
the test observes handler entry and active-handler count directly.

Each Python `Server` creates a bounded per-server Tokio multi-thread runtime
with 2 worker threads. This reduces per-server thread overhead by ~78% on a
16-core host with no measurable throughput regression. Per-server runtime
ownership, start/stop lifecycle, and independence between server instances are
preserved.

## Supported modules

`eggserve.server` exports exactly:

- `HTTPServer` and `ThreadingHTTPServer`;
- `HTTPSServer` and `ThreadingHTTPSServer`;
- `BaseHTTPRequestHandler` and `SimpleHTTPRequestHandler`.

The classes use the Rust runtime for socket ownership, HTTP/1.1 parsing,
timeouts, response framing, static path resolution, and file streaming.
Handlers are synchronous and receive bounded `rfile`/`wfile` adapters, never
raw sockets. HTTPS uses the shared core rustls PEM loader, accepts a combined
certificate/key PEM when `keyfile` is omitted, and restricts ALPN to
`http/1.1`. The CLI has the analogous `--tls-cert` fallback when
`--tls-key` is omitted.

Compatibility addresses are normalized in the Python façade only at the API
boundary: `""` becomes `0.0.0.0`, and literal unspecified IPv4/IPv6 addresses
carry explicit wildcard intent to the existing native server. Hostname and
literal resolution remains native, and the published `server_address` is the
actual native `(host, port)` tuple.

`eggserve.lowlevel` contains the advanced PyO3 wrappers (`SecureRoot`,
`StaticPolicy`, `RequestTarget`, canonical HTTP types, and body/response
primitives) plus the Plan 166 public runtime substrate: frozen `RuntimeConfig`
(projected via the single `_native_kwargs()` helper; Plan 182),
handler-only `Server(config, handler)` over the shared native runtime (no
second accept loop, no static root), bounded `Response.stream` (16-chunk
backpressured bridge; HEAD/body-forbidden never advance the iterator; sync
iterables only; `Transfer-Encoding` never service-set), and caller-owned
`StaticResponder` composition. Plan 204 adds the experimental async substrate
in the same namespace (manual asyncio bridge, no new native dependency):
`AsyncServer(config, async_handler, max_async_tasks)` over the shared runtime
(H1-only), `AsyncRequest`/`AsyncBody` (byte-fidelity metadata, incremental
`read_chunk`/`aiter_chunks`/`trailers`, lifecycle/interim/tunnel handles),
`AsyncResponse.stream` (async iterables via a bounded 16-queue plus
`stream_with_trailers`), and `TunnelCapability`/`Tunnel` (one-shot accept,
bounded duplex). Sync/async handler logic is separate in `lowlevel.py`; shared canonical
conversion stays in `server.rs` submodules (Plan 206 Track B): `errors.rs`
centralizes exception mapping; `body_bridge` and `response_bridge` each own
one channel state machine; `request_bridge` owns `PyRequest`; `tunnel_bridge`
owns tunnel duplex; `static_responder` owns caller-owned composition (no
routing, no event-loop mixing); `sync_handler` owns the single
Python-to-canonical conversion; `runtime` owns `PyServer` lifecycle on the
shared native runtime; `lifecycle` documents the state machine; `async_handler`
points to the Python-side Plan 204 asyncio bridge (no duplicated Rust
conversion). Cross-module fields/methods are `pub(super)` (parent-visible,
not crate-wide); PyO3 getters unchanged. Async lives in
`python/eggserve/lowlevel.py`. `eggserve.subprocess` is the canonical owner of
`ServeConfig`, `ServerProcess`, `StaticPolicy`, and the `serve_directory`
convenience (Plan 182); `eggserve.server` keeps compatibility re-exports of
those names without expanding its six-class `__all__`. The top-level package
only re-exports the version, `serve_directory` (from `eggserve.subprocess`),
and the six façade classes.

The native callback `Server` backs both the facade and `lowlevel.Server`;
`StaticResponder`, `ServerSecureRoot`, and `ServerBodySource` back the public
lowlevel composition primitives (previously internal/test-only).

The canonical executable facade demonstrations are
[`examples/python_http_server_static.py`](../examples/python_http_server_static.py)
and [`examples/python_custom_handler.py`](../examples/python_custom_handler.py).
The optional subprocess and low-level primitive examples are listed in
[`examples/README.md`](../examples/README.md); none is a replacement for the
supported `eggserve.server` surface.

## Structure

```
crates/eggserve-python/
├── Cargo.toml          # cdylib and feature-scoped Rust dependencies
├── pyproject.toml      # maturin metadata and entry points
├── src/
│   ├── lib.rs          # PyO3 module registration
│   └── server.rs       # facade: declares mods + re-exports Py* types for lib.rs
│       ├── errors.rs       # exception mapping
│       ├── body_bridge.rs  # channel state machine for request bodies
│       ├── request_bridge.rs  # PyRequest
│       ├── response_bridge.rs # PyResponse channel state machine
│       ├── tunnel_bridge.rs   # tunnel duplex
│       ├── static_responder.rs # caller-owned composition (no routing)
│       ├── sync_handler.rs    # single Python-to-canonical conversion
│       ├── runtime.rs         # PyServer lifecycle on shared native runtime
│       ├── lifecycle.rs       # state machine pointer
│       └── async_handler.rs   # pointer to Python-side Plan 204 asyncio bridge
└── python/eggserve/
    ├── __init__.py     # small supported top-level namespace
    ├── _bin.py         # CLI entry point via native _run_cli
    ├── __main__.py     # python -m eggserve support
    ├── server.py       # six-class Rust-runtime compatibility façade (plus subprocess compat re-exports)
    ├── lowlevel.py     # advanced native exports + handler/runtime substrate
    └── subprocess.py   # canonical subprocess/CLI convenience owner
```

## Security boundary

Python cannot choose a filesystem path per request, reopen translated paths,
provide a raw socket, or override runtime-owned framing. Static handler roots
and policy flags are captured during server construction. Invalid TLS
configuration fails before the native server reports readiness; key material
is never logged. The platform qualifications in `docs/security-review.md` and
the Windows trusted-content limitation in `docs/toolchain-support.md` continue
to apply.

The callback bridge (used only for subclass/custom handlers, not stock static
serving) stages status, ordered headers, body ownership, and
`Content-Length` validation before constructing a canonical response. Native
file and byte bodies are one-shot; consumed or malformed structural bodies are
errors rather than empty-body fallbacks. Handler failures are logged with
fixed categories only. MIME hooks provide metadata to the native responder;
they never perform Python path translation, `stat`, open, or reopen operations.
The facade accepts only `protocol_version = "HTTP/1.1"`; response errors use
the configurable `responses`, `error_message_format`, and
`error_content_type` hooks with bounded HTML-escaped bodies.
The [Python compatibility contract](../docs/python-http-server-compatibility.md)
owns the source-compatibility boundary. Installed-wheel checks run in routine
Python CI; cross-platform qualification is documented in
[`toolchain-support.md`](../docs/toolchain-support.md). Windows is functionally
qualified for the executed classes but remains trusted/local-content only
because two open-descendant root-rename cases are rejected by NTFS path-rename
semantics.

## Verification

The installed-wheel harness is `scripts/test-python-wheel.sh`. It builds the
wheel, installs it into a clean CPython environment (CI default: 3.14 with
`PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1`), checks the import boundary, and runs
the focused compatibility, TLS, low-level, lifecycle, and boundary tests with
`unittest`. The CI default is a test-interpreter constraint, not a package
requirement; release wheels are built against the CPython 3.11 ABI baseline.
Subprocess helpers are canonically owned by `eggserve.subprocess`
(`eggserve.server` retains compatibility re-exports). Release version and
`[profile.dist]` sync is guarded cheaply by
`scripts/check-python-release-metadata.py` before wheel builds.
