"""Standalone packaging smoke test — lifecycle and shutdown validation.

Tests server lifecycle methods (wait_ready, shutdown, force_shutdown, wait,
state property) from an installed wheel. Verifies lifecycle parity with the
Rust runtime.

Must be run from an installed wheel (pip install eggserve), NOT from the
source tree. Uses only stdlib + eggserve.
"""

import os
import shutil
import tempfile
import time
import unittest

from eggserve import Server


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


if __name__ == "__main__":
    unittest.main()
