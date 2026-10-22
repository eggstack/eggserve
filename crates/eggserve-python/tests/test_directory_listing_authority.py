"""Plan 311: the Python facade serves the static authority's listing bytes.

The Python bridge no longer renders directory listings itself. It calls the
`eggserve-static` authority for both the bytes and the byte ceiling, so the
Python surface is byte-identical to the Rust surface and enforces the same
denial-of-service bound on the same directories.
"""

import functools
import http.client
import os
import tempfile
import threading
import unittest

from eggserve._native import ServerSecureRoot, StaticPolicyWrapper, StaticResponder
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


# The Rust surface's listing wrapper, byte for byte.
RUST_LISTING_PREFIX = (
    b"<!DOCTYPE html>\n"
    b"<html>\n"
    b"<head>\n"
    b'<meta charset="utf-8">\n'
    b"<title>Directory listing</title>\n"
    b"</head>\n"
    b"<body>\n"
    b"<h1>Directory listing</h1>\n"
    b"<ul>\n"
)
RUST_LISTING_SUFFIX = b"</ul>\n</body>\n</html>\n"

# The authority's default listing byte ceiling (1 MiB) and entry ceiling.
LISTING_BYTE_CEILING = 1024 * 1024
LISTING_ENTRY_CEILING = 4096

INDEX_PAGES = ["index.html", "index.htm"]

# A long name renders as ~526 bytes per link, so this many entries push the
# rendered listing past the 1 MiB byte ceiling.
OVERSIZED_ENTRY_COUNT = 2100


def _responder(root, directory_listing=True):
    policy = StaticPolicyWrapper(directory_listing=directory_listing)
    return StaticResponder(ServerSecureRoot(root, policy=policy))


class ListingAuthorityTests(unittest.TestCase):
    """The bridge delegates listing rendering to the static authority."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def _write(self, name, payload=b"x"):
        with open(os.path.join(self.root, name), "wb") as stream:
            stream.write(payload)

    def test_listing_bytes_are_identical_to_the_rust_surface(self):
        self._write("a.txt")
        self._write("b&c.txt")
        os.mkdir(os.path.join(self.root, "sub"))

        response = _responder(self.root).respond("GET", "/", index_pages=INDEX_PAGES)
        self.assertEqual(response.status, 200)
        self.assertEqual(response.headers["content-type"], "text/html; charset=utf-8")
        body = response.body.read_all()
        expected = (
            RUST_LISTING_PREFIX
            + b'<li><a href="a.txt">a.txt</a></li>\n'
            + b'<li><a href="b%26c.txt">b&amp;c.txt</a></li>\n'
            + b'<li><a href="sub/">sub/</a></li>\n'
            + RUST_LISTING_SUFFIX
        )
        self.assertEqual(body, expected)

    def test_listing_requires_opt_in(self):
        self._write("a.txt")
        response = _responder(self.root, directory_listing=False).respond(
            "GET", "/", index_pages=INDEX_PAGES
        )
        self.assertEqual(response.status, 403)
        body = response.body.read_all()
        self.assertNotIn(b"<title>Directory listing", body)

    def test_oversized_listing_is_refused_with_a_clean_exception(self):
        for index in range(OVERSIZED_ENTRY_COUNT):
            self._write(f"{index:05d}" + "x" * 245)

        with self.assertRaises(RuntimeError) as caught:
            _responder(self.root).respond("GET", "/", index_pages=INDEX_PAGES)
        message = str(caught.exception)
        self.assertIn("directory listing", message)
        self.assertIn("exceeds configured bound", message)


class ListingHandler(SimpleHTTPRequestHandler):
    directory_listing = True


class ListingFacadeTests(unittest.TestCase):
    """The HTTP facade serves the authority's bytes and the same refusals."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def _write(self, name, payload=b"x"):
        with open(os.path.join(self.root, name), "wb") as stream:
            stream.write(payload)

    def _serve(self, handler_class):
        server = ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(handler_class, directory=self.root)
        )
        server._start()
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(thread.join, 5)
        self.addCleanup(server.server_close)
        return server.server_address

    def _request(self, address, method, path):
        connection = http.client.HTTPConnection(*address, timeout=30)
        connection.request(method, path, headers={})
        response = connection.getresponse()
        body = response.read()
        connection.close()
        return response, body

    def test_facade_listing_is_byte_identical_to_the_rust_surface(self):
        self._write("a.txt")
        self._write("b&c.txt")
        os.mkdir(os.path.join(self.root, "sub"))

        address = self._serve(ListingHandler)
        response, body = self._request(address, "GET", "/")
        self.assertEqual(response.status, 200)
        self.assertEqual(response.getheader("Content-Type"), "text/html; charset=utf-8")
        expected = (
            RUST_LISTING_PREFIX
            + b'<li><a href="a.txt">a.txt</a></li>\n'
            + b'<li><a href="b%26c.txt">b&amp;c.txt</a></li>\n'
            + b'<li><a href="sub/">sub/</a></li>\n'
            + RUST_LISTING_SUFFIX
        )
        self.assertEqual(body, expected)

    def test_facade_listing_requires_opt_in(self):
        self._write("a.txt")
        address = self._serve(SimpleHTTPRequestHandler)
        response, body = self._request(address, "GET", "/")
        self.assertIn(response.status, (403, 404))
        self.assertNotIn(b"<title>Directory listing", body)

    def test_facade_refuses_an_oversized_listing(self):
        for index in range(OVERSIZED_ENTRY_COUNT):
            self._write(f"{index:05d}" + "x" * 245)
        self._write("a.txt")

        address = self._serve(ListingHandler)
        response, body = self._request(address, "GET", "/")
        # The refusal is a clean failure: no listing body, no oversized
        # response, and no panic (the server keeps serving).
        self.assertEqual(response.status, 500)
        self.assertNotIn(b"<title>Directory listing", body)
        self.assertLess(len(body), LISTING_BYTE_CEILING)

        # The process survived the refusal and still serves requests.
        served, served_body = self._request(address, "GET", "/a.txt")
        self.assertEqual(served.status, 200)
        self.assertEqual(served_body, b"x")
