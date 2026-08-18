"""Serve static files over HTTPS using EggServe's Rust TLS backend.

Requires the ``tls`` feature (enabled in published PyPI wheels).

Generate a self-signed certificate for local testing:

    openssl req -x509 -newkey rsa:2048 -nodes \\
        -keyout key.pem -out cert.pem -days 30 \\
        -subj '/CN=localhost'

Then run:

    python examples/python_https_server.py
"""

from functools import partial
from pathlib import Path

from eggserve.server import (
    SimpleHTTPRequestHandler,
    ThreadingHTTPSServer,
)

SITE = Path(__file__).with_name("site")
CERT = Path(__file__).parent / "cert.pem"
KEY = Path(__file__).parent / "key.pem"


def create_server(
    address=("127.0.0.1", 8443),
    directory=SITE,
    certfile=CERT,
    keyfile=KEY,
):
    handler = partial(SimpleHTTPRequestHandler, directory=directory)
    return ThreadingHTTPSServer(address, handler, certfile=certfile, keyfile=keyfile)


def main():
    with create_server() as server:
        print(f"Serving on https://{server.server_address[0]}:{server.server_address[1]}")
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
