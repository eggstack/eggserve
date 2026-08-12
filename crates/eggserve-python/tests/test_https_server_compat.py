"""Focused TLS compatibility tests for the installed Python façade."""

import os
import functools
import socket
import ssl
import tempfile
import threading
import unittest

from eggserve.server import (
    BaseHTTPRequestHandler,
    HTTPSServer,
    SimpleHTTPRequestHandler,
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
        self.seen_addresses = []
        with open(os.path.join(self.tmp.name, "hello.txt"), "wb") as stream:
            stream.write(b"hello over tls")
        fixture_dir = os.path.join(os.path.dirname(__file__), "fixtures")
        self.cert = os.path.join(fixture_dir, "localhost-test.crt")
        self.key = os.path.join(fixture_dir, "localhost-test.key")

    def tearDown(self):
        self.tmp.cleanup()

    def run_server(self, server_class=HTTPSServer, **kwargs):
        seen_addresses = self.seen_addresses

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen_addresses.append((self.client_address, self.server.server_address))
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

    def run_static_server(self):
        handler = functools.partial(SimpleHTTPRequestHandler, directory=self.tmp.name)
        server = HTTPSServer(("127.0.0.1", 0), handler, certfile=self.cert, keyfile=self.key)
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

    def test_tls_addresses_are_structured_tuples(self):
        server = self.run_server()
        response = _request(server, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", response)
        client_address, server_address = self.seen_addresses[-1]
        self.assertIsInstance(client_address, tuple)
        self.assertIsInstance(server_address, tuple)
        self.assertEqual(len(client_address), 2)
        self.assertEqual(len(server_address), 2)

    def test_static_get_head_and_range_over_tls(self):
        server = self.run_static_server()
        response = _request(server, b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", response)
        self.assertTrue(response.endswith(b"hello over tls"))
        head = _request(server, b"HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", head)
        self.assertIn(b"content-length: 14", head.lower())
        self.assertTrue(head.endswith(b"\r\n\r\n"))
        ranged = _request(server, b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n")
        self.assertIn(b"206 Partial Content", ranged)
        self.assertTrue(ranged.endswith(b"hello"))

    def test_plaintext_request_does_not_succeed_on_tls_listener(self):
        server = self.run_server()
        try:
            with socket.create_connection(server.server_address, timeout=2) as raw:
                raw.settimeout(2)
                raw.sendall(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                plaintext = raw.recv(1024)
        except OSError:
            plaintext = b""
        self.assertFalse(plaintext.startswith(b"HTTP/1.1 200"))

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
                        certfile=self.cert, keyfile=os.path.join(self.tmp.name, "missing"))
        with self.assertRaises(ValueError):
            HTTPSServer(("127.0.0.1", 0), BaseHTTPRequestHandler,
                        certfile=self.cert, keyfile=self.key, alpn_protocols=["h2"])
        with self.assertRaises(ValueError):
            HTTPSServer(("127.0.0.1", 0), BaseHTTPRequestHandler,
                        certfile=self.cert, keyfile=self.key, password="secret")

    def test_context_manager_shutdown_completes(self):
        with HTTPSServer(("127.0.0.1", 0), BaseHTTPRequestHandler,
                         certfile=self.cert, keyfile=self.key) as server:
            server._start()
            self.assertGreater(server.server_port, 0)


class HttpsNativeFastPathTests(unittest.TestCase):
    """Verify native fast path is active for stock SimpleHTTPRequestHandler over TLS."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        with open(os.path.join(self.tmp.name, "hello.txt"), "wb") as stream:
            stream.write(b"tls native")
        fixture_dir = os.path.join(os.path.dirname(__file__), "fixtures")
        self.cert = os.path.join(fixture_dir, "localhost-test.crt")
        self.key = os.path.join(fixture_dir, "localhost-test.key")

    def tearDown(self):
        self.tmp.cleanup()

    def test_https_server_native_fast_path_eligible(self):
        handler = functools.partial(SimpleHTTPRequestHandler, directory=self.tmp.name)
        server = HTTPSServer(("127.0.0.1", 0), handler,
                             certfile=self.cert, keyfile=self.key)
        self.assertTrue(server._native_fast_path)
        server.server_close()

    def test_threading_https_server_native_fast_path_eligible(self):
        handler = functools.partial(SimpleHTTPRequestHandler, directory=self.tmp.name)
        server = ThreadingHTTPSServer(("127.0.0.1", 0), handler,
                                      certfile=self.cert, keyfile=self.key)
        self.assertTrue(server._native_fast_path)
        server.server_close()

    def test_https_stock_handler_serves_file(self):
        handler = functools.partial(SimpleHTTPRequestHandler, directory=self.tmp.name)
        server = HTTPSServer(("127.0.0.1", 0), handler,
                             certfile=self.cert, keyfile=self.key)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            response = _request(server, b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            self.assertIn(b"200 OK", response)
            self.assertTrue(response.endswith(b"tls native"))
        finally:
            server.server_close()
            thread.join(5)

    def test_https_subclass_falls_back(self):
        class CustomHandler(SimpleHTTPRequestHandler):
            def guess_type(self, path):
                return "application/x-custom"

        handler = functools.partial(CustomHandler, directory=self.tmp.name)
        server = HTTPSServer(("127.0.0.1", 0), handler,
                             certfile=self.cert, keyfile=self.key)
        self.assertFalse(server._native_fast_path)
        server.server_close()


if __name__ == "__main__":
    unittest.main()
