"""Standalone packaging smoke test — Server lifecycle and behavior.

Tests server startup on ephemeral loopback port, context-manager lifecycle,
Python callback handler, static fallback, HEAD responses, range responses,
and public-bind acknowledgement behavior. Uses eggserve.Server directly.

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


class TestServerStartupAndStop(unittest.TestCase):
    """Basic server lifecycle: create, start, verify, stop."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "hello.txt"), "w") as f:
            f.write("hello world")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_start_and_stop(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        self.assertIsNotNone(addr)
        self.assertIn(":", addr)
        url = f"http://{addr}/hello.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"hello world")
        s.stop()

    def test_port_zero_assigns_ephemeral_port(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        self.assertIsNotNone(addr)
        host, port_str = addr.split(":")
        port = int(port_str)
        self.assertGreater(port, 0)
        self.assertLessEqual(port, 65535)
        s.stop()


class TestServerContextManager(unittest.TestCase):
    """Server must work as a context manager with proper cleanup."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.html"), "w") as f:
            f.write("<h1>Hello</h1>")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_context_manager_lifecycle(self):
        with Server(root=self._td, port=0) as s:
            addr = s.addr
            self.assertIsNotNone(addr)
            url = f"http://{addr}/index.html"
            self.assertTrue(_wait_for_server(url))
            resp = urllib.request.urlopen(url, timeout=2)
            self.assertEqual(resp.read(), b"<h1>Hello</h1>")
        self.assertIsNone(s.addr)

    def test_double_start_raises_lifecycle_error(self):
        from eggserve import LifecycleError

        s = Server(root=self._td, port=0)
        s.start()
        with self.assertRaises(LifecycleError):
            s.start()
        s.stop()

    def test_stop_without_start_is_noop(self):
        s = Server(root=self._td, port=0)
        s.stop()

    def test_repr_not_started(self):
        s = Server(root=self._td, port=0)
        self.assertIn("not started", repr(s))


class TestCallbackHandler(unittest.TestCase):
    """Python callback handlers intercept requests before static fallback."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "static.txt"), "w") as f:
            f.write("static content")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_callback_returns_custom_response(self):
        def handler(req):
            return Response.text(200, "from handler")

        s = Server(root=self._td, port=0, handler=handler)
        s.start()
        addr = s.addr
        url = f"http://{addr}/anything"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"from handler")
        s.stop()

    def test_callback_receives_correct_method(self):
        captured = []

        def handler(req):
            captured.append(req.method)
            return Response.text(200, "ok")

        s = Server(root=self._td, port=0, handler=handler)
        s.start()
        addr = s.addr
        url = f"http://{addr}/test"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertEqual(captured[-1], "GET")
        s.stop()

    def test_callback_receives_correct_path(self):
        captured = []

        def handler(req):
            captured.append(req.path)
            return Response.text(200, "ok")

        s = Server(root=self._td, port=0, handler=handler)
        s.start()
        addr = s.addr
        url = f"http://{addr}/some/path"
        self.assertTrue(_wait_for_server(url))
        urllib.request.urlopen(url, timeout=2)
        self.assertEqual(captured[-1], "/some/path")
        s.stop()

    def test_callback_exception_returns_500(self):
        def handler(req):
            raise RuntimeError("boom")

        s = Server(root=self._td, port=0, handler=handler)
        s.start()
        addr = s.addr
        url = f"http://{addr}/test"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()


class TestStaticFallback(unittest.TestCase):
    """Without a handler, static files are served from root."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "file.txt"), "w") as f:
            f.write("file content")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_static_file_served(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/file.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"file content")
        s.stop()

    def test_not_found_returns_404(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/nonexistent"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 404)
        finally:
            s.stop()


class TestHeadResponses(unittest.TestCase):
    """HEAD requests return headers without body."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "data.txt"), "w") as f:
            f.write("test data")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_head_returns_200_empty_body(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/data.txt"
        self.assertTrue(_wait_for_server(url))
        req = urllib.request.Request(url, method="HEAD")
        resp = urllib.request.urlopen(req, timeout=2)
        self.assertEqual(resp.status, 200)
        data = resp.read()
        self.assertEqual(data, b"")
        s.stop()


class TestRangeResponses(unittest.TestCase):
    """Range requests return 206 Partial Content."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "range.bin"), "wb") as f:
            f.write(b"\x00" * 1024)

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_range_request_returns_206(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/range.bin"
        self.assertTrue(_wait_for_server(url))
        req = urllib.request.Request(url, headers={"Range": "bytes=0-1023"})
        resp = urllib.request.urlopen(req, timeout=5)
        self.assertEqual(resp.status, 206)
        data = resp.read()
        self.assertEqual(len(data), 1024)
        resp.close()
        s.stop()

    def test_full_request_returns_200(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/range.bin"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.status, 200)
        resp.close()
        s.stop()


class TestPublicBindAcknowledgement(unittest.TestCase):
    """Binding to 0.0.0.0 requires public=True."""

    def test_bind_0_0_0_0_requires_public(self):
        from eggserve import ServeConfig

        with self.assertRaises(ValueError) as ctx:
            ServeConfig(bind="0.0.0.0", port=8000, public=False)
        self.assertIn("public", str(ctx.exception).lower())

    def test_bind_0_0_0_0_with_public_succeeds(self):
        from eggserve import ServeConfig

        config = ServeConfig(bind="0.0.0.0", port=8000, public=True)
        self.assertTrue(config.public)

    def test_loopback_does_not_require_public(self):
        from eggserve import ServeConfig

        config = ServeConfig(bind="127.0.0.1", port=8000, public=False)
        self.assertFalse(config.public)


class TestInvalidRoot(unittest.TestCase):
    """Server must reject non-existent root directory."""

    def test_missing_root_raises(self):
        with self.assertRaises(ValueError):
            Server(root="/nonexistent_root_xyz_12345")


if __name__ == "__main__":
    unittest.main()
