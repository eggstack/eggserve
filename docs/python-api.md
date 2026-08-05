# Python API

Plan 108 ownership correction: custom Python handlers start with runtime-only
configuration and do not pin or otherwise depend on a static responder root.
Static handlers construct one confined static service. Both paths use the
running server's single file-stream admission pool for canonical file bodies.

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
`allow_dotfiles`, and `follow_symlinks` are explicit opt-ins. GET and HEAD
retain Rust-native conditional, range, and streaming behavior.

`extensions_map` values and `guess_type()` results are response metadata, not
filesystem operations. They must be strings without prohibited header
characters; invalid values fail closed with a generic 500. `extensions_map`
also applies to native-selected index files. A subclass `guess_type()` hook is
defined for direct file targets with a suffix, not for index names resolved
only inside Rust.

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
client certificates, ACME, or certificate reload is provided.

## Convenience and advanced namespaces

`serve_directory()` is a blocking convenience at `eggserve.serve_directory`.
`ServeConfig`, `ServerProcess`, and `StaticPolicy` live in
`eggserve.subprocess`. Security-sensitive embedding primitives such as
`SecureRoot`, `StaticPolicy`, `RequestTarget`, and canonical HTTP types live
in `eggserve.lowlevel`.

The six-class `eggserve.server` façade is the primary supported Python server
surface. The bespoke native callback `Server` and the experimental Python HTTP
client are not part of the supported default package surface. The Rust client may
remain an opt-in core feature, but the Python wheel does not compile it.

## Compatibility boundary

The façade is not ASGI, WSGI, a routing framework, middleware, a proxy, or a
general-purpose HTTP server. It intentionally does not expose socketserver
internals, `fileno()`, authoritative `translate_path()`, or one-request
`handle_request()` mode. Platform security qualifications, especially the
unfinished independent Windows adversarial review, remain in
[`README.md`](../README.md) and [`security-review.md`](security-review.md).

Python handler responses are validated in one Rust-owned conversion boundary.
Malformed structural bodies, failed body reads, consumed one-shot bodies,
invalid headers, and length mismatches produce a generic 500; malformed state
is never treated as an empty successful response. Operational diagnostics use
bounded categories and do not include handler exception text or response data.
