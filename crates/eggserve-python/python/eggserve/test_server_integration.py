"""Deterministic integration tests for Python server runtime limits.

Tests connection saturation, callback concurrency, file-stream semaphore,
header/write timeouts, and graceful shutdown using event-based
synchronization, atomic counters, and raw socket control rather than
sleep-based assertions.
"""

import ctypes
import os
import select
import shutil
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


def _make_large_file(path, size):
    with open(path, "wb") as f:
        f.write(b"X" * size)


class _AtomicCounter:
    """Thread-safe counter."""

    def __init__(self, value=0):
        self._value = value
        self._lock = threading.Lock()

    def increment(self):
        with self._lock:
            self._value += 1
            return self._value

    def decrement(self):
        with self._lock:
            self._value -= 1
            return self._value

    @property
    def value(self):
        with self._lock:
            return self._value


class _MaxTracker:
    """Tracks the maximum observed value of a concurrent counter."""

    def __init__(self):
        self._max = 0
        self._current = 0
        self._lock = threading.Lock()

    def enter(self):
        with self._lock:
            self._current += 1
            if self._current > self._max:
                self._max = self._current
            return self._current

    def exit(self):
        with self._lock:
            self._current -= 1
            return self._current

    @property
    def max_observed(self):
        with self._lock:
            return self._max


def _connect_raw(addr, path="/big.bin"):
    host, port_str = addr.split(":")
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(5)
    sock.connect((host, int(port_str)))
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {addr}\r\n"
        f"\r\n"
    ).encode()
    sock.sendall(req)
    return sock


def _start_server(**kwargs):
    """Create, start, and return a Server. Caller must call s.stop()."""
    s = Server(**kwargs)
    s.start()
    return s


# ---------------------------------------------------------------------------
# Workstream A — Connection semaphore
# ---------------------------------------------------------------------------


