"""Representative installed-wheel typing fixture (Plan 246)."""

from collections.abc import Iterable
from pathlib import Path

import eggserve
from eggserve import lowlevel
from eggserve.server import HTTPServer, SimpleHTTPRequestHandler, ThreadingHTTPServer
from eggserve.subprocess import ServeConfig, StaticPolicy


class Handler(SimpleHTTPRequestHandler):
    pass


def sync_handler(request: lowlevel.Request) -> lowlevel.Response:
    return lowlevel.Response.text(200, request.path)


async def async_handler(request: lowlevel.AsyncRequest) -> lowlevel.Response:
    return lowlevel.AsyncResponse.text(200, request.path)


def streamed(request: lowlevel.Request) -> lowlevel.Response:
    chunks: Iterable[bytes] = (part for part in (b"a", b"b"))
    return lowlevel.Response.stream(200, chunks)


def build(path: Path) -> None:
    policy = StaticPolicy(directory_listing=False)
    config = ServeConfig(directory=path, policy=policy)
    _server = HTTPServer(("127.0.0.1", 0), Handler)
    _threaded = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    _process_config = config
    runtime = lowlevel.RuntimeConfig(bind="127.0.0.1", port=0)
    _sync = lowlevel.Server(config=runtime, handler=sync_handler)
    _async = lowlevel.AsyncServer(config=runtime, handler=async_handler)
    _secure_root = lowlevel.ServerSecureRoot(path)
    _responder = lowlevel.StaticResponder(_secure_root)
    _ = (eggserve, _process_config, _sync, _async, _responder, streamed)
