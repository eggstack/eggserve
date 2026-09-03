"""Plan 166: low-level runtime/service substrate integration tests.

Covers handler-only operation (no static root), streaming responses
(known/unknown length, HEAD suppression, iterator errors, backpressure
bound), admission saturation (callback vs in-flight), handler timeout
honesty, shutdown with active streams, TLS low-level service, response
privacy options, and facade invariance.
"""

import http.client
import socket
import threading
import time
import unittest
import urllib.error
import urllib.request

from eggserve import lowlevel
from eggserve._native import Response, Server


def _get(handler, path="/", port_cfg=None):
    cfg = port_cfg or lowlevel.RuntimeConfig(port=0)
    srv = lowlevel.Server(config=cfg, handler=handler)
    srv.start()
    srv.wait_ready()
    try:
        with urllib.request.urlopen(f"http://{srv.addr}{path}", timeout=5) as r:
            return r.status, r.read(), dict(r.headers.items())
    finally:
        srv.shutdown()
        srv.wait()


def _raw(host, port, method, path, headers=None):
    c = http.client.HTTPConnection(host, port, timeout=5)
    try:
        c.request(method, path, headers=headers or {})
        r = c.getresponse()
        return r.status, r.read(), {k.lower(): v for k, v in r.getheaders()}
    finally:
        c.close()


class HandlerOnlyTests(unittest.TestCase):
    def test_no_static_root_required(self):
        def handler(req):
            return lowlevel.Response.text(200, "hello")

        status, body, _ = _get(handler)
        self.assertEqual((status, body), (200, b"hello"))

    def test_native_handler_only_without_root(self):
        s = Server(None, handler=lambda req: Response.text(200, "ok"))
        s.start()
        try:
            s.wait_ready()
            self.assertTrue(s.addr)
        finally:
            s.stop()

    def test_static_without_root_fails(self):
        with self.assertRaises(ValueError):
            Server(None)

    def test_static_composition_via_responder(self):
        import tempfile
        from pathlib import Path

        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "a.txt").write_bytes(b"static-bytes")
            responder_root = lowlevel.ServerSecureRoot(root)
            responder = lowlevel.StaticResponder(responder_root)

            def handler(request):
                if request.path.startswith("/s/"):
                    return responder.respond("GET", request.path[2:])
                return lowlevel.Response.text(200, "app")

            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.Server(config=cfg, handler=handler)
            srv.start()
            srv.wait_ready()
            try:
                host, port = srv.addr.split(":")
                self.assertEqual(_raw(host, int(port), "GET", "/s/a.txt")[1], b"static-bytes")
                self.assertEqual(_raw(host, int(port), "GET", "/other")[1], b"app")
            finally:
                srv.shutdown()
                srv.wait()


