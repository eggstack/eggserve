"""Canonical low-level runtime/service substrate (Plan 166).

This module is the public embedding surface for building a bounded HTTP
application server without the ``http.server`` compatibility facade::

    from eggserve import lowlevel

    config = lowlevel.RuntimeConfig(bind="127.0.0.1", port=0)
    server = lowlevel.Server(config=config, handler=my_handler)
    server.start()
    server.wait_ready()
    ...
    server.shutdown()
    server.wait()

The runtime owns sockets, parsing, framing, timeouts, admission, and
shutdown. Python handlers receive only canonical values (``Request`` /
``Response``) and never raw sockets, Hyper objects, or Tokio objects.
Network I/O stays in Rust/Tokio; at most ``max_python_callbacks`` handlers
execute concurrently per server. Generic in-flight admission is acquired by
the runtime before the Python callback permit, so limits cannot deadlock.

Request bodies are one-shot (``read()`` vs ``iter_chunks()`` are mutually
exclusive, ceilings enforced by Rust). Responses may be buffered
(``Response.bytes``/``text``/``empty``) or incrementally streamed via
``Response.stream(status, iterable, headers, content_length)`` through a
bounded 16-chunk bridge: client backpressure eventually stops iterator
advancement, HEAD never advances the iterator, and iterator failures close
the connection with sanitized diagnostics only. Async producers are not
supported; keep asyncio ownership downstream.

Static composition belongs to the caller (no routing in EggServe)::

    static = lowlevel.StaticResponder(lowlevel.ServerSecureRoot("public"))
    def handler(request):
        if request.path.startswith("/static/"):
            return static.respond("GET", request.path)
        return lowlevel.Response.text(200, "hello")

``eggserve.server`` remains the stdlib-shaped facade; both surfaces share
the same Rust runtime internally.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, Optional

from eggserve._native import (
    BodySource, BodySourceError, BodyChunkIterator, ConnectionInfo,
    DuplicateHeaderError, EggserveError, HeaderBlock, HeaderError,
    HttpVersion, HttpVersionError, Method, MethodError, PathPolicy,
    PathPolicyError, Request, RequestBody, RequestBodyCancelledError,
    RequestBodyConsumedError, RequestBodyDisconnectedError,
    RequestBodyError, RequestBodyIncompleteError, RequestBodyRejectedError,
    RequestBodyTimeoutError, RequestBodyTooLargeError, RequestTarget,
    RequestTargetError, RequestValidationError, ResolvedDirectory,
    ResolvedFile, ResolvedResource, Response, ResponseConstructionError,
    SecureRoot, SecureRootError, StaticPolicy, generate_etag, parse_http_version,
    parse_method, validate_method, validate_request_body, validate_request_target,
)
from eggserve._native import (
    Server as _NativeServer,
    ServerBodySource,
    ServerRequestError,
    ServerSecureRoot,
    StaticPolicyWrapper,
    StaticResponder,
)

__all__ = [
    "BodySource", "BodySourceError", "BodyChunkIterator", "ConnectionInfo",
    "DuplicateHeaderError", "EggserveError", "HeaderBlock", "HeaderError",
    "HttpVersion", "HttpVersionError", "Method", "MethodError", "PathPolicy",
    "PathPolicyError", "Request", "RequestBody", "RequestBodyCancelledError",
    "RequestBodyConsumedError", "RequestBodyDisconnectedError", "RequestBodyError",
    "RequestBodyIncompleteError", "RequestBodyRejectedError", "RequestBodyTimeoutError",
    "RequestBodyTooLargeError", "RequestTarget", "RequestTargetError",
    "RequestValidationError", "ResolvedDirectory", "ResolvedFile", "ResolvedResource",
    "Response", "ResponseConstructionError", "SecureRoot", "SecureRootError",
    "StaticPolicy", "generate_etag", "parse_http_version", "parse_method",
    "validate_method", "validate_request_body", "validate_request_target",
    # Plan 166 runtime/service substrate (public, backed by _native).
    "RuntimeConfig", "Server", "ServerBodySource", "ServerRequestError",
    "ServerSecureRoot", "StaticPolicyWrapper", "StaticResponder",
]


@dataclass(frozen=True)
class RuntimeConfig:
    """Validated runtime/service configuration for :class:`Server`.

    Only operator-meaningful controls are exposed; Rust internals such as
    Tokio objects, Hyper tuning, and custom clock providers stay private.
    ``None`` disables a control where applicable; zero is never overloaded
    as unlimited (``max_requests_per_connection=None`` means unlimited,
    ``0`` is rejected).
    """

    bind: str = "127.0.0.1"
    port: int = 8000
    public: bool = False
    max_connections: int = 64
    max_file_streams: int = 32
    max_python_callbacks: int = 8
    max_in_flight_requests: int = 64
    header_timeout_secs: int = 10
    connection_total_timeout_secs: int = 60
    handler_timeout_secs: int = 30
    body_timeout_secs: int = 30
    graceful_shutdown_timeout_secs: int = 10
    keep_alive_idle_timeout_secs: int = 60
    max_requests_per_connection: Optional[int] = None
    response_write_timeout_secs: int = 30
    max_buf_size: int = 65536
    max_headers: int = 100
    max_header_bytes: int = 32768
    max_request_target_bytes: int = 8192
    request_body_mode: str = "reject"
    max_request_body_bytes: int = 0
    tls_certfile: Optional[str] = None
    tls_keyfile: Optional[str] = None
    server_header: Optional[str] = None
    date_policy: str = "system"
    stripped_response_headers: tuple = ()
    error_policy: str = "minimal"

    def __post_init__(self) -> None:
        if self.request_body_mode not in ("reject", "buffer", "stream"):
            raise ValueError("request_body_mode must be 'reject', 'buffer', or 'stream'")
        if self.date_policy not in ("system", "suppress"):
            raise ValueError("date_policy must be 'system' or 'suppress'")
        if self.error_policy not in ("minimal", "empty"):
            raise ValueError("error_policy must be 'minimal' or 'empty'")
        if self.max_requests_per_connection is not None and self.max_requests_per_connection <= 0:
            raise ValueError("max_requests_per_connection must be >= 1 or None (unlimited)")


class Server:
    """Handler-only low-level server over the shared Rust runtime.

    Requires no static root. The runtime owns sockets, parsing, framing,
    timeouts, admission, and shutdown; ``handler`` is a synchronous
    ``Callable[[Request], Response]`` executed under ``max_python_callbacks``
    admission. Coroutine handlers are rejected.

    Timeout honesty: EggServe can stop waiting and close the HTTP request,
    but cannot kill arbitrary executing Python code ("HTTP request timed
    out" != "Python thread forcibly terminated").
    """

    def __init__(
        self,
        config: Optional[RuntimeConfig] = None,
        handler: Optional[Callable] = None,
        *,
        _native: Optional[object] = None,
    ) -> None:
        if _native is not None:
            self._native = _native
            return
        if handler is None:
            raise ValueError("lowlevel.Server requires a synchronous handler callable")
        if not callable(handler):
            raise TypeError("handler must be callable")
        cfg = config or RuntimeConfig()
        # Handler-only: no static root is constructed or validated.
        self._native = _NativeServer(
            None,
            bind=cfg.bind,
            port=cfg.port,
            handler=handler,
            public=cfg.public,
            max_connections=cfg.max_connections,
            max_file_streams=cfg.max_file_streams,
            max_python_callbacks=cfg.max_python_callbacks,
            header_timeout_secs=cfg.header_timeout_secs,
            connection_total_timeout_secs=cfg.connection_total_timeout_secs,
            handler_timeout_secs=cfg.handler_timeout_secs,
            graceful_shutdown_timeout_secs=cfg.graceful_shutdown_timeout_secs,
            request_body_mode=cfg.request_body_mode,
            max_request_body_bytes=cfg.max_request_body_bytes,
            body_timeout_secs=cfg.body_timeout_secs,
            tls_certfile=cfg.tls_certfile,
            tls_keyfile=cfg.tls_keyfile,
            max_in_flight_requests=cfg.max_in_flight_requests,
            max_buf_size=cfg.max_buf_size,
            max_headers=cfg.max_headers,
            max_header_bytes=cfg.max_header_bytes,
            max_request_target_bytes=cfg.max_request_target_bytes,
            keep_alive_idle_timeout_secs=cfg.keep_alive_idle_timeout_secs,
            max_requests_per_connection=cfg.max_requests_per_connection,
            response_write_timeout_secs=cfg.response_write_timeout_secs,
            server_header=cfg.server_header,
            date_policy=cfg.date_policy,
            stripped_response_headers=list(cfg.stripped_response_headers),
            error_policy=cfg.error_policy,
        )

    @property
    def addr(self):
        return self._native.addr

    @property
    def state(self):
        return self._native.state

    def start(self) -> None:
        self._native.start()

    def wait_ready(self) -> None:
        self._native.wait_ready()

    def shutdown(self) -> None:
        self._native.shutdown()

    def stop(self) -> None:
        self._native.stop()

    def wait(self):
        return self._native.wait()

    def force_shutdown(self, timeout_secs: float = 10.0):
        return self._native.force_shutdown(timeout_secs)

    def __enter__(self) -> "Server":
        self.start()
        self.wait_ready()
        return self

    def __exit__(self, *args) -> bool:
        try:
            self.stop()
        finally:
            return False

    def __repr__(self) -> str:
        return f"<lowlevel.Server {self.addr or 'not started'}>"
