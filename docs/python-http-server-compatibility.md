# Python `http.server` compatibility

eggserve provides a narrow `http.server`-shaped API over the Rust-owned
listener, Hyper parser, response validator, and shutdown machinery:

```python
from eggserve.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"ok\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

The supported classes are `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`,
`ThreadingHTTPSServer`, `BaseHTTPRequestHandler`, and
`SimpleHTTPRequestHandler`. `HTTPServer` serializes handler callbacks;
`ThreadingHTTPServer` uses bounded Rust-managed callback concurrency and does
not create one Python thread per connection.

`rfile` and `wfile` are bounded in-memory facades. Request headers are exposed
as an ordered, duplicate-preserving view (`get`, `get_all`, `items`, and
membership). Response headers are validated and remain ordered. The runtime
owns connection persistence, `Date`, `Content-Length`, and all other framing;
handlers cannot supply `Connection`, `Keep-Alive`, `Upgrade`, or
`Transfer-Encoding`.

The default ceilings are 1 MiB for request bodies and 16 MiB for handler
responses. Override them with `max_request_body_bytes` and
`max_handler_response_bytes`. Handlers are synchronous; coroutine returns,
uncaught exceptions, invalid responses, and oversized bodies fail closed.

Static usage follows the familiar shape:

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

`directory=None` captures the current directory during server construction.
Roots are validated and pinned before serving. `index_pages` defaults to
`("index.html", "index.htm")`; listing is opt-in with `directory_listing=True`.
`follow_symlinks` and `allow_dotfiles` are explicit opt-ins. Policies are
captured at startup. GET and HEAD preserve native conditional and single-range
semantics, while file bodies remain Rust-owned streams.

Unlike the stdlib handler, `translate_path()` is intentionally unavailable,
`list_directory()` never receives a raw host path, unknown MIME types use
`application/octet-stream`, and static GET/HEAD request bodies are rejected.
Backslashes, traversal, dotfiles, and denied symlinks remain protected by the
native resolver.

`poll_interval` is accepted for source compatibility but the runtime uses
event-driven shutdown. Raw sockets, `fileno()`, exact one-request
`handle_request()`, socketserver internals, and async handlers are outside
this foundation. TLS uses rustls and accepts only HTTP/1.1 ALPN; it does not
accept `ssl.SSLContext`, expose wrapped sockets, select multiple certificates,
or manage certificates. Static file compatibility is defined separately by
Plan 097; TLS compatibility by Plan 098.

Low-level Rust-backed primitives remain available only through the explicitly
advanced `eggserve.lowlevel` namespace.
