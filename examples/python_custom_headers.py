"""Serve static files with custom default content type and extra response headers.

Shows the Python 3.15-shaped static metadata hooks:

- ``default_content_type`` is class-attribute metadata: set it on a
  ``SimpleHTTPRequestHandler`` subclass. A subclass disables the native fast
  path, so requests are served through the Python callback path.
- ``extra_response_headers`` adds ordered headers to final 200 responses.
  Pass a sequence of ``(name, value)`` pairs. These cannot override
  runtime-owned metadata (Content-Length, ETag, etc.).
"""

from functools import partial
from pathlib import Path

from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

SITE = Path(__file__).with_name("site")


class CustomHeadersHandler(SimpleHTTPRequestHandler):
    """Static metadata hooks must be class attributes, not init kwargs."""

    default_content_type = "application/octet-stream"


def create_server(address=("127.0.0.1", 8000), directory=SITE):
    handler = partial(
        CustomHeadersHandler,
        directory=directory,
        extra_response_headers=[
            ("X-Served-By", "eggserve"),
            ("Cache-Control", "no-cache"),
        ],
    )
    return ThreadingHTTPServer(address, handler)


def main():
    with create_server() as server:
        print(f"Serving on http://{server.server_address[0]}:{server.server_address[1]}")
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
