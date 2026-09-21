"""Representative installed-wheel typing fixture (Plans 246, 252).

Exercises property access and subclass overrides against the installed
wheel, not only construction, so stub drift against the runtime surface
fails strict checking.
"""

import asyncio
from collections.abc import AsyncIterator, Iterable
from pathlib import Path
from typing import Any, assert_type

import eggserve
from eggserve import lowlevel
from eggserve._native import TunnelRequest
from eggserve.server import (
    BaseHTTPRequestHandler,
    HTTPServer,
    SimpleHTTPRequestHandler,
    ThreadingHTTPServer,
)
from eggserve.subprocess import ServeConfig, StaticPolicy


class Handler(SimpleHTTPRequestHandler):
    pass


class LoggingHandler(SimpleHTTPRequestHandler):
    def log_request(self, code: Any = "-", size: Any = "-") -> None:
        return None

    def log_error(self, format: str, *args: Any) -> None:
        return None

    def log_message(self, format: str, *args: Any) -> None:
        return None


class BindServer(HTTPServer):
    def server_bind(self) -> None:
        super().server_bind()

    def server_activate(self) -> None:
        super().server_activate()


def sync_handler(request: lowlevel.Request) -> lowlevel.Response:
    return lowlevel.Response.text(200, request.path)


async def async_handler(request: lowlevel.AsyncRequest) -> lowlevel.Response:
    return lowlevel.AsyncResponse.text(200, request.path)


def streamed(request: lowlevel.Request) -> lowlevel.Response:
    chunks: Iterable[bytes] = (part for part in (b"a", b"b"))
    return lowlevel.Response.stream(200, chunks)


def use_async_request(request: lowlevel.AsyncRequest) -> None:
    assert_type(request.method, str)
    assert_type(request.path, str)
    assert_type(request.query, str)
    assert_type(request.headers, dict[str, str])
    assert_type(request.header_items, list[tuple[str, str]])
    assert_type(request.header_items_bytes, list[tuple[bytes, bytes]])
    assert_type(request.raw_target_bytes, bytes)
    assert_type(request.path_bytes, bytes)
    assert_type(request.query_bytes, bytes | None)
    assert_type(request.http_version, str)
    assert_type(request.authority, str | None)
    assert_type(request.scheme, str | None)
    assert_type(request.remote_addr, str | None)
    assert_type(request.remote_address, tuple[str, int] | None)
    assert_type(request.local_addr, str | None)
    assert_type(request.local_address, tuple[str, int] | None)
    assert_type(request.effective_addr, str | None)
    assert_type(request.effective_address, tuple[str, int] | None)
    assert_type(request.effective_scheme, str | None)
    assert_type(request.effective_authority, str | None)
    assert_type(request.proxy_provenance, str | None)
    assert_type(request.forwarded_provenance, str | None)
    assert_type(request.proxy_source, str | None)
    assert_type(request.proxy_destination, str | None)
    assert_type(request.tls_protocol_version, str | None)
    assert_type(request.tls_server_name, str | None)
    assert_type(request.tls_alpn, str | None)
    assert_type(request.client_authenticated, bool)
    assert_type(request.peer_certificates_present, bool)
    assert_type(request.has_body, bool)
    assert_type(request.body, lowlevel.AsyncBody | None)
    assert_type(request.is_disconnected(), bool)
    assert_type(request.cancellation_reason(), str | None)
    assert_type(request.has_tunnel(), bool)
    assert_type(request.tunnel_request(), TunnelRequest | None)
    assert_type(request.take_tunnel(), lowlevel.AsyncTunnelCapability | None)


async def use_async_body_and_lifecycle(request: lowlevel.AsyncRequest) -> None:
    body = request.body
    if body is not None:
        assert_type(body.declared_length, int | None)
        assert_type(body.bytes_received, int)
        assert_type(body.complete, bool)
        data: bytes = await body.aread()
        assert_type(data, bytes)
        async for chunk in body.aiter_chunks():
            assert_type(chunk, bytes)
        assert_type(await body.trailers(), list[tuple[str, str]] | None)
    assert_type(await request.wait_disconnected(), bool)
    assert_type(await request.wait_disconnected(timeout_secs=1.0), bool)
    assert_type(await request.send_interim(103), str)


def use_async_tunnel(cap: lowlevel.AsyncTunnelCapability) -> None:
    assert_type(cap.kind, str)
    assert_type(cap.protocol, str | None)
    assert_type(cap.authority, str | None)
    handshake, tunnel = cap.accept(headers=[("x-tunnel", "1")])
    assert_type(handshake, lowlevel.Response)
    assert_type(tunnel, lowlevel.AsyncTunnel)


async def use_async_tunnel_io(tunnel: lowlevel.AsyncTunnel) -> None:
    assert_type(tunnel.is_closed(), bool)
    assert_type(await tunnel.recv(), bytes | None)
    await tunnel.send(b"ping")
    tunnel.close()


def use_async_responses() -> None:
    assert_type(lowlevel.AsyncResponse.empty(204), lowlevel.Response)
    assert_type(lowlevel.AsyncResponse.bytes(200, b"ok"), lowlevel.Response)
    assert_type(lowlevel.AsyncResponse.text(200, "ok"), lowlevel.Response)

    async def gen() -> AsyncIterator[bytes]:
        yield b"chunk"

    marker: object = lowlevel.AsyncResponse.stream(200, gen())
    assert_type(marker, object)
    with_trailers: object = lowlevel.AsyncResponse.stream(
        200, gen(), content_length=None, trailers=[("x-sum", "1")]
    )
    assert_type(with_trailers, object)


async def use_async_server(server: lowlevel.AsyncServer) -> None:
    assert_type(server.addr, tuple[str, int] | None)
    assert_type(server.max_async_tasks, int)
    task: asyncio.Task[Any] = asyncio.ensure_future(asyncio.sleep(0))
    assert_type(server.track(task), asyncio.Task[Any])
    await server.start()
    await server.shutdown()


def use_base_handler(handler: BaseHTTPRequestHandler) -> None:
    assert_type(handler.error_message_format, str)
    assert_type(handler.error_content_type, str)
    assert_type(handler.responses, dict[int, tuple[str, str]])
    assert_type(handler.request, Any)
    assert_type(handler.server, Any)
    assert_type(handler.close_connection, bool)
    assert_type(handler.requestline, str)
    handler.log_request(200, 12)
    handler.log_error("code %d", 500)
    handler.log_message("hello %s", "world")


def build(path: Path) -> None:
    policy = StaticPolicy(directory_listing=False)
    config = ServeConfig(directory=path, policy=policy)
    _server = HTTPServer(("127.0.0.1", 0), Handler)
    _threaded = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    _logging = HTTPServer(("127.0.0.1", 0), LoggingHandler)
    _bound = BindServer(("127.0.0.1", 0), Handler)
    _process_config = config
    runtime = lowlevel.RuntimeConfig(bind="127.0.0.1", port=0)
    _sync = lowlevel.Server(config=runtime, handler=sync_handler)
    _async = lowlevel.AsyncServer(config=runtime, handler=async_handler)
    _secure_root = lowlevel.ServerSecureRoot(path)
    _responder = lowlevel.StaticResponder(_secure_root)
    _ = (eggserve, _process_config, _sync, _async, _responder, streamed)
    _ = (_logging, _bound)
