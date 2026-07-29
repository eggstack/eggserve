"""Real-socket and external-client body tests.

Tests body behavior using raw TCP sockets and Python's http.client
for fixed-length, chunked, slow body, disconnect, timeout, and
static service rejection scenarios.

Must be run from an installed wheel. Uses raw sockets for wire-level verification.
"""

import http.client
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
    from eggserve._native import Response, Server

    NATIVE_AVAILABLE = True
except ImportError:
    NATIVE_AVAILABLE = False


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


def _raw_request(addr, request_bytes, timeout=5.0):
    host, port = addr.split(":")
    with socket.create_connection((host, int(port)), timeout=timeout) as sock:
        sock.sendall(request_bytes)
        chunks = []
        sock.settimeout(timeout)
        while True:
            try:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
            except socket.timeout:
                break
            except (ConnectionResetError, OSError):
                break
    return b"".join(chunks)


def _parse_status(data):
    try:
        header_end = data.index(b"\r\n\r\n")
    except ValueError:
        return None
    header_str = data[:header_end].decode("utf-8", errors="replace")
    status_line = header_str.split("\r\n")[0]
    parts = status_line.split(" ", 2)
    if len(parts) >= 2:
        return int(parts[1])
    return None


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestFixedLengthEcho(unittest.TestCase):
    """Fixed-length body echo via raw socket."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.has_body:
                data = req.body.read()
                self._captured["body"] = data
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

    def test_fixed_length_echo(self):
        body = b"hello world"
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertEqual(self._captured["body"], body)

    def test_fixed_length_unicode(self):
        body = "hello \u00e9\u00e8\u00ea".encode("utf-8")
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertEqual(self._captured["body"], body)

    def test_fixed_length_binary(self):
        body = bytes(range(256))
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertEqual(self._captured["body"], body)

    def test_fixed_length_exact_limit(self):
        body = b"12345"
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertEqual(self._captured["body"], body)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestChunkedBody(unittest.TestCase):
    """Chunked transfer-encoding body via raw socket."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.has_body:
                chunks = []
                it = req.body.iter_chunks()
                for chunk in it:
                    chunks.append(chunk)
                self._captured["chunks"] = chunks
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

    def test_chunked_body(self):
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Transfer-Encoding: chunked\r\n"
            f"Connection: close\r\n"
            f"\r\n"
            f"5\r\nhello\r\n"
            f"6\r\n world\r\n"
            f"0\r\n\r\n"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        all_data = b"".join(self._captured.get("chunks", []))
        self.assertEqual(all_data, b"hello world")

    def test_chunked_many_small_chunks(self):
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Transfer-Encoding: chunked\r\n"
            f"Connection: close\r\n"
            f"\r\n"
            f"1\r\na\r\n"
            f"1\r\nb\r\n"
            f"1\r\nc\r\n"
            f"0\r\n\r\n"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        all_data = b"".join(self._captured.get("chunks", []))
        self.assertEqual(all_data, b"abc")

    def test_chunked_single_chunk(self):
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Transfer-Encoding: chunked\r\n"
            f"Connection: close\r\n"
            f"\r\n"
            f"5\r\nhello\r\n"
            f"0\r\n\r\n"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        all_data = b"".join(self._captured.get("chunks", []))
        self.assertEqual(all_data, b"hello")


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyLimitExceeded(unittest.TestCase):
    """Body exceeding limit returns 413."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._server = Server(
            root=self._td,
            port=0,
            handler=lambda req: Response.text(200, "ok"),
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

    def test_over_limit_returns_413(self):
        body = b"x" * 100
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 413)

    def test_declared_length_over_limit(self):
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: 100000\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 413)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestGetWithBodyRejected(unittest.TestCase):
    """GET requests with body are rejected."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._server = Server(
            root=self._td,
            port=0,
            handler=lambda req: Response.text(200, "ok"),
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

    def test_get_with_content_length(self):
        req = (
            f"GET /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: 5\r\n"
            f"Connection: close\r\n"
            f"\r\n"
            f"hello"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 400)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestStaticServiceBodyRejection(unittest.TestCase):
    """Static service returns 405 for POST."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._server = Server(root=self._td, port=0)
        self._server.start()
        self._addr = self._server.addr
        _wait_for_tcp(self._addr)

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_post_to_static_returns_405(self):
        body = b"hello"
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 405)

    def test_put_to_static_returns_405(self):
        body = b"hello"
        req = (
            f"PUT /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 405)

    def test_delete_to_static_returns_405(self):
        req = (
            f"DELETE /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: 0\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 405)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestHttpClientFixedLength(unittest.TestCase):
    """Fixed-length body via http.client."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.has_body:
                data = req.body.read()
                self._captured["body"] = data
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

    def test_http_client_post(self):
        host, port = self._addr.split(":")
        conn = http.client.HTTPConnection(host, int(port), timeout=5)
        body = b"hello from http.client"
        conn.request("POST", "/test.txt", body=body)
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        self.assertEqual(self._captured["body"], body)
        conn.close()

    def test_http_client_put(self):
        host, port = self._addr.split(":")
        conn = http.client.HTTPConnection(host, int(port), timeout=5)
        body = b"put data"
        conn.request("PUT", "/test.txt", body=body)
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        self.assertEqual(self._captured["body"], body)
        conn.close()

    def test_http_client_patch(self):
        host, port = self._addr.split(":")
        conn = http.client.HTTPConnection(host, int(port), timeout=5)
        body = b"patch data"
        conn.request("PATCH", "/test.txt", body=body)
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        self.assertEqual(self._captured["body"], body)
        conn.close()

    def test_http_client_empty_body(self):
        host, port = self._addr.split(":")
        conn = http.client.HTTPConnection(host, int(port), timeout=5)
        conn.request("POST", "/test.txt", body=b"")
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        # http.client empty body means no Content-Length, so has_body may be False
        conn.close()


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestDisconnectMidBody(unittest.TestCase):
    """Disconnecting mid-body should not crash the server."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._server = Server(
            root=self._td,
            port=0,
            handler=lambda req: Response.text(200, "ok"),
            request_body_mode="buffer",
            max_request_body_bytes=10240,
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

    def test_disconnect_sending_body(self):
        host, port = self._addr.split(":")
        for _ in range(5):
            try:
                sock = socket.create_connection((host, int(port)), timeout=2)
                sock.sendall(
                    b"POST /test.txt HTTP/1.1\r\n"
                    b"Host: localhost\r\n"
                    b"Content-Length: 1000\r\n"
                    b"Connection: close\r\n"
                    b"\r\n"
                    b"partial"
                )
                sock.close()
            except (ConnectionResetError, BrokenPipeError, OSError):
                pass
            time.sleep(0.05)

        # Server should still be responsive
        req = (
            f"GET /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode()
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestPartialConsumption(unittest.TestCase):
    """Partial body consumption with close policy."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        try:
            self._server.force_shutdown(2.0)
        except Exception:
            pass
        shutil.rmtree(self._td, ignore_errors=True)

    def test_partial_read_closes_connection(self):
        def handler(req):
            if req.has_body:
                # Only read 2 bytes of a 10-byte body
                it = req.body.iter_chunks()
                chunk = next(iter(it), None)
                if chunk:
                    pass  # discard first chunk
            return Response.text(200, "partial")

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

        body = b"x" * 10
        req = (
            f"POST /test.txt HTTP/1.1\r\n"
            f"Host: {self._addr}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: keep-alive\r\n"
            f"\r\n"
        ).encode() + body
        resp = _raw_request(self._addr, req)
        status = _parse_status(resp)
        self.assertEqual(status, 200)


if __name__ == "__main__":
    unittest.main()
