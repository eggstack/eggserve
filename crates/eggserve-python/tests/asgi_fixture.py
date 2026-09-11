"""Plan 204 Track J: test/example ASGI adapter (not the production product).

Maps `eggserve.lowlevel.AsyncServer` to ASGI 3 HTTP/WebSocket semantics
sufficient to qualify the async bridge (incremental bodies, disconnects,
trailers, interim, tunnels). No worker processes, reloaders, lifespan
ownership, router/framework loading, or Gunicorn integration — a downstream
real ASGI server owns those. WebSocket framing lives here (fixture, not
core); EggServe core never parses WS frames.
"""

from __future__ import annotations

import asyncio
import base64
import hashlib
import struct
from typing import Any, Awaitable, Callable, Dict, List, Optional, Tuple

from eggserve import lowlevel


# ---------------------------------------------------------------------------
# Minimal WebSocket codec (fixture only, not core).
# ---------------------------------------------------------------------------

_WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

#: Live tunnel drivers (fixture-global strong refs; no orphan: each driver
#: ends on tunnel close/disconnect/app end; tests close explicitly).
_LIVE_TUNNELS: set["asyncio.Task"] = set()


def websocket_accept_key_to_accept(key: str) -> str:
    digest = hashlib.sha1((key.strip() + _WS_GUID).encode("ascii")).digest()
    return base64.b64encode(digest).decode("ascii")


class _WsReader:
    """Buffered WS frame reader (preserves leftovers across recvs)."""

    def __init__(self, tunnel: lowlevel.AsyncTunnel) -> None:
        self._tunnel = tunnel
        self._buf = bytearray()

    async def _fill(self, n: int) -> Optional[bytes]:
        while len(self._buf) < n:
            chunk = await self._tunnel.recv()
            if chunk is None:
                return None
            self._buf.extend(chunk)
            if len(self._buf) > 10 * 1024 * 1024:
                raise ValueError("ws frame too large")
        out = bytes(self._buf[:n])
        del self._buf[:n]
        return out

    async def read_frame(self) -> Optional[Tuple[int, bytes]]:
        """Read one WS frame. Returns (opcode, payload) or None on EOF."""
        hdr = await self._fill(2)
        if hdr is None:
            return None
        b1, b2 = hdr[0], hdr[1]
        opcode = b1 & 0x0F
        masked = bool(b2 & 0x80)
        length = b2 & 0x7F
        if length == 126:
            ext = await self._fill(2)
            if ext is None:
                return None
            (length,) = struct.unpack("!H", ext)
        elif length == 127:
            ext = await self._fill(8)
            if ext is None:
                return None
            (length,) = struct.unpack("!Q", ext)
        mask = b""
        if masked:
            m = await self._fill(4)
            if m is None:
                return None
            mask = m
        payload = await self._fill(length)
        if payload is None:
            return None
        if masked:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        return opcode, payload


async def _ws_read_frame(tunnel: lowlevel.AsyncTunnel) -> Optional[Tuple[int, bytes]]:
    # Backward-compatible one-shot read (no leftover preservation; prefer
    # `_WsReader` for pipelined frames).
    return await _WsReader(tunnel).read_frame()


async def _ws_send_frame(tunnel: lowlevel.AsyncTunnel, opcode: int, payload: bytes) -> None:
    hdr = bytes([(0x80 | (opcode & 0x0F))])
    n = len(payload)
    if n < 126:
        hdr += bytes([n])
    elif n < 65536:
        hdr += bytes([126]) + struct.pack("!H", n)
    else:
        hdr += bytes([127]) + struct.pack("!Q", n)
    await tunnel.send(hdr + payload)


# ---------------------------------------------------------------------------
# ASGI HTTP bridge.
# ---------------------------------------------------------------------------

