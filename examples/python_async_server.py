"""Canonical async low-level service (Plan 204, experimental, H1-only).

Buffered plus bounded streamed responses over the shared native runtime
without the `http.server` compatibility facade. Run with
`python examples/python_async_server.py`; the demo binds loopback on an
ephemeral port when imported via `create_server()` for smoke tests.
"""

from __future__ import annotations

import asyncio

from eggserve import lowlevel


async def handler(request: lowlevel.AsyncRequest):
    if request.path == "/stream":
        async def gen():
            yield b"chunk-"
            yield b"one"

        return lowlevel.AsyncResponse.stream(200, gen())
    if request.path == "/echo" and request.body is not None:
        data = b"".join([c async for c in request.body.aiter_chunks()])
        return lowlevel.AsyncResponse.bytes(200, b"echo:" + data)
    return lowlevel.AsyncResponse.text(200, "hello-async")


def create_server(port: int = 0) -> lowlevel.AsyncServer:
    config = lowlevel.RuntimeConfig(
        bind="127.0.0.1",
        port=port,
        request_body_mode="stream",
        max_request_body_bytes=1 << 20,
    )
    return lowlevel.AsyncServer(config=config, handler=handler)


async def main() -> None:
    server = create_server(port=8000)
    await server.start()
    print(f"serving on {server.addr} (Ctrl+C to stop)")
    try:
        while True:
            await asyncio.sleep(3600)
    except KeyboardInterrupt:
        pass
    finally:
        await server.shutdown()


if __name__ == "__main__":
    asyncio.run(main())
