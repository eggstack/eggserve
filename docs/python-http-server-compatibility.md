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

The supported classes are `HTTPServer`, `ThreadingHTTPServer`, and
`BaseHTTPRequestHandler`. `HTTPServer` serializes handler callbacks;
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

`poll_interval` is accepted for source compatibility but the runtime uses
event-driven shutdown. Raw sockets, `fileno()`, exact one-request
`handle_request()`, socketserver internals, directory serving, TLS classes,
and async handlers are outside this foundation. Static file compatibility is
defined separately by Plan 097; TLS compatibility by Plan 098.

This API is distinct from the lower-level native `eggserve.Server`, which
accepts callback functions and exposes the Rust runtime directly.