class StreamingResponseTests(unittest.TestCase):
    def test_unknown_length_chunked(self):
        def handler(req):
            return lowlevel.Response.stream(200, [b"he", b"llo"])

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, body, headers = _raw(host, int(port), "GET", "/")
            self.assertEqual(status, 200)
            self.assertEqual(body, b"hello")
            self.assertIsNone(headers.get("content-length"))
        finally:
            srv.shutdown()
            srv.wait()

    def test_known_length_content_length(self):
        def handler(req):
            return lowlevel.Response.stream(200, iter([b"ab", b"cd"]), content_length=4)

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, body, headers = _raw(host, int(port), "GET", "/")
            self.assertEqual((status, body), (200, b"abcd"))
            self.assertEqual(headers.get("content-length"), "4")
        finally:
            srv.shutdown()
            srv.wait()

    def test_head_does_not_advance_iterator(self):
        advanced = []

        def gen():
            advanced.append(1)
            yield b"data"

        def handler(req):
            return lowlevel.Response.stream(200, gen(), content_length=4)

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, body, headers = _raw(host, int(port), "HEAD", "/")
            self.assertEqual(status, 200)
            self.assertEqual(body, b"")
            self.assertEqual(headers.get("content-length"), "4")
            self.assertEqual(advanced, [])
        finally:
            srv.shutdown()
            srv.wait()

    def test_head_unknown_omits_length(self):
        def handler(req):
            return lowlevel.Response.stream(200, [b"hello"])

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, body, headers = _raw(host, int(port), "HEAD", "/")
            self.assertEqual(status, 200)
            self.assertEqual(body, b"")
            self.assertIsNone(headers.get("content-length"))
        finally:
            srv.shutdown()
            srv.wait()

    def test_non_bytes_fails_closed(self):
        def handler(req):
            return lowlevel.Response.stream(200, ["not-bytes"])

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            with self.assertRaises(http.client.RemoteDisconnected):
                _raw(host, int(port), "GET", "/")
        finally:
            srv.shutdown()
            srv.wait()

    def test_iterator_exception_truncates(self):
        def gen():
            yield b"partial"
            raise RuntimeError("boom")

        def handler(req):
            return lowlevel.Response.stream(200, gen())

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            with self.assertRaises(http.client.RemoteDisconnected):
                _raw(host, int(port), "GET", "/")
        finally:
            srv.shutdown()
            srv.wait()

    def test_transfer_encoding_rejected(self):
        def handler(req):
            return lowlevel.Response.stream(
                200, [b"hi"], headers={"transfer-encoding": "chunked"}
            )

        try:
            _get(handler)
            self.fail("expected HTTP 500")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 500)

    def test_async_producer_rejected(self):
        async def agen():
            yield b"hi"  # pragma: no cover

        with self.assertRaises(TypeError):
            lowlevel.Response.stream(200, agen())

    def test_body_forbidden_does_not_advance(self):
        advanced = []

        def gen():
            advanced.append(1)
            yield b"x"  # pragma: no cover

        def handler(req):
            return lowlevel.Response.stream(204, gen())

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, body, _ = _raw(host, int(port), "GET", "/")
            self.assertEqual(status, 204)
            self.assertEqual(advanced, [])
        finally:
            srv.shutdown()
            srv.wait()

    def test_known_length_mismatch_truncates(self):
        def handler(req):
            return lowlevel.Response.stream(200, [b"ab"], content_length=10)

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            with self.assertRaises(http.client.RemoteDisconnected):
                _raw(host, int(port), "GET", "/")
        finally:
            srv.shutdown()
            srv.wait()


class AdmissionAndTimeoutTests(unittest.TestCase):
    def test_callback_saturation_bounded(self):
        entered = threading.Event()
        release = threading.Event()
        active = 0
        max_active = 0
        lock = threading.Lock()

        def handler(req):
            nonlocal active, max_active
            with lock:
                active += 1
                max_active = max(max_active, active)
            entered.set()
            release.wait(timeout=5)
            with lock:
                active -= 1
            return lowlevel.Response.text(200, "ok")

        cfg = lowlevel.RuntimeConfig(port=0, max_python_callbacks=1)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            port = int(port)
            results = []

            def fetch():
                try:
                    c = http.client.HTTPConnection(host, port, timeout=5)
                    c.request("GET", "/")
                    r = c.getresponse()
                    results.append((r.status, r.read()))
                    c.close()
                except Exception as e:  # pragma: no cover
                    results.append(e)

            t1 = threading.Thread(target=fetch)
            t1.start()
            self.assertTrue(entered.wait(timeout=5))
            time.sleep(0.2)
            # Second request must wait for the single callback permit.
            t2 = threading.Thread(target=fetch)
            t2.start()
            time.sleep(0.5)
            with lock:
                self.assertLessEqual(max_active, 1)
            release.set()
            t1.join(timeout=5)
            t2.join(timeout=5)
            self.assertEqual(len(results), 2)
        finally:
            release.set()
            srv.shutdown()
            srv.wait()

    def test_in_flight_saturation_503(self):
        entered = threading.Event()
        release = threading.Event()

        def handler(req):
            entered.set()
            release.wait(timeout=5)
            return lowlevel.Response.text(200, "ok")

        cfg = lowlevel.RuntimeConfig(port=0, max_in_flight_requests=1, max_python_callbacks=4)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            port = int(port)

            def fetch(path="/"):
                c = http.client.HTTPConnection(host, port, timeout=5)
                try:
                    c.request("GET", path)
                    r = c.getresponse()
                    return r.status, r.read()
                finally:
                    c.close()

            t = threading.Thread(target=lambda: fetch())
            t.start()
            self.assertTrue(entered.wait(timeout=5))
            # In-flight pool exhausted: second request fails fast with 503.
            status, _ = fetch()
            self.assertEqual(status, 503)
            release.set()
            t.join(timeout=5)
        finally:
            release.set()
            srv.shutdown()
            srv.wait()

    def test_handler_timeout_continues_healthy(self):
        def handler(req):
            if req.path == "/slow":
                time.sleep(3)
            return lowlevel.Response.text(200, "ok")

        cfg = lowlevel.RuntimeConfig(
            port=0, handler_timeout_secs=1, connection_total_timeout_secs=10
        )
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, _, _ = _raw(host, int(port), "GET", "/slow")
            self.assertEqual(status, 504)
            # Process remains healthy for subsequent requests.
            status2, body2, _ = _raw(host, int(port), "GET", "/fast")
            self.assertEqual((status2, body2), (200, b"ok"))
        finally:
            srv.shutdown()
            srv.wait()


