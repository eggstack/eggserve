"""Installed-wheel integration checks for the secure static handler."""

import functools
import http.client
import os
import tempfile
import threading
import unittest

from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


class SimpleHandlerCompatibilityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        os.mkdir(os.path.join(root, "docs"))
        with open(os.path.join(root, "hello.txt"), "wb") as stream:
            stream.write(b"hello from rust\n")
        with open(os.path.join(root, "docs", "index.htm"), "wb") as stream:
            stream.write(b"index htm\n")
        with open(os.path.join(root, "docs", "index.blob"), "wb") as stream:
            stream.write(b"index blob\n")
        with open(os.path.join(root, ".secret"), "wb") as stream:
            stream.write(b"hidden\n")

        handler = functools.partial(SimpleHTTPRequestHandler, directory=root)
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address

    def tearDown(self):
        self.server.server_close()
        self.thread.join(5)
        self.tmp.cleanup()

    def request(self, method, path, headers=None):
        connection = http.client.HTTPConnection(*self.address, timeout=5)
        connection.request(method, path, headers=headers or {})
        response = connection.getresponse()
        body = response.read()
        connection.close()
        return response, body

    def test_file_get_and_head(self):
        response, body = self.request("GET", "/hello.txt")
        self.assertEqual(response.status, 200)
        self.assertEqual(body, b"hello from rust\n")
        head, head_body = self.request("HEAD", "/hello.txt")
        self.assertEqual(head.status, 200)
        self.assertEqual(head.getheader("Content-Length"), str(len(body)))
        self.assertEqual(head_body, b"")

    def test_range_and_unknown_mime(self):
        response, body = self.request("GET", "/hello.txt", {"Range": "bytes=0-4"})
        self.assertEqual(response.status, 206)
        self.assertEqual(body, b"hello")
        with open(os.path.join(self.tmp.name, "unknown.blob"), "wb") as stream:
            stream.write(b"blob")
        response, _ = self.request("GET", "/unknown.blob")
        self.assertEqual(response.getheader("Content-Type"), "application/octet-stream")
        self.assertEqual(response.getheader("X-Content-Type-Options"), "nosniff")

    def test_extensions_map_and_guess_type_overrides(self):
        with open(os.path.join(self.tmp.name, "custom.blob"), "wb") as stream:
            stream.write(b"custom")

        class MapHandler(SimpleHTTPRequestHandler):
            extensions_map = {".blob": "application/x-map"}
            index_pages = ("index.blob",)

        self.server.server_close()
        self.thread.join(5)
        self.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(MapHandler, directory=self.tmp.name)
        )
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address
        response, _ = self.request("GET", "/custom.blob")
        self.assertEqual(response.getheader("Content-Type"), "application/x-map")
        head, head_body = self.request("HEAD", "/custom.blob")
        self.assertEqual(head.getheader("Content-Type"), "application/x-map")
        self.assertEqual(head_body, b"")
        ranged, ranged_body = self.request("GET", "/custom.blob", {"Range": "bytes=0-2"})
        self.assertEqual(ranged.getheader("Content-Type"), "application/x-map")
        self.assertEqual(ranged_body, b"cus")
        indexed, indexed_body = self.request("GET", "/docs/")
        self.assertEqual(indexed.getheader("Content-Type"), "application/x-map")
        self.assertEqual(indexed_body, b"index blob\n")

        class GuessHandler(SimpleHTTPRequestHandler):
            index_pages = ("index.blob",)

            def guess_type(self, path):
                if path.endswith(".blob"):
                    return "application/x-guess"
                return super().guess_type(path)

        self.server.server_close()
        self.thread.join(5)
        self.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(GuessHandler, directory=self.tmp.name)
        )
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address
        response, _ = self.request("GET", "/custom.blob")
        self.assertEqual(response.getheader("Content-Type"), "application/x-guess")
        head, head_body = self.request("HEAD", "/custom.blob")
        self.assertEqual(head.getheader("Content-Type"), "application/x-guess")
        self.assertEqual(head_body, b"")
        ranged, ranged_body = self.request("GET", "/custom.blob", {"Range": "bytes=0-2"})
        self.assertEqual(ranged.getheader("Content-Type"), "application/x-guess")
        self.assertEqual(ranged_body, b"cus")
        indexed, indexed_body = self.request("GET", "/docs/")
        self.assertEqual(indexed.getheader("Content-Type"), "application/octet-stream")
        self.assertEqual(indexed_body, b"index blob\n")

        class NoPythonPath(SimpleHTTPRequestHandler):
            def translate_path(self, path):
                raise AssertionError("Python path translation must not be used")

        self.server.server_close()
        self.thread.join(5)
        self.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(NoPythonPath, directory=self.tmp.name)
        )
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address
        response, body = self.request("GET", "/custom.blob")
        self.assertEqual(response.status, 200)
        self.assertEqual(body, b"custom")

    def test_invalid_mime_values_fail_closed(self):
        with open(os.path.join(self.tmp.name, "invalid.blob"), "wb") as stream:
            stream.write(b"invalid")

        class InvalidMap(SimpleHTTPRequestHandler):
            extensions_map = {".blob": "text/plain\r\nX-Leak: yes"}

        self.server.server_close()
        self.thread.join(5)
        self.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(InvalidMap, directory=self.tmp.name)
        )
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address
        response, _ = self.request("GET", "/invalid.blob")
        self.assertEqual(response.status, 500)

        class InvalidGuess(SimpleHTTPRequestHandler):
            def guess_type(self, path):
                return None

        self.server.server_close()
        self.thread.join(5)
        self.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(InvalidGuess, directory=self.tmp.name)
        )
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address
        response, _ = self.request("GET", "/invalid.blob")
        self.assertEqual(response.status, 500)

    def test_directory_redirect_index_and_safe_defaults(self):
        response, body = self.request("GET", "/docs?x=1")
        self.assertEqual(response.status, 301)
        self.assertEqual(response.getheader("Location"), "/docs/?x=1")
        self.assertEqual(body, b"")
        response, body = self.request("GET", "/docs/")
        self.assertEqual(response.status, 200)
        self.assertEqual(body, b"index htm\n")
        denied, _ = self.request("GET", "/.secret")
        self.assertEqual(denied.status, 403)

    def test_traversal_is_denied(self):
        response, _ = self.request("GET", "/../hello.txt")
        self.assertIn(response.status, (400, 403))


class ListingHandler(SimpleHTTPRequestHandler):
    directory_listing = True


class ListingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        with open(os.path.join(self.tmp.name, "<x>.txt"), "wb") as stream:
            stream.write(b"x")
        self.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(ListingHandler, directory=self.tmp.name)
        )
        self.server._start()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.address = self.server.server_address

    def tearDown(self):
        self.server.server_close()
        self.thread.join(5)
        self.tmp.cleanup()

    def request(self, method, path, headers=None):
        connection = http.client.HTTPConnection(*self.address, timeout=5)
        connection.request(method, path, headers=headers or {})
        response = connection.getresponse()
        body = response.read()
        connection.close()
        return response, body

    def test_listing_is_escaped_and_head_matches(self):
        response, body = self.request("GET", "/")
        self.assertEqual(response.status, 200)
        self.assertIn(b"&lt;x&gt;.txt", body)
        head, head_body = self.request("HEAD", "/")
        self.assertEqual(head.status, 200)
        self.assertEqual(head.getheader("Content-Length"), str(len(body)))
        self.assertEqual(head_body, b"")
