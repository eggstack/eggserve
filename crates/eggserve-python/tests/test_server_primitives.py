"""Tests for eggserve server primitives.

Uses stdlib unittest to validate StaticPolicyWrapper, ServerSecureRoot,
StaticResponder, Response, BodySource, and Server lifecycle.
"""

import os
import tempfile
import time
import unittest
import urllib.error
import urllib.request

from eggserve._native import (
    BodySource,
    Request,
    Response,
    Server,
    ServerBodySource,
    ServerRequestError,
    ServerSecureRoot,
    StaticPolicyWrapper,
    StaticResponder,
)


class TestStaticPolicyWrapper(unittest.TestCase):
    def test_defaults(self):
        p = StaticPolicyWrapper()
        self.assertFalse(p.directory_listing)
        self.assertFalse(p.follow_symlinks)
        self.assertFalse(p.allow_dotfiles)

    def test_custom_values(self):
        p = StaticPolicyWrapper(
            directory_listing=True, follow_symlinks=True, allow_dotfiles=True
        )
        self.assertTrue(p.directory_listing)
        self.assertTrue(p.follow_symlinks)
        self.assertTrue(p.allow_dotfiles)

    def test_partial_override(self):
        p = StaticPolicyWrapper(directory_listing=True)
        self.assertTrue(p.directory_listing)
        self.assertFalse(p.follow_symlinks)
        self.assertFalse(p.allow_dotfiles)

    def test_frozen(self):
        p = StaticPolicyWrapper()
        with self.assertRaises(AttributeError):
            p.directory_listing = True  # type: ignore[misc]


class TestServerSecureRoot(unittest.TestCase):
    def test_construction(self):
        with tempfile.TemporaryDirectory() as td:
            sr = ServerSecureRoot(td)
            self.assertEqual(os.path.realpath(sr.root_path), os.path.realpath(td))

    def test_construction_with_policy(self):
        with tempfile.TemporaryDirectory() as td:
            p = StaticPolicyWrapper(allow_dotfiles=True)
            sr = ServerSecureRoot(td, policy=p)
            self.assertEqual(os.path.realpath(sr.root_path), os.path.realpath(td))

    def test_missing_root_raises(self):
        with self.assertRaises(ValueError):
            ServerSecureRoot("/nonexistent_root_xyz_12345")

    def test_frozen(self):
        with tempfile.TemporaryDirectory() as td:
            sr = ServerSecureRoot(td)
            with self.assertRaises(AttributeError):
                sr.root_path = "/other"  # type: ignore[misc]


class TestResponse(unittest.TestCase):
    def test_empty(self):
        r = Response.empty(204)
        self.assertEqual(r.status, 204)
        self.assertEqual(r.headers, {})

    def test_bytes(self):
        r = Response.bytes(200, b"hello")
        self.assertEqual(r.status, 200)
        self.assertEqual(r.headers, {})

    def test_bytes_with_headers(self):
        r = Response.bytes(200, b"x", headers={"content-type": "application/octet-stream"})
        self.assertEqual(r.headers["content-type"], "application/octet-stream")

    def test_text(self):
        r = Response.text(200, "hello world")
        self.assertEqual(r.status, 200)
        self.assertEqual(r.headers["content-type"], "text/plain; charset=utf-8")

    def test_text_with_custom_content_type(self):
        r = Response.text(200, "ok", headers={"content-type": "text/html"})
        self.assertEqual(r.headers["content-type"], "text/html")

    def test_repr(self):
        r = Response.empty(404)
        self.assertIn("404", repr(r))


class TestBodySource(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "test.txt"), "wb") as f:
            f.write(b"hello world")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_kind_full(self):
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        r = responder.respond("GET", "/test.txt")
        self.assertEqual(r.status, 200)
        self.assertIn("content-length", r.headers)

    def test_kind_range(self):
        root = ServerSecureRoot(self._td)
        responder = StaticResponder(root)
        r = responder.respond("GET", "/test.txt", headers={"range": "bytes=0-4"})
        self.assertEqual(r.status, 206)
        self.assertIn("content-range", r.headers)


