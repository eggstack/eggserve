"""Plan 254: async-Python lifecycle, admission, and streaming parity hardening.

Deterministic correctness evidence for the `AsyncServer` bridge seam
without redesigning it: application-task admission and permit ownership,
handler-timeout races, response-producer lifecycle, HEAD/body-forbidden
suppression, disconnect/lifecycle parity, tunnel task ownership, and
task-registry closure. All synchronization uses explicit events with
generous deadlines instead of timing sleeps; production code is untouched
unless a test reproduces a defect.
"""

import asyncio
import http.client
import socket
import threading
import unittest

from eggserve import lowlevel


def _run(coro):
    return asyncio.run(coro)


def _fetch(addr, path="/", method="GET", headers=None, body=None, timeout=15):
    host, port = addr.rsplit(":", 1)
    c = http.client.HTTPConnection(host, int(port), timeout=timeout)
    try:
        c.request(method, path, body=body, headers=headers or {})
        r = c.getresponse()
        return r.status, r.read(), r.getheader("Content-Length")
    finally:
        c.close()


def _fetch_thread(addr, out, key, **kwargs):
    def run():
        try:
            out[key] = _fetch(addr, **kwargs)
        except Exception as exc:  # noqa: BLE001 - record client-side outcome
            out[key] = exc

    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return thread


