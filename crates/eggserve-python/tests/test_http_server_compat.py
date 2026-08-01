"""Focused installed-wheel contract tests for the http.server facade."""

import socket
import threading
import unittest

from eggserve.server import BaseHTTPRequestHandler, HTTPServer, ThreadingHTTPServer


def request(server, payload):
    with socket.create_connection(server.server_address, timeout=3) as sock:
        sock.sendall(payload)
        chunks = []
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                return b"".join(chunks)
            chunks.append(chunk)


class CompatTests(unittest.TestCase):
    def run_server(self, handler, server_class=HTTPServer, **kwargs):
        server = server_class(("127.0.0.1", 0), handler, **kwargs)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        self.addCleanup(thread.join, 5)
        self.addCleanup(server.server_close)
        while server.server_port == 0:
            thread.join(0.01)
        return server, thread

    def test_get_duplicate_headers_and_query_path(self):
        seen = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append((self.path, self.headers.get_all("X-Test")))
                self.send_response(200)
                self.send_header("X-Reply", "one")
                self.send_header("X-Reply", "two")
                self.end_headers()
                self.wfile.write(b"ok")

        server, _ = self.run_server(Handler)
        response = request(server, b"GET /hello?q=1 HTTP/1.1\r\nHost: test\r\nX-Test: a\r\nX-Test: b\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", response)
        self.assertIn(b"x-reply: one\r\nx-reply: two", response.lower())
        self.assertTrue(response.endswith(b"ok"))
        self.assertEqual(seen, [("/hello?q=1", ["a", "b"])])

    def test_localhost_and_peer_addresses_are_structured(self):
        seen = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append(self.client_address)
                self.send_response(200)
                self.end_headers()

        server = HTTPServer(("localhost", 0), Handler)
        self.assertNotEqual(server.server_address[1], 0)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        self.addCleanup(thread.join, 5)
        self.addCleanup(server.server_close)
        response = request(server, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", response)
        self.assertEqual(len(server.server_address), 2)
        self.assertIsInstance(server.server_address[1], int)
        self.assertEqual(len(seen[-1]), 2)
        self.assertIsInstance(seen[-1][1], int)

    def test_empty_host_is_explicit_ipv4_wildcard(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

        server = HTTPServer(("", 0), Handler)
        self.addCleanup(server.server_close)
        self.assertEqual(server.server_address[0], "0.0.0.0")
        self.assertGreater(server.server_port, 0)
        self.assertIsInstance(server.server_address, tuple)

    def test_explicit_ipv4_wildcard_is_accepted(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

        server = HTTPServer(("0.0.0.0", 0), Handler)
        self.addCleanup(server.server_close)
        self.assertEqual(server.server_address[0], "0.0.0.0")
        self.assertGreater(server.server_address[1], 0)

    def test_ipv6_loopback_is_structured_when_supported(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

        try:
            server = HTTPServer(("::1", 0), Handler)
        except OSError as exc:
            self.skipTest(f"IPv6 loopback unavailable: {exc}")
        self.addCleanup(server.server_close)
        self.assertNotIn("[", server.server_address[0])
        self.assertNotIn("]", server.server_address[0])
        self.assertGreater(server.server_port, 0)

    def test_ipv6_wildcard_is_accepted_when_supported(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

        try:
            server = HTTPServer(("::", 0), Handler)
        except OSError as exc:
            self.skipTest(f"IPv6 wildcard unavailable: {exc}")
        self.addCleanup(server.server_close)
        self.assertEqual(server.server_address[0], "::")
        self.assertGreater(server.server_address[1], 0)

    def test_invalid_hostname_fails_during_activation(self):
        class Handler(BaseHTTPRequestHandler):
            pass

        with self.assertRaises(OSError):
            HTTPServer(("does-not-exist.invalid", 0), Handler)

    def test_deferred_activation_and_close_lifecycle(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

        server = HTTPServer(("127.0.0.1", 0), Handler, bind_and_activate=False)
        self.assertEqual(server.server_port, 0)
        server.server_activate()
        self.assertEqual(server.server_port, 0)
        server._start()
        self.assertGreater(server.server_port, 0)
        server.server_close()
        with self.assertRaises(RuntimeError):
            server._start()

    def test_post_reads_bounded_body(self):
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        server, _ = self.run_server(Handler, max_request_body_bytes=32)
        response = request(server, b"POST / HTTP/1.1\r\nHost: test\r\nContent-Length: 3\r\nConnection: close\r\n\r\nhey")
        self.assertTrue(response.endswith(b"hey"))

    def test_missing_method_and_exception_are_sanitized(self):
        class Missing(BaseHTTPRequestHandler):
            pass

        server, _ = self.run_server(Missing)
        self.assertIn(b"501 Not Implemented", request(server, b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"))

        class Broken(BaseHTTPRequestHandler):
            def do_GET(self):
                raise RuntimeError("secret")

        server, _ = self.run_server(Broken)
        response = request(server, b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        self.assertIn(b"500 Internal Server Error", response)
        self.assertNotIn(b"secret", response)

    def test_head_body_is_suppressed(self):
        class Handler(BaseHTTPRequestHandler):
            def do_HEAD(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"hidden")

        server, _ = self.run_server(Handler)
        response = request(server, b"HEAD / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        self.assertTrue(response.endswith(b"\r\n\r\n"))
        self.assertNotIn(b"hidden", response)

    def test_shutdown_unblocks_forever_and_threading_variant_is_bounded(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"ok")

        server, thread = self.run_server(Handler)
        server.shutdown()
        thread.join(3)
        self.assertFalse(thread.is_alive())

        threaded, _ = self.run_server(Handler, ThreadingHTTPServer, max_workers=2)
        self.assertEqual(threaded._max_workers, 2)


if __name__ == "__main__":
    unittest.main()
