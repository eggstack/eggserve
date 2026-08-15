"""Serve the example site with EggServe's ``http.server`` facade.

The stock handler configuration is eligible for EggServe's native static fast
path. Unlike the stdlib defaults, safe filesystem and loopback policies remain
active; see docs/python-http-server-compatibility.md.
"""

from functools import partial
from pathlib import Path

from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


SITE = Path(__file__).with_name("site")


def create_server(address=("127.0.0.1", 8000), directory=SITE):
    handler = partial(SimpleHTTPRequestHandler, directory=directory)
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