class TestStaticResponder(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "hello.txt"), "w") as f:
            f.write("hello world")
        with open(os.path.join(self._td, "data.bin"), "wb") as f:
            f.write(b"\x00" * 100)
        os.makedirs(os.path.join(self._td, "subdir"))
        with open(os.path.join(self._td, "subdir", "nested.txt"), "w") as f:
            f.write("nested")
        self._root = ServerSecureRoot(self._td)
        self._responder = StaticResponder(self._root)

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_get_file(self):
        r = self._responder.respond("GET", "/hello.txt")
        self.assertEqual(r.status, 200)
        self.assertIn("content-type", r.headers)
        self.assertIn("content-length", r.headers)

    def test_head_file(self):
        r = self._responder.respond("HEAD", "/hello.txt")
        self.assertEqual(r.status, 200)
        self.assertIn("content-length", r.headers)

    def test_not_found(self):
        r = self._responder.respond("GET", "/nonexistent.txt")
        self.assertEqual(r.status, 404)

    def test_directory_forbidden(self):
        r = self._responder.respond("GET", "/subdir")
        self.assertEqual(r.status, 403)

    def test_method_not_allowed(self):
        with self.assertRaises(ValueError):
            self._responder.respond("POST", "/hello.txt")

    def test_method_not_allowed_put(self):
        with self.assertRaises(ValueError):
            self._responder.respond("PUT", "/hello.txt")

    def test_target_no_leading_slash(self):
        with self.assertRaises(ValueError):
            self._responder.respond("GET", "hello.txt")

    def test_body_not_allowed(self):
        with self.assertRaises(ValueError):
            self._responder.respond("GET", "/hello.txt", has_body=True)

    def test_range_request(self):
        r = self._responder.respond("GET", "/hello.txt", headers={"range": "bytes=0-4"})
        self.assertEqual(r.status, 206)
        self.assertIn("content-range", r.headers)

    def test_conditional_304(self):
        r1 = self._responder.respond("GET", "/hello.txt")
        etag = r1.headers.get("etag")
        self.assertIsNotNone(etag)
        r2 = self._responder.respond(
            "GET", "/hello.txt", headers={"if-none-match": etag}
        )
        self.assertEqual(r2.status, 304)

    def test_conditional_200_no_match(self):
        r = self._responder.respond(
            "GET", "/hello.txt", headers={"if-none-match": "W/\"bogus\""}
        )
        self.assertEqual(r.status, 200)

    def test_dotfile_denied_by_default(self):
        with open(os.path.join(self._td, ".hidden"), "w") as f:
            f.write("secret")
        r = self._responder.respond("GET", "/.hidden")
        self.assertEqual(r.status, 403)

    def test_symlink_denied_by_default(self):
        target = os.path.join(self._td, "hello.txt")
        link = os.path.join(self._td, "link.txt")
        os.symlink(target, link)
        r = self._responder.respond("GET", "/link.txt")
        self.assertEqual(r.status, 403)

    def test_repr(self):
        r = repr(self._responder)
        self.assertIn("StaticResponder", r)


class TestStaticResponderWithPolicy(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, ".env"), "w") as f:
            f.write("SECRET=123")
        policy = StaticPolicyWrapper(allow_dotfiles=True)
        self._root = ServerSecureRoot(self._td, policy=policy)
        self._responder = StaticResponder(self._root)

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_dotfile_served_with_policy(self):
        r = self._responder.respond("GET", "/.env")
        self.assertEqual(r.status, 200)


