"""Serve a directory with the secure http.server-compatible facade."""

from functools import partial

from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