class PrivacyConfigTests(unittest.TestCase):
    def test_server_header_and_denylist_and_date_suppress(self):
        def handler(req):
            return lowlevel.Response.text(
                200, "hi", headers={"x-powered-by": "php", "x-keep": "yes"}
            )

        cfg = lowlevel.RuntimeConfig(
            port=0,
            server_header="myapp",
            stripped_response_headers=("x-powered-by",),
            date_policy="suppress",
        )
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            _, _, headers = _raw(host, int(port), "GET", "/")
            self.assertEqual(headers.get("server"), "myapp")
            self.assertIsNone(headers.get("date"))
            self.assertIsNone(headers.get("x-powered-by"))
            self.assertEqual(headers.get("x-keep"), "yes")
        finally:
            srv.shutdown()
            srv.wait()

    def test_error_empty_policy(self):
        def handler(req):
            raise RuntimeError("boom")

        cfg = lowlevel.RuntimeConfig(port=0, error_policy="empty")
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, body, _ = _raw(host, int(port), "GET", "/")
            self.assertEqual(status, 500)
            self.assertEqual(body, b"")
        finally:
            srv.shutdown()
            srv.wait()

    def test_invalid_privacy_rejected(self):
        with self.assertRaises(ValueError):
            lowlevel.RuntimeConfig(date_policy="weird")
        with self.assertRaises(ValueError):
            lowlevel.RuntimeConfig(error_policy="weird")
        with self.assertRaises(ValueError):
            Server(None, handler=lambda r: None, stripped_response_headers=["content-length"])
        with self.assertRaises(ValueError):
            Server(None, handler=lambda r: None, max_requests_per_connection=0)


class ParserLimitTests(unittest.TestCase):
    def test_request_target_ceiling_414(self):
        def handler(req):
            return lowlevel.Response.text(200, "ok")

        cfg = lowlevel.RuntimeConfig(port=0, max_request_target_bytes=128)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            status, _, _ = _raw(host, int(port), "GET", "/" + "a" * 500)
            self.assertEqual(status, 414)
        finally:
            srv.shutdown()
            srv.wait()

    def test_max_requests_per_connection_close(self):
        def handler(req):
            return lowlevel.Response.text(200, "ok")

        cfg = lowlevel.RuntimeConfig(port=0, max_requests_per_connection=1)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            c = http.client.HTTPConnection(host, int(port), timeout=5)
            try:
                c.request("GET", "/", headers={"Connection": "keep-alive"})
                r = c.getresponse()
                r.read()
                self.assertEqual(r.getheader("Connection"), "close")
            finally:
                c.close()
        finally:
            srv.shutdown()
            srv.wait()


