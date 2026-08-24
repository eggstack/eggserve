# Python `http.server` compatibility contract

EggServe provides a narrow, bounded `http.server`-shaped facade over a
Rust-owned listener, HTTP/1 parser, response validator, and lifecycle. It is a
static-server compatibility layer plus a synchronous custom-handler boundary,
not a general `socketserver` implementation or application framework.

## Product comparison

| Capability | `python -m http.server` | EggServe CLI | EggServe Python | EggServe Rust |
|---|---|---|---|---|
| Static GET/HEAD | Yes | Yes | Yes | Yes |
| Secure loopback default | No; ordinary invocation binds broadly | Yes | Use the documented loopback tuple; empty host is explicit wildcard intent | Configurable; safe defaults are loopback |
| Directory listing default | Enabled | Disabled | Disabled | Disabled by policy |
| Symlink following default | Follows | Denied | Denied | Denied by policy |
| Dotfiles default | Served | Denied | Denied | Denied by policy |
| Ranges/conditional requests | Limited/version-dependent | Supported contract | Supported on static path | Supported on static path |
| Custom handler responses | Subclass handlers | No CLI handler API | Bounded synchronous `BaseHTTPRequestHandler` | `Service` boundary |
| Raw socket access | Available through socketserver internals | No | No | Listener/runtime APIs only |
| `translate_path()` | Available | N/A | Intentionally unavailable | Hardened resolver primitives |
| Raw `list_directory()` path | Available to handler | N/A | Intentionally unavailable | N/A |
| Static metadata hooks | `--content-type`, `-H/--header` | `--content-type`, `-H/--header` | `default_content_type`, `extra_response_headers` | Builder metadata |
| ASGI/WSGI | No | No | No | No |

The matrix describes product boundaries rather than Python-version trivia. The
[library capability matrix](library-capability-matrix.md) has the more detailed
Rust/Python API inventory.

## Supported facade

The supported classes are exactly:

- `HTTPServer` and `ThreadingHTTPServer`;
- `HTTPSServer` and `ThreadingHTTPSServer`;
- `BaseHTTPRequestHandler` and `SimpleHTTPRequestHandler`.

The canonical static pattern is:

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

Stock `SimpleHTTPRequestHandler` with default settings uses the native static
fast path. The exact eligibility contract is the bare class, or a
`functools.partial` whose `.func` is exactly `SimpleHTTPRequestHandler`, whose
`.args` is empty, and whose keywords are limited to `directory` and
`extra_response_headers`. Subclasses, extra partial arguments/keywords, and
non-default static settings use the bounded Python callback path. A custom
`default_content_type` and safe ordered extra headers remain native metadata
settings; extras are emitted only for final `200` responses and never replace
runtime-owned headers.

Both paths use Rust for socket ownership, request parsing, framing, timeouts,
path confinement, and file streaming. The callback path is synchronous;
`ThreadingHTTPServer(max_workers=N)` provides bounded callback concurrency.
When the native fast path is active, `HTTPServer`/`HTTPSServer` are effectively
limited to one connection and `ThreadingHTTPServer(N)`/
`ThreadingHTTPSServer(N)` to `N` through native admission control.

## Source-familiar behavior

The facade supports the compatibility behaviors that are useful for porting a
small `http.server` handler:

- stdlib-shaped `(host, port)` tuples, including port `0` publication after
  native readiness;
- `serve_forever()`, `shutdown()`, context-manager cleanup, and the bounded
  lifecycle methods exposed by the facade;
- `send_response()`, `send_header()`, `end_headers()`, and bounded `rfile`/
  `wfile` adapters;
- duplicate-preserving request-header access through `get()`, `get_all()`,
  `items()`, and membership;
- `BaseHTTPRequestHandler.send_error()` with class-level `responses`,
  `error_message_format`, and `error_content_type` hooks, plus
  `log_date_time_string()` and MIME-oriented request-header helpers;
- `SimpleHTTPRequestHandler(directory=...)`, `GET`/`HEAD` static semantics,
  ranges, conditional requests, index selection, and bounded `guess_type()` /
  `extensions_map`, `default_content_type`, and ordered
  `extra_response_headers` metadata hooks;
- rustls-backed `HTTPSServer` and `ThreadingHTTPSServer` with PEM paths and
  HTTP/1.1 ALPN only.

Static roots are validated and pinned at construction. Handler classes that set
an incompatible `protocol_version` are rejected at construction because the
runtime is HTTP/1.1 only. `rfile` and `wfile` are
bounded in-memory facades: the default request-body ceiling is 1 MiB and the
default handler-response ceiling is 16 MiB. Handlers do not receive raw
sockets, and Rust owns `Date`, `Content-Length`, connection persistence, and
hop-by-hop framing headers.

`server_version`, `sys_version`, and the formatting methods remain available
for source-compatible logging/customization. They do not create a Python-owned
`Server` header; transport metadata remains Rust-owned.

## Intentional incompatibilities

These behaviors are unavailable by design, not pending work:

| Unavailable behavior | Boundary |
|---|---|
| Raw socket ownership or `fileno()` | Rust transport ownership |
| Exact `socketserver` internals | Compatibility scope control |
| One-request `handle_request()` mode | Runtime lifecycle is event-driven |
| Authoritative `translate_path()` or raw host paths in `list_directory()` | Security confinement |
| Python thread-per-connection behavior | Bounded native/callback admission |
| Arbitrary `ssl.SSLContext`, SNI multi-cert selection, or client certificates | Rustls facade constraints |
| Async handler coroutines | Synchronous handler contract |
| Unbounded streaming Python response bodies | Bounded response policy |

The facade also does not expose routing, middleware, proxying, decompression,
cookies, retries, or ASGI/WSGI adaptation.

## Address and lifecycle details

`client_address` and `server_address` are `(host, port)` tuples, including for
IPv6 (the host is unbracketed). In this facade only, `""` is normalized to the
explicit IPv4 wildcard `"0.0.0.0"`; literal `0.0.0.0` and `::` are also
accepted. This does not change the CLI rule that wildcard binds require
`--public`. Port `0` is not published until native activation has completed.

`poll_interval` is accepted for source compatibility but shutdown is
event-driven. `server_bind()` and `server_activate()` are bounded lifecycle
facades; they do not expose the native listener.

A server that has been started must be stopped explicitly with `stop()` or by
leaving the context manager. Abandoning a running server without `stop()`
drops the native runtime when the last Python reference disappears, and that
teardown waits for in-flight handler tasks; interpreter shutdown or garbage
collection can therefore stall until active connections drain. There is no
`__del__`-based shutdown.

## Failure and security behavior

Malformed handler responses fail closed with a generic 500: invalid status or
headers, forbidden framing headers, body-read failures, one-shot body reuse,
non-byte bodies, and `Content-Length` mismatches are not treated as successful
empty responses. Diagnostics use fixed categories and do not log untrusted
exception text or response data.

Static resolution rejects traversal, backslashes, dotfiles, and denied
symlinks before a host path can be reopened. Unknown MIME types use
`application/octet-stream`, and static responses retain `nosniff`.

For the full Python surface see [python-api.md](python-api.md). For the
runtime ownership model see [architecture/runtime.md](../architecture/runtime.md).
