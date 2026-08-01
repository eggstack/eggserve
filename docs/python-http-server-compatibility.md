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

`client_address` and `server_address` are `(host, port)` tuples, including for
IPv6 (the host is unbracketed). Empty-host, localhost, IPv4, and supported
IPv6 constructor forms are resolved by the native listener. Port `0` is
published after native activation. In the compatibility façade only, `""` is
normalized to the explicit IPv4 wildcard `"0.0.0.0"`; literal `0.0.0.0` and
`::` are also accepted. This does not change the CLI rule that wildcard binds
require `--public`. `server_bind()`/`server_activate()` are a bounded lifecycle
façade; raw socket ownership and exact `socketserver` internals are intentionally
not exposed.

`SimpleHTTPRequestHandler.extensions_map` and subclass `guess_type()`
overrides affect the Content-Type of the already-resolved native response.
The selected value is retained for GET, HEAD, range, and conditional metadata.
Values must be valid response metadata; invalid strings or non-string results
fail closed with a generic 500. Unknown suffixes remain
`application/octet-stream`; static responses retain
`X-Content-Type-Options: nosniff`. `extensions_map` applies to native-selected
index files. A subclass `guess_type()` override is promised for direct request
targets with a suffix, but not for an index filename Python never resolves.
File-stream limits apply to built-in and compatibility static responses.

Handler responses are converted atomically at the Rust callback boundary. The
supported native `Response`/`BodySource` forms and the internal handler
response form must provide an explicit body; unknown body kinds, failed
`read_all()` calls, non-byte results, consumed one-shot bodies, invalid headers,
and mismatched `Content-Length` values become a generic 500. Deliberate empty
responses remain valid. Handler exception and response-validation logs contain
fixed failure categories, not exception text, response reprs, or raw header
values.

`poll_interval` is accepted for source compatibility but the runtime uses
event-driven shutdown. Raw sockets, `fileno()`, exact one-request
`handle_request()`, socketserver internals, and async handlers are outside
this foundation. TLS uses rustls and accepts only HTTP/1.1 ALPN; it does not
accept `ssl.SSLContext`, expose wrapped sockets, select multiple certificates,
or manage certificates. Static file compatibility is defined separately by
Plan 097; TLS compatibility by Plan 098.

Low-level Rust-backed primitives remain available only through the explicitly
advanced `eggserve.lowlevel` namespace.
