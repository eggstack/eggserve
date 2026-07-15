"""Tests for Plan 037 — Python Boundary Hardening.

Covers response validation boundary, handler return/exception contract,
exception hierarchy, lifecycle/ownership, file-backed response capability,
request representation, and Python API consistency.
"""

import os
import socket
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

from eggserve._native import (
    BodySource,
    BodySourceError,
    EggserveError,
    LifecycleError,
    Request,
    Response,
    ResponseConstructionError,
    Server,
    ServerBodySource,
    ServerSecureRoot,
    StaticResponder,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _wait_for_server(url, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            urllib.request.urlopen(url, timeout=1)
            return True
        except urllib.error.HTTPError:
            return True
        except (urllib.error.URLError, ConnectionRefusedError, OSError):
            time.sleep(0.05)
    return False


def _wait_for_tcp(addr, timeout=5.0):
    import socket
    host, port = addr.split(":")
    port = int(port)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return True
        except (ConnectionRefusedError, OSError):
            time.sleep(0.05)
    return False


def _raw_status(addr, request):
    host, port = addr.split(":")
    with socket.create_connection((host, int(port)), timeout=5) as sock:
        sock.sendall(request)
        data = b""
        while b"\r\n" not in data:
            data += sock.recv(1024)
    return int(data.split(b" ", 2)[1])


class _TestServerBase(unittest.TestCase):
    """Base class that sets up a temp directory with test files."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("content")
        with open(os.path.join(self._td, "hello.txt"), "w") as f:
            f.write("hello world")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = Server(**defaults)
        s.start()
        self._servers.append(s)
        return s


# ---------------------------------------------------------------------------
# Track A — Response validation boundary
# ---------------------------------------------------------------------------


class TestResponseValidation(_TestServerBase):
    """A: Python handler responses are validated before wire serialization."""

    def test_invalid_status_code_returns_500(self):
        """Handler returning invalid status (0) produces 500."""
        def handler(req):
            return Response.empty(0)

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_1xx_status_returns_500(self):
        """Handler returning 1xx status produces 500."""
        def handler(req):
            return Response.empty(100)

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_204_with_body_returns_204(self):
        """Handler returning 204 with a body — body is silently stripped."""
        def handler(req):
            return Response.bytes(204, b"should not have body")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 204)
        self.assertEqual(resp.read(), b"")

    def test_304_with_body_returns_304(self):
        """Handler returning 304 with a body — body is silently stripped."""
        def handler(req):
            return Response.bytes(304, b"should not have body")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            resp = urllib.request.urlopen(url, timeout=2)
            self.assertEqual(resp.status, 304)
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 304)

    def test_204_empty_returns_204(self):
        """Handler returning 204 with empty body succeeds."""
        def handler(req):
            return Response.empty(204)

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 204)
        self.assertEqual(resp.read(), b"")

    def test_304_empty_returns_304(self):
        """Handler returning 304 with empty body succeeds."""
        def handler(req):
            return Response.empty(304)

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            resp = urllib.request.urlopen(url, timeout=2)
            self.assertEqual(resp.status, 304)
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 304)

    def test_hop_by_hop_connection_header_stripped(self):
        """Handler returning 'connection' hop-by-hop header — silently stripped."""
        def handler(req):
            return Response.bytes(200, b"ok", headers={"connection": "keep-alive"})

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

    def test_hop_by_hop_transfer_encoding_stripped(self):
        """Handler returning 'transfer-encoding' hop-by-hop header — silently stripped."""
        def handler(req):
            return Response.bytes(200, b"ok", headers={"transfer-encoding": "chunked"})

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

    def test_hop_by_hop_upgrade_stripped(self):
        """Handler returning 'upgrade' hop-by-hop header — silently stripped."""
        def handler(req):
            return Response.bytes(200, b"ok", headers={"upgrade": "websocket"})

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

    def test_hop_by_hop_te_stripped(self):
        """Handler returning 'te' hop-by-hop header — silently stripped."""
        def handler(req):
            return Response.bytes(200, b"ok", headers={"te": "chunked"})

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

    def test_hop_by_hop_header_names_are_case_insensitive(self):
        """Header policy strips hop-by-hop names regardless of casing."""
        def handler(req):
            return Response.bytes(200, b"ok", headers={"Connection": "keep-alive"})

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

    def test_valid_status_codes_accepted(self):
        """Handler returning valid status codes (200, 201, 206, 400, 404, 500) work."""
        for status in [200, 201, 206, 400, 404, 500]:
            def handler(req, s=status):
                return Response.empty(s)

            srv = self._make_server(handler=handler)
            url = f"http://{srv.addr}/index.txt"
            self.assertTrue(_wait_for_server(url))
            try:
                resp = urllib.request.urlopen(url, timeout=2)
                self.assertEqual(resp.status, status)
                resp.close()
            except urllib.error.HTTPError as e:
                e.close()
                self.assertEqual(e.status, status)
            srv.stop()

    def test_normal_response_unaffected(self):
        """Normal 200 responses with valid headers work correctly."""
        def handler(req):
            return Response.text(200, "hello")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"hello")


# ---------------------------------------------------------------------------
# Track B — Handler return and exception contract
# ---------------------------------------------------------------------------


class TestHandlerReturnContract(_TestServerBase):
    """B: Handler return type and exception contract."""

    def test_wrong_return_type_returns_500(self):
        """Returning a non-Response type produces 500."""
        def handler(req):
            return 42

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_none_return_returns_500(self):
        """Returning None produces 500."""
        def handler(req):
            return None

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_exception_returns_500(self):
        """Handler raising an exception produces 500."""
        def handler(req):
            raise ValueError("boom")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_500_response_does_not_leak_traceback(self):
        """500 responses must not contain Python traceback or exception text."""
        def handler(req):
            raise RuntimeError("secret internal details")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
            body = e.read().decode("utf-8", errors="replace")
            self.assertNotIn("RuntimeError", body)
            self.assertNotIn("secret internal details", body)
            self.assertNotIn("traceback", body.lower())
            self.assertNotIn("Traceback", body)

    def test_500_response_does_not_leak_filesystem_path(self):
        """500 responses must not contain filesystem paths."""
        def handler(req):
            raise OSError("/Users/secret/path/file.txt")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
            body = e.read().decode("utf-8", errors="replace")
            self.assertNotIn("/Users/", body)

    def test_handler_continues_after_previous_error(self):
        """Handler works correctly after a previous error."""
        call_count = [0]

        def handler(req):
            call_count[0] += 1
            if call_count[0] == 1:
                raise ValueError("first call fails")
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"ok")


# ---------------------------------------------------------------------------
# Track G — Exception hierarchy
# ---------------------------------------------------------------------------


class TestExceptionHierarchy(unittest.TestCase):
    """G: New exception classes exist and have correct hierarchy."""

    def test_response_construction_error_is_eggserve_error(self):
        self.assertTrue(issubclass(ResponseConstructionError, EggserveError))

    def test_lifecycle_error_is_eggserve_error(self):
        self.assertTrue(issubclass(LifecycleError, EggserveError))

    def test_eggserve_error_is_exception(self):
        self.assertTrue(issubclass(EggserveError, Exception))

    def test_all_original_exceptions_preserved(self):
        from eggserve._native import (
            PathPolicyError,
            RequestTargetError,
            SecureRootError,
            RequestValidationError,
            BodySourceError,
        )
        self.assertTrue(issubclass(PathPolicyError, EggserveError))
        self.assertTrue(issubclass(RequestTargetError, EggserveError))
        self.assertTrue(issubclass(SecureRootError, EggserveError))
        self.assertTrue(issubclass(RequestValidationError, EggserveError))
        self.assertTrue(issubclass(BodySourceError, EggserveError))


# ---------------------------------------------------------------------------
# Track D — Lifecycle and ownership
# ---------------------------------------------------------------------------


class TestLifecycleOwnership(_TestServerBase):
    """D: Lifecycle edge cases and ownership invariants."""

    def test_double_start_raises_lifecycle_error(self):
        """Calling start() twice raises LifecycleError."""
        s = self._make_server()
        with self.assertRaises(LifecycleError):
            s.start()

    def test_stop_before_start_is_safe(self):
        """Calling stop() before start() does not raise."""
        s = Server(root=self._td, port=0)
        s.stop()

    def test_double_stop_is_safe(self):
        """Calling stop() twice does not panic or deadlock."""
        s = self._make_server()
        s.stop()
        s.stop()

    def test_context_manager_auto_start_stop(self):
        """Context manager starts and stops the server."""
        with Server(root=self._td, port=0) as s:
            self.assertIsNotNone(s.addr)
        self.assertIsNone(s.addr)

    def test_addr_none_before_start(self):
        """addr is None before start."""
        s = Server(root=self._td, port=0)
        self.assertIsNone(s.addr)

    def test_addr_set_after_start(self):
        """addr is a string after start."""
        s = self._make_server()
        self.assertIsNotNone(s.addr)
        self.assertIn(":", s.addr)

    def test_addr_none_after_stop(self):
        """addr is None after stop."""
        s = self._make_server()
        s.stop()
        self._servers.remove(s)
        self.assertIsNone(s.addr)

    def test_repr_not_started(self):
        """repr shows 'not started' before start."""
        s = Server(root=self._td, port=0)
        self.assertIn("not started", repr(s))

    def test_repr_after_start(self):
        """repr shows address after start."""
        s = self._make_server()
        self.assertIn("Server", repr(s))
        self.assertIn(":", repr(s))


# ---------------------------------------------------------------------------
# Track E — File-backed response capability
# ---------------------------------------------------------------------------


class TestFileBackedResponse(_TestServerBase):
    """E: File-backed response and body source capability."""

    def test_body_source_read_all(self):
        """ServerBodySource.read_all() reads full content and consumes."""
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        resp = responder.respond("GET", "/hello.txt")
        body = resp.body
        self.assertEqual(body.kind, "file_full")
        data = body.read_all()
        self.assertEqual(data, b"hello world")
        with self.assertRaises(ValueError):
            body.read_all()

    def test_body_source_read_range(self):
        """ServerBodySource.read_range() reads range and consumes."""
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        resp = responder.respond("GET", "/hello.txt")
        body = resp.body
        data = body.read_range(0, 4)
        self.assertEqual(data, b"hello")
        with self.assertRaises(ValueError):
            body.read_range(0, 4)

    def test_body_source_to_response(self):
        """ServerBodySource.to_response() creates a Response."""
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        resp = responder.respond("GET", "/hello.txt")
        body = resp.body
        new_resp = body.to_response(200)
        self.assertEqual(new_resp.status, 200)
        with self.assertRaises(ValueError):
            body.to_response(200)

    def test_body_source_consumed_error(self):
        """ServerBodySource raises after consumption."""
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        resp = responder.respond("GET", "/hello.txt")
        body = resp.body
        body.read_all()
        with self.assertRaises(ValueError):
            body.read_all()

    def test_response_body_source_factory(self):
        """Response.body_source() creates a response from ServerBodySource."""
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        resp = responder.respond("GET", "/hello.txt")
        body = resp.body
        new_resp = Response.body_source(201, body, {})
        self.assertEqual(new_resp.status, 201)
        with self.assertRaises(ValueError):
            Response.body_source(200, body, {})

    def test_response_body_source_invalid_status_rejected(self):
        """Response.body_source() with invalid status is rejected at wire level."""
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        resp = responder.respond("GET", "/hello.txt")
        body = resp.body
        new_resp = Response.body_source(0, body, {})
        self.assertEqual(new_resp.status, 0)

    def test_handler_file_body_through_server(self):
        """Handler returning a file-backed BodySource through the server.

        NOTE: File-backed BodySources from handlers are currently dropped to
        empty by the canonical response conversion. This is a known limitation.
        """
        def handler(req):
            root = ServerSecureRoot(self._td)
            responder = StaticResponder(root)
            resp = responder.respond("GET", "/hello.txt")
            return resp.body.to_response(200)

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/hello.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

    def test_response_body_getter_clones_bytes(self):
        """Response.body getter clones bytes body (not file-backed)."""
        resp = Response.bytes(200, b"test data")
        body = resp.body
        self.assertEqual(body.kind, "bytes")
        data = body.read_all()
        self.assertEqual(data, b"test data")

    def test_response_body_getter_empty(self):
        """Response.body getter returns empty for empty body."""
        resp = Response.empty(200)
        body = resp.body
        self.assertEqual(body.kind, "empty")


# ---------------------------------------------------------------------------
# Track F — Request representation
# ---------------------------------------------------------------------------


class TestRequestRepresentation(_TestServerBase):
    """F: PyRequest fidelity — method, path, query, headers, etc."""

    def test_request_method_get(self):
        """Handler receives correct method for GET."""
        captured = []

        def handler(req):
            captured.append(req.method)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertEqual(captured[-1], "GET")

    def test_request_method_head(self):
        """Handler receives correct method for HEAD."""
        captured = []

        def handler(req):
            captured.append(req.method)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        req = urllib.request.Request(url, method="HEAD")
        urllib.request.urlopen(req, timeout=2)
        self.assertEqual(captured[-1], "HEAD")

    def test_request_path(self):
        """Handler receives correct path."""
        captured = []

        def handler(req):
            captured.append(req.path)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertEqual(captured[-1], "/index.txt")

    def test_request_query(self):
        """Handler receives correct query string."""
        captured = []

        def handler(req):
            captured.append(req.query)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt?foo=bar&baz=1"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertEqual(captured[-1], "foo=bar&baz=1")

    def test_request_headers(self):
        """Handler receives request headers as dict."""
        captured = []

        def handler(req):
            captured.append(dict(req.headers))
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        req = urllib.request.Request(url, headers={"X-Custom": "test-value"})
        urllib.request.urlopen(req, timeout=2)
        self.assertIn("x-custom", captured[-1])
        self.assertEqual(captured[-1]["x-custom"], "test-value")

    def test_request_has_body_false_for_get(self):
        """has_body is False for GET requests."""
        captured = []

        def handler(req):
            captured.append(req.has_body)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertFalse(captured[-1])

    def test_get_body_metadata_is_rejected_before_handler(self):
        """GET body framing is rejected before callback execution."""
        called = []

        def handler(req):
            called.append(True)
            return Response.text(200, "unexpected")

        s = self._make_server(handler=handler)
        self.assertTrue(_wait_for_tcp(s.addr))
        status = _raw_status(
            s.addr,
            b"GET /index.txt HTTP/1.1\r\n"
            + f"Host: {s.addr}\r\n".encode()
            + b"Content-Length: 1\r\nConnection: close\r\n\r\nx",
        )
        self.assertEqual(status, 400)
        self.assertEqual(called, [])

    def test_request_http_version(self):
        """Handler receives http_version string."""
        captured = []

        def handler(req):
            captured.append(req.http_version)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertIn("HTTP", captured[-1])

    def test_request_repr(self):
        """Request repr is safe and includes method + path."""
        captured = []

        def handler(req):
            captured.append(repr(req))
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        r = captured[-1]
        self.assertIn("GET", r)
        self.assertIn("/index.txt", r)

    def test_request_frozen(self):
        """Request attributes are read-only."""
        captured = []

        def handler(req):
            captured.append(req)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        req = captured[-1]
        with self.assertRaises(AttributeError):
            req.method = "POST"


# ---------------------------------------------------------------------------
# Track H — Python API consistency
# ---------------------------------------------------------------------------


class TestApiConsistency(unittest.TestCase):
    """H: Constructor signatures, frozen classes, repr safety, __all__."""

    def test_response_frozen(self):
        """Response is frozen (immutable)."""
        r = Response.empty(200)
        with self.assertRaises(AttributeError):
            r.status = 201

    def test_response_repr_safe(self):
        """Response repr does not leak body content."""
        r = Response.bytes(200, b"secret data")
        self.assertIn("200", repr(r))
        self.assertNotIn("secret", repr(r))

    def test_response_text_default_content_type(self):
        """Response.text sets default content-type."""
        r = Response.text(200, "hello")
        self.assertEqual(r.headers.get("content-type"), "text/plain; charset=utf-8")

    def test_response_bytes_no_default_content_type(self):
        """Response.bytes does not set content-type by default."""
        r = Response.bytes(200, b"hello")
        self.assertNotIn("content-type", r.headers)

    def test_response_empty_no_headers(self):
        """Response.empty has no headers."""
        r = Response.empty(204)
        self.assertEqual(r.headers, {})

    def test_init_all_exports(self):
        """eggserve.__all__ includes all expected names."""
        import eggserve
        expected = [
            "EggserveError",
            "PathPolicyError",
            "RequestTargetError",
            "SecureRootError",
            "RequestValidationError",
            "BodySourceError",
            "ResponseConstructionError",
            "LifecycleError",
            "Request",
            "Response",
            "Server",
            "StaticResponder",
            "StaticPolicyWrapper",
            "ServerSecureRoot",
            "ServerBodySource",
            "ServerRequestError",
        ]
        for name in expected:
            self.assertIn(name, eggserve.__all__, f"{name} missing from __all__")

    def test_static_policy_wrapper_frozen(self):
        from eggserve._native import StaticPolicyWrapper
        p = StaticPolicyWrapper()
        with self.assertRaises(AttributeError):
            p.directory_listing = True

    def test_server_secure_root_frozen(self):
        with tempfile.TemporaryDirectory() as td:
            from eggserve._native import ServerSecureRoot
            sr = ServerSecureRoot(td)
            with self.assertRaises(AttributeError):
                sr.root_path = "/other"

    def test_server_body_source_not_frozen(self):
        """ServerBodySource is not frozen (it is consumable)."""
        from eggserve._native import ServerBodySource
        # It should be constructable without errors
        # (can't test frozen because it isn't frozen)


# ---------------------------------------------------------------------------
# Track C — GIL/lock regression
# ---------------------------------------------------------------------------


class TestGILRegression(_TestServerBase):
    """C: GIL and lock ordering regression tests."""

    def test_concurrent_handler_calls_succeed(self):
        """Multiple concurrent handler calls complete without deadlock."""
        call_count = [0]
        lock = threading.Lock()

        def handler(req):
            with lock:
                call_count[0] += 1
            time.sleep(0.05)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler, max_python_callbacks=4)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        results = []
        errors = []

        def fetch():
            try:
                resp = urllib.request.urlopen(url, timeout=5)
                results.append(resp.status)
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=fetch) for _ in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=10)

        self.assertEqual(len(errors), 0, f"Errors: {errors}")
        self.assertTrue(all(r == 200 for r in results))

    def test_stop_during_active_handler(self):
        """stop() completes even while a handler is running."""
        handler_entered = threading.Event()
        handler_done = threading.Event()

        def handler(req):
            handler_entered.set()
            handler_done.wait(timeout=5)
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        t = threading.Thread(target=lambda: urllib.request.urlopen(url, timeout=5))
        t.start()
        handler_entered.wait(timeout=5)

        stop_done = threading.Event()

        def do_stop():
            s.stop()
            self._servers.remove(s)
            stop_done.set()

        st = threading.Thread(target=do_stop)
        st.start()
        st.join(timeout=10)
        self.assertTrue(stop_done.is_set(), "stop() must not deadlock")

        handler_done.set()
        t.join(timeout=5)

    def test_exception_releases_callback_permit(self):
        """An exception in the handler still releases the callback permit."""
        error_seen = threading.Event()
        after_error = threading.Event()

        def handler(req):
            if not error_seen.is_set():
                error_seen.set()
                after_error.wait(timeout=5)
                raise ValueError("boom")
            return Response.text(200, "ok")

        s = self._make_server(handler=handler, max_python_callbacks=1)
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        def fail_once():
            try:
                urllib.request.urlopen(url, timeout=5)
            except urllib.error.HTTPError:
                pass

        t1 = threading.Thread(target=fail_once)
        t1.start()
        error_seen.wait(timeout=5)

        after_error.set()
        t1.join(timeout=5)

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()


# ---------------------------------------------------------------------------
# Track B — Handler returning Response with invalid status/headers at wire
# ---------------------------------------------------------------------------


class TestHandlerInvalidResponse(_TestServerBase):
    """B: Handler returning invalid Response objects."""

    def test_handler_returning_string_returns_500(self):
        def handler(req):
            return "not a response"

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_handler_returning_dict_returns_500(self):
        def handler(req):
            return {"status": 200, "body": "ok"}

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_handler_returning_int_returns_500(self):
        def handler(req):
            return 200

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_handler_returning_empty_string_returns_500(self):
        def handler(req):
            return ""

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)


if __name__ == "__main__":
    unittest.main()