class TestServer(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "index.html"), "w") as f:
            f.write("<h1>Hello</h1>")
        with open(os.path.join(self._td, "data.txt"), "w") as f:
            f.write("test data")

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def _wait_for_server(self, url, timeout=5.0):
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

    def test_construction(self):
        s = Server(root=self._td, port=0)
        self.assertIsNone(s.addr)
        self.assertIn("not started", repr(s))

    def test_start_and_stop(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        self.assertIsNotNone(addr)
        self.assertIn(":", addr)
        s.stop()

    def test_addr_after_start(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        self.assertIsNotNone(addr)
        url = f"http://{addr}/data.txt"
        self.assertTrue(self._wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.read(), b"test data")
        s.stop()

    def test_context_manager(self):
        with Server(root=self._td, port=0) as s:
            addr = s.addr
            self.assertIsNotNone(addr)
            url = f"http://{addr}/index.html"
            self.assertTrue(self._wait_for_server(url))
            resp = urllib.request.urlopen(url, timeout=2)
            self.assertEqual(resp.read(), b"<h1>Hello</h1>")
        self.assertIsNone(s.addr)

    def test_double_start_raises(self):
        from eggserve._native import LifecycleError
        s = Server(root=self._td, port=0)
        s.start()
        with self.assertRaises(LifecycleError):
            s.start()
        s.stop()

    def test_stop_without_start(self):
        s = Server(root=self._td, port=0)
        s.stop()

    def test_not_found_returns_404(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/nonexistent"
        self.assertTrue(self._wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 404)
        finally:
            s.stop()

    def test_method_not_allowed_returns_405(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/data.txt"
        self.assertTrue(self._wait_for_server(url))
        req = urllib.request.Request(url, method="POST")
        try:
            urllib.request.urlopen(req, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 405)
        finally:
            s.stop()

    def test_head_request(self):
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/data.txt"
        self.assertTrue(self._wait_for_server(url))
        req = urllib.request.Request(url, method="HEAD")
        resp = urllib.request.urlopen(req, timeout=2)
        self.assertEqual(resp.status, 200)
        data = resp.read()
        self.assertEqual(data, b"")
        s.stop()

    def test_repr_not_started(self):
        s = Server(root=self._td, port=0)
        self.assertIn("not started", repr(s))

    def test_invalid_root_raises(self):
        with self.assertRaises(ValueError):
            Server(root="/nonexistent_root_xyz_12345")

    def test_custom_policy(self):
        policy = StaticPolicyWrapper(allow_dotfiles=True)
        with open(os.path.join(self._td, ".secret"), "w") as f:
            f.write("key")
        s = Server(root=self._td, port=0, policy=policy)
        s.start()
        addr = s.addr
        url = f"http://{addr}/.secret"
        self.assertTrue(self._wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.read(), b"key")
        s.stop()

    def test_default_policy_denies_dotfiles(self):
        with open(os.path.join(self._td, ".secret"), "w") as f:
            f.write("key")
        s = Server(root=self._td, port=0)
        s.start()
        addr = s.addr
        url = f"http://{addr}/.secret"
        self.assertTrue(self._wait_for_server(url))
        try:
            urllib.request.urlopen(url, timeout=2)
            self.fail("Expected HTTPError")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 403)
        finally:
            s.stop()

    def test_state_created(self):
        """State is 'created' before start."""
        s = Server(root=self._td, port=0)
        self.assertEqual(s.state, "created")

    def test_state_running_after_start(self):
        """State is 'running' after start."""
        s = Server(root=self._td, port=0)
        s.start()
        self.assertEqual(s.state, "running")
        s.stop()

    def test_state_stopped_after_stop(self):
        """State is 'stopped' after stop."""
        s = Server(root=self._td, port=0)
        s.start()
        s.stop()
        self.assertEqual(s.state, "stopped")

    def test_wait_ready_before_start(self):
        """wait_ready raises before start."""
        from eggserve._native import LifecycleError
        s = Server(root=self._td, port=0)
        with self.assertRaises(LifecycleError):
            s.wait_ready()

    def test_wait_ready_after_start(self):
        """wait_ready succeeds after start."""
        s = Server(root=self._td, port=0)
        s.start()
        s.wait_ready()
        s.stop()

    def test_shutdown_nonblocking(self):
        """shutdown() returns immediately without blocking."""
        s = Server(root=self._td, port=0)
        s.start()
        s.shutdown()
        self.assertIn(s.state, ("running", "draining"))  # state may still be running briefly
        s.wait()

    def test_wait_returns_stopped(self):
        """wait() returns 'stopped' string."""
        s = Server(root=self._td, port=0)
        s.start()
        s.shutdown()
        result = s.wait()
        self.assertEqual(result, "stopped")

    def test_force_shutdown_returns_string(self):
        """force_shutdown returns 'clean' or 'timeout'."""
        s = Server(root=self._td, port=0)
        s.start()
        result = s.force_shutdown(timeout_secs=5.0)
        self.assertIn(result, ("clean", "timeout"))

    def test_handler_timeout_secs_validation(self):
        """Zero handler_timeout_secs is rejected."""
        with self.assertRaises(ValueError):
            Server(root=self._td, port=0, handler_timeout_secs=0)

    def test_graceful_shutdown_timeout_secs_validation(self):
        """Zero graceful_shutdown_timeout_secs is rejected."""
        with self.assertRaises(ValueError):
            Server(root=self._td, port=0, graceful_shutdown_timeout_secs=0)

    def test_handler_timeout(self):
        """Handler that exceeds handler_timeout still completes."""
        import time
        def slow_handler(req):
            time.sleep(0.5)
            return Response.text(200, "ok")

        s = Server(root=self._td, port=0, handler=slow_handler, handler_timeout_secs=1)
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.html"
        self.assertTrue(self._wait_for_server(url))
        resp = urllib.request.urlopen(url, timeout=2)
        self.assertEqual(resp.read(), b"ok")
        s.stop()

    def test_coroutine_handler_rejected(self):
        """Handler returning a coroutine is rejected with 500."""
        async def async_handler(req):
            return Response.text(200, "ok")

        s = Server(root=self._td, port=0, handler=async_handler)
        s.start()
        addr = s.addr
        url = f"http://{addr}/index.html"
        self.assertTrue(self._wait_for_server(url))
        try:
            resp = urllib.request.urlopen(url, timeout=2)
            self.assertEqual(resp.status, 500)
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)
        finally:
            s.stop()


class TestServerConstructorValidation(unittest.TestCase):
    def test_zero_max_connections(self):
        with self.assertRaises(ValueError) as ctx:
            Server(root=".", max_connections=0)
        self.assertIn("max_connections", str(ctx.exception))

    def test_zero_max_file_streams(self):
        with self.assertRaises(ValueError) as ctx:
            Server(root=".", max_file_streams=0)
        self.assertIn("max_file_streams", str(ctx.exception))

    def test_zero_header_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            Server(root=".", header_timeout_secs=0)
        self.assertIn("header_timeout_secs", str(ctx.exception))

    def test_zero_connection_total_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            Server(root=".", connection_total_timeout_secs=0)
        self.assertIn("connection_total_timeout_secs", str(ctx.exception))

    def test_negative_max_connections(self):
        with self.assertRaises(OverflowError):
            Server(root=".", max_connections=-1)

    def test_negative_header_timeout(self):
        with self.assertRaises(OverflowError):
            Server(root=".", header_timeout_secs=-1)

    def test_nan_header_timeout(self):
        with self.assertRaises(TypeError):
            Server(root=".", header_timeout_secs=float("nan"))

    def test_inf_header_timeout(self):
        with self.assertRaises(TypeError):
            Server(root=".", header_timeout_secs=float("inf"))

    def test_float_max_connections(self):
        with self.assertRaises(TypeError):
            Server(root=".", max_connections=1.5)


class TestServerRequestError(unittest.TestCase):
    def test_method_not_allowed_message(self):
        try:
            with tempfile.TemporaryDirectory() as td:
                root = ServerSecureRoot(td)
                responder = StaticResponder(root)
                responder.respond("POST", "/file")
        except ValueError as e:
            self.assertIn("Method not allowed", str(e))
            return
        self.fail("Expected ValueError")

    def test_invalid_target_message(self):
        try:
            with tempfile.TemporaryDirectory() as td:
                root = ServerSecureRoot(td)
                responder = StaticResponder(root)
                responder.respond("GET", "no-slash")
        except ValueError as e:
            self.assertIn("Invalid request target", str(e))
            return
        self.fail("Expected ValueError")

    def test_body_not_allowed_message(self):
        try:
            with tempfile.TemporaryDirectory() as td:
                root = ServerSecureRoot(td)
                responder = StaticResponder(root)
                responder.respond("GET", "/file", has_body=True)
        except ValueError as e:
            self.assertIn("not allowed", str(e))
            return
        self.fail("Expected ValueError")


if __name__ == "__main__":
    unittest.main()