async def _asgi_http_handler(areq: lowlevel.AsyncRequest, app) -> Any:
    scope: Dict[str, Any] = {
        "type": "http",
        "asgi": {"version": "3.0", "spec_version": "2.5"},
        "http_version": areq.http_version.replace("HTTP/", "") if areq.http_version else "1.1",
        "method": areq.method,
        "scheme": (areq.effective_scheme or areq.scheme or "http"),
        "path": areq.path,
        "raw_path": areq.path_bytes,
        "query_string": areq.query_bytes or b"",
        "root_path": b"",
        "headers": [(n.lower(), v) for (n, v) in areq.header_items_bytes],
        "client": tuple(areq.remote_address) if areq.remote_address else None,
        "server": tuple(areq.local_address) if areq.local_address else None,
        "extensions": {"http.response.trailers": {}},
    }
    # Body iterator (incremental, no read_all for streaming path).
    body_iter = None
    if areq.body is not None:
        async def _gen():
            async for chunk in areq.body.aiter_chunks():
                yield chunk
        body_iter = _gen()
        body_exhausted = False
    else:
        body_exhausted = True

    response_started: Dict[str, Any] = {}
    send_queue: asyncio.Queue = asyncio.Queue(maxsize=16)
    response_start_event = asyncio.Event()

    async def receive() -> Dict[str, Any]:
        nonlocal body_exhausted, body_iter
        if not body_exhausted:
            if areq.is_disconnected():
                return {"type": "http.disconnect"}
            try:
                assert body_iter is not None
                chunk = await body_iter.__anext__()
                # Peek disconnect? If disconnected during chunk, next call
                # returns disconnect (transport will close anyway).
                return {"type": "http.request", "body": chunk, "more_body": True}
            except StopAsyncIteration:
                body_exhausted = True
                return {"type": "http.request", "body": b"", "more_body": False}
        # Body done: wait for disconnect (long-poll notification).
        disconnected = await areq.wait_disconnected()
        if disconnected:
            return {"type": "http.disconnect"}
        return {"type": "http.disconnect"}

    async def send(event: Dict[str, Any]) -> None:
        t = event.get("type")
        if t == "http.response.start":
            response_started["status"] = int(event["status"])
            response_started["headers"] = list(event.get("headers", []))
            response_started["trailers"] = bool(event.get("trailers", False))
            response_start_event.set()
        elif t == "http.response.body":
            await send_queue.put(event)
        elif t == "http.response.trailers":
            await send_queue.put(event)
        elif t == "http.response.debug":
            pass
        else:
            raise RuntimeError(f"unsupported ASGI send type {t!r}")

    async def _run_app():
        try:
            await app(scope, receive, send)
        finally:
            # App returned: terminate the body stream (EOF + trailers if any).
            try:
                send_queue.put_nowait({"type": "__app_done__"})
            except asyncio.QueueFull:
                pass

    app_task = asyncio.create_task(_run_app())
    try:
        # Wait for response-start (bounded by handler timeout honesty:
        # use 30s cap; app that never responds -> 500 via outer shim).
        try:
            await asyncio.wait_for(response_start_event.wait(), timeout=30)
        except asyncio.TimeoutError:
            app_task.cancel()
            from eggserve._native import Response as _Resp

            return _Resp.text(500, "Internal Server Error")
        status = int(response_started.get("status", 500))
        headers = {}
        for (n, v) in response_started.get("headers", []):
            headers[n.decode("latin-1").lower()] = v.decode("latin-1")

        async def _body_producer():
            trailers_out: Optional[List[Tuple[bytes, bytes]]] = None
            while True:
                event = await send_queue.get()
                et = event.get("type")
                if et == "__app_done__":
                    break
                if et == "http.response.body":
                    body = bytes(event.get("body", b""))
                    more = bool(event.get("more_body", False))
                    if body:
                        yield body
                    if not more:
                        break
                elif et == "http.response.trailers":
                    trailers_out = list(event.get("headers", []))
                    break
                else:
                    break
            # Drain app task (permit hygiene; app already done or finishing).
            try:
                await asyncio.wait_for(asyncio.shield(app_task), timeout=5)
            except (asyncio.TimeoutError, asyncio.CancelledError, Exception):
                pass

        # Trailers: ASGI `http.response.trailers` event carries headers;
        # EggServe needs them at commitment (before streaming). Our producer
        # learns trailers only at the end (after body). To satisfy the
        # canonical `with_trailers` (trailers at construction), buffer
        # trailers from `http.response.start`? ASGI advertises trailers via
        # `trailers: True` in start, actual headers arrive later in
        # `http.response.trailers`. EggServe's `stream_with_trailers`
        # requires trailers upfront. For the fixture, collect trailers by
        # waiting for app completion when `trailers` was advertised (bounded
        # buffering of trailers only, not body? Body already streamed via
        # queue above — but trailers unknown upfront breaks `with_trailers`.
        #
        # Pragmatic fixture path: if trailers advertised, buffer the full
        # body (bounded by ceiling? No — trailers use-case is small bodies;
        # large streaming + trailers is out of scope for the fixture).
        # Document the limitation: streaming + trailers requires upfront
        # trailers (fixture buffers when advertised).
        if response_started.get("trailers"):
            # Buffer (bounded: fail 500 if body exceeds 1 MiB in fixture).
            chunks: List[bytes] = []
            total = 0
            found_trailers: List[Tuple[str, str]] = []
            async for chunk in _body_producer():
                chunks.append(chunk)
                total += len(chunk)
                if total > 1024 * 1024:
                    from eggserve._native import Response as _Resp

                    return _Resp.text(500, "Internal Server Error")
            # `http.response.trailers` headers were consumed as `trailers_out`
            # inside `_body_producer` (not returned). Re-derive: for the
            # fixture, trailers arrive as the last queue event; since we
            # broke on body end, re-check the queue for a trailers event.
            # Simplification: fixture apps put trailers via `send` which we
            # already queued; `_body_producer` stopped at body end without
            # exposing them. Fix by re-reading one more event if available.
            try:
                extra = send_queue.get_nowait()
                if isinstance(extra, dict) and extra.get("type") == "http.response.trailers":
                    for (n, v) in extra.get("headers", []):
                        found_trailers.append((n.decode("latin-1"), v.decode("latin-1")))
            except asyncio.QueueEmpty:
                pass
            from eggserve._native import Response as _Resp

            return _Resp.stream_with_trailers(status, chunks, headers, None, found_trailers)
        return lowlevel.AsyncResponse.stream(status, _body_producer(), headers)
    finally:
        # If the handler returns without the app finishing (streaming),
        # the app task continues as the producer (permit transferred via
        # AsyncServer streaming bridge). Here we do not cancel on success;
        # `_body_producer` drains the app task. On early error paths above
        # (500 returns), cancel the app task (no orphan).
        if app_task.done():
            pass


