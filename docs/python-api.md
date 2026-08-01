# Python API

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

The bespoke native callback `Server` and the experimental Python HTTP client
are not part of the supported default package surface. The Rust client may
remain an opt-in core feature, but the Python wheel does not compile it.

## Compatibility boundary

The façade is not ASGI, WSGI, a routing framework, middleware, a proxy, or a
general-purpose HTTP server. It intentionally does not expose socketserver
internals, `fileno()`, authoritative `translate_path()`, or one-request
`handle_request()` mode. Platform security qualifications, especially the
unfinished independent Windows adversarial review, remain in
[`README.md`](../README.md) and [`security-review.md`](security-review.md).
