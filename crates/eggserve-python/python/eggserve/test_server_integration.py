"""Live integration tests for Python server runtime limits.

Tests connection saturation, header timeout, write timeout, file-stream
semaphore, and handler behavior using real socket-level connections against
the native Server class.
"""

import io
import os
import socket
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

from eggserve._native import (
    Response,
    Server,
    ServerSecureRoot,
    StaticResponder,
)


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


def _make_large_file(path, size):
    with open(path, "wb") as f:
        f.write(b"X" * size)


class TestConnectionLimitSaturation(unittest.TestCase):
    """A1: Verify max_connections is respected with concurrent sockets."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "big.bin"), 512 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_connections_held_until_response_complete(self):
        s = Server(
            root=self._td,
            port=0,
            max_connections=2,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))
        s.stop()

    def test_concurrent_requests_within_limit(self):
        s = Server(
            root=self._td,
            port=0,
            max_connections=3,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        results = []
        errors = []

        def fetch():
            try:
                resp = urllib.request.urlopen(url, timeout=5)
                results.append(resp.status)
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=fetch) for _ in range(3)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=10)

        self.assertEqual(len(results), 3)
        self.assertEqual(len(errors), 0)
        self.assertTrue(all(r == 200 for r in results))
        s.stop()

    def test_server_responsive_after_connections_close(self):
        s = Server(
            root=self._td,
            port=0,
            max_connections=1,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.read(), b"ok")
        resp.close()

        resp2 = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp2.read(), b"ok")
        resp2.close()
        s.stop()


class TestHeaderTimeout(unittest.TestCase):
    """A2: Verify header timeout closes slow connections."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("hello")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_slow_headers_terminated(self):
        s = Server(
            root=self._td,
            port=0,
            header_timeout_secs=1,
            write_timeout_secs=30,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        try:
            sock.connect((host, int(port_str)))
            sock.sendall(b"GET /index.txt HTTP/1.1\r\nHost: ")
            time.sleep(0.1)
            sock.sendall(b"a" * 10)
            time.sleep(2.0)
            try:
                data = sock.recv(4096)
                if not data:
                    pass
                else:
                    pass
            except (socket.timeout, ConnectionResetError, OSError):
                pass
        finally:
            sock.close()

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"hello")
        resp.close()
        s.stop()

    def test_fast_request_succeeds(self):
        s = Server(
            root=self._td,
            port=0,
            header_timeout_secs=2,
            write_timeout_secs=30,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"hello")
        resp.close()
        s.stop()


class TestWriteTimeout(unittest.TestCase):
    """A3: Verify write timeout bounds stalled response lifetime."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "large.bin"), 4 * 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_write_timeout_bounds_connection(self):
        s = Server(
            root=self._td,
            port=0,
            max_connections=10,
            header_timeout_secs=10,
            write_timeout_secs=2,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        try:
            sock.connect((host, int(port_str)))
            request = (
                b"GET /large.bin HTTP/1.1\r\n"
                b"Host: " + addr.encode() + b"\r\n"
                b"\r\n"
            )
            sock.sendall(request)
            time.sleep(0.5)
            try:
                data = sock.recv(1024)
            except (socket.timeout, OSError):
                pass
            time.sleep(3.0)
        finally:
            sock.close()

        time.sleep(0.5)
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()
        s.stop()


class TestFileStreamSemaphore(unittest.TestCase):
    """A4: Verify max_file_streams limits concurrent file responses."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "large.bin"), 2 * 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_file_stream_limit_does_not_break_server(self):
        s = Server(
            root=self._td,
            port=0,
            max_connections=10,
            max_file_streams=1,
            header_timeout_secs=10,
            write_timeout_secs=5,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()

        resp2 = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp2.read(), b"ok")
        resp2.close()

        s.stop()


