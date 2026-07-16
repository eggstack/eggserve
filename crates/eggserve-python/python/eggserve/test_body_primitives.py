"""Tests for eggserve request body primitives.

Uses stdlib unittest to validate RequestBody properties, read/iter_chunks,
one-shot enforcement, empty body behavior, body error hierarchy, Server
constructor body policy parameters, and Request.has_body/body accessors.
"""

import os
import shutil
import socket
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

try:
    from eggserve._native import (
        BodyChunkIterator,
        EggserveError,
        Request,
        RequestBody,
        RequestBodyCancelledError,
        RequestBodyConsumedError,
        RequestBodyDisconnectedError,
        RequestBodyError,
        RequestBodyIncompleteError,
        RequestBodyRejectedError,
        RequestBodyTimeoutError,
        RequestBodyTooLargeError,
        Response,
        Server,
    )

    NATIVE_AVAILABLE = True
except ImportError:
    NATIVE_AVAILABLE = False


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


def _send_post(addr, path, body=b"", headers=None, keep_open=False):
    """Send a raw POST request. Closes socket unless keep_open=True."""
    host, port_str = addr.split(":")
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(5)
    sock.connect((host, int(port_str)))
    hdrs = headers or {}
    hdrs.setdefault("Host", addr)
    hdrs.setdefault("Content-Length", str(len(body)))
    hdrs.setdefault("Connection", "close")
    req_line = f"POST {path} HTTP/1.1\r\n"
    header_lines = "".join(f"{k}: {v}\r\n" for k, v in hdrs.items())
    sock.sendall((req_line + header_lines + "\r\n").encode())
    if body:
        sock.sendall(body)
    if not keep_open:
        # Wait for the response before closing to ensure the server
        # has fully processed the request.
        try:
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
        except (socket.timeout, OSError):
            pass
        sock.close()
    return sock


def _read_response(sock):
    """Read a complete HTTP response from the socket."""
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(4096)
        if not chunk:
            break
        data += chunk
    header_end = data.index(b"\r\n\r\n") + 4
    headers_raw = data[:header_end].decode()
    status_line = headers_raw.split("\r\n")[0]
    status_code = int(status_line.split(" ", 1)[1].split(" ")[0])
    body = data[header_end:]
    while True:
        try:
            chunk = sock.recv(4096)
            if not chunk:
                break
            body += chunk
        except (socket.timeout, OSError):
            break
    return status_code, body, headers_raw


