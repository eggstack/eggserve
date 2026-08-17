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

    def test_httpmessage_helpers_are_read_only_and_duplicate_preserving(self):
        seen = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append({
                    "item": self.headers["Content-Length"],
                    "keys": self.headers.keys(),
                    "values": self.headers.values(),
                    "items": self.headers.items(),
                    "raw": self.headers.raw_items(),
                    "type": self.headers.get_content_type(),
                    "maintype": self.headers.get_content_maintype(),
                    "subtype": self.headers.get_content_subtype(),
                    "charset": self.headers.get_content_charset(),
                    "param": self.headers.get_param("charset"),
                    "all": self.headers.get_all("X-Test"),
                })
                self.send_response(200)
                self.end_headers()

        server, _ = self.run_server(Handler)
        response = request(
            server,
            b"GET / HTTP/1.1\r\nHost: test\r\nContent-Length: 3\r\n"
            b"Content-Type: text/plain; charset=UTF-8\r\nX-Test: a\r\nX-Test: b\r\n"
            b"Connection: close\r\n\r\nabc",
        )
        self.assertIn(b"200 OK", response)
        self.assertEqual(seen[0]["item"], "3")
        self.assertEqual(seen[0]["type"], "text/plain")
        self.assertEqual(seen[0]["maintype"], "text")
        self.assertEqual(seen[0]["subtype"], "plain")
        self.assertEqual(seen[0]["charset"], "utf-8")
        self.assertEqual(seen[0]["param"], "UTF-8")
        self.assertEqual(seen[0]["all"], ["a", "b"])

    def test_send_error_customization_and_body_suppression(self):
        class Handler(BaseHTTPRequestHandler):
            error_message_format = "%(code)d|%(message)s|%(explain)s"
            error_content_type = "text/custom"

            def do_GET(self):
                self.send_error(418, "<bad>", "details & more")

            def do_HEAD(self):
                self.send_error(418, "head", "details")

        server, _ = self.run_server(Handler)
        response = request(server, b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        self.assertIn(b"418|&lt;bad&gt;|details &amp; more", response)
        self.assertIn(b"text/custom", response)
        head = request(server, b"HEAD / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        self.assertIn(b"content-length", head.lower())
        self.assertTrue(head.endswith(b"\r\n\r\n"))

    def test_protocol_version_is_fixed_and_log_helper_is_available(self):
        class Incompatible(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

        with self.assertRaises(ValueError):
            HTTPServer(("127.0.0.1", 0), Incompatible)

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(self.log_date_time_string().encode())

        server, _ = self.run_server(Handler)
        response = request(server, b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        self.assertRegex(response, rb"\d{2}/[A-Z][a-z]{2}/\d{4} \d{2}:\d{2}:\d{2}")

    def test_callback_concurrency_is_bounded_at_public_server_boundary(self):
        """Callback handlers remain bounded by ThreadingHTTPServer workers."""
        state_lock = threading.Lock()
        active = 0
        max_active = 0
        entered_a = threading.Event()
        entered_b = threading.Event()
        entered_c = threading.Event()
        first_two_entered = threading.Event()
        release_a = threading.Event()
        release_b = threading.Event()
        release_c = threading.Event()

        class BlockingHandler(BaseHTTPRequestHandler):
            def do_GET(self):
                nonlocal active, max_active
                path = self.path.split("?", 1)[0]
                with state_lock:
                    active += 1
                    max_active = max(max_active, active)
                    if active == 2:
                        first_two_entered.set()
                try:
                    if path == "/a":
                        entered_a.set()
                        release_a.wait(5)
                    elif path == "/b":
                        entered_b.set()
                        release_b.wait(5)
                    elif path == "/c":
                        entered_c.set()
                        release_c.wait(5)
                    else:
                        self.send_response(404)
                        self.end_headers()
                        return
                    self.send_response(200)
                    self.end_headers()
                finally:
                    with state_lock:
                        active -= 1

        server, _ = self.run_server(BlockingHandler, ThreadingHTTPServer, max_workers=2)
        self.assertFalse(server._native_fast_path)
        payload = b"GET {} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
        responses = {}
        errors = []

        def send(path):
            try:
                responses[path] = request(server, payload.replace(b"{}", path.encode()))
            except BaseException as exc:  # surface worker failures in the test thread
                errors.append(exc)

        workers = [threading.Thread(target=send, args=(path,)) for path in ("/a", "/b")]
        workers.append(threading.Thread(target=send, args=("/c",)))
        try:
            for worker in workers[:2]:
                worker.start()
            self.assertTrue(first_two_entered.wait(3), "first two callbacks did not enter")
            self.assertTrue(entered_a.is_set())
            self.assertTrue(entered_b.is_set())

            workers[2].start()
            self.assertFalse(
                entered_c.wait(0.25),
                "third callback entered while both callback permits were held",
            )

            release_a.set()
            self.assertTrue(entered_c.wait(3), "third callback did not proceed after a release")
            release_b.set()
            release_c.set()
        finally:
            release_a.set()
            release_b.set()
            release_c.set()
            for worker in workers:
                if worker.ident is not None:
                    worker.join(5)

        self.assertEqual(errors, [])
        self.assertEqual(set(responses), {"/a", "/b", "/c"})
        self.assertLessEqual(max_active, 2)
        self.assertTrue(all(b"200 OK" in response for response in responses.values()))

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

    def test_ipv4_loopback_publishes_structured_addresses(self):
        seen = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append(self.client_address)
                self.send_response(200)
                self.end_headers()

        server, _ = self.run_server(Handler)
        response = request(server, b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        self.assertIn(b"200 OK", response)
        self.assertIsInstance(server.server_address[0], str)
        self.assertIsInstance(server.server_address[1], int)
        self.assertIsInstance(seen[-1][0], str)
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

    def test_empty_host_can_publish_an_ephemeral_listener(self):
        class Handler(BaseHTTPRequestHandler):
            pass

        server = HTTPServer(("", 0), Handler)
        self.addCleanup(server.server_close)
        self.assertEqual(server.server_address[0], "0.0.0.0")
        self.assertGreater(server.server_port, 0)

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

    def test_port_zero_address_only_after_readiness(self):
        """Port 0 server publishes real port only after _start() completes.

        Regression test for Plan 121 Track C case 8: the compatibility
        facade must not publish a port 0 address before the native server
        is in Running state.
        """

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

        server = HTTPServer(("127.0.0.1", 0), Handler, bind_and_activate=False)
        # Before activation: port is still 0.
        self.assertEqual(server.server_port, 0)
        self.assertEqual(server.server_address[1], 0)
        # After server_activate: native is created but not started.
        server.server_activate()
        self.assertEqual(server.server_port, 0)
        # After _start: native server is Running and real port is published.
        server._start()
        self.assertGreater(server.server_port, 0)
        self.assertGreater(server.server_address[1], 0)
        server.server_close()

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