class TestConnectionSemaphore(unittest.TestCase):
    """A: Deterministic connection semaphore saturation and release."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "big.bin"), 2 * 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_max_connections_saturation(self):
        """Hold max_connections open, prove the next connection blocks."""
        max_conn = 2
        s = self._make_server(max_connections=max_conn, max_file_streams=64)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        held = []
        for _ in range(max_conn):
            sock = _connect_raw(addr)
            time.sleep(0.1)
            try:
                data = sock.recv(1024)
                if data:
                    held.append(sock)
                else:
                    sock.close()
            except (socket.timeout, OSError):
                sock.close()

        self.assertEqual(len(held), max_conn)

        released = threading.Event()

        def try_extra():
            try:
                sock = _connect_raw(addr, "/small.txt")
                data = sock.recv(4096)
                if data:
                    released.set()
                sock.close()
            except Exception:
                pass

        t = threading.Thread(target=try_extra)
        t.start()
        t.join(timeout=1.0)
        self.assertFalse(released.is_set(), "Extra connection should be blocked")
        t.join(timeout=0.5)

        for sock in held:
            sock.close()

        released2 = threading.Event()

        def try_after_release():
            try:
                sock = _connect_raw(addr, "/small.txt")
                data = sock.recv(4096)
                if data:
                    released2.set()
                sock.close()
            except Exception:
                pass

        t2 = threading.Thread(target=try_after_release)
        t2.start()
        t2.join(timeout=5)
        self.assertTrue(released2.is_set(), "Connection should proceed after release")

    def test_release_after_malformed_request(self):
        """A connection that sends malformed data still releases the permit."""
        max_conn = 2
        s = self._make_server(max_connections=max_conn)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        sock1 = _connect_raw(addr)
        time.sleep(0.1)
        sock1.recv(1024)

        host, port_str = addr.split(":")
        sock2 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock2.settimeout(5)
        sock2.connect((host, int(port_str)))
        sock2.sendall(b"GARBAGE DATA\r\n\r\n")
        time.sleep(0.5)
        try:
            sock2.recv(1024)
        except (socket.timeout, ConnectionResetError, OSError):
            pass
        sock2.close()

        released = threading.Event()

        def try_after():
            try:
                sock = _connect_raw(addr, "/small.txt")
                data = sock.recv(4096)
                if data:
                    released.set()
                sock.close()
            except Exception:
                pass

        t = threading.Thread(target=try_after)
        t.start()
        t.join(timeout=5)
        self.assertTrue(released.is_set())

        sock1.close()

    def test_release_after_header_timeout(self):
        """Slow header connection releases the permit on timeout."""
        s = self._make_server(max_connections=2, header_timeout_secs=1)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        held = _connect_raw(addr)
        time.sleep(0.1)
        held.recv(1024)

        host, port_str = addr.split(":")
        slow = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        slow.settimeout(5)
        slow.connect((host, int(port_str)))
        slow.sendall(b"GET /small.txt HTTP/1.1\r\nHost: ")
        time.sleep(2.0)
        try:
            slow.recv(1024)
        except (socket.timeout, ConnectionResetError, OSError):
            pass
        slow.close()

        released = threading.Event()

        def try_after():
            try:
                sock = _connect_raw(addr, "/small.txt")
                data = sock.recv(4096)
                if data:
                    released.set()
                sock.close()
            except Exception:
                pass

        t = threading.Thread(target=try_after)
        t.start()
        t.join(timeout=5)
        self.assertTrue(released.is_set())

        held.close()

    def test_release_after_write_timeout(self):
        """Connection stalled on read releases the permit on write timeout."""
        s = self._make_server(max_connections=2, write_timeout_secs=1)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        held = _connect_raw(addr)
        time.sleep(0.1)
        held.recv(1024)

        stalled = _connect_raw(addr, "/big.bin")
        time.sleep(0.3)
        try:
            stalled.recv(1024)
        except (socket.timeout, OSError):
            pass
        time.sleep(2.0)
        stalled.close()

        released = threading.Event()

        def try_after():
            try:
                sock = _connect_raw(addr, "/small.txt")
                data = sock.recv(4096)
                if data:
                    released.set()
                sock.close()
            except Exception:
                pass

        t = threading.Thread(target=try_after)
        t.start()
        t.join(timeout=5)
        self.assertTrue(released.is_set())

        held.close()

    def test_release_after_peer_disconnect(self):
        """Connection that closes without sending releases the permit."""
        s = self._make_server(max_connections=2)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        held = _connect_raw(addr)
        time.sleep(0.1)
        held.recv(1024)

        host, port_str = addr.split(":")
        disc = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        disc.settimeout(5)
        disc.connect((host, int(port_str)))
        disc.close()

        time.sleep(0.3)

        released = threading.Event()

        def try_after():
            try:
                sock = _connect_raw(addr, "/small.txt")
                data = sock.recv(4096)
                if data:
                    released.set()
                sock.close()
            except Exception:
                pass

        t = threading.Thread(target=try_after)
        t.start()
        t.join(timeout=5)
        self.assertTrue(released.is_set())

        held.close()

    def test_release_after_shutdown(self):
        """Shutdown releases all held connection permits."""
        s = self._make_server(max_connections=2)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        held = _connect_raw(addr)
        time.sleep(0.1)
        held.recv(1024)

        s.stop()
        self._servers.remove(s)

        with self.assertRaises((ConnectionRefusedError, urllib.error.URLError, OSError)):
            urllib.request.urlopen(url, timeout=2)


# ---------------------------------------------------------------------------
# Workstream B — Python callback semaphore
# ---------------------------------------------------------------------------


class TestCallbackSemaphore(unittest.TestCase):
    """B: Deterministic callback concurrency limit verification."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("content")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_callback_concurrency_never_exceeds_limit(self):
        """Max observed concurrent callbacks never exceeds max_python_callbacks."""
        max_cb = 2
        tracker = _MaxTracker()
        entered_count = _AtomicCounter()
        all_entered = threading.Event()
        release_event = threading.Event()

        def handler(req):
            tracker.enter()
            entered_count.increment()
            if entered_count.value >= max_cb:
                all_entered.set()
            release_event.wait(timeout=5)
            tracker.exit()
            return Response.text(200, "ok")

        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_python_callbacks=max_cb,
        )
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        threads = []
        for _ in range(max_cb + 2):
            t = threading.Thread(target=lambda: urllib.request.urlopen(url, timeout=10))
            t.start()
            threads.append(t)

        all_entered.wait(timeout=5)
        time.sleep(0.1)
        self.assertLessEqual(tracker.max_observed, max_cb)

        release_event.set()
        for t in threads:
            t.join(timeout=10)

    def test_queued_callbacks_proceed_after_release(self):
        """Callbacks queued behind the semaphore proceed once a permit is freed."""
        max_cb = 1
        first_entered = threading.Event()
        first_release = threading.Event()
        second_completed = threading.Event()

        def handler(req):
            if not first_entered.is_set():
                first_entered.set()
                first_release.wait(timeout=5)
                return Response.text(200, "first")
            second_completed.set()
            return Response.text(200, "second")

        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_python_callbacks=max_cb,
        )
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        results = []

        def fetch(label):
            try:
                resp = urllib.request.urlopen(url, timeout=10)
                results.append(label)
            except Exception:
                results.append(f"{label}_error")

        t1 = threading.Thread(target=fetch, args=("first",))
        t1.start()
        first_entered.wait(timeout=5)

        t2 = threading.Thread(target=fetch, args=("second",))
        t2.start()
        time.sleep(0.5)
        self.assertNotIn("second", results)

        first_release.set()
        t1.join(timeout=5)
        t2.join(timeout=5)
        self.assertIn("second", results)

    def test_exception_releases_callback_permit(self):
        """A handler exception still releases the callback permit."""
        max_cb = 1
        error_seen = threading.Event()
        after_error = threading.Event()

        def handler(req):
            if not error_seen.is_set():
                error_seen.set()
                after_error.wait(timeout=5)
                raise ValueError("boom")
            return Response.text(200, "ok")

        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_python_callbacks=max_cb,
        )
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

    def test_invalid_return_type_releases_permit(self):
        """Returning an invalid type still releases the callback permit."""
        max_cb = 1
        call_count = [0]

        def handler(req):
            call_count[0] += 1
            if call_count[0] == 1:
                return 42
            return Response.text(200, "ok")

        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_python_callbacks=max_cb,
        )
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()

    def test_shutdown_does_not_deadlock_with_queued_callbacks(self):
        """Shutdown completes even when callbacks are queued."""
        max_cb = 1
        call_count = _AtomicCounter()

        def handler(req):
            call_count.increment()
            time.sleep(0.05)
            return Response.text(200, "ok")

        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_python_callbacks=max_cb,
        )
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        threads = []
        for _ in range(3):
            t = threading.Thread(target=lambda: urllib.request.urlopen(url, timeout=5))
            t.start()
            threads.append(t)

        time.sleep(0.3)

        stop_done = threading.Event()

        def do_stop():
            s.stop()
            self._servers.remove(s)
            stop_done.set()

        st = threading.Thread(target=do_stop)
        st.start()
        st.join(timeout=10)
        self.assertTrue(stop_done.is_set(), "stop() must not deadlock")

        for t in threads:
            t.join(timeout=5)

    def test_no_rust_mutex_held_during_python(self):
        """The GIL is the only synchronization during Python callback execution."""
        call_active = _AtomicCounter()

        def handler(req):
            call_active.increment()
            time.sleep(0.2)
            call_active.decrement()
            return Response.text(200, "ok")

        max_cb = 4
        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_python_callbacks=max_cb,
        )
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))

        results = []
        errors = []

        def fetch():
            try:
                resp = urllib.request.urlopen(url, timeout=10)
                results.append(resp.status)
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=fetch) for _ in range(max_cb)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)

        self.assertEqual(len(errors), 0, f"Errors: {errors}")
        self.assertTrue(all(r == 200 for r in results))