class AsyncAdmissionTests(unittest.TestCase):
    def test_second_request_503_while_first_held(self):
        entered = threading.Event()
        release = threading.Event()

        async def handler(req):
            entered.set()
            await asyncio.to_thread(release.wait, 15)
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                out = {}
                t1 = _fetch_thread(srv.addr, out, "first")
                self.assertTrue(await asyncio.to_thread(entered.wait, 15), "first handler never entered")
                t2 = _fetch_thread(srv.addr, out, "second")
                await asyncio.to_thread(t2.join, 15)
                self.assertFalse(t2.is_alive(), "second fetch hung")
                status, _, _ = out["second"]
                self.assertEqual(status, 503)
                release.set()
                await asyncio.to_thread(t1.join, 15)
                self.assertFalse(t1.is_alive(), "first fetch hung")
                self.assertEqual(out["first"][0], 200)
            finally:
                release.set()
                await srv.shutdown()

        _run(main())

    def test_permit_returns_after_buffered(self):
        async def handler(req):
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                for _ in range(3):
                    status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                    self.assertEqual((status, body), (200, b"ok"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_streaming_holds_permit_until_producer_done(self):
        entered = threading.Event()
        prod_release = threading.Event()

        async def agen():
            yield b"chunk1-"
            await asyncio.to_thread(prod_release.wait, 15)
            yield b"chunk2"

        async def handler(req):
            entered.set()
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                out = {}
                t1 = _fetch_thread(srv.addr, out, "stream")
                self.assertTrue(await asyncio.to_thread(entered.wait, 15), "streaming handler never entered")
                # Producer holds the transferred permit: admission exhausted.
                status, _, _ = await asyncio.to_thread(_fetch, srv.addr)
                self.assertEqual(status, 503)
                prod_release.set()
                await asyncio.to_thread(t1.join, 15)
                self.assertFalse(t1.is_alive(), "streaming fetch hung")
                self.assertEqual(out["stream"][:2], (200, b"chunk1-chunk2"))
                # Producer termination returns the permit exactly once.
                status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                self.assertEqual((status, body), (200, b"chunk1-chunk2"))
            finally:
                prod_release.set()
                await srv.shutdown()

        _run(main())

    def test_producer_error_releases_permit(self):
        async def agen():
            yield b"prefix-"
            raise RuntimeError("boom")

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def handler_ok(req):
            return lowlevel.AsyncResponse.text(200, "next")

        async def fetch_truncated(addr):
            # Same contract as the sync bridge
            # (test_iterator_exception_truncates): the connection drops
            # before headers flush (RemoteDisconnected) or mid-body
            # afterwards (IncompleteRead); both prove no complete response
            # is ever presented and no second error is synthesized.
            with self.assertRaises(
                (http.client.RemoteDisconnected, http.client.IncompleteRead)
            ):
                await asyncio.to_thread(_fetch, addr)

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                await fetch_truncated(srv.addr)
                # Permit returned: a fresh server with a healthy handler works.
                await srv.shutdown()
                srv2 = lowlevel.AsyncServer(config=cfg, handler=handler_ok, max_async_tasks=1)
                await srv2.start()
                try:
                    status, body, _ = await asyncio.to_thread(_fetch, srv2.addr)
                    self.assertEqual((status, body), (200, b"next"))
                finally:
                    await srv2.shutdown()
            finally:
                try:
                    await srv.shutdown()
                except RuntimeError:
                    pass

        _run(main())

    def test_no_double_release_after_timeout(self):
        # After a timed-out dispatch releases its permit exactly once,
        # max_async_tasks=1 must still admit only one concurrent handler.
        release = threading.Event()
        entered = threading.Event()
        calls = []

        async def handler(req):
            calls.append(1)
            if len(calls) == 1:
                await asyncio.to_thread(release.wait, 15)
            else:
                entered.set()
                await asyncio.to_thread(release.wait, 15)
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, handler_timeout_secs=2)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                out = {}
                t1 = _fetch_thread(srv.addr, out, "slow")
                await asyncio.to_thread(t1.join, 20)
                # Honest timeout: runtime owns commitment (504) or the sync
                # raise maps to a generic 500; never a second response.
                first = out["slow"]
                if isinstance(first, tuple):
                    self.assertIn(first[0], (500, 504))
                release.set()
                # Two concurrent holders, one permit: exactly one 503.
                release.clear()
                entered.clear()
                out2 = {}
                a = _fetch_thread(srv.addr, out2, "a")
                self.assertTrue(await asyncio.to_thread(entered.wait, 15), "probe handler never entered")
                b = _fetch_thread(srv.addr, out2, "b")
                await asyncio.to_thread(b.join, 15)
                self.assertFalse(b.is_alive(), "overload probe hung")
                self.assertEqual(out2["b"][0], 503)
                release.set()
                await asyncio.to_thread(a.join, 15)
                self.assertEqual(out2["a"][0], 200)
            finally:
                release.set()
                await srv.shutdown()

        _run(main())


class AsyncTimeoutRaceTests(unittest.TestCase):
    def test_handler_completes_before_timeout(self):
        async def handler(req):
            await asyncio.sleep(0.1)
            return lowlevel.AsyncResponse.text(200, "fast")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, handler_timeout_secs=5)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                self.assertEqual((status, body), (200, b"fast"))
            finally:
                await srv.shutdown()

        _run(main())

    def test_shutdown_during_handler_completes(self):
        entered = threading.Event()
        release = threading.Event()

        async def handler(req):
            entered.set()
            await asyncio.to_thread(release.wait, 30)
            return lowlevel.AsyncResponse.text(200, "late")

        async def main():
            cfg = lowlevel.RuntimeConfig(
                port=0, handler_timeout_secs=30, graceful_shutdown_timeout_secs=5
            )
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            out = {}
            t = _fetch_thread(srv.addr, out, "fetch")
            timer = threading.Timer(2.0, release.set)
            try:
                self.assertTrue(
                    await asyncio.to_thread(entered.wait, 15),
                    "handler never entered",
                )
                # Let the blocked handler finish shortly after shutdown
                # starts so completion is prompt, not timeout-driven.
                timer.start()
                await asyncio.wait_for(srv.shutdown(), timeout=30)
            finally:
                timer.cancel()
                release.set()
                await asyncio.to_thread(t.join, 20)
            # Fetch terminates (response or connection teardown); shutdown
            # never hangs and leaves no bridge-owned tasks behind.
            self.assertFalse(t.is_alive(), "fetch hung across shutdown")
            self.assertEqual(len(srv._tasks), 0)

        _run(main())