def create_asgi_handler(app) -> Callable:
    async def handler(areq: lowlevel.AsyncRequest):
        # WebSocket scope when a tunnel is present and the client asked for
        # `websocket` (H1 Upgrade or Extended CONNECT `:protocol`).
        cap = None
        try:
            if areq.has_tunnel():
                cap = areq.take_tunnel()
        except Exception:
            cap = None
        if cap is not None and (cap.protocol == "websocket" or cap.kind == "http1-upgrade"):
            return await _asgi_websocket_handler(areq, cap, app)
        if cap is not None:
            # Non-websocket tunnel for a WS-only fixture app: deny ordinary.
            from eggserve._native import Response as _Resp

            return _Resp.text(400, "Bad Request")
        return await _asgi_http_handler(areq, app)

    return handler


async def _asgi_websocket_handler(areq: lowlevel.AsyncRequest, cap, app) -> Any:
    scope: Dict[str, Any] = {
        "type": "websocket",
        "asgi": {"version": "3.0", "spec_version": "2.5"},
        "http_version": "1.1",
        "scheme": (areq.effective_scheme or areq.scheme or "ws"),
        "path": areq.path,
        "raw_path": areq.path_bytes,
        "query_string": areq.query_bytes or b"",
        "root_path": b"",
        "headers": [(n.lower(), v) for (n, v) in areq.header_items_bytes],
        "client": tuple(areq.remote_address) if areq.remote_address else None,
        "server": tuple(areq.local_address) if areq.local_address else None,
        "subprotocols": [],
        "extensions": {},
    }
    recv_queue: asyncio.Queue = asyncio.Queue(maxsize=16)
    send_queue: asyncio.Queue = asyncio.Queue(maxsize=16)
    accepted = asyncio.Event()
    accept_headers: List[Tuple[bytes, bytes]] = []

    # Seed `websocket.connect`.
    await recv_queue.put({"type": "websocket.connect"})

    async def receive() -> Dict[str, Any]:
        return await recv_queue.get()

    async def send(event: Dict[str, Any]) -> None:
        t = event.get("type")
        if t == "websocket.accept":
            # App accepts: capture handshake headers (including
            # Sec-WebSocket-Accept computed by the app via the fixture
            # helper) for the native accept below.
            for (n, v) in event.get("headers", []):
                accept_headers.append((bytes(n), bytes(v)))
            accepted.set()
        elif t in ("websocket.send", "websocket.close"):
            # Forward data frames to the tunnel driver incrementally.
            # The tunnel object is created after accept below; queue data
            # sends until then (bounded 16).
            await send_queue.put(event)
        elif t == "websocket.disconnect":
            pass
        else:
            raise RuntimeError(f"unsupported WS send {t!r}")

    async def _run_app():
        try:
            await app(scope, receive, send)
        finally:
            try:
                send_queue.put_nowait({"type": "__app_done__"})
            except asyncio.QueueFull:
                pass
            try:
                recv_queue.put_nowait({"type": "websocket.disconnect", "code": 1000})
            except asyncio.QueueFull:
                pass

    import asyncio as _aio

    app_task = _aio.create_task(_run_app())
    try:
        try:
            await _aio.wait_for(accepted.wait(), timeout=10)
        except _aio.TimeoutError:
            app_task.cancel()
            from eggserve._native import Response as _Resp

            return _Resp.text(500, "Internal Server Error")
        # Native accept (one-shot, validated). Handshake headers from the app
        # (text). Runtime owns 101/framing (no raw socket).
        text_headers = [(n.decode("latin-1"), v.decode("latin-1")) for (n, v) in accept_headers]
        handshake, tunnel = cap.accept(text_headers)

        async def _driver():
            # Incoming: tunnel recv -> WS frames -> recv_queue.
            # Outgoing: send_queue (app `websocket.send`/`close`) -> WS frames -> tunnel send.
            # Peer close/reset/shutdown ends both (no orphan).
            async def _incoming():
                reader = _WsReader(tunnel)
                while True:
                    frame = await reader.read_frame()
                    if frame is None:
                        try:
                            recv_queue.put_nowait({"type": "websocket.disconnect", "code": 1000})
                        except asyncio.QueueFull:
                            pass
                        break
                    opcode, payload = frame
                    if opcode == 0x8:  # close
                        try:
                            recv_queue.put_nowait({"type": "websocket.disconnect", "code": 1000})
                        except asyncio.QueueFull:
                            pass
                        break
                    elif opcode == 0x9:  # ping -> pong
                        try:
                            await _ws_send_frame(tunnel, 0xA, payload)
                        except Exception:
                            break
                    elif opcode == 0xA:  # pong -> ignore
                        continue
                    elif opcode in (0x1, 0x2):
                        try:
                            recv_queue.put_nowait(
                                {
                                    "type": "websocket.receive",
                                    "text": payload.decode("utf-8") if opcode == 0x1 else None,
                                    "bytes": payload if opcode == 0x2 else None,
                                }
                            )
                        except asyncio.QueueFull:
                            break
                    else:
                        continue

            async def _outgoing():
                while True:
                    event = await send_queue.get()
                    et = event.get("type") if isinstance(event, dict) else None
                    if et == "__app_done__":
                        break
                    if et == "websocket.send":
                        try:
                            if event.get("text") is not None:
                                await _ws_send_frame(tunnel, 0x1, event["text"].encode("utf-8"))
                            elif event.get("bytes") is not None:
                                await _ws_send_frame(tunnel, 0x2, bytes(event["bytes"]))
                        except Exception:
                            break
                    elif et == "websocket.close":
                        try:
                            await _ws_send_frame(tunnel, 0x8, b"")
                        except Exception:
                            pass
                        break
                    else:
                        continue

            incoming = _aio.create_task(_incoming())
            outgoing = _aio.create_task(_outgoing())
            done, pending = await _aio.wait(
                [incoming, outgoing, app_task], return_when=_aio.FIRST_COMPLETED
            )
            for t in pending:
                t.cancel()
            try:
                tunnel.close()
            except Exception:
                pass

        driver = _aio.create_task(_driver())
        # Keep a strong ref (no orphan/GC-cancel): driver ends on tunnel
        # close/disconnect/app end; tests close explicitly; shutdown with
        # active tunnel is qualified by client close -> driver end.
        _LIVE_TUNNELS.add(driver)

        def _driver_done(t):
            _LIVE_TUNNELS.discard(t)
            if not app_task.done():
                app_task.cancel()

        driver.add_done_callback(_driver_done)
        return handshake
    finally:
        pass