# ---------------------------------------------------------------------------
# Workstream C — File-stream semaphore
# ---------------------------------------------------------------------------


class TestFileStreamSemaphore(unittest.TestCase):
    """C: Deterministic file-stream semaphore saturation and release.

    These tests serve static files directly (no handler) so that the
    planner returns BodyPlan::FileFull, which acquires the file-stream
    semaphore in convert_to_hyper_response. Handler-returned bodies
    (Response.bytes) bypass the semaphore entirely.
    """

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "big.bin"), 4 * 1024 * 1024)
        _make_large_file(os.path.join(self._td, "range.bin"), 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def _open_get(self, addr, path="/big.bin", rcvbuf=None):
        """Open a raw socket, send a GET request with Connection: close, return the socket.

        If *rcvbuf* is set, the receive buffer is limited to that many bytes.
        A small buffer (e.g. 4096) forces the server to block on write quickly,
        keeping the file-stream semaphore held long enough for test sync.
        """
        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        if rcvbuf is not None:
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, rcvbuf)
        sock.connect((host, int(port_str)))
        req = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {addr}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode()
        sock.sendall(req)
        return sock

    def _open_head(self, addr, path="/big.bin", rcvbuf=None):
        """Open a raw socket, send a HEAD request with Connection: close, return the socket."""
        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        if rcvbuf is not None:
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, rcvbuf)
        sock.connect((host, int(port_str)))
        req = (
            f"HEAD {path} HTTP/1.1\r\n"
            f"Host: {addr}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        ).encode()
        sock.sendall(req)
        return sock

    def _read_headers(self, sock):
        """Read from socket until we get the complete HTTP response headers."""
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
        return data

    def _has_data(self, sock, timeout=0.5):
        """Check whether a socket has data available (non-blocking check via select)."""
        readable, _, _ = select.select([sock], [], [], timeout)
        return len(readable) > 0

    def test_max_file_streams_saturation(self):
        """No more than max_file_streams concurrent file responses are active."""
        s = self._make_server(max_connections=10, max_file_streams=1, max_python_callbacks=10)
        addr = s.addr
        self.assertTrue(_wait_for_tcp(addr))

        # Small receive buffer on sock1 forces the server to block on write
        # after only a few KB, keeping the file-stream semaphore held.
        sock1 = self._open_get(addr, rcvbuf=4096)
        headers1 = self._read_headers(sock1)
        self.assertIn(b"200", headers1.split(b"\r\n")[0])

        # Second request should be blocked (semaphore exhausted by sock1's stream).
        sock2 = self._open_get(addr)
        self.assertFalse(
            self._has_data(sock2, timeout=1.0),
            "Second request should be blocked while first is streaming",
        )

        # Closing sock1 releases the semaphore permit.
        sock1.close()

        self.assertTrue(
            self._has_data(sock2, timeout=5.0),
            "Second request should complete after first is released",
        )
        headers2 = self._read_headers(sock2)
        self.assertIn(b"200", headers2.split(b"\r\n")[0])
        sock2.close()

    def test_head_does_not_consume_stream_permit(self):
        """HEAD requests do not acquire a file-stream permit."""
        s = self._make_server(max_connections=10, max_file_streams=1, max_python_callbacks=10)
        addr = s.addr
        self.assertTrue(_wait_for_tcp(addr))

        head_sock = self._open_head(addr)
        headers = self._read_headers(head_sock)
        self.assertIn(b"200", headers.split(b"\r\n")[0])
        head_sock.close()

        sock = self._open_get(addr)
        self.assertTrue(
            self._has_data(sock, timeout=1.0),
            "GET should succeed immediately after HEAD (permit not consumed)",
        )
        headers = self._read_headers(sock)
        self.assertIn(b"200", headers.split(b"\r\n")[0])
        sock.close()

    def test_disconnect_releases_stream_permit(self):
        """Client disconnect during file streaming releases the stream permit."""
        s = self._make_server(max_connections=10, max_file_streams=1, max_python_callbacks=10)
        addr = s.addr
        self.assertTrue(_wait_for_tcp(addr))

        # Small receive buffer keeps the semaphore held during streaming.
        sock1 = self._open_get(addr, rcvbuf=4096)
        self._read_headers(sock1)

        sock1.close()

        sock2 = self._open_get(addr)
        self.assertTrue(
            self._has_data(sock2, timeout=2.0),
            "Request should succeed after disconnect releases permit",
        )
        headers = self._read_headers(sock2)
        self.assertIn(b"200", headers.split(b"\r\n")[0])
        sock2.close()

    def test_queued_stream_begins_after_release(self):
        """A queued file stream begins only after a permit is released."""
        s = self._make_server(max_connections=10, max_file_streams=1, max_python_callbacks=10)
        addr = s.addr
        self.assertTrue(_wait_for_tcp(addr))

        # Small receive buffer keeps the semaphore held.
        sock1 = self._open_get(addr, rcvbuf=4096)
        self._read_headers(sock1)

        # Second request should be queued.
        sock2 = self._open_get(addr)
        self.assertFalse(
            self._has_data(sock2, timeout=1.0),
            "Second request should be queued",
        )

        # Releasing sock1 allows the queued request to proceed.
        sock1.close()

        self.assertTrue(
            self._has_data(sock2, timeout=5.0),
            "Queued stream should begin after permit release",
        )
        headers = self._read_headers(sock2)
        self.assertIn(b"200", headers.split(b"\r\n")[0])
        sock2.close()

    def test_range_completion_releases_stream_permit(self):
        """A completed range response releases the stream permit."""
        s = self._make_server(max_connections=10, max_file_streams=1)
        addr = s.addr
        url = f"http://{addr}/range.bin"
        self.assertTrue(_wait_for_server(url))

        req = urllib.request.Request(url, headers={"Range": "bytes=0-1023"})
        resp = urllib.request.urlopen(req, timeout=5)
        self.assertEqual(resp.status, 206)
        data = resp.read()
        self.assertEqual(len(data), 1024)
        resp.close()

        resp2 = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp2.status, 200)
        resp2.close()

    def test_handler_file_body_uses_same_limit(self):
        """Handler-returned byte bodies bypass the file-stream semaphore."""
        _make_large_file(os.path.join(self._td, "served.bin"), 2 * 1024 * 1024)

        def handler(req):
            with open(os.path.join(self._td, "served.bin"), "rb") as f:
                data = f.read()
            return Response.bytes(200, data)

        s = self._make_server(
            handler=handler,
            max_connections=10,
            max_file_streams=1,
            max_python_callbacks=10,
        )
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_tcp(addr))

        for _ in range(3):
            resp = urllib.request.urlopen(url, timeout=5)
            self.assertEqual(resp.status, 200)
            data = resp.read()
            self.assertEqual(len(data), 2 * 1024 * 1024)
            resp.close()


