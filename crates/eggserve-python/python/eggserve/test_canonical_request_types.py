"""Installed-wheel tests for canonical HTTP request types (Plan 047).

Verifies that the new experimental canonical request types can be imported,
constructed, inspected, and compared from an installed eggserve wheel. These
tests exercise the Python projections of the Rust primitives.
"""

import unittest

from eggserve._native import (
    CanonicalRequest,
    ConnectionInfo,
    DuplicateHeaderError,
    EggserveError,
    HeaderBlock,
    HeaderError,
    HttpVersion,
    HttpVersionError,
    Method,
    MethodError,
    parse_http_version,
    parse_method,
)


class TestMethod(unittest.TestCase):
    def test_construct_get(self):
        m = Method("GET")
        self.assertEqual(m.as_str, "GET")

    def test_construct_extension(self):
        m = Method("PURGE")
        self.assertEqual(m.as_str, "PURGE")

    def test_static_factories(self):
        self.assertEqual(Method.get().as_str, "GET")
        self.assertEqual(Method.head().as_str, "HEAD")
        self.assertEqual(Method.post().as_str, "POST")
        self.assertEqual(Method.put().as_str, "PUT")
        self.assertEqual(Method.delete().as_str, "DELETE")
        self.assertEqual(Method.patch().as_str, "PATCH")

    def test_empty_rejected(self):
        with self.assertRaises(MethodError):
            Method("")

    def test_invalid_token_rejected(self):
        with self.assertRaises(MethodError):
            Method("GET POST")

    def test_is_safe(self):
        self.assertTrue(Method.get().is_safe)
        self.assertTrue(Method.head().is_safe)
        self.assertFalse(Method.post().is_safe)

    def test_is_idempotent(self):
        self.assertTrue(Method.get().is_idempotent)
        self.assertTrue(Method.put().is_idempotent)
        self.assertFalse(Method.post().is_idempotent)

    def test_permits_static_resolution(self):
        self.assertTrue(Method.get().permits_static_resolution)
        self.assertTrue(Method.head().permits_static_resolution)
        self.assertFalse(Method.post().permits_static_resolution)

    def test_frozen(self):
        m = Method("GET")
        with self.assertRaises(AttributeError):
            m.as_str = "POST"  # type: ignore[misc]

    def test_str(self):
        self.assertEqual(str(Method.get()), "GET")

    def test_repr(self):
        self.assertIn("Method", repr(Method.get()))

    def test_eq(self):
        self.assertEqual(Method.get(), Method.get())
        self.assertNotEqual(Method.get(), Method.post())

    def test_hash(self):
        self.assertEqual(hash(Method.get()), hash(Method.get()))
        self.assertNotEqual(hash(Method.get()), hash(Method.post()))

    def test_parse_method_function(self):
        m = parse_method("DELETE")
        self.assertEqual(m.as_str, "DELETE")

    def test_parse_method_invalid(self):
        with self.assertRaises(MethodError):
            parse_method("")


class TestHttpVersion(unittest.TestCase):
    def test_construct_1_1(self):
        v = HttpVersion("HTTP/1.1")
        self.assertEqual(v.major, 1)
        self.assertEqual(v.minor, 1)

    def test_construct_1_0(self):
        v = HttpVersion("HTTP/1.0")
        self.assertEqual(v.major, 1)
        self.assertEqual(v.minor, 0)

    def test_static_factories(self):
        self.assertEqual(HttpVersion.http10().major, 1)
        self.assertEqual(HttpVersion.http10().minor, 0)
        self.assertEqual(HttpVersion.http11().major, 1)
        self.assertEqual(HttpVersion.http11().minor, 1)

    def test_unsupported_rejected(self):
        with self.assertRaises(HttpVersionError):
            HttpVersion("HTTP/2.0")

    def test_frozen(self):
        v = HttpVersion("HTTP/1.1")
        with self.assertRaises(AttributeError):
            v.major = 2  # type: ignore[misc]

    def test_str(self):
        self.assertEqual(str(HttpVersion("HTTP/1.1")), "HTTP/1.1")

    def test_repr(self):
        self.assertIn("HttpVersion", repr(HttpVersion("HTTP/1.1")))

    def test_eq(self):
        self.assertEqual(HttpVersion("HTTP/1.1"), HttpVersion("HTTP/1.1"))
        self.assertNotEqual(HttpVersion("HTTP/1.0"), HttpVersion("HTTP/1.1"))

    def test_hash(self):
        self.assertEqual(hash(HttpVersion("HTTP/1.1")), hash(HttpVersion("HTTP/1.1")))

    def test_parse_http_version_function(self):
        v = parse_http_version("HTTP/1.0")
        self.assertEqual(v.minor, 0)

    def test_parse_http_version_invalid(self):
        with self.assertRaises(HttpVersionError):
            parse_http_version("HTTP/2.0")


