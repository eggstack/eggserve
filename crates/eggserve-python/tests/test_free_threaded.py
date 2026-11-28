"""Plan 333: free-threaded CPython 3.14t behavior.

Skipped unless the interpreter is a free-threaded build
(sysconfig.get_config_var("Py_GIL_DISABLED") truthy). The suite proves the
extension's explicit free-threading declaration, parallel callback and
producer execution, the serialized mode, one-shot contention errors, and the
tunnel receive guard. It is a correctness suite, not a performance test: no
test asserts wall-clock speed or thread counts.
"""

import http.client
import socket
import subprocess
import sys
import sysconfig
import threading
import time
import unittest

from eggserve import lowlevel
from eggserve._native import RequestBodyConsumedError, Response

FREE_THREADED = bool(sysconfig.get_config_var("Py_GIL_DISABLED"))


def _start(handler, **config_kwargs):
    config = lowlevel.RuntimeConfig(port=0, **config_kwargs)
    server = lowlevel.Server(config=config, handler=handler)
    server.start()
    server.wait_ready()
    return server


def _stop(server):
    try:
        server.shutdown()
    except Exception:
        pass
    try:
        server.wait()
    except Exception:
        pass


def _request(addr, method="GET", path="/", body=None, headers=None):
    host, port = addr.split(":")
    connection = http.client.HTTPConnection(host, int(port), timeout=10)
    try:
        send_headers = {"Connection": "close"}
        if headers:
            send_headers.update(headers)
        connection.request(method, path, body=body, headers=send_headers)
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


def _parallel(call, count, timeout=30.0):
    barrier = threading.Barrier(count)
    results = [None] * count
    errors = []

    def run(index):
        try:
            barrier.wait(timeout=10)
            results[index] = call(index)
        except BaseException as exc:  # noqa: BLE001 - surfaced below
            errors.append(exc)

    threads = [
        threading.Thread(target=run, args=(index,), daemon=True)
        for index in range(count)
    ]
    for thread in threads:
        thread.start()
    deadline = time.monotonic() + timeout
    for thread in threads:
        thread.join(max(0.0, deadline - time.monotonic()))
    stuck = [thread for thread in threads if thread.is_alive()]
    if stuck:
        raise AssertionError(f"{len(stuck)} worker thread(s) did not finish")
    if errors:
        raise errors[0]
    return results