# ---------------------------------------------------------------------------
# Test body properties via real server with buffer mode
# ---------------------------------------------------------------------------


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyProperties(unittest.TestCase):
    """RequestBody properties: declared_length, bytes_received, complete."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}
        self._server_ready = threading.Event()

        def handler(req):
            if req.method == "POST" and req.has_body:
                body = req.body
                self._captured["declared_length"] = body.declared_length
                self._captured["bytes_received"] = body.bytes_received
                self._captured["complete"] = body.complete
                data = body.read()
                self._captured["read_data"] = data
                self._captured["after_read_bytes"] = body.bytes_received
                self._captured["after_read_complete"] = body.complete
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_declared_length_matches_content_length(self):
        body = b"hello world"
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.2)
        self.assertEqual(self._captured["declared_length"], len(body))

    def test_bytes_received_after_read(self):
        body = b"hello world"
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.2)
        self.assertEqual(self._captured["after_read_bytes"], len(body))

    def test_complete_after_read(self):
        body = b"hello world"
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.2)
        self.assertTrue(self._captured["after_read_complete"])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyRead(unittest.TestCase):
    """RequestBody.read() returns the full body bytes."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.method == "POST" and req.has_body:
                data = req.body.read()
                self._captured["read_data"] = data
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=65536,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_read_returns_body_bytes(self):
        body = b"hello world"
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.2)
        self.assertEqual(self._captured["read_data"], body)

    def test_read_returns_empty_bytes_for_empty_body(self):
        _send_post(self._addr, "/index.txt", b"")
        time.sleep(0.2)
        self.assertEqual(self._captured["read_data"], b"")

    def test_read_returns_unicode_body(self):
        body = "hello \u00e9\u00e8\u00ea".encode("utf-8")
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.2)
        self.assertEqual(self._captured["read_data"], body)

    def test_read_returns_binary_body(self):
        body = bytes(range(256))
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.2)
        self.assertEqual(self._captured["read_data"], body)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyIterChunks(unittest.TestCase):
    """RequestBody.iter_chunks() returns a BodyChunkIterator."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.method == "POST" and req.has_body:
                chunks = []
                it = req.body.iter_chunks()
                for chunk in it:
                    chunks.append(chunk)
                self._captured["chunks"] = chunks
                self._captured["complete"] = req.body.complete
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=65536,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_iter_chunks_yields_full_body(self):
        body = b"hello world"
        _send_post(self._addr, "/index.txt", body)
        time.sleep(0.3)
        chunks = self._captured.get("chunks", [])
        self.assertEqual(b"".join(chunks), body)

    def test_iter_chunks_yields_empty_for_empty_body(self):
        _send_post(self._addr, "/index.txt", b"")
        time.sleep(0.3)
        chunks = self._captured.get("chunks", [])
        self.assertEqual(chunks, [])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyOneShot(unittest.TestCase):
    """RequestBody enforces one-shot consumption."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.method == "POST" and req.has_body:
                body = req.body
                body.read()
                try:
                    body.read()
                    self._captured["second_read_error"] = None
                except RequestBodyConsumedError as e:
                    self._captured["second_read_error"] = str(e)
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_second_read_raises_consumed_error(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertIsNotNone(self._captured.get("second_read_error"))
        self.assertIn("already consumed", self._captured["second_read_error"])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyOneShotIterChunks(unittest.TestCase):
    """iter_chunks also consumes the body; second iter_chunks is rejected."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.method == "POST" and req.has_body:
                body = req.body
                it1 = body.iter_chunks()
                list(it1)
                try:
                    body.iter_chunks()
                    self._captured["second_iter_error"] = None
                except RequestBodyConsumedError as e:
                    self._captured["second_iter_error"] = str(e)
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_second_iter_chunks_raises_consumed_error(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertIsNotNone(self._captured.get("second_iter_error"))
        self.assertIn("already consumed", self._captured["second_iter_error"])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyOneShotReadThenIter(unittest.TestCase):
    """read() followed by iter_chunks() is rejected."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.method == "POST" and req.has_body:
                body = req.body
                body.read()
                try:
                    body.iter_chunks()
                    self._captured["error"] = None
                except RequestBodyConsumedError:
                    self._captured["error"] = "consumed"
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_iter_chunks_after_read_raises_consumed_error(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertEqual(self._captured.get("error"), "consumed")


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestEmptyBody(unittest.TestCase):
    """GET requests (no body) result in has_body=False, body=None."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            self._captured["has_body"] = req.has_body
            self._captured["body"] = req.body
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_get_request_has_no_body(self):
        url = f"http://{self._addr}/index.txt"
        urllib.request.urlopen(url, timeout=2)
        time.sleep(0.2)
        self.assertFalse(self._captured["has_body"])
        self.assertIsNone(self._captured["body"])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyErrorHierarchy(unittest.TestCase):
    """All body error types form a proper exception hierarchy."""

    def test_base_is_eggserve_error(self):
        self.assertTrue(issubclass(RequestBodyError, EggserveError))

    def test_rejected_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyRejectedError, RequestBodyError))

    def test_too_large_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyTooLargeError, RequestBodyError))

    def test_timeout_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyTimeoutError, RequestBodyError))

    def test_disconnected_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyDisconnectedError, RequestBodyError))

    def test_incomplete_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyIncompleteError, RequestBodyError))

    def test_consumed_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyConsumedError, RequestBodyError))

    def test_cancelled_is_body_error(self):
        self.assertTrue(issubclass(RequestBodyCancelledError, RequestBodyError))

    def test_rejected_is_exception(self):
        self.assertTrue(issubclass(RequestBodyRejectedError, Exception))

    def test_all_errors_have_message(self):
        for cls in (
            RequestBodyError,
            RequestBodyRejectedError,
            RequestBodyTooLargeError,
            RequestBodyTimeoutError,
            RequestBodyDisconnectedError,
            RequestBodyIncompleteError,
            RequestBodyConsumedError,
            RequestBodyCancelledError,
        ):
            err = cls("test message")
            self.assertIn("test message", str(err))


# ---------------------------------------------------------------------------
# Server constructor body policy validation
# ---------------------------------------------------------------------------


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestServerBodyPolicyConstructor(unittest.TestCase):
    """Server constructor accepts and validates body policy parameters."""

    def setUp(self):
        self._td = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_default_body_mode_is_reject(self):
        s = Server(root=self._td, port=0)
        self.assertEqual(s.state, "created")

    def test_explicit_reject_mode(self):
        s = Server(
            root=self._td,
            port=0,
            request_body_mode="reject",
            max_request_body_bytes=0,
        )
        self.assertEqual(s.state, "created")

    def test_buffer_mode_with_max_bytes(self):
        s = Server(
            root=self._td,
            port=0,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self.assertEqual(s.state, "created")

    def test_stream_mode_with_max_bytes(self):
        s = Server(
            root=self._td,
            port=0,
            request_body_mode="stream",
            max_request_body_bytes=2048,
        )
        self.assertEqual(s.state, "created")

    def test_body_timeout_secs_accepted(self):
        s = Server(
            root=self._td,
            port=0,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
            body_timeout_secs=60,
        )
        self.assertEqual(s.state, "created")

    def test_incomplete_body_policy_close(self):
        s = Server(
            root=self._td,
            port=0,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
            incomplete_body_policy="close",
        )
        self.assertEqual(s.state, "created")

    def test_incomplete_body_policy_drain(self):
        s = Server(
            root=self._td,
            port=0,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
            incomplete_body_policy="drain",
        )
        self.assertEqual(s.state, "created")

    def test_invalid_body_mode_rejected(self):
        with self.assertRaises(ValueError) as ctx:
            Server(
                root=self._td,
                port=0,
                request_body_mode="invalid",
                max_request_body_bytes=1024,
            )
        self.assertIn("request_body_mode", str(ctx.exception))

    def test_buffer_mode_zero_max_bytes_rejected(self):
        with self.assertRaises(ValueError) as ctx:
            Server(
                root=self._td,
                port=0,
                request_body_mode="buffer",
                max_request_body_bytes=0,
            )
        self.assertIn("max_request_body_bytes", str(ctx.exception))

    def test_stream_mode_zero_max_bytes_rejected(self):
        with self.assertRaises(ValueError) as ctx:
            Server(
                root=self._td,
                port=0,
                request_body_mode="stream",
                max_request_body_bytes=0,
            )
        self.assertIn("max_request_body_bytes", str(ctx.exception))

    def test_invalid_incomplete_body_policy_rejected(self):
        with self.assertRaises(ValueError) as ctx:
            Server(
                root=self._td,
                port=0,
                request_body_mode="buffer",
                max_request_body_bytes=1024,
                incomplete_body_policy="invalid",
            )
        self.assertIn("incomplete_body_policy", str(ctx.exception))

    def test_zero_body_timeout_rejected(self):
        with self.assertRaises(ValueError):
            Server(
                root=self._td,
                port=0,
                request_body_mode="buffer",
                max_request_body_bytes=1024,
                body_timeout_secs=0,
            )


# ---------------------------------------------------------------------------
# Request.has_body and Request.body accessors
# ---------------------------------------------------------------------------


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestHasBody(unittest.TestCase):
    """Request.has_body property is True when a body is present."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            self._captured["has_body"] = req.has_body
            self._captured["body"] = req.body
            if req.has_body and req.body is not None:
                self._captured["body_type"] = type(req.body).__name__
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_post_with_body_has_body_true(self):
        _send_post(self._addr, "/index.txt", b"test data")
        time.sleep(0.2)
        self.assertTrue(self._captured["has_body"])

    def test_post_body_is_request_body_type(self):
        _send_post(self._addr, "/index.txt", b"test data")
        time.sleep(0.2)
        self.assertEqual(self._captured["body_type"], "RequestBody")

    def test_get_has_body_false(self):
        url = f"http://{self._addr}/index.txt"
        urllib.request.urlopen(url, timeout=2)
        time.sleep(0.2)
        self.assertFalse(self._captured["has_body"])
        self.assertIsNone(self._captured["body"])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyNoneWhenNoBody(unittest.TestCase):
    """Request.body is None when there is no body."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            self._captured["body"] = req.body
            self._captured["has_body"] = req.has_body
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_get_request_body_is_none(self):
        url = f"http://{self._addr}/index.txt"
        urllib.request.urlopen(url, timeout=2)
        time.sleep(0.2)
        self.assertIsNone(self._captured["body"])
        self.assertFalse(self._captured["has_body"])

    def test_post_request_body_is_not_none(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.2)
        self.assertIsNotNone(self._captured["body"])
        self.assertTrue(self._captured["has_body"])


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyIsRequestObjectType(unittest.TestCase):
    """Request.body returns a RequestBody when a body is available."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.has_body:
                self._captured["body_class"] = req.body.__class__.__name__
                self._captured["body_repr"] = repr(req.body)
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_body_is_request_body_class(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.2)
        self.assertEqual(self._captured.get("body_class"), "RequestBody")

    def test_body_repr_contains_request_body(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.2)
        self.assertIn("RequestBody", self._captured.get("body_repr", ""))


# ---------------------------------------------------------------------------
# BodyChunkIterator is a proper iterator
# ---------------------------------------------------------------------------


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyChunkIteratorProtocol(unittest.TestCase):
    """BodyChunkIterator implements the Python iterator protocol."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.method == "POST" and req.has_body:
                it = req.body.iter_chunks()
                self._captured["is_iterator"] = hasattr(it, "__next__")
                self._captured["is_iterable"] = hasattr(it, "__iter__")
                self._captured["repr"] = repr(it)
                first = next(it)
                self._captured["first_chunk"] = first
                self._captured["first_type"] = type(first).__name__
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=1024,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_has_next_method(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertTrue(self._captured.get("is_iterator"))

    def test_has_iter_method(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertTrue(self._captured.get("is_iterable"))

    def test_first_chunk_is_bytes(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertEqual(self._captured.get("first_type"), "bytes")
        self.assertEqual(self._captured.get("first_chunk"), b"data")

    def test_repr_contains_body_chunk_iterator(self):
        _send_post(self._addr, "/index.txt", b"data")
        time.sleep(0.3)
        self.assertIn("BodyChunkIterator", self._captured.get("repr", ""))


# ---------------------------------------------------------------------------
# Buffer mode body rejection when body exceeds max_request_body_bytes
# ---------------------------------------------------------------------------


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyRejection(unittest.TestCase):
    """Body exceeding max_request_body_bytes is rejected."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            self._captured["called"] = True
            if req.has_body:
                data = req.body.read()
                self._captured["data"] = data
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="buffer",
            max_request_body_bytes=10,
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_body_within_limit_accepted(self):
        _send_post(self._addr, "/index.txt", b"small")
        time.sleep(0.2)
        self.assertEqual(self._captured.get("data"), b"small")

    def test_body_exceeding_limit_returns_413(self):
        sock = _send_post(self._addr, "/index.txt", b"x" * 100, keep_open=True)
        status, _, _ = _read_response(sock)
        sock.close()
        self.assertEqual(status, 413)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestRequestBodyRejectionMode(unittest.TestCase):
    """Reject mode blocks all bodies."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            self._captured["called"] = True
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="reject",
        )
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_post_body_rejected_returns_413(self):
        sock = _send_post(self._addr, "/index.txt", b"data", keep_open=True)
        status, _, _ = _read_response(sock)
        sock.close()
        self.assertEqual(status, 413)


if __name__ == "__main__":
    unittest.main()