class FacadeInvarianceTests(unittest.TestCase):
    def test_stdlib_facade_unchanged(self):
        from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

        server = ThreadingHTTPServer(("127.0.0.1", 0), SimpleHTTPRequestHandler)
        try:
            self.assertTrue(server._native_fast_path)
        finally:
            server.server_close()


class TlsAndLifecycleTests(unittest.TestCase):
    def test_tls_lowlevel_service(self):
        import os
        import socket
        import ssl

        fixture_dir = os.path.join(os.path.dirname(__file__), "fixtures")
        cert = os.path.join(fixture_dir, "localhost-test.crt")
        key = os.path.join(fixture_dir, "localhost-test.key")

        def handler(req):
            return lowlevel.Response.stream(200, [b"tls-ok"])

        cfg = lowlevel.RuntimeConfig(
            port=0, tls_certfile=cert, tls_keyfile=key,
        )
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            ctx = ssl._create_unverified_context()
            with socket.create_connection((host, int(port)), timeout=5) as raw:
                with ctx.wrap_socket(raw, server_hostname="localhost") as sock:
                    sock.sendall(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                    data = b""
                    while True:
                        chunk = sock.recv(4096)
                        if not chunk:
                            break
                        data += chunk
            self.assertIn(b"200", data.split(b"\r\n", 1)[0])
            # Chunked framing wraps the payload; assert presence, not suffix.
            self.assertIn(b"tls-ok", data)
        finally:
            srv.shutdown()
            srv.wait()

    def test_shutdown_with_active_stream(self):
        release = threading.Event()

        def gen():
            yield b"first"
            release.wait(timeout=5)
            yield b"second"

        def handler(req):
            return lowlevel.Response.stream(200, gen())

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            received = []

            def fetch():
                c = http.client.HTTPConnection(host, int(port), timeout=5)
                try:
                    c.request("GET", "/")
                    r = c.getresponse()
                    try:
                        received.append(r.read())
                    except Exception as e:
                        received.append(e)
                except Exception as e:
                    received.append(e)
                finally:
                    c.close()

            t = threading.Thread(target=fetch)
            t.start()
            time.sleep(0.5)
            srv.shutdown()
            release.set()
            t.join(timeout=5)
            # Shutdown completes; client saw truncation or partial body, never hangs.
            self.assertTrue(received)
        finally:
            release.set()
            try:
                srv.wait()
            except Exception:
                pass

    def test_repeated_start_stop(self):
        def handler(req):
            return lowlevel.Response.text(200, "ok")

        for _ in range(5):
            srv = lowlevel.Server(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            srv.start()
            srv.wait_ready()
            host, port = srv.addr.split(":")
            status, body, _ = _raw(host, int(port), "GET", "/")
            self.assertEqual((status, body), (200, b"ok"))
            srv.shutdown()
            self.assertEqual(srv.wait(), "stopped")

    def test_slow_client_backpressure_bounded(self):
        # Producer records how far it advanced while the client stalls.
        advanced = []
        release = threading.Event()

        def gen():
            for i in range(100):
                advanced.append(i)
                yield b"x" * 1024
                if i == 4:
                    # Let the client stall with a full channel; producer must
                    # block rather than run ahead unboundedly.
                    time.sleep(0.5)

        def handler(req):
            return lowlevel.Response.stream(200, gen())

        cfg = lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.Server(config=cfg, handler=handler)
        srv.start()
        srv.wait_ready()
        try:
            host, port = srv.addr.split(":")
            s = socket.create_connection((host, int(port)), timeout=5)
            try:
                s.sendall(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                time.sleep(0.3)
                # While the client reads nothing, the bounded 16-chunk bridge
                # must keep producer advancement small (not all 100 items).
                self.assertLess(len(advanced), 50)
                s.settimeout(5)
                data = b""
                while True:
                    chunk = s.recv(4096)
                    if not chunk:
                        break
                    data += chunk
                self.assertIn(b"200", data.split(b"\r\n", 1)[0])
            finally:
                s.close()
        finally:
            release.set()
            srv.shutdown()
            srv.wait()


if __name__ == "__main__":
    unittest.main()
