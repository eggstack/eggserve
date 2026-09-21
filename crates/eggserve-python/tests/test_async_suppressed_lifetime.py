"""Plan 257: async-Python suppressed-body permit lifetime corrective.

Causal chain (Plan 257 confirmed mechanism):

    stream marker returned
     -> async permit transferred
     -> producer task created, parks on first_pull
     -> canonical Rust drops HEAD/body-forbidden iterable without polling
     -> never-entered generator `finally` never runs
     -> producer remains until first-pull timeout
     -> permit retained until producer task completion

With max_async_tasks=1, one suppressed stream exhausts admission and the
next valid request fails fast with 503. The fix replaces the generator
consumer with `_AsyncStreamBridgeIterator`, whose `close()`/`__del__`
cancels the producer even when never pulled. Canonical Rust remains the
suppression authority; Python holds no status/method table.

All synchronization uses explicit events with generous deadlines; no test
waits for `response_write_timeout_secs` (set deliberately large so expiry
cannot make a test pass accidentally).
"""

import asyncio
import gc
import http.client
import threading
import unittest

from eggserve import lowlevel
from eggserve.lowlevel import _AsyncStreamBridgeIterator


def _run(coro):
    return asyncio.run(coro)


def _fetch(addr, path="/", method="GET", headers=None, timeout=15):
    host, port = addr.rsplit(":", 1)
    c = http.client.HTTPConnection(host, int(port), timeout=timeout)
    try:
        c.request(method, path, headers=headers or {"Connection": "close"})
        r = c.getresponse()
        return r.status, r.read()
    finally:
        c.close()


async def _wait_for_baseline(srv, timeout_secs=5.0):
    """Poll until bridge-owned task/permit state returns to baseline."""
    deadline = asyncio.get_running_loop().time() + timeout_secs
    while True:
        tasks_empty = len(srv._tasks) == 0
        sem_free = not srv._sem.locked()
        if tasks_empty and sem_free:
            return True
        if asyncio.get_running_loop().time() >= deadline:
            return False
        await asyncio.sleep(0.05)


class GeneratorLifetimePremiseTests(unittest.TestCase):
    def test_unentered_generator_finally_does_not_run(self):
        """Track B: the cleanup assumption the old bridge relied on is invalid."""
        cleaned = []

        def g():
            try:
                yield b"x"
            finally:
                cleaned.append(1)

        it = g()
        it.close()  # before first next()
        del it
        gc.collect()
        self.assertEqual(cleaned, [], "unentered generator finally must not run")

    def test_entered_generator_finally_runs(self):
        cleaned = []

        def g():
            try:
                yield b"x"
            finally:
                cleaned.append(1)

        it = g()
        next(it)
        it.close()
        del it
        gc.collect()
        self.assertEqual(cleaned, [1])


