"""Standalone packaging smoke test — lifecycle and shutdown validation.

Tests server lifecycle methods (wait_ready, shutdown, force_shutdown, wait,
state property) from an installed wheel. Verifies lifecycle parity with the
Rust runtime.

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


class TestLifecycleMethods(unittest.TestCase):
    """Lifecycle methods work correctly from installed wheel."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "ok.txt"), "w") as f:
            f.write("ok")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_state_created_before_start(self):
        s = Server(root=self._td, port=0)
        self.assertEqual(s.state, "created")
        s.stop()

    def test_state_running_after_start(self):
        s = Server(root=self._td, port=0)
        s.start()
        self.assertEqual(s.state, "running")
        s.stop()

    def test_state_stopped_after_stop(self):
        s = Server(root=self._td, port=0)
        s.start()
        s.stop()
        self.assertEqual(s.state, "stopped")

    def test_wait_ready_returns_when_running(self):
        s = Server(root=self._td, port=0)
        s.start()
        s.wait_ready()
        s.stop()

    def test_wait_ready_raises_when_not_started(self):
        from eggserve import LifecycleError
        s = Server(root=self._td, port=0)
        with self.assertRaises(LifecycleError):
            s.wait_ready()

    def test_shutdown_non_blocking(self):
        s = Server(root=self._td, port=0)
        s.start()
        start = time.monotonic()
        s.shutdown()
        elapsed = time.monotonic() - start
        self.assertLess(elapsed, 1.0)
        s.wait()
        s.stop()

    def test_force_shutdown_returns_string(self):
        s = Server(root=self._td, port=0)
        s.start()
        result = s.force_shutdown(timeout_secs=5.0)
        self.assertIn(result, ("clean", "timeout"))
        s.stop()

    def test_wait_returns_stopped(self):
        s = Server(root=self._td, port=0)
        s.start()
        s.shutdown()
        result = s.wait()
        self.assertEqual(result, "stopped")
        s.stop()

    def test_double_start_raises_lifecycle_error(self):
        from eggserve import LifecycleError
        s = Server(root=self._td, port=0)
        s.start()
        with self.assertRaises(LifecycleError):
            s.start()
        s.stop()


class TestContextManagerLifecycle(unittest.TestCase):
    """Context manager provides proper lifecycle management."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "ctx.txt"), "w") as f:
            f.write("context")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_context_manager_start_stop(self):
        with Server(root=self._td, port=0) as s:
            s.wait_ready()
            self.assertEqual(s.state, "running")
        self.assertEqual(s.state, "stopped")


class TestStaticResponse(unittest.TestCase):
    """Static file responses work correctly from installed wheel."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "hello.txt"), "w") as f:
            f.write("hello world")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_static_file_served(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/hello.txt"
        self.assertTrue(_wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"hello world")
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


class TestCallbackResponse(unittest.TestCase):
    """Python callback handlers return correct responses."""

    def setUp(self):
        self._td = tempfile.mkdtemp()

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


class TestCallbackException(unittest.TestCase):
    """Callback exceptions return 500 without leaking tracebacks."""

    def setUp(self):
        self._td = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

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


class TestCallbackTimeout(unittest.TestCase):
    """Handler timeout returns deterministic response for slow callbacks."""

    def setUp(self):
        self._td = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_slow_callback_times_out(self):
        import threading

        def handler(req):
            time.sleep(10)
            return Response.text(200, "should not reach")

        s = Server(root=self._td, port=0, handler=handler, handler_timeout_secs=1)
        s.start()
        addr = s.addr
        url = f"http://{addr}/test"
        self.assertTrue(_wait_for_server(url))
        start = time.monotonic()
        try:
            urllib.request.urlopen(url, timeout=5)
            self.fail("Expected HTTPError or timeout")
        except (urllib.error.HTTPError, urllib.error.URLError, socket.timeout, OSError):
            pass
        elapsed = time.monotonic() - start
        # Should respond quickly (timeout) rather than waiting for the handler
        self.assertLess(elapsed, 5.0, "timeout should respond within deadline")
        s.stop()

    def test_handler_timeout_concurrency_limit(self):
        """Timed-out callbacks still count against concurrency limit."""
        active = []
        max_active = []

        def handler(req):
            active.append(1)
            max_active.append(len(active))
            time.sleep(10)
            active.pop()
            return Response.text(200, "done")

        s = Server(
            root=self._td,
            port=0,
            handler=handler,
            handler_timeout_secs=1,
            max_python_callbacks=2,
        )
        s.start()
        addr = s.addr
        url = f"http://{addr}/test"
        self.assertTrue(_wait_for_server(url))

        # Fire several requests — after 2 concurrent handlers, new ones should
        # be rejected or queued (not spawn unbounded threads).
        threads = []
        for _ in range(5):
            t = threading.Thread(
                target=lambda: urllib.request.urlopen(url, timeout=5)
                or None
            )
            threads.append(t)
            t.start()
        for t in threads:
            t.join(timeout=10)

        # Even though we sent 5 requests, concurrency should be bounded
        if max_active:
            self.assertLessEqual(
                max_active[-1], 2, "concurrency should be bounded by max_python_callbacks"
            )
        s.stop()


class TestRepeatedStartStop(unittest.TestCase):
    """New Server instances can be created and cycled repeatedly."""

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "cycle.txt"), "w") as f:
            f.write("cycle")

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_repeated_start_stop_new_instances(self):
        for _ in range(3):
            s = Server(root=self._td, port=0)
            s.start()
            s.wait_ready()
            addr = s.addr
            url = f"http://{addr}/cycle.txt"
            self.assertTrue(_wait_for_server(url))
            resp = urllib.request.urlopen(url, timeout=2)
            self.assertEqual(resp.status, 200)
            s.stop()


class TestAddressReuse(unittest.TestCase):
    """Address is reusable after server stop."""

    def setUp(self):
        self._td = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self._td, ignore_errors=True)

    def test_same_port_reuse_after_stop(self):
        s1 = Server(root=self._td, port=0)
        s1.start()
        addr = s1.addr
        s1.stop()
        time.sleep(0.1)

        # Create a second server bound to the same port — should succeed
        host, port_str = addr.split(":")
        s2 = Server(root=self._td, host=host, port=int(port_str))
        s2.start()
        self.assertEqual(s2.addr, addr)
        s2.stop()


class TestNoSourceTreeFallback(unittest.TestCase):
    """Installed wheel does not import from source tree."""

    def test_import_does_not_use_source_tree(self):
        import sys
        # Verify that the eggserve module is not being imported from the
        # source tree (crates/eggserve-python/python/).
        eggserve_mod = sys.modules.get("eggserve")
        if eggserve_mod is not None:
            mod_file = getattr(eggserve_mod, "__file__", "") or ""
            self.assertNotIn(
                "crates/eggserve-python",
                mod_file,
                f"eggserve imported from source tree: {mod_file}",
            )


if __name__ == "__main__":
    unittest.main()
