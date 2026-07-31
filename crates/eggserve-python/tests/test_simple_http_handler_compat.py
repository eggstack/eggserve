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
