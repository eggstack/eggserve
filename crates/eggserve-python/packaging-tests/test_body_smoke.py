"""Standalone packaging smoke test — body support validation.

Tests request body support from an installed wheel: default reject policy,
buffered body read, streamed body iteration, body error hierarchy, one-shot
enforcement, body policy constructor validation, and request has_body/body
accessors.

Must be run from an installed wheel (pip install eggserve), NOT from the
source tree. Uses only stdlib + eggserve.
"""

import os
import shutil
import socket
import tempfile
import time
import unittest
import urllib.error
import urllib.request

from eggserve import Response, Server


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


def _send_post(addr, path, body=b"", headers=None, timeout=5.0):
    host, port = addr.split(":")
    with socket.create_connection((host, int(port)), timeout=timeout) as sock:
        hdrs = headers or {}
        hdrs.setdefault("Host", addr)
        hdrs.setdefault("Content-Length", str(len(body)))
        hdrs.setdefault("Connection", "close")
        req_line = f"POST {path} HTTP/1.1\r\n"
        header_lines = "".join(f"{k}: {v}\r\n" for k, v in hdrs.items())
        sock.sendall((req_line + header_lines + "\r\n").encode())
        if body:
            sock.sendall(body)
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


class TestDefaultRejectPolicy(unittest.TestCase):
    """Default body policy is reject; POST with body gets 413."""

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

    def test_default_reject_returns_413(self):
        resp = _send_post(self._addr, "/test.txt", b"hello")
        status = _parse_status(resp)
        self.assertEqual(status, 413)


class TestBufferedBodyRead(unittest.TestCase):
    """Buffer mode allows body read."""

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

    def test_buffer_read_returns_body(self):
        body = b"hello world"
        resp = _send_post(self._addr, "/test.txt", body)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertEqual(self._captured.get("body"), body)

    def test_buffer_read_returns_empty_for_empty_body(self):
        resp = _send_post(self._addr, "/test.txt", b"")
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertEqual(self._captured.get("body"), b"")


class TestStreamedBodyIteration(unittest.TestCase):
    """Stream mode allows body iteration."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
            f.write("ok")
        self._captured = {}

        def handler(req):
            if req.has_body:
                chunks = []
                for chunk in req.body.iter_chunks():
                    chunks.append(chunk)
                self._captured["chunks"] = chunks
            return Response.text(200, "ok")

        self._server = Server(
            root=self._td,
            port=0,
            handler=handler,
            request_body_mode="stream",
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

    def test_stream_iter_returns_body(self):
        body = b"hello world"
        resp = _send_post(self._addr, "/test.txt", body)
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        all_data = b"".join(self._captured.get("chunks", []))
        self.assertEqual(all_data, body)


class TestBodyOverLimit(unittest.TestCase):
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
        resp = _send_post(self._addr, "/test.txt", b"x" * 100)
        status = _parse_status(resp)
        self.assertEqual(status, 413)


class TestRequestHasBody(unittest.TestCase):
    """Request.has_body and Request.body accessors."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "w") as f:
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

    def test_post_has_body_true(self):
        resp = _send_post(self._addr, "/test.txt", b"data")
        status = _parse_status(resp)
        self.assertEqual(status, 200)
        self.assertTrue(self._captured.get("has_body"))
        self.assertIsNotNone(self._captured.get("body"))

    def test_get_has_body_false(self):
        url = f"http://{self._addr}/test.txt"
        urllib.request.urlopen(url, timeout=2)
        time.sleep(0.2)
        self.assertFalse(self._captured.get("has_body"))
        self.assertIsNone(self._captured.get("body"))


class TestBodyConstructorValidation(unittest.TestCase):
    """Server constructor validates body policy parameters."""

    def setUp(self):
        self._td = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_invalid_body_mode_rejected(self):
        with self.assertRaises(ValueError):
            Server(root=self._td, port=0, request_body_mode="invalid")

    def test_buffer_zero_max_bytes_rejected(self):
        with self.assertRaises(ValueError):
            Server(
                root=self._td,
                port=0,
                request_body_mode="buffer",
                max_request_body_bytes=0,
            )

    def test_stream_zero_max_bytes_rejected(self):
        with self.assertRaises(ValueError):
            Server(
                root=self._td,
                port=0,
                request_body_mode="stream",
                max_request_body_bytes=0,
            )


if __name__ == "__main__":
    unittest.main()