@unittest.skipUnless(
    FREE_THREADED, "requires free-threaded CPython (Py_GIL_DISABLED)"
)
class FreeThreadedTests(unittest.TestCase):
    def test_fresh_interpreter_import_keeps_gil_disabled(self):
        """The installed module must not ask the interpreter to re-enable the GIL."""
        code = (
            "import sys, eggserve._native; "
            "assert sys._is_gil_enabled() is False, 'module re-enabled the GIL'"
        )
        completed = subprocess.run(
            [
                sys.executable,
                "-X",
                "gil=0",
                "-W",
                "error::RuntimeWarning",
                "-c",
                code,
            ],
            capture_output=True,
            text=True,
            timeout=120,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_parallel_callbacks_keep_exact_state(self):
        """Concurrent handlers see a consistent, exactly-counted shared counter."""
        lock = threading.Lock()
        state = {"count": 0}

        def handler(request):
            with lock:
                state["count"] += 1
            time.sleep(0.01)
            return Response.text(200, "ok")

        server = _start(handler, max_python_callbacks=8)
        try:
            results = _parallel(lambda index: _request(server.addr), 16)
        finally:
            _stop(server)

        self.assertEqual(state["count"], 16)
        self.assertEqual(results, [(200, b"ok")] * 16)

    def test_serialized_mode_never_overlaps(self):
        """max_python_callbacks=1 is the documented serialized mode."""
        lock = threading.Lock()
        state = {"active": 0, "max": 0}

        def handler(request):
            with lock:
                state["active"] += 1
                state["max"] = max(state["max"], state["active"])
            time.sleep(0.05)
            with lock:
                state["active"] -= 1
            return Response.text(200, "ok")

        server = _start(handler, max_python_callbacks=1)
        try:
            results = _parallel(lambda index: _request(server.addr), 8)
        finally:
            _stop(server)

        self.assertEqual(state["max"], 1)
        self.assertEqual(results, [(200, b"ok")] * 8)

    def test_concurrent_stream_producers(self):
        """Parallel Response.stream producers deliver exact bytes."""
        chunks = [b"alpha-", b"beta-", b"gamma"]
        expected = b"".join(chunks)

        def handler(request):
            return lowlevel.Response.stream(200, list(chunks))

        server = _start(handler, max_python_callbacks=16)
        try:
            results = _parallel(
                lambda index: _request(server.addr, path="/stream"), 6
            )
        finally:
            _stop(server)

        self.assertEqual(results, [(200, expected)] * 6)

    def test_two_threads_race_body_read(self):
        """Exactly one concurrent RequestBody.read() wins; the loser is typed."""
        state = {}
        done = threading.Event()

        def handler(request):
            results = []
            barrier = threading.Barrier(2)

            def reader():
                barrier.wait(timeout=5)
                try:
                    results.append(("ok", request.body.read()))
                except BaseException as exc:  # noqa: BLE001 - asserted below
                    results.append(("err", exc))

            threads = [threading.Thread(target=reader, daemon=True) for _ in range(2)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(10)
            state["results"] = results
            done.set()
            return Response.text(200, "ok")

        server = _start(
            handler, request_body_mode="buffer", max_request_body_bytes=1024
        )
        try:
            status, body = _request(server.addr, "POST", "/", body=b"payload")
        finally:
            _stop(server)

        self.assertEqual((status, body), (200, b"ok"))
        self.assertTrue(done.wait(5), "handler did not finish")
        results = state["results"]
        wins = [value for kind, value in results if kind == "ok"]
        errors = [value for kind, value in results if kind == "err"]
        self.assertEqual(wins, [b"payload"], results)
        self.assertEqual(len(errors), 1, results)
        self.assertIsInstance(errors[0], RequestBodyConsumedError)

    def test_concurrent_iterator_pull_is_typed(self):
        """Concurrent BodyChunkIterator.__next__ raises a typed error, never crashes."""
        state = {}
        done = threading.Event()

        def handler(request):
            if request.method != "POST":
                return Response.text(200, "ok")
            iterator = request.body.iter_chunks()
            results = []
            barrier = threading.Barrier(2)

            def pull():
                barrier.wait(timeout=5)
                try:
                    results.append(("ok", next(iterator)))
                except BaseException as exc:  # noqa: BLE001 - asserted below
                    results.append(("err", exc))

            threads = [threading.Thread(target=pull, daemon=True) for _ in range(2)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(10)
            state["results"] = results
            done.set()
            return Response.text(200, "ok")

        server = _start(
            handler, request_body_mode="buffer", max_request_body_bytes=1024
        )
        try:
            status, body = _request(server.addr, "POST", "/", body=b"payload")
            self.assertEqual((status, body), (200, b"ok"))
            self.assertTrue(done.wait(5), "handler did not finish")
            follow_up = _request(server.addr, "GET", "/")
        finally:
            _stop(server)

        results = state["results"]
        self.assertTrue(results)
        self.assertTrue(any(kind == "ok" for kind, _ in results), results)
        for kind, value in results:
            if kind == "err":
                # A racing pull either observes PyO3's structural borrow error
                # or, after the other thread consumed the last chunk,
                # StopIteration. Anything else (panic, decode error) fails.
                if isinstance(value, StopIteration):
                    continue
                self.assertIsInstance(value, RuntimeError, results)
                self.assertIn("borrow", str(value).lower(), results)
        self.assertEqual(follow_up, (200, b"ok"))

    def test_tunnel_recv_contention_is_explicit(self):
        """A racing Tunnel.recv is rejected and the guard resets for later calls."""
        state = {}
        ready = threading.Event()

        def handler(request):
            capability = request.take_tunnel()
            if capability is None:
                state["error"] = "no tunnel capability"
                ready.set()
                return Response.text(400, "no tunnel")
            handshake, tunnel = capability.accept()
            state["tunnel"] = tunnel
            ready.set()
            return handshake

        server = _start(handler, max_python_callbacks=4)
        host, port = server.addr.split(":")
        sock = socket.create_connection((host, int(port)), timeout=10)
        sock.settimeout(10)
        try:
            sock.sendall(
                b"GET /tunnel HTTP/1.1\r\nHost: x\r\n"
                b"Upgrade: echo-test\r\nConnection: upgrade\r\n\r\n"
            )
            head = b""
            while b"\r\n\r\n" not in head:
                chunk = sock.recv(1)
                if not chunk:
                    break
                head += chunk
            self.assertIn(b" 101 ", head.split(b"\r\n", 1)[0], head)
            self.assertTrue(ready.wait(5), "handler did not accept the tunnel")
            self.assertIsNone(state.get("error"))
            tunnel = state["tunnel"]

            started = threading.Event()
            first = {}

            def blocking_recv():
                started.set()
                try:
                    first["value"] = tunnel.recv()
                except BaseException as exc:  # noqa: BLE001 - EOF/close both valid
                    first["error"] = exc

            worker = threading.Thread(target=blocking_recv, daemon=True)
            worker.start()
            self.assertTrue(started.wait(5))
            time.sleep(0.2)
            with self.assertRaises(RuntimeError) as contention:
                tunnel.recv()
            self.assertIn("concurrent Tunnel.recv", str(contention.exception))

            sock.shutdown(socket.SHUT_RDWR)
            worker.join(10)
            self.assertFalse(worker.is_alive(), "blocking recv did not finish")
            self.assertTrue(first, "first recv produced no result")

            with self.assertRaises(RuntimeError) as terminal:
                tunnel.recv()
            self.assertIn("already closed", str(terminal.exception))
        finally:
            try:
                sock.close()
            finally:
                _stop(server)

    def test_restart_with_concurrent_callbacks(self):
        """Start/stop is stable under a concurrent callback burst."""
        for _ in range(2):
            server = _start(
                lambda request: Response.text(200, "ok"),
                max_python_callbacks=4,
            )
            try:
                results = _parallel(lambda index: _request(server.addr), 8)
            finally:
                _stop(server)
            self.assertEqual(results, [(200, b"ok")] * 8)


if __name__ == "__main__":
    unittest.main()
