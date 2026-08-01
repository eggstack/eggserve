"""Focused TLS compatibility tests for the installed Python façade."""

import os
import socket
import ssl
import tempfile
import threading
import unittest

from eggserve.server import (
    BaseHTTPRequestHandler,
    HTTPSServer,
    ThreadingHTTPSServer,
)


def _request(server, payload):
    context = ssl._create_unverified_context()
    with socket.create_connection(server.server_address, timeout=3) as raw:
        with context.wrap_socket(raw, server_hostname="localhost") as sock:
            sock.sendall(payload)
            chunks = []
            while True:
                data = sock.recv(4096)
                if not data:
                    return b"".join(chunks)
                chunks.append(data)


class HttpsCompatTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        fixture_dir = os.path.join(os.path.dirname(__file__), "fixtures")
        self.cert = os.path.join(fixture_dir, "localhost-test.crt")
        self.key = os.path.join(fixture_dir, "localhost-test.key")

    def tearDown(self):
        self.tmp.cleanup()

    def run_server(self, server_class=HTTPSServer, **kwargs):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write((self.request.scheme or "missing").encode())

        server = server_class(("127.0.0.1", 0), Handler, certfile=self.cert,
                              keyfile=self.key, **kwargs)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(thread.join, 5)
        while server.server_port == 0:
            thread.join(0.01)
        return server

    def test_https_handler_and_scheme(self):
        server = self.run_server()
        response = _request(server, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", response)
        self.assertTrue(response.endswith(b"https"))

    def test_threading_https_server(self):
        server = self.run_server(ThreadingHTTPSServer, max_workers=2)
        self.assertEqual(server._max_workers, 2)
        self.assertIn(b"https", _request(server, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"))

    def test_invalid_configuration_fails_before_start(self):
        with self.assertRaises(ValueError):
            HTTPSServer(("127.0.0.1", 0), BaseHTTPRequestHandler,
                        certfile=os.path.join(self.tmp.name, "missing"), keyfile=self.key)
        with self.assertRaises(ValueError):
            HTTPSServer(("127.0.0.1", 0), BaseHTTPRequestHandler,
                        certfile=self.cert, keyfile=self.key, alpn_protocols=["h2"])
        with self.assertRaises(ValueError):
            HTTPSServer(("127.0.0.1", 0), BaseHTTPRequestHandler,
                        certfile=self.cert, keyfile=self.key, password="secret")


if __name__ == "__main__":
    unittest.main()