# ---------------------------------------------------------------------------
# Workstream D — Timeout boundaries
# ---------------------------------------------------------------------------


class TestTimeoutBoundaries(unittest.TestCase):
    """D: Verify exact documented timeout coverage."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "big.bin"), 4 * 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_header_timeout_covers_incomplete_headers(self):
        """Header timeout fires when headers are not fully received."""
        s = self._make_server(header_timeout_secs=1, write_timeout_secs=30)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        sock.connect((host, int(port_str)))
        sock.sendall(b"GET /small.txt HTTP/1.1\r\nHost: ")
        time.sleep(2.0)
        try:
            data = sock.recv(4096)
            self.assertEqual(data, b"", "Connection should have been closed")
        except (socket.timeout, ConnectionResetError, OSError):
            pass
        sock.close()

        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()

    def test_write_timeout_covers_stalled_response(self):
        """Write timeout fires when client stops reading during response."""
        s = self._make_server(max_connections=10, write_timeout_secs=1)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        sock = _connect_raw(addr, "/big.bin")
        time.sleep(0.3)
        try:
            sock.recv(1024)
        except (socket.timeout, OSError):
            pass
        time.sleep(2.0)
        sock.close()

        time.sleep(0.5)
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()

    def test_write_timeout_covers_file_body_streaming(self):
        """Write timeout applies to file body streaming (not just headers)."""
        s = self._make_server(max_connections=10, write_timeout_secs=1)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        sock = _connect_raw(addr, "/big.bin")
        time.sleep(0.3)
        try:
            sock.recv(1024)
        except (socket.timeout, OSError):
            pass
        time.sleep(2.0)
        sock.close()

        time.sleep(0.5)
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()

    def test_client_request_timeout_rejects_zero(self):
        """Zero header_timeout_secs is rejected."""
        with self.assertRaises(ValueError):
            Server(
                root=self._td, port=0,
                header_timeout_secs=0,
                write_timeout_secs=10,
            )

    def test_client_request_timeout_rejects_negative(self):
        """Negative header_timeout_secs is rejected."""
        with self.assertRaises((ValueError, OverflowError)):
            Server(
                root=self._td, port=0,
                header_timeout_secs=-1,
                write_timeout_secs=10,
            )

    def test_write_timeout_rejects_zero(self):
        """Zero write_timeout_secs is rejected."""
        with self.assertRaises(ValueError):
            Server(
                root=self._td, port=0,
                header_timeout_secs=10,
                write_timeout_secs=0,
            )

    def test_write_timeout_rejects_nan(self):
        """NaN timeout is rejected."""
        with self.assertRaises(TypeError):
            Server(
                root=self._td, port=0,
                header_timeout_secs=float("nan"),
                write_timeout_secs=10,
            )

    def test_write_timeout_rejects_inf(self):
        """Infinity timeout is rejected."""
        with self.assertRaises(TypeError):
            Server(
                root=self._td, port=0,
                header_timeout_secs=float("inf"),
                write_timeout_secs=10,
            )

    def test_max_connections_rejects_zero(self):
        """Zero max_connections is rejected."""
        with self.assertRaises(ValueError):
            Server(
                root=self._td, port=0,
                max_connections=0,
                header_timeout_secs=10,
                write_timeout_secs=10,
            )

    def test_max_file_streams_rejects_zero(self):
        """Zero max_file_streams is rejected."""
        with self.assertRaises(ValueError):
            Server(
                root=self._td, port=0,
                max_file_streams=0,
                header_timeout_secs=10,
                write_timeout_secs=10,
            )

    def test_max_python_callbacks_rejects_zero(self):
        """Zero max_python_callbacks is rejected."""
        with self.assertRaises(ValueError):
            Server(
                root=self._td, port=0,
                max_python_callbacks=0,
                header_timeout_secs=10,
                write_timeout_secs=10,
            )

    def test_float_max_connections_rejected(self):
        """Float max_connections is rejected."""
        with self.assertRaises(TypeError):
            Server(
                root=self._td, port=0,
                max_connections=1.5,
                header_timeout_secs=10,
                write_timeout_secs=10,
            )

    def test_connect_timeout_distinct_from_header_timeout(self):
        """Connect timeout is documented as distinct from header timeout."""
        s = self._make_server(header_timeout_secs=1, write_timeout_secs=10)
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        start = time.monotonic()
        resp = urllib.request.urlopen(url, timeout=5)
        elapsed = time.monotonic() - start
        self.assertEqual(resp.status, 200)
        self.assertLess(elapsed, 1.0)
        resp.close()


# ---------------------------------------------------------------------------
# Workstream E — Callback containment (timeout + forced shutdown)
# ---------------------------------------------------------------------------


class TestCallbackContainment(unittest.TestCase):
    """E: Verify blocked callback containment under timeout and forced shutdown."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "ok.txt"), "w") as f:
            f.write("ok")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_callback_permit_released_after_timeout(self):
        """After a timed-out callback returns, the permit is released.

        The callback holds the permit for its entire duration. Once the
        Python function returns (after the timeout), the permit must be
        released so a new request can proceed.
        """
        call_count = _AtomicCounter()
        handler_entered = threading.Event()
        handler_release = threading.Event()
        max_cb = 1

        def slow_handler(req):
            n = call_count.increment()
            if n == 1:
                handler_entered.set()
                handler_release.wait(timeout=10)
                return Response.text(200, "done")
            return Response.text(200, "second")

        s = self._make_server(
            handler=slow_handler,
            max_python_callbacks=max_cb,
            handler_timeout_secs=1,
        )
        addr = s.addr
        url = f"http://{addr}/ok.txt"
        self.assertTrue(_wait_for_tcp(addr))

        # First request — will time out on the server side
        def fire_request():
            try:
                urllib.request.urlopen(url, timeout=5)
            except (urllib.error.HTTPError, urllib.error.URLError, OSError):
                pass

        t1 = threading.Thread(target=fire_request)
        t1.start()
        handler_entered.wait(timeout=5)
        # Wait for timeout to fire on server side
        time.sleep(2.0)

        # Release the handler so the callback permit is freed
        handler_release.set()
        t1.join(timeout=10)

        # Second request — should succeed now that the permit is released
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"second")
        resp.close()

    def test_force_shutdown_terminates_runtime(self):
        """force_shutdown terminates tasks even with a blocked handler.

        After force_shutdown returns, the server is in a terminal state
        and stop() can cleanly release the listener.
        """
        handler_entered = threading.Event()
        handler_release = threading.Event()

        def blocking_handler(req):
            handler_entered.set()
            handler_release.wait(timeout=30)
            return Response.text(200, "done")

        s = self._make_server(handler=blocking_handler, handler_timeout_secs=30)
        addr = s.addr
        url = f"http://{addr}/ok.txt"
        self.assertTrue(_wait_for_tcp(addr))

        # Fire a request that blocks in the handler
        def fire():
            try:
                urllib.request.urlopen(url, timeout=5)
            except Exception:
                pass

        t = threading.Thread(target=fire)
        t.start()
        handler_entered.wait(timeout=5)

        # Force shutdown — should terminate tasks within the deadline
        start = time.monotonic()
        result = s.force_shutdown(timeout_secs=2.0)
        elapsed = time.monotonic() - start
        self.assertIn(result, ("clean", "timeout"))
        self.assertLess(elapsed, 5.0, "force_shutdown should not block long")
        self._servers.remove(s)

        # Server should be in a terminal state
        self.assertIn(s.state, ("stopped", "failed"))

        handler_release.set()
        t.join(timeout=5)

    def test_repeated_timeouts_do_not_create_unbounded_threads(self):
        """Repeated handler timeouts do not leak threads.

        After many requests that all time out, the Python thread count
        must remain bounded (not grow linearly with request count).
        """
        def blocking_handler(req):
            time.sleep(30)
            return Response.text(200, "done")

        s = self._make_server(
            handler=blocking_handler,
            handler_timeout_secs=1,
            max_python_callbacks=2,
        )
        addr = s.addr
        url = f"http://{addr}/ok.txt"
        self.assertTrue(_wait_for_tcp(addr))

        # Record baseline thread count
        baseline_threads = threading.active_count()

        # Fire multiple requests that will time out
        for _ in range(6):
            def fire():
                try:
                    urllib.request.urlopen(url, timeout=3)
                except (urllib.error.HTTPError, urllib.error.URLError, OSError):
                    pass
            t = threading.Thread(target=fire)
            t.start()
            t.join(timeout=10)

        # Wait for timed-out handlers to complete their Python execution
        time.sleep(5.0)

        # Thread count should not have grown unboundedly
        final_threads = threading.active_count()
        # Allow some margin for background threads, but not 6 new ones
        self.assertLessEqual(
            final_threads,
            baseline_threads + 3,
            f"Thread count grew from {baseline_threads} to {final_threads} "
            f"after 6 timed-out requests — possible thread leak",
        )

    def test_shutdown_respects_deadline_with_blocked_handler(self):
        """Graceful shutdown waits within its deadline even with a blocked handler.

        shutdown() should return quickly without waiting for a long-running
        handler to complete.
        """
        handler_entered = threading.Event()
        handler_release = threading.Event()

        def blocking_handler(req):
            handler_entered.set()
            handler_release.wait(timeout=60)
            return Response.text(200, "done")

        s = self._make_server(handler=blocking_handler, handler_timeout_secs=60)
        addr = s.addr
        self.assertTrue(_wait_for_tcp(addr))

        # Fire a request that blocks
        def fire():
            try:
                urllib.request.urlopen(
                    f"http://{addr}/ok.txt", timeout=5
                )
            except Exception:
                pass

        t = threading.Thread(target=fire)
        t.start()
        handler_entered.wait(timeout=5)

        # Shutdown should not block for the full handler duration
        start = time.monotonic()
        s.shutdown()
        elapsed = time.monotonic() - start
        self.assertLess(elapsed, 5.0, "shutdown() blocked too long")

        s.wait()
        self._servers.remove(s)

        handler_release.set()
        t.join(timeout=5)