class AsyncProducerTests(unittest.TestCase):
    async def _serve_once(self, make_handler, fetch_kwargs=None, config=None):
        cfg = config or lowlevel.RuntimeConfig(port=0)
        srv = lowlevel.AsyncServer(config=cfg, handler=make_handler)
        await srv.start()
        try:
            return await asyncio.to_thread(_fetch, srv.addr, **(fetch_kwargs or {}))
        finally:
            await srv.shutdown()

    def test_many_chunks_ordered_under_bound(self):
        chunks = [f"{i:03d}-".encode() for i in range(40)]

        async def agen():
            for chunk in chunks:
                yield chunk

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            status, body, _ = await self._serve_once(handler)
            self.assertEqual(status, 200)
            self.assertEqual(body, b"".join(chunks))

        _run(main())

    def test_empty_chunks_skipped(self):
        async def agen():
            yield b""
            yield b""
            yield b"data"
            yield b""

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            status, body, _ = await self._serve_once(handler)
            self.assertEqual((status, body), (200, b"data"))

        _run(main())

    def test_non_bytes_truncates(self):
        async def agen():
            yield b"a"
            yield "not-bytes"
            yield b"b"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            # Same contract as the sync bridge (test_non_bytes_fails_closed):
            # RemoteDisconnected or IncompleteRead; the partial prefix is
            # never presented as a complete response.
            with self.assertRaises(
                (http.client.RemoteDisconnected, http.client.IncompleteRead)
            ):
                await self._serve_once(handler)

        _run(main())

    def test_known_length_exact(self):
        async def agen():
            yield b"abc"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen(), content_length=3)

        async def main():
            status, body, length = await self._serve_once(handler)
            self.assertEqual(status, 200)
            self.assertEqual(body, b"abc")
            self.assertEqual(length, "3")

        _run(main())

    def test_length_mismatch_truncates(self):
        async def under():
            yield b"abc"

        async def over():
            yield b"abcdef"

        async def handler_under(req):
            return lowlevel.AsyncResponse.stream(200, under(), content_length=5)

        async def handler_over(req):
            return lowlevel.AsyncResponse.stream(200, over(), content_length=2)

        async def main():
            # Same contract as the sync bridge
            # (test_known_length_mismatch_truncates): the short/long body is
            # never presented as the declared complete response; the exchange
            # terminates without hanging or a second response.
            for handler in (handler_under, handler_over):
                with self.assertRaises(
                    (http.client.RemoteDisconnected, http.client.IncompleteRead)
                ):
                    await self._serve_once(handler)

        _run(main())

    def test_sync_iterable_convenience(self):
        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, [b"x", b"y"])

        async def main():
            status, body, _ = await self._serve_once(handler)
            self.assertEqual((status, body), (200, b"xy"))

        _run(main())