class TestHeaderBlock(unittest.TestCase):
    def test_construct_empty(self):
        hb = HeaderBlock()
        self.assertTrue(hb.is_empty)
        self.assertEqual(hb.len, 0)

    def test_construct_with_fields(self):
        hb = HeaderBlock([("Content-Type", "text/html"), ("X-Custom", "val")])
        self.assertEqual(hb.len, 2)

    def test_get_first(self):
        hb = HeaderBlock([("Content-Type", "text/html")])
        self.assertEqual(hb.get_first("content-type"), "text/html")
        self.assertEqual(hb.get_first("CONTENT-TYPE"), "text/html")

    def test_get_first_missing(self):
        hb = HeaderBlock()
        self.assertIsNone(hb.get_first("missing"))

    def test_get_all(self):
        hb = HeaderBlock([("Set-Cookie", "a=1"), ("Set-Cookie", "b=2")])
        all_vals = hb.get_all("set-cookie")
        self.assertEqual(all_vals, ["a=1", "b=2"])

    def test_get_unique_single(self):
        hb = HeaderBlock([("Content-Type", "text/html")])
        self.assertEqual(hb.get_unique("content-type"), "text/html")

    def test_get_unique_absent(self):
        hb = HeaderBlock()
        self.assertIsNone(hb.get_unique("missing"))

    def test_get_unique_duplicate_error(self):
        hb = HeaderBlock([("Set-Cookie", "a=1"), ("Set-Cookie", "b=2")])
        with self.assertRaises(DuplicateHeaderError):
            hb.get_unique("set-cookie")

    def test_contains(self):
        hb = HeaderBlock([("Content-Type", "text/html")])
        self.assertTrue(hb.contains("content-type"))
        self.assertTrue(hb.contains("CONTENT-TYPE"))
        self.assertFalse(hb.contains("missing"))

    def test_iteration(self):
        hb = HeaderBlock([("a", "1"), ("b", "2")])
        pairs = list(hb)
        self.assertEqual(len(pairs), 2)
        self.assertEqual(pairs[0], ("a", "1"))
        self.assertEqual(pairs[1], ("b", "2"))

    def test_frozen(self):
        hb = HeaderBlock()
        with self.assertRaises(AttributeError):
            hb.fields = []  # type: ignore[misc]

    def test_invalid_header_name(self):
        with self.assertRaises(HeaderError):
            HeaderBlock([("", "value")])

    def test_invalid_header_value_cr(self):
        with self.assertRaises(HeaderError):
            HeaderBlock([("X-Test", "foo\rbar")])

    def test_invalid_header_value_lf(self):
        with self.assertRaises(HeaderError):
            HeaderBlock([("X-Test", "foo\nbar")])

    def test_repr(self):
        hb = HeaderBlock([("a", "1")])
        self.assertIn("HeaderBlock", repr(hb))