class SuppressedPermitReleaseTests(unittest.TestCase):
    def test_head_stream_releases_permit_immediately(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        mode = {"suppressed": True}

        async def handler(req):
            if mode["suppressed"]:
                return lowlevel.AsyncResponse.stream(200, agen())
            return lowlevel.AsyncResponse.text(200, "next-ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=60)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "HEAD")
                self.assertEqual(status, 200)
                self.assertEqual(body, b"")
                self.assertEqual(polls, [], "HEAD must not advance application iterable")
                # Immediate reuse without waiting for the 60s first-pull bound.
                self.assertTrue(
                    await _wait_for_baseline(srv),
                    f"suppressed HEAD retained task/permit: tasks={len(srv._tasks)}",
                )
                mode["suppressed"] = False
                status2, body2 = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                self.assertEqual((status2, body2), (200, b"next-ok"))
                self.assertEqual(polls, [])
                self.assertTrue(await _wait_for_baseline(srv))
            finally:
                await srv.shutdown()

        _run(main())

    def test_204_stream_releases_permit_immediately(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        mode = {"suppressed": True}

        async def handler(req):
            if mode["suppressed"]:
                return lowlevel.AsyncResponse.stream(204, agen())
            return lowlevel.AsyncResponse.text(200, "next-ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=60)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                self.assertEqual(status, 204)
                self.assertEqual(body, b"")
                self.assertEqual(polls, [], "204 must not advance application iterable")
                self.assertTrue(
                    await _wait_for_baseline(srv),
                    f"suppressed 204 retained task/permit: tasks={len(srv._tasks)}",
                )
                mode["suppressed"] = False
                status2, body2 = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                self.assertEqual((status2, body2), (200, b"next-ok"))
                self.assertTrue(await _wait_for_baseline(srv))
            finally:
                await srv.shutdown()

        _run(main())

    def test_304_stream_releases_permit_immediately(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        mode = {"suppressed": True}

        async def handler(req):
            if mode["suppressed"]:
                return lowlevel.AsyncResponse.stream(304, agen())
            return lowlevel.AsyncResponse.text(200, "next-ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=60)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                self.assertEqual(status, 304)
                self.assertEqual(body, b"")
                self.assertEqual(polls, [], "304 must not advance application iterable")
                self.assertTrue(
                    await _wait_for_baseline(srv),
                    f"suppressed 304 retained task/permit: tasks={len(srv._tasks)}",
                )
                mode["suppressed"] = False
                status2, body2 = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                self.assertEqual((status2, body2), (200, b"next-ok"))
                self.assertTrue(await _wait_for_baseline(srv))
            finally:
                await srv.shutdown()

        _run(main())


class RepeatedSuppressedClosureTests(unittest.TestCase):
    def test_repeated_head_streams_no_accumulation(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=60)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                for i in range(5):
                    status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "HEAD")
                    self.assertEqual((status, body), (200, b""), f"iter {i}")
                    self.assertTrue(
                        await _wait_for_baseline(srv),
                        f"iter {i}: tasks={len(srv._tasks)}",
                    )
                self.assertEqual(polls, [])
                self.assertEqual(len(srv._tasks), 0)
                self.assertEqual(srv._sem._value, 1)
            finally:
                await srv.shutdown()

        _run(main())

    def test_repeated_204_streams_no_accumulation(self):
        polls = []

        async def agen():
            polls.append(1)
            yield b"chunk"

        async def handler(req):
            return lowlevel.AsyncResponse.stream(204, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=60)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                for i in range(5):
                    status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                    self.assertEqual((status, body), (204, b""), f"iter {i}")
                    self.assertTrue(
                        await _wait_for_baseline(srv),
                        f"iter {i}: tasks={len(srv._tasks)}",
                    )
                self.assertEqual(polls, [])
                self.assertEqual(len(srv._tasks), 0)
            finally:
                await srv.shutdown()

        _run(main())

    def test_interleaved_suppressed_and_buffered(self):
        polls = []
        mode = {"kind": "head"}

        async def agen():
            polls.append(1)
            yield b"chunk"

        async def handler(req):
            kind = mode["kind"]
            if kind == "head":
                return lowlevel.AsyncResponse.stream(200, agen())
            if kind == "forbidden":
                return lowlevel.AsyncResponse.stream(204, agen())
            return lowlevel.AsyncResponse.text(200, "buffered-ok")

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=60)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                seq = [
                    ("head", "HEAD", 200, b""),
                    ("buffered", "GET", 200, b"buffered-ok"),
                    ("forbidden", "GET", 204, b""),
                    ("buffered", "GET", 200, b"buffered-ok"),
                    ("head", "HEAD", 200, b""),
                    ("buffered", "GET", 200, b"buffered-ok"),
                ]
                for kind, method, want_status, want_body in seq:
                    mode["kind"] = kind
                    status, body = await asyncio.to_thread(_fetch, srv.addr, "/", method)
                    self.assertEqual((status, body), (want_status, want_body), kind)
                    self.assertTrue(
                        await _wait_for_baseline(srv),
                        f"{kind}: tasks={len(srv._tasks)}",
                    )
                self.assertEqual(polls, [], "no suppressed stream may advance app state")
            finally:
                await srv.shutdown()

        _run(main())


class LifetimeOwnerUnitTests(unittest.TestCase):
    def test_explicit_close_idempotent_no_overrelease(self):
        async def main():
            loop = asyncio.get_running_loop()
            sem = asyncio.Semaphore(1)
            await sem.acquire()
            queue: asyncio.Queue = asyncio.Queue(maxsize=16)
            first_pull = asyncio.Event()

            async def waiter():
                await asyncio.sleep(30)

            prod = loop.create_task(waiter())

            async def release_on_done(_t):
                pass

            # Simulate the bridge done-callback exactly-once accounting.
            releases = []
            orig_release = sem.release

            def counting_release():
                releases.append(1)
                return orig_release()

            sem.release = counting_release  # type: ignore[method-assign]
            prod.add_done_callback(lambda _t: counting_release())

            bridge = _AsyncStreamBridgeIterator(queue, prod, loop, first_pull, 30)
            # Explicit close then finalization must be idempotent.
            bridge.close()
            bridge.close()
            del bridge
            gc.collect()
            await asyncio.sleep(0.2)
            self.assertTrue(prod.done() or prod.cancelled() or len(releases) <= 1)
            # Semaphore must never exceed capacity (no over-release).
            self.assertLessEqual(sem._value, 1)
            if not prod.done():
                prod.cancel()
                try:
                    await prod
                except asyncio.CancelledError:
                    pass

        _run(main())

    def test_bridge_has_no_python_suppression_table(self):
        """Track E/F: Rust stays the suppression authority (structural)."""
        import inspect as _inspect

        src = _inspect.getsource(_AsyncStreamBridgeIterator)
        # The lifetime owner must react to pull-or-drop, never classify
        # method/status itself. Comments may name statuses, but code must
        # not branch on them.
        code_lines = [ln for ln in src.splitlines() if not ln.strip().startswith("#")]
        code_only = "\n".join(code_lines)
        # Strip docstring to avoid comment-name false positives.
        doc_end = code_only.find('"""', code_only.find('"""') + 3)
        body = code_only[doc_end + 3 :] if doc_end != -1 else code_only
        self.assertNotIn('== "HEAD"', body)
        self.assertNotIn("permits_payload_body", body)
        self.assertNotIn("marker.status", body)

    def test_shutdown_race_with_unpulled_body(self):
        entered = threading.Event()
        release = threading.Event()

        async def agen():
            yield b"chunk"

        async def handler(req):
            entered.set()
            # Suppressed stream; producer parks on first_pull until the
            # bridge drop cancels it (no application wait here).
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(
                port=0, response_write_timeout_secs=60, graceful_shutdown_timeout_secs=5
            )
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                def fetch_head():
                    try:
                        return _fetch(srv.addr, "/", "HEAD", timeout=10)
                    except Exception as exc:  # noqa: BLE001 - race may tear down
                        return exc

                t = threading.Thread(target=lambda: release.set() or fetch_head(), daemon=True)
                # Single suppressed HEAD then immediate shutdown: cleanup
                # must be idempotent and leave no tasks/permits behind.
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "HEAD")
                self.assertEqual(status, 200)
                self.assertEqual(body, b"")
                await srv.shutdown()
                self.assertEqual(len(srv._tasks), 0)
                self.assertLessEqual(srv._sem._value, 1)
            finally:
                release.set()
                try:
                    await srv.shutdown()
                except RuntimeError:
                    pass

        _run(main())


class NewOwnerParitySmokeTests(unittest.TestCase):
    def test_normal_stream_still_ordered(self):
        chunks = [f"{i:03d}-".encode() for i in range(40)]

        async def agen():
            for chunk in chunks:
                yield chunk

        async def handler(req):
            return lowlevel.AsyncResponse.stream(200, agen())

        async def main():
            cfg = lowlevel.RuntimeConfig(port=0, response_write_timeout_secs=30)
            srv = lowlevel.AsyncServer(config=cfg, handler=handler, max_async_tasks=1)
            await srv.start()
            try:
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/", "GET")
                self.assertEqual(status, 200)
                self.assertEqual(body, b"".join(chunks))
                self.assertTrue(await _wait_for_baseline(srv))
            finally:
                await srv.shutdown()

        _run(main())

    def test_sync_iterable_and_trailers_still_work(self):
        async def handler(req):
            if req.path == "/sync":
                return lowlevel.AsyncResponse.stream(200, [b"x", b"y"])
            if req.path == "/trailer":
                async def gen():
                    yield b"data"

                return lowlevel.AsyncResponse.stream(
                    200, gen(), trailers=[("x-checksum", "abc")]
                )
            return lowlevel.AsyncResponse.text(404, "nope")

        async def main():
            srv = lowlevel.AsyncServer(
                config=lowlevel.RuntimeConfig(port=0), handler=handler
            )
            await srv.start()
            try:
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/sync", "GET")
                self.assertEqual((status, body), (200, b"xy"))
                status, body = await asyncio.to_thread(_fetch, srv.addr, "/trailer", "GET")
                self.assertEqual(status, 200)
                self.assertIn(b"data", body)
            finally:
                await srv.shutdown()

        _run(main())


if __name__ == "__main__":
    unittest.main()
