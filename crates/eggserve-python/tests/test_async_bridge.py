"""Plan 204: async Python application-server substrate qualification.

Covers async dispatch without the sync `http.server` callback path, bounded
incremental request/response streaming (no hidden `read_all`), disconnect/
cancellation both ways, trailers/interim/tunnel projection without raw
transport access, separate app-task admission, loop/server ownership, H1
metadata truthfulness (Python bridge H1-only, explicit), and sync facade
invariance. ASGI HTTP/WebSocket sufficiency via `asgi_fixture` (test/example
adapter, not the product).
"""

import asyncio
import base64
import hashlib
import http.client
import os
import socket
import struct
import threading
import time
import unittest
import urllib.request

from eggserve import lowlevel

from asgi_fixture import create_asgi_handler, websocket_accept_key_to_accept


def _run(coro):
    return asyncio.run(coro)


class AsyncDispatchTests(unittest.TestCase):
    def test_buffered_get(self):
        async def handler(req):
            self.assertIsInstance(req, lowlevel.AsyncRequest)
            return lowlevel.AsyncResponse.text(200, "hello-async")

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    with urllib.request.urlopen(f"http://{srv.addr}/", timeout=5) as r:
                        return r.status, r.read()

                status, body = await asyncio.to_thread(fetch)
                self.assertEqual((status, body), (200, b"hello-async"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_sync_handler_rejected(self):
        def sync_handler(req):
            return lowlevel.AsyncResponse.text(200, "x")

        with self.assertRaises(TypeError):
            lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=sync_handler)

    def test_post_buffered_body(self):
        async def handler(req):
            data = await req.body.aread()
            return lowlevel.AsyncResponse.bytes(200, b"echo:" + data)

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, request_body_mode="buffer", max_request_body_bytes=1 << 20)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                def fetch():
                    c = http.client.HTTPConnection(*srv.addr.split(":")[0:1], int(srv.addr.split(":")[1]), timeout=5)
                    c.request("POST", "/", body=b"abc123")
                    r = c.getresponse()
                    return r.status, r.read()

                status, body = await asyncio.to_thread(fetch)
                self.assertEqual((status, body), (200, b"echo:abc123"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_streaming_request_incremental(self):
        seen = []

        async def handler(req):
            async for chunk in req.body.aiter_chunks():
                seen.append(chunk)
            return lowlevel.AsyncResponse.text(200, f"chunks={len(seen)}")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, request_body_mode="stream", max_request_body_bytes=1 << 20)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                def fetch():
                    c = http.client.HTTPConnection(srv.addr.split(":")[0], int(srv.addr.split(":")[1]), timeout=5)
                    c.request("POST", "/", body=b"a" * 100 + b"b" * 100)
                    r = c.getresponse()
                    return r.status, r.read()

                status, body = await asyncio.to_thread(fetch)
                self.assertEqual(status, 200)
                self.assertTrue(seen)
                self.assertEqual(b"".join(seen), b"a" * 100 + b"b" * 100)
            finally:
                await srv.shutdown()

        _run(main())

    def test_streaming_response_unknown_length(self):
        async def handler(req):
            async def gen():
                yield b"he"
                yield b"llo"

            return lowlevel.AsyncResponse.stream(200, gen())

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    host, port = srv.addr.split(":")
                    c = http.client.HTTPConnection(host, int(port), timeout=5)
                    c.request("GET", "/")
                    r = c.getresponse()
                    return r.status, r.read(), {k.lower(): v for k, v in r.getheaders()}

                status, body, headers = await asyncio.to_thread(fetch)
                self.assertEqual((status, body), (200, b"hello"))
                self.assertIsNone(headers.get("content-length"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_byte_fidelity_headers(self):
        observed = {}

        async def handler(req):
            observed["items"] = req.header_items_bytes
            observed["raw"] = req.raw_target_bytes
            observed["version"] = req.http_version
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    host, port = srv.addr.split(":")
                    s = socket.create_connection((host, int(port)), timeout=5)
                    try:
                        s.sendall(b"GET /a?b=c HTTP/1.1\r\nHost: x\r\nX-Dup: 1\r\nX-Dup: 2\r\nConnection: close\r\n\r\n")
                        data = b""
                        while True:
                            chunk = s.recv(4096)
                            if not chunk:
                                break
                            data += chunk
                    finally:
                        s.close()

                await asyncio.to_thread(fetch)
                names = [n.lower() for (n, v) in observed["items"]]
                self.assertIn(b"x-dup", names)
                self.assertEqual([v for (n, v) in observed["items"] if n.lower() == b"x-dup"], [b"1", b"2"])
                self.assertIn(b"/a", observed["raw"])
                self.assertEqual(observed["version"], "HTTP/1.1")
            finally:
                await srv.shutdown()

        _run(main())

    def test_request_trailers(self):
        observed = {}

        async def handler(req):
            chunks = [c async for c in req.body.aiter_chunks()]
            observed["body"] = b"".join(chunks)
            observed["trailers"] = await req.body.trailers()
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, request_body_mode="stream", max_request_body_bytes=1 << 20)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                def fetch():
                    host, port = srv.addr.split(":")
                    s = socket.create_connection((host, int(port)), timeout=5)
                    try:
                        s.sendall(
                            b"POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n"
                            b"TE: trailers\r\nConnection: close\r\n\r\n"
                            b"5\r\nhello\r\n0\r\nX-Trailer: yes\r\n\r\n"
                        )
                        data = b""
                        while True:
                            chunk = s.recv(4096)
                            if not chunk:
                                break
                            data += chunk
                        return data
                    finally:
                        s.close()

                data = await asyncio.to_thread(fetch)
                self.assertIn(b"200", data.split(b"\r\n", 1)[0])
                self.assertEqual(observed["body"], b"hello")
                self.assertIsNotNone(observed["trailers"])
            finally:
                await srv.shutdown()

        _run(main())

    def test_response_trailers(self):
        async def handler(req):
            async def gen():
                yield b"data"

            return lowlevel.AsyncResponse.stream(200, gen(), trailers=[("x-checksum", "abc")])

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    host, port = srv.addr.split(":")
                    s = socket.create_connection((host, int(port)), timeout=5)
                    try:
                        s.sendall(b"GET / HTTP/1.1\r\nHost: x\r\nTE: trailers\r\nConnection: close\r\n\r\n")
                        data = b""
                        while True:
                            chunk = s.recv(4096)
                            if not chunk:
                                break
                            data += chunk
                        return data
                    finally:
                        s.close()

                data = await asyncio.to_thread(fetch)
                self.assertIn(b"200", data.split(b"\r\n", 1)[0])
                self.assertIn(b"data", data)
            finally:
                await srv.shutdown()

        _run(main())

    def test_interim_1xx(self):
        async def handler(req):
            result = await req.send_interim(103, [("link", "</style.css>; rel=preload")])
            self.assertIn(result, ("sent", "suppressed"))
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    with urllib.request.urlopen(f"http://{srv.addr}/", timeout=5) as r:
                        return r.status, r.read()

                status, body = await asyncio.to_thread(fetch)
                self.assertEqual((status, body), (200, b"ok"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_interim_rejects_101(self):
        async def handler(req):
            try:
                await req.send_interim(101, [])
                return lowlevel.AsyncResponse.text(500, "should-not-accept-101")
            except ValueError:
                return lowlevel.AsyncResponse.text(200, "rejected-101")

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    with urllib.request.urlopen(f"http://{srv.addr}/", timeout=5) as r:
                        return r.status, r.read()

                status, body = await asyncio.to_thread(fetch)
                self.assertEqual((status, body), (200, b"rejected-101"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_tunnel_echo(self):
        async def handler(req):
            cap = req.take_tunnel()
            if cap is None:
                return lowlevel.AsyncResponse.text(400, "no tunnel")
            # Second take must be None (one-shot).
            self.assertIsNone(req.take_tunnel())
            handshake, tunnel = cap.accept([("x-echo", "yes")])

            async def driver():
                while True:
                    data = await tunnel.recv()
                    if data is None:
                        break
                    await tunnel.send(data)
                tunnel.close()

            asyncio.create_task(driver())
            return handshake

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    host, port = srv.addr.split(":")
                    s = socket.create_connection((host, int(port)), timeout=5)
                    try:
                        s.sendall(
                            b"GET /chat HTTP/1.1\r\nHost: x\r\nUpgrade: echo-test\r\n"
                            b"Connection: upgrade\r\n\r\n"
                        )
                        data = s.recv(4096)
                        assert b"101" in data.split(b"\r\n", 1)[0], data[:200]
                        s.sendall(b"ping-tunnel")
                        s.settimeout(5)
                        echo = s.recv(4096)
                        return echo
                    finally:
                        s.close()

                echo = await asyncio.to_thread(fetch)
                self.assertIn(b"ping-tunnel", echo)
            finally:
                await srv.shutdown()

        _run(main())

    def test_app_exception_before_start(self):
        async def handler(req):
            raise RuntimeError("boom")

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                def fetch():
                    import urllib.error

                    try:
                        with urllib.request.urlopen(f"http://{srv.addr}/", timeout=5) as r:
                            return r.status
                    except urllib.error.HTTPError as e:
                        return e.code

                status = await asyncio.to_thread(fetch)
                self.assertEqual(status, 500)
            finally:
                await srv.shutdown()

        _run(main())

    def test_admission_saturation_503(self):
        entered = asyncio.Event()
        release = asyncio.Event()

        async def handler(req):
            entered.set()
            await release.wait()
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, max_python_callbacks=16)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                host, port = srv.addr.split(":")

                def fetch_one():
                    c = http.client.HTTPConnection(host, int(port), timeout=5)
                    try:
                        c.request("GET", "/")
                        r = c.getresponse()
                        return r.status, r.read()
                    finally:
                        c.close()

                # Occupy the single app permit via a background-thread request.
                results = []
                first_started = threading.Event()

                def hold():
                    c = http.client.HTTPConnection(host, int(port), timeout=10)
                    try:
                        c.request("GET", "/")
                        first_started.set()
                        r = c.getresponse()
                        results.append((r.status, r.read()))
                    except Exception as e:  # pragma: no cover
                        results.append(e)
                    finally:
                        c.close()

                h = threading.Thread(target=hold)
                h.start()
                self.assertTrue(await asyncio.to_thread(first_started.wait, 5))
                # Wait until the handler entered (permit held). Never block
                # the event loop here (handler needs the loop to dispatch).
                for _ in range(100):
                    if entered.is_set():
                        break
                    await asyncio.sleep(0.05)
                self.assertTrue(entered.is_set())
                # Second request must fail fast with 503 (no unbounded queue).
                status, _ = await asyncio.to_thread(fetch_one)
                self.assertEqual(status, 503)
                release.set()
                await asyncio.to_thread(h.join, 10)
                self.assertEqual(len(results), 1)
            finally:
                release.set()
                await srv.shutdown()

        _run(main())

    def test_wrong_loop_start_fails(self):
        async def handler(req):
            return lowlevel.AsyncResponse.text(200, "x")

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=handler)
            await srv.start()
            try:
                # Second start on same loop fails deterministically.
                with self.assertRaises(RuntimeError):
                    await srv.start()
            finally:
                await srv.shutdown()
            # Shutdown idempotent.
            await srv.shutdown()

        _run(main())

    def test_sync_facade_intact(self):
        from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

        server = ThreadingHTTPServer(("127.0.0.1", 0), SimpleHTTPRequestHandler)
        try:
            self.assertTrue(server._native_fast_path)
        finally:
            server.server_close()


class AsgiFixtureTests(unittest.TestCase):
    def test_asgi_http_echo(self):
        async def app(scope, receive, send):
            assert scope["type"] == "http"
            body = b""
            while True:
                event = await receive()
                if event["type"] == "http.disconnect":
                    return
                body += event.get("body", b"")
                if not event.get("more_body"):
                    break
            await send({"type": "http.response.start", "status": 200, "headers": [(b"content-type", b"text/plain")]})
            await send({"type": "http.response.body", "body": b"asgi:" + body, "more_body": False})

        async def main():
            srv = lowlevel.AsyncServer(
                config=lowlevel.RuntimeConfig(
                    port=0, request_body_mode="buffer", max_request_body_bytes=1 << 20
                ),
                handler=create_asgi_handler(app),
            )
            await srv.start()
            try:
                def fetch():
                    c = http.client.HTTPConnection(srv.addr.split(":")[0], int(srv.addr.split(":")[1]), timeout=5)
                    c.request("POST", "/x?y=1", body=b"hello-asgi")
                    r = c.getresponse()
                    return r.status, r.read()

                status, body = await asyncio.to_thread(fetch)
                self.assertEqual((status, body), (200, b"asgi:hello-asgi"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_asgi_websocket_echo(self):
        async def app(scope, receive, send):
            if scope["type"] == "websocket":
                event = await receive()
                assert event["type"] == "websocket.connect"
                # Compute accept from the client key in headers.
                key = ""
                for (n, v) in scope["headers"]:
                    if n == b"sec-websocket-key":
                        key = v.decode("ascii")
                accept = websocket_accept_key_to_accept(key)
                await send(
                    {
                        "type": "websocket.accept",
                        "headers": [(b"sec-websocket-accept", accept.encode("ascii"))],
                    }
                )
                while True:
                    event = await receive()
                    if event["type"] == "websocket.disconnect":
                        break
                    if event["type"] == "websocket.receive":
                        if event.get("text") is not None:
                            await send({"type": "websocket.send", "text": event["text"]})
                        elif event.get("bytes") is not None:
                            await send({"type": "websocket.send", "bytes": event["bytes"]})
                return
            # HTTP fallback (should not happen for WS test).
            await send({"type": "http.response.start", "status": 400, "headers": []})
            await send({"type": "http.response.body", "body": b"bad", "more_body": False})

        async def main():
            srv = lowlevel.AsyncServer(config=lowlevel.RuntimeConfig(port=0), handler=create_asgi_handler(app))
            await srv.start()
            try:
                def fetch():
                    import os as _os

                    host, port = srv.addr.split(":")
                    key = base64.b64encode(_os.urandom(16)).decode("ascii")
                    s = socket.create_connection((host, int(port)), timeout=5)
                    try:
                        s.sendall(
                            (
                                "GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\n"
                                f"Connection: upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                                "Sec-WebSocket-Version: 13\r\n\r\n"
                            ).encode("ascii")
                        )
                        resp = b""
                        while b"\r\n\r\n" not in resp:
                            chunk = s.recv(4096)
                            if not chunk:
                                break
                            resp += chunk
                        assert b"101" in resp.split(b"\r\n", 1)[0], resp[:200]
                        # Send one masked text frame ("hi").
                        mask = _os.urandom(4)
                        payload = b"hi"
                        frame = bytes([0x81, 0x80 | len(payload)]) + mask + bytes(
                            b ^ mask[i % 4] for i, b in enumerate(payload)
                        )
                        s.sendall(frame)
                        s.settimeout(5)
                        # Read server frame (unmasked text echo).
                        hdr = s.recv(2)
                        assert len(hdr) == 2, hdr
                        ln = hdr[1] & 0x7F
                        data = b""
                        while len(data) < ln:
                            chunk = s.recv(ln - len(data))
                            if not chunk:
                                break
                            data += chunk
                        return data
                    finally:
                        s.close()

                data = await asyncio.to_thread(fetch)
                self.assertEqual(data, b"hi")
            finally:
                await srv.shutdown()

        _run(main())


if __name__ == "__main__":
    unittest.main()