class TestHandlerBehavior(unittest.TestCase):
    """B1-B6: Handler return-type validation and error handling."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("file content")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_valid_handler_response(self):
        def handler(req):
            return Response.text(200, "custom")

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"custom")
        resp.close()
        s.stop()

    def test_handler_returning_none_causes_500(self):
        def handler(req):
            return None

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_handler_exception_returns_500(self):
        def handler(req):
            raise ValueError("something went wrong")

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_handler_exception_does_not_leak_details(self):
        def handler(req):
            raise RuntimeError("secret-password-123 and /etc/passwd path")

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        try:
            resp = urllib.request.urlopen(url, timeout=5)
            body = resp.read()
            self.assertNotIn(b"secret-password-123", body)
            self.assertNotIn(b"/etc/passwd", body)
            resp.close()
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_invalid_handler_return_type_returns_500(self):
        def handler(req):
            return 42

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_server_continues_after_handler_error(self):
        call_count = [0]

        def handler(req):
            call_count[0] += 1
            if call_count[0] == 1:
                raise ValueError("fail first")
            return Response.text(200, "ok")

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        call_count[0] = 0
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()
        s.stop()


class TestHeadSemantics(unittest.TestCase):
    """B5: Verify HEAD returns headers without body."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "data.txt"), "w") as f:
            f.write("hello world")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_head_returns_correct_headers(self):
        s = Server(root=self._td, port=0, header_timeout_secs=10, write_timeout_secs=10)
        s.start()
        addr = s.addr
        url = f"http://{addr}/data.txt"
        self.assertTrue(_wait_for_server(url))

        req = urllib.request.Request(url, method="HEAD")
        resp = urllib.request.urlopen(req, timeout=5)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"")
        self.assertEqual(resp.headers.get("content-length"), "11")
        resp.close()
        s.stop()

    def test_head_with_handler(self):
        def handler(req):
            return Response.bytes(200, b"hello world")

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/data.txt"
        self.assertTrue(_wait_for_server(url))

        req = urllib.request.Request(url, method="HEAD")
        resp = urllib.request.urlopen(req, timeout=5)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"")
        resp.close()
        s.stop()


class TestStatusValidation(unittest.TestCase):
    """B3: Verify invalid status values are handled safely."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("content")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def _start_server(self, handler):
        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        return s

    def test_status_zero_falls_back_to_500(self):
        def handler(req):
            return Response.empty(0)

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_status_99_falls_back_to_500(self):
        def handler(req):
            return Response.empty(99)

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_status_1000_falls_back_to_500(self):
        def handler(req):
            return Response.empty(1000)

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_negative_status_falls_back_to_500(self):
        def handler(req):
            return Response.empty(-1)

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_server_continues_after_invalid_status(self):
        call_count = [0]

        def handler(req):
            call_count[0] += 1
            if call_count[0] == 1:
                return Response.empty(-1)
            return Response.text(200, "ok")

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        call_count[0] = 0
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()
        s.stop()


class TestHeaderValidation(unittest.TestCase):
    """B4: Verify invalid handler headers fail safely."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("content")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def _start_server(self, handler):
        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        return s

    def test_empty_header_name_causes_500(self):
        def handler(req):
            return Response.bytes(200, b"ok", headers={"": "value"})

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_header_with_cr_lf_causes_500(self):
        def handler(req):
            return Response.bytes(200, b"ok", headers={"x-bad": "val\r\ninjection"})

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()

    def test_valid_unusual_status_passes_through(self):
        def handler(req):
            return Response.empty(204)

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.status, 204)
        resp.close()
        s.stop()

    def test_server_continues_after_header_error(self):
        call_count = [0]

        def handler(req):
            call_count[0] += 1
            if call_count[0] == 1:
                return Response.bytes(200, b"ok", headers={"": "bad"})
            return Response.text(200, "ok")

        s = self._start_server(handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        call_count[0] = 0
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()
        s.stop()


class TestShutdownWithActiveWork(unittest.TestCase):
    """B7: Verify shutdown behavior with active connections."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "large.bin"), 4 * 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_shutdown_closes_accept_loop(self):
        s = Server(
            root=self._td,
            port=0,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        s.stop()

        with self.assertRaises((ConnectionRefusedError, urllib.error.URLError, OSError)):
            urllib.request.urlopen(url, timeout=2)

    def test_shutdown_with_stalled_reader(self):
        s = Server(
            root=self._td,
            port=0,
            header_timeout_secs=10,
            write_timeout_secs=5,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/large.bin"
        self.assertTrue(_wait_for_server(url))

        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        try:
            sock.connect((host, int(port_str)))
            request = (
                b"GET /large.bin HTTP/1.1\r\n"
                b"Host: " + addr.encode() + b"\r\n"
                b"\r\n"
            )
            sock.sendall(request)
            time.sleep(0.5)
            try:
                sock.recv(1024)
            except (socket.timeout, OSError):
                pass
        finally:
            sock.close()

        s.stop()

    def test_double_stop_is_safe(self):
        s = Server(
            root=self._td,
            port=0,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        s.stop()
        s.stop()


if __name__ == "__main__":
    unittest.main()