class TestConnectionInfo(unittest.TestCase):
    def test_construct_http(self):
        ci = ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345")
        self.assertEqual(ci.local_addr, "127.0.0.1:8000")
        self.assertEqual(ci.remote_addr, "127.0.0.1:12345")
        self.assertEqual(ci.scheme, "http")
        self.assertFalse(ci.is_tls)

    def test_construct_https(self):
        ci = ConnectionInfo(
            "0.0.0.0:443",
            "10.0.0.1:54321",
            scheme="https",
            tls_protocol_version="TLSv1.3",
            tls_server_name="example.com",
        )
        self.assertEqual(ci.scheme, "https")
        self.assertTrue(ci.is_tls)
        self.assertEqual(ci.tls_protocol_version, "TLSv1.3")
        self.assertEqual(ci.tls_server_name, "example.com")

    def test_invalid_scheme(self):
        with self.assertRaises(ValueError):
            ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345", scheme="ftp")

    def test_frozen(self):
        ci = ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345")
        with self.assertRaises(AttributeError):
            ci.scheme = "https"  # type: ignore[misc]

    def test_repr(self):
        ci = ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345")
        self.assertIn("ConnectionInfo", repr(ci))


class TestCanonicalRequest(unittest.TestCase):
    def test_construct_minimal(self):
        cr = CanonicalRequest(method="GET", path="/")
        self.assertEqual(cr.method, "GET")
        self.assertEqual(cr.path, "/")
        self.assertEqual(cr.version, "HTTP/1.1")
        self.assertIsNone(cr.query)
        self.assertTrue(cr.is_get)
        self.assertFalse(cr.is_head)

    def test_construct_with_query(self):
        cr = CanonicalRequest(method="GET", path="/foo", query="bar=baz")
        self.assertEqual(cr.query, "bar=baz")

    def test_construct_with_headers(self):
        cr = CanonicalRequest(
            method="GET",
            path="/",
            headers=[("Content-Type", "text/html"), ("X-Custom", "val")],
        )
        self.assertEqual(len(cr.headers), 2)

    def test_head_request(self):
        cr = CanonicalRequest(method="HEAD", path="/")
        self.assertTrue(cr.is_head)
        self.assertFalse(cr.is_get)

    def test_header_block_conversion(self):
        cr = CanonicalRequest(
            method="GET",
            path="/",
            headers=[("Set-Cookie", "a=1"), ("Set-Cookie", "b=2")],
        )
        hb = cr.header_block()
        self.assertEqual(hb.len, 2)
        all_vals = hb.get_all("set-cookie")
        self.assertEqual(all_vals, ["a=1", "b=2"])

    def test_invalid_method(self):
        with self.assertRaises(MethodError):
            CanonicalRequest(method="", path="/")

    def test_invalid_version(self):
        with self.assertRaises(HttpVersionError):
            CanonicalRequest(method="GET", path="/", version="HTTP/2.0")

    def test_path_must_start_with_slash(self):
        with self.assertRaises(ValueError):
            CanonicalRequest(method="GET", path="foo")

    def test_connection_info_fields(self):
        cr = CanonicalRequest(
            method="GET",
            path="/",
            remote_addr="127.0.0.1:12345",
            local_addr="127.0.0.1:8000",
            scheme="https",
        )
        self.assertEqual(cr.remote_addr, "127.0.0.1:12345")
        self.assertEqual(cr.local_addr, "127.0.0.1:8000")
        self.assertEqual(cr.scheme, "https")

    def test_frozen(self):
        cr = CanonicalRequest(method="GET", path="/")
        with self.assertRaises(AttributeError):
            cr.method = "POST"  # type: ignore[misc]

    def test_repr(self):
        cr = CanonicalRequest(method="GET", path="/test")
        self.assertIn("CanonicalRequest", repr(cr))


class TestExceptionHierarchy(unittest.TestCase):
    def test_method_error_is_eggserve_error(self):
        self.assertTrue(issubclass(MethodError, EggserveError))

    def test_http_version_error_is_eggserve_error(self):
        self.assertTrue(issubclass(HttpVersionError, EggserveError))

    def test_header_error_is_eggserve_error(self):
        self.assertTrue(issubclass(HeaderError, EggserveError))

    def test_duplicate_header_error_is_eggserve_error(self):
        self.assertTrue(issubclass(DuplicateHeaderError, EggserveError))


if __name__ == "__main__":
    unittest.main()