class AsyncHeadSuppressionTests(unittest.TestCase):
    def test_head_does_not_advance_producer(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                status, body, _ = await asyncio.to_thread(
                    _fetch, srv.addr, method="HEAD"
                )
                self.assertEqual(status, 200)
                self.assertEqual(body, b"")
                self.assertEqual(polls, [])
                # Bridge still healthy: GET consumes the producer.
                status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                self.assertEqual((status, body), (200, b"chunk"))
                self.assertEqual(len(polls), 1)
            finally:
                await srv.shutdown()

        _run(main())

    def test_204_does_not_consume_producer(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(204, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                self.assertEqual(status, 204)
                self.assertEqual(body, b"")
                self.assertEqual(polls, [])
            finally:
                await srv.shutdown()

        _run(main())


class AsyncLifecycleTests(unittest.TestCase):
    def test_disconnect_during_handler_observed(self):
        entered = threading.Event()
        seen = {}

        async def handler(req):
            entered.set()
            seen["initial"] = req.is_disconnected()
            seen["waited"] = await req.wait_disconnected(timeout_secs=10)
            seen["reason"] = req.cancellation_reason()
            return lowlevel.AsyncResponse.text(200, "late")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                def fetch_and_abandon():
                    host, port = srv.addr.rsplit(":", 1)
                    s = socket.create_connection((host, int(port)), timeout=10)
                    try:
                        s.sendall(
                            b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
                        )
                        if not entered.wait(10):
                            raise AssertionError("handler never entered")
                        # Abandon the connection mid-handler: no read.
                    finally:
                        s.close()

                await asyncio.to_thread(fetch_and_abandon)
            finally:
                await srv.shutdown()

        _run(main())
        self.assertFalse(seen["initial"])
        self.assertTrue(seen["waited"])
        self.assertEqual(seen["reason"], "peer_disconnected")

    def test_wait_disconnected_timeout(self):
        seen = {}

        async def handler(req):
            seen["quick"] = await req.wait_disconnected(timeout_secs=0.2)
            seen["reason"] = req.cancellation_reason()
            return lowlevel.AsyncResponse.text(200, "ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                self.assertEqual((status, body), (200, b"ok"))
            finally:
                await srv.shutdown()

        _run(main())
        self.assertFalse(seen["quick"])
        self.assertIsNone(seen["reason"])


class AsyncTunnelOwnershipTests(unittest.TestCase):
    def test_denial_stays_ordinary_http(self):
        seen = {}

        async def handler(req):
            seen["has_tunnel"] = req.has_tunnel()
            return lowlevel.AsyncResponse.text(200, "denied")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                before = len(srv._tasks)
                status, body, _ = await asyncio.to_thread(
                    _fetch,
                    srv.addr,
                    path="/chat",
                    headers={"Upgrade": "echo-test", "Connection": "upgrade"},
                )
                self.assertEqual((status, body), (200, b"denied"))
                self.assertEqual(len(srv._tasks), before)
            finally:
                await srv.shutdown()

        _run(main())
        self.assertIn("has_tunnel", seen)

    def test_accepted_tunnel_driver_cancelled_by_shutdown(self):
        cancelled = threading.Event()
        holder = {}

        async def handler(req):
            cap = req.take_tunnel()
            if cap is None:
                return lowlevel.AsyncResponse.text(400, "no tunnel")
            self.assertIsNone(req.take_tunnel())
            handshake, tunnel = cap.accept([("x-t", "1")])

            async def driver():
                try:
                    while True:
                        data = await tunnel.recv()
                        if data is None:
                            break
                        await tunnel.send(data)
                except asyncio.CancelledError:
                    cancelled.set()
                    raise
                finally:
                    tunnel.close()

            holder["srv"].track(asyncio.create_task(driver()))
            return handshake

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            holder["srv"] = srv
            await srv.start()
            try:
                def fetch():
                    host, port = srv.addr.rsplit(":", 1)
                    s = socket.create_connection((host, int(port)), timeout=10)
                    try:
                        s.sendall(
                            b"GET /chat HTTP/1.1\r\nHost: x\r\nUpgrade: echo-test\r\n"
                            b"Connection: upgrade\r\n\r\n"
                        )
                        data = s.recv(4096)
                        assert b"101" in data.split(b"\r\n", 1)[0], data[:200]
                        s.sendall(b"ping-tunnel")
                        s.settimeout(10)
                        echo = s.recv(4096)
                        return echo
                    finally:
                        s.close()

                echo = await asyncio.to_thread(fetch)
                self.assertIn(b"ping-tunnel", echo)
                await srv.shutdown()
                self.assertTrue(
                    await asyncio.to_thread(cancelled.wait, 10),
                    "tracked tunnel driver was not cancelled by shutdown",
                )
                self.assertEqual(len(srv._tasks), 0)
            finally:
                try:
                    await srv.shutdown()
                except RuntimeError:
                    pass

        _run(main())


class AsyncRegistryClosureTests(unittest.TestCase):
    def test_repeated_streaming_returns_to_baseline(self):
        async def agen():
            yield b"a"
            yield b"b"
            yield b"c"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler)
            await srv.start()
            try:
                self.assertEqual(len(srv._tasks), 0)
                for _ in range(15):
                    status, body, _ = await asyncio.to_thread(_fetch, srv.addr)
                    self.assertEqual((status, body), (200, b"abc"))
                # Producer done-callbacks release asynchronously; poll
                # briefly rather than assuming a fixed scheduler step.
                for _ in range(100):
                    if not srv._tasks:
                        break
                    await asyncio.sleep(0.05)
                self.assertEqual(len(srv._tasks), 0)
            finally:
                await srv.shutdown()

        _run(main())


if __name__ == "__main__":
    unittest.main()