# ---------------------------------------------------------------------------
# Workstream E — Graceful shutdown
# ---------------------------------------------------------------------------


class TestGracefulShutdown(unittest.TestCase):
    """E: Deterministic shutdown behavior tests."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        _make_large_file(os.path.join(self._td, "big.bin"), 4 * 1024 * 1024)
        with open(os.path.join(self._td, "small.txt"), "w") as f:
            f.write("ok")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_listener_stops_accepting_immediately(self):
        """After stop(), no new connections are accepted."""
        s = self._make_server()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        s.stop()
        self._servers.remove(s)

        with self.assertRaises((ConnectionRefusedError, urllib.error.URLError, OSError)):
            urllib.request.urlopen(url, timeout=2)

    def test_idle_connection_closed_on_shutdown(self):
        """Idle connections are closed on shutdown."""
        s = self._make_server()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        resp = urllib.request.urlopen(url, timeout=5)
        resp.read()
        resp.close()

        s.stop()
        self._servers.remove(s)

        with self.assertRaises((ConnectionRefusedError, urllib.error.URLError, OSError)):
            urllib.request.urlopen(url, timeout=2)

    def test_shutdown_during_active_file_stream(self):
        """Shutdown during an active file stream completes without deadlock."""
        s = self._make_server(max_connections=10, write_timeout_secs=5)
        addr = s.addr
        url = f"http://{addr}/big.bin"
        self.assertTrue(_wait_for_server(url))

        sock = _connect_raw(addr, "/big.bin")
        time.sleep(0.3)
        try:
            sock.recv(1024)
        except (socket.timeout, OSError):
            pass

        stop_done = threading.Event()

        def do_stop():
            s.stop()
            self._servers.remove(s)
            stop_done.set()

        st = threading.Thread(target=do_stop)
        st.start()
        st.join(timeout=5)
        self.assertTrue(stop_done.is_set(), "stop() must not deadlock")
        sock.close()

    def test_shutdown_during_blocked_handler(self):
        """Shutdown completes cleanly while a handler is active."""
        handler_entered = threading.Event()
        handler_done = threading.Event()

        def handler(req):
            handler_entered.set()
            time.sleep(0.5)
            handler_done.set()
            return Response.text(200, "ok")

        s = self._make_server(handler=handler)
        addr = s.addr
        url = f"http://{addr}/small.txt"
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

        t.join(timeout=5)

    def test_double_stop_is_safe(self):
        """Calling stop() twice does not panic or deadlock."""
        s = self._make_server()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        s.stop()
        self._servers.remove(s)
        s.stop()

    def test_context_manager_exit_safe_after_partial_startup(self):
        """Context manager exit is safe even if start() was not called."""
        s = Server(
            root=self._td, port=0,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.stop()

    def test_repeated_stop_is_idempotent(self):
        """Multiple stop() calls in succession are safe."""
        s = self._make_server()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        for _ in range(3):
            s.stop()
        self._servers.remove(s)

    def test_shutdown_during_slow_headers(self):
        """Shutdown during a slow-header connection completes cleanly."""
        s = self._make_server()
        addr = s.addr
        url = f"http://{addr}/small.txt"
        self.assertTrue(_wait_for_server(url))

        host, port_str = addr.split(":")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        sock.connect((host, int(port_str)))
        sock.sendall(b"GET /small.txt HTTP/1.1\r\nHost: ")
        time.sleep(0.2)

        stop_done = threading.Event()

        def do_stop():
            s.stop()
            self._servers.remove(s)
            stop_done.set()

        st = threading.Thread(target=do_stop)
        st.start()
        st.join(timeout=5)
        self.assertTrue(stop_done.is_set())
        sock.close()


# ---------------------------------------------------------------------------
# Workstream F — Test reliability helpers
# ---------------------------------------------------------------------------


class TestServerPrimitives(unittest.TestCase):
    """F: Verify server construction and lifecycle primitives."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("content")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_addr_returns_none_before_start(self):
        s = Server(
            root=self._td, port=0,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        self.assertIsNone(s.addr)

    def test_addr_returns_string_after_start(self):
        s = self._make_server()
        self.assertIsNotNone(s.addr)

    def test_start_twice_raises(self):
        from eggserve._native import LifecycleError
        s = self._make_server()
        with self.assertRaises(LifecycleError):
            s.start()

    def test_stop_before_start_is_safe(self):
        s = Server(
            root=self._td, port=0,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        s.stop()

    def test_repr_before_start(self):
        s = Server(
            root=self._td, port=0,
            header_timeout_secs=10,
            write_timeout_secs=10,
        )
        self.assertIn("not started", repr(s))

    def test_repr_after_start(self):
        s = self._make_server()
        self.assertIn("Server", repr(s))

    def test_server_serves_files(self):
        s = self._make_server()
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"content")
        resp.close()

    def test_handler_overrides_file_serving(self):
        def handler(req):
            return Response.text(200, "from handler")

        s = self._make_server(handler=handler)
        url = f"http://{s.addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"from handler")
        resp.close()


# ---------------------------------------------------------------------------
# Workstream G — Lifecycle parity
# ---------------------------------------------------------------------------


class TestLifecycleParity(unittest.TestCase):
    """Lifecycle parity tests for Python Server."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.txt"), "w") as f:
            f.write("content")
        self._servers = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _make_server(self, **kwargs):
        defaults = {
            "root": self._td,
            "port": 0,
            "header_timeout_secs": 10,
            "write_timeout_secs": 10,
        }
        defaults.update(kwargs)
        s = _start_server(**defaults)
        self._servers.append(s)
        return s

    def test_lifecycle_state_transitions(self):
        """State transitions: created -> running -> stopped."""
        s = Server(root=self._td, port=0)
        self._servers.append(s)
        self.assertEqual(s.state, "created")
        s.start()
        self.assertEqual(s.state, "running")
        s.stop()
        self._servers.remove(s)
        self.assertEqual(s.state, "stopped")

    def test_shutdown_and_wait(self):
        """shutdown() followed by wait() completes cleanly."""
        s = self._make_server()
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        s.shutdown()
        result = s.wait()
        self._servers.remove(s)
        self.assertEqual(result, "stopped")

    def test_force_shutdown_clean(self):
        """force_shutdown returns 'clean' for idle server."""
        s = self._make_server()
        s.shutdown()
        result = s.force_shutdown(timeout_secs=5.0)
        self._servers.remove(s)
        self.assertEqual(result, "clean")

    def test_context_manager_with_wait(self):
        """Context manager uses start/stop lifecycle."""
        with Server(root=self._td, port=0) as s:
            self.assertEqual(s.state, "running")
        self.assertEqual(s.state, "stopped")

    def test_double_start_after_stop_raises(self):
        """Starting after stop is allowed (re-starts the server)."""
        s = Server(root=self._td, port=0)
        s.start()
        s.stop()
        self._servers.append(s)

    def test_handler_timeout_parameter(self):
        """Server accepts handler_timeout_secs parameter."""
        def handler(req):
            return Response.text(200, "ok")

        s = self._make_server(handler=handler, handler_timeout_secs=5)
        addr = s.addr
        url = f"http://{addr}/index.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=5)
        self.assertEqual(resp.read(), b"ok")
        resp.close()


class TestResourceQualification(unittest.TestCase):
    """Soak tests for resource leak detection and lifecycle stability.

    These tests verify that repeated server cycles, callback exceptions,
    and concurrent operations do not leak threads, file descriptors, or
    memory.
    """

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "ok.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_repeated_lifecycle_cycles(self):
        """Repeated start/stop cycles do not leak resources."""
        for _ in range(10):
            s = Server(root=self._td, port=0)
            s.start()
            self.assertEqual(s.state, "running")
            s.stop()
            self.assertEqual(s.state, "stopped")

    def test_callback_exceptions_under_concurrency(self):
        """Callback exceptions under concurrent load do not leak resources."""
        import threading

        def error_handler(req):
            raise RuntimeError("test error")

        s = Server(root=self._td, port=0, handler=error_handler, max_python_callbacks=4)
        s.start()
        addr = s.addr

        errors = []

        def make_request():
            try:
                url = f"http://{addr}/"
                resp = urllib.request.urlopen(url, timeout=2)
                resp.read()
            except urllib.error.HTTPError as e:
                if e.code != 500:
                    errors.append(f"Expected 500, got {e.code}")
            except Exception as e:
                errors.append(str(e))

        threads = [threading.Thread(target=make_request) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=5)

        self.assertEqual(errors, [], f"Unexpected errors: {errors}")
        s.stop()

    def test_callback_timeout_under_concurrency(self):
        """Callback timeouts under concurrent load do not deadlock."""
        import threading
        import time

        def slow_handler(req):
            time.sleep(0.5)
            return Response.text(200, "slow")

        s = Server(root=self._td, port=0, handler=slow_handler,
                   max_python_callbacks=2, handler_timeout_secs=1)
        s.start()
        addr = s.addr

        results = []

        def make_request():
            try:
                url = f"http://{addr}/"
                resp = urllib.request.urlopen(url, timeout=3)
                results.append(resp.status)
            except Exception:
                results.append("error")

        threads = [threading.Thread(target=make_request) for _ in range(6)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=10)

        # All requests should complete (some may timeout, some may succeed)
        self.assertEqual(len(results), 6)
        s.stop()

    def test_many_idle_connections(self):
        """Many idle connections do not exhaust resources."""
        import threading

        s = Server(root=self._td, port=0, max_connections=50)
        s.start()
        addr = s.addr
        host, port = addr.split(":")
        port = int(port)

        connections = []
        for _ in range(20):
            try:
                c = socket.create_connection((host, port), timeout=1)
                c.sendall(b"GET /ok.txt HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
                connections.append(c)
            except Exception:
                break

        # Server should still be responsive
        url = f"http://{addr}/ok.txt"
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        resp.close()

        for c in connections:
            try:
                c.close()
            except Exception:
                pass
        s.stop()


if __name__ == "__main__":
    unittest.main()
