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

import asyncio
import concurrent.futures
import inspect
from dataclasses import dataclass
from typing import Any, Callable, Optional

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
    # Plan 204 async substrate (experimental, H1-only, asyncio-owned).
    "AsyncServer", "AsyncRequest", "AsyncBody", "AsyncResponse",
    "AsyncTunnel", "AsyncTunnelCapability",
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
    # Plan 202 trusted-proxy policy (safe defaults: nothing trusted).
    # `trusted_proxies` lists exact IPs/CIDRs trusted as immediate peers
    # (loopback included only when listed explicitly; no DNS). `proxy_protocol`
    # enables HAProxy PROXY v1/v2 preamble parsing before TLS/HTTP (only from
    # trusted peers). `forwarded_standard`/`forwarded_legacy` honor
    # `Forwarded` / `X-Forwarded-*` from trusted peers into
    # provenance-tagged effective fields (`effective_*` on Request);
    # `remote_addr` never changes for compatibility.
    trusted_proxies: tuple = ()
    trust_unix_local: bool = False
    proxy_protocol: bool = False
    forwarded_standard: bool = False
    forwarded_legacy: bool = False

    def __post_init__(self) -> None:
        if self.request_body_mode not in ("reject", "buffer", "stream"):
            raise ValueError("request_body_mode must be 'reject', 'buffer', or 'stream'")
        if self.date_policy not in ("system", "suppress"):
            raise ValueError("date_policy must be 'system' or 'suppress'")
        if self.error_policy not in ("minimal", "empty"):
            raise ValueError("error_policy must be 'minimal' or 'empty'")
        if self.max_requests_per_connection is not None and self.max_requests_per_connection <= 0:
            raise ValueError("max_requests_per_connection must be >= 1 or None (unlimited)")

    def _native_kwargs(self) -> dict:
        """Project this config into ``_NativeServer`` keyword arguments.

        Single Python-to-native projection path (Plan 182): ``Server``
        consumes this helper instead of listing every field independently,
        so a new/renamed runtime field cannot get a Python default without
        being forwarded. Only Python-domain enum/``None`` checks live in
        ``__post_init__``; Rust remains the final authority for runtime
        limits. ``stripped_response_headers`` is materialized as a list for
        the native constructor.
        """
        return {
            "bind": self.bind,
            "port": self.port,
            "public": self.public,
            "max_connections": self.max_connections,
            "max_file_streams": self.max_file_streams,
            "max_python_callbacks": self.max_python_callbacks,
            "header_timeout_secs": self.header_timeout_secs,
            "connection_total_timeout_secs": self.connection_total_timeout_secs,
            "handler_timeout_secs": self.handler_timeout_secs,
            "graceful_shutdown_timeout_secs": self.graceful_shutdown_timeout_secs,
            "request_body_mode": self.request_body_mode,
            "max_request_body_bytes": self.max_request_body_bytes,
            "body_timeout_secs": self.body_timeout_secs,
            "tls_certfile": self.tls_certfile,
            "tls_keyfile": self.tls_keyfile,
            "max_in_flight_requests": self.max_in_flight_requests,
            "max_buf_size": self.max_buf_size,
            "max_headers": self.max_headers,
            "max_header_bytes": self.max_header_bytes,
            "max_request_target_bytes": self.max_request_target_bytes,
            "keep_alive_idle_timeout_secs": self.keep_alive_idle_timeout_secs,
            "max_requests_per_connection": self.max_requests_per_connection,
            "response_write_timeout_secs": self.response_write_timeout_secs,
            "server_header": self.server_header,
            "date_policy": self.date_policy,
            "stripped_response_headers": list(self.stripped_response_headers),
            "error_policy": self.error_policy,
            "trusted_proxies": list(self.trusted_proxies),
            "trust_unix_local": self.trust_unix_local,
            "proxy_protocol": self.proxy_protocol,
            "forwarded_standard": self.forwarded_standard,
            "forwarded_legacy": self.forwarded_legacy,
        }


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
        # Single projection path: every RuntimeConfig field flows through
        # _native_kwargs() so defaults cannot drift from forwarding.
        self._native = _NativeServer(
            None,
            handler=handler,
            **cfg._native_kwargs(),
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


# ---------------------------------------------------------------------------
# Plan 204 async substrate (experimental).
#
# Manual asyncio bridge (Track A decision): no new PyO3 async helper crate,
# no extra supply-chain surface, abi3-py311 compatible. Rust owns transport,
# parsing, framing, admission, timeouts, and shutdown; Python owns the
# asyncio event loop, app-task admission, and streaming producers.
#
# GIL discipline: Rust releases the GIL during all network/body waits
# (`allow_threads` + `blocking_*`); Python never blocks the event loop on
# Rust (all blocking native calls go via `asyncio.to_thread`). Bounded
# queues (16 chunks, matching the sync bridge) preserve backpressure in
# both directions; no unbounded cross-runtime queue exists. Cancellation
# propagates both ways (lifecycle -> Python futures via channel close;
# Python cancel -> Rust via task cancel + channel close + permit release).
# Event-loop ownership is explicit and tied to server lifetime (see
# `AsyncServer`); cross-loop misuse fails deterministically. No raw sockets,
# Hyper/h2/h3/Quinn objects, or Tokio handles cross into Python. H1-only:
# the Python bridge does not enable H2/H3 (Rust-native H2/H3 remain
# experimental and Rust-only); H1 metadata is truthful, H2/H3 limitations
# explicit. `eggserve.server` sync surface remains intact.
# ---------------------------------------------------------------------------

#: Bound for async->sync response bridge (matches sync 16-chunk bridge).
_ASYNC_BRIDGE_BOUND = 16


class AsyncBody:
    """Incremental async request body over the native one-shot body.

    Buffered ``aread()`` uses the bounded native read (ceiling enforced by
    Rust); streaming ``aiter_chunks()`` is genuinely incremental via
    ``read_chunk()`` (no hidden ``read_all``), preserving backpressure
    (Rust stops reading when Python is not consuming). One-shot: ``aread``
    vs ``aiter_chunks`` are mutually exclusive (second use raises
    ``RequestBodyConsumedError``, mirroring sync). Trailers available after
    terminal state via ``trailers()`` (``None`` when absent).
    """

    def __init__(self, sync_body) -> None:
        self._body = sync_body

    @property
    def declared_length(self):
        return self._body.declared_length

    @property
    def bytes_received(self):
        return self._body.bytes_received

    @property
    def complete(self):
        return self._body.complete

    async def aread(self) -> bytes:
        # Buffered path (bounded by Rust ceiling). Streaming callers must
        # use `aiter_chunks` (which never calls `read_all` internally).
        return bytes(await asyncio.to_thread(self._body.read))

    async def aiter_chunks(self, chunk_size: Optional[int] = None):
        # Incremental path: `read_chunk` preserves the body for trailers
        # (unlike `read`/`iter_chunks` which consume). `chunk_size` re-chunks
        # native chunks into exactly-sized pieces (final partial flushed).
        if chunk_size is not None and chunk_size <= 0:
            raise ValueError("chunk_size must be greater than zero")
        pending = bytearray()
        while True:
            chunk = await asyncio.to_thread(self._body.read_chunk)
            if chunk is None:
                if pending:
                    yield bytes(pending)
                break
            if chunk_size is None:
                yield bytes(chunk)
            else:
                pending.extend(chunk)
                while len(pending) >= chunk_size:
                    out = bytes(pending[:chunk_size])
                    del pending[:chunk_size]
                    yield out

    async def trailers(self):
        return await asyncio.to_thread(self._body.trailers)


class AsyncTunnel:
    """Bounded async duplex over the runtime-owned tunnel (no WS framing)."""

    def __init__(self, sync_tunnel) -> None:
        self._tunnel = sync_tunnel

    def is_closed(self) -> bool:
        return bool(self._tunnel.is_closed())

    async def recv(self) -> Optional[bytes]:
        data = await asyncio.to_thread(self._tunnel.recv)
        return None if data is None else bytes(data)

    async def send(self, data: bytes) -> None:
        if not isinstance(data, (bytes, bytearray, memoryview)):
            raise TypeError("tunnel send requires bytes-like")
        await asyncio.to_thread(self._tunnel.send, bytes(data))

    def close(self) -> None:
        self._tunnel.close()


class AsyncTunnelCapability:
    """One-shot tunnel acceptance (double-take/double-accept fails)."""

    def __init__(self, sync_cap) -> None:
        self._cap = sync_cap

    @property
    def kind(self) -> str:
        return self._cap.kind

    @property
    def protocol(self):
        return self._cap.protocol

    @property
    def authority(self):
        return self._cap.authority

    def accept(self, headers=None):
        # Fast validation only (no network wait, GIL briefly). Returns
        # `(handshake, tunnel)`: `handshake` is the `Response` the handler
        # must return as its final result (101 H1 / 200 otherwise, runtime
        # owns framing, no raw socket); `tunnel` is the bounded duplex.
        # Denial stays ordinary HTTP (return normal `Response` without accept).
        handshake, tunnel = self._cap.accept(headers or [])
        return handshake, AsyncTunnel(tunnel)


class AsyncRequest:
    """Async request with byte-fidelity metadata + bounded primitives."""

    def __init__(self, sync_request) -> None:
        self._req = sync_request
        body = sync_request.body
        self._body = AsyncBody(body) if body is not None else None

    # -- sync metadata (no await, no network wait) --
    @property
    def method(self) -> str:
        return self._req.method

    @property
    def path(self) -> str:
        return self._req.path

    @property
    def query(self) -> str:
        return self._req.query

    @property
    def headers(self):
        return self._req.headers

    @property
    def header_items(self):
        return self._req.header_items

    @property
    def header_items_bytes(self):
        return [(bytes(n), bytes(v)) for (n, v) in self._req.header_items_bytes]

    @property
    def raw_target_bytes(self) -> bytes:
        return bytes(self._req.raw_target_bytes)

    @property
    def path_bytes(self) -> bytes:
        return bytes(self._req.path_bytes)

    @property
    def query_bytes(self):
        q = self._req.query_bytes
        return None if q is None else bytes(q)

    @property
    def http_version(self) -> str:
        return self._req.http_version

    @property
    def authority(self):
        return self._req.authority

    @property
    def scheme(self):
        return self._req.scheme

    @property
    def remote_addr(self):
        return self._req.remote_addr

    @property
    def remote_address(self):
        return self._req.remote_address

    @property
    def local_addr(self):
        return self._req.local_addr

    @property
    def local_address(self):
        return self._req.local_address

    @property
    def effective_addr(self):
        return self._req.effective_addr

    @property
    def effective_address(self):
        return self._req.effective_address

    @property
    def effective_scheme(self):
        return self._req.effective_scheme

    @property
    def effective_authority(self):
        return self._req.effective_authority

    @property
    def proxy_provenance(self):
        return self._req.proxy_provenance

    @property
    def forwarded_provenance(self):
        return self._req.forwarded_provenance

    @property
    def proxy_source(self):
        return self._req.proxy_source

    @property
    def proxy_destination(self):
        return self._req.proxy_destination

    @property
    def tls_protocol_version(self):
        return self._req.tls_protocol_version

    @property
    def tls_server_name(self):
        return self._req.tls_server_name

    @property
    def tls_alpn(self):
        return self._req.tls_alpn

    @property
    def client_authenticated(self) -> bool:
        return bool(self._req.client_authenticated)

    @property
    def peer_certificates_present(self) -> bool:
        return bool(self._req.peer_certificates_present)

    @property
    def has_body(self) -> bool:
        return self._body is not None

    @property
    def body(self):
        return self._body

    # -- lifecycle (sync check + async wait) --
    def is_disconnected(self) -> bool:
        return bool(self._req.is_disconnected())

    def cancellation_reason(self):
        return self._req.cancellation_reason()

    async def wait_disconnected(self, timeout_secs: Optional[float] = None) -> bool:
        return bool(await asyncio.to_thread(self._req.wait_disconnected, timeout_secs))

    # -- interim (fast, native-enforced; no network wait) --
    async def send_interim(self, status: int, headers=None) -> str:
        # `send_interim` is validation + recording only (no wait); call
        # directly without `to_thread` (GIL briefly, no network wait).
        return self._req.send_interim(status, headers or [])

    # -- tunnel (one-shot take + accept; handshake returned for service) --
    def has_tunnel(self) -> bool:
        try:
            return bool(self._req.has_tunnel())
        except Exception:
            return False

    def tunnel_request(self):
        try:
            return self._req.tunnel_request()
        except Exception:
            return None

    def take_tunnel(self):
        cap = self._req.take_tunnel()
        return None if cap is None else AsyncTunnelCapability(cap)


class _AsyncStreamMarker:
    """Handler return for incremental async response streaming."""

    def __init__(self, status, async_iterable, headers=None, content_length=None, trailers=None):
        self.status = status
        self.async_iterable = async_iterable
        self.headers = headers or {}
        self.content_length = content_length
        self.trailers = trailers


class AsyncResponse:
    """Async response factory (buffered sync + incremental async streaming)."""

    @staticmethod
    def empty(status: int):
        from eggserve._native import Response as _Resp

        return _Resp.empty(status)

    @staticmethod
    def bytes(status: int, data: bytes, headers=None):
        from eggserve._native import Response as _Resp

        return _Resp.bytes(status, bytes(data), headers)

    @staticmethod
    def text(status: int, text: str, headers=None):
        from eggserve._native import Response as _Resp

        return _Resp.text(status, text, headers)

    @staticmethod
    def stream(status: int, async_iterable, headers=None, content_length=None, trailers=None):
        # Async iterable (async generator / async iterator yielding
        # bytes-like). Bridged to sync `Response.stream` via a bounded
        # 16-queue (backpressure); HEAD/body-forbidden never advance the
        # async iterator (producer cancelled on drop); unknown length
        # allowed (chunked); trailers validated before commitment (no data
        # after). Non-bytes/iterator errors truncate with sanitized
        # type-only diagnostics (no content leak).
        if content_length is not None and content_length < 0:
            raise ValueError("content_length must be >= 0")
        return _AsyncStreamMarker(status, async_iterable, headers, content_length, trailers)


class AsyncServer:
    """Handler-only async server over the shared native runtime (H1-only).

    ``handler`` is ``async def handler(request: AsyncRequest) -> Response``.
    The runtime owns sockets, parsing, framing, timeouts, admission, and
    shutdown; ``handler`` runs on the caller's event loop (captured at
    ``start``) without holding the GIL across Rust waits. At most
    ``max_async_tasks`` app tasks run concurrently (separate from the
    pre-response ``max_in_flight_requests`` permit, which is held before the
    callback permit so limits cannot deadlock); overload fails fast with
    ``503``. Streaming producers and tunnel drivers hold the permit until
    completion (not just until response-start). Disconnect/cancel/shutdown
    returns the permit exactly once and wakes blocked sends with a stable
    exception. H2/H3 remain Rust-only experimental (Python bridge H1-only,
    explicit). ``eggserve.server`` sync surface remains intact.
    """

    def __init__(
        self,
        config: Optional[RuntimeConfig] = None,
        handler: Optional[Callable] = None,
        *,
        max_async_tasks: Optional[int] = None,
    ) -> None:
        if handler is None:
            raise ValueError("lowlevel.AsyncServer requires an async handler callable")
        if not callable(handler):
            raise TypeError("handler must be callable")
        if not (asyncio.iscoroutinefunction(handler) or inspect.iscoroutinefunction(handler)):
            raise TypeError("AsyncServer handler must be an async callable (async def)")
        self._config = config or RuntimeConfig()
        self._handler = handler
        if max_async_tasks is None:
            max_async_tasks = self._config.max_python_callbacks
        if not isinstance(max_async_tasks, int) or max_async_tasks <= 0:
            raise ValueError("max_async_tasks must be an integer >= 1")
        self._max_async_tasks = max_async_tasks
        self._native_server = None
        self._loop = None
        self._tasks: set[asyncio.Task] = set()
        self._sem: Optional[asyncio.Semaphore] = None
        self._started = False

    @property
    def addr(self):
        return self._native_server.addr if self._native_server else None

    @property
    def max_async_tasks(self) -> int:
        return self._max_async_tasks

    async def start(self) -> None:
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError as e:
            raise RuntimeError("AsyncServer.start() must be called from a running event loop") from e
        if self._started:
            if self._loop is not loop:
                raise RuntimeError("AsyncServer already started on a different event loop")
            raise RuntimeError("AsyncServer already started")
        if loop.is_closed():
            raise RuntimeError("event loop is closed")
        self._loop = loop
        self._sem = asyncio.Semaphore(self._max_async_tasks)
        # Sync shim bridges Rust blocking threads -> event loop via
        # `run_coroutine_threadsafe` + `Future.result` (GIL released during
        # wait so the loop can drive the coroutine). No unbounded queue.
        shim = self._make_sync_shim(loop)
        cfg = self._config
        # Reuse the sync native runtime (no second accept loop).
        self._native_server = Server(config=cfg, handler=shim)
        # `start`/`wait_ready` block; offload so the loop stays responsive.
        await asyncio.to_thread(self._native_server.start)
        await asyncio.to_thread(self._native_server.wait_ready)
        self._started = True

    async def shutdown(self) -> None:
        if not self._started:
            return
        loop = self._loop
        try:
            running = asyncio.get_running_loop()
        except RuntimeError:
            running = None
        if running is not loop:
            raise RuntimeError("AsyncServer.shutdown() must be called from its start loop")
        # Cancel app tasks deterministically (graceful timeout from config).
        tasks = list(self._tasks)
        for t in tasks:
            t.cancel()
        if tasks:
            timeout = float(getattr(self._config, "graceful_shutdown_timeout_secs", 10))
            try:
                await asyncio.wait_for(asyncio.gather(*tasks, return_exceptions=True), timeout)
            except (asyncio.TimeoutError, asyncio.CancelledError):
                pass
        self._tasks.clear()
        if self._native_server is not None:
            try:
                await asyncio.to_thread(self._native_server.shutdown)
            finally:
                try:
                    await asyncio.to_thread(self._native_server.wait)
                except Exception:
                    pass
        self._started = False

    async def __aenter__(self) -> "AsyncServer":
        await self.start()
        return self

    async def __aexit__(self, *args) -> bool:
        await self.shutdown()
        return False

    def __repr__(self) -> str:
        return f"<lowlevel.AsyncServer {self.addr or 'not started'} max_async_tasks={self._max_async_tasks}>"

    # -- internal bridge --
    def _make_sync_shim(self, loop: asyncio.AbstractEventLoop):
        handler = self._handler
        sem_holder: dict[str, Any] = {}
        # Semaphore created in `start` (bound to loop); capture here.
        outer = self

        def shim(sync_req):
            # Pre-response admission (Rust `max_in_flight_requests`) already
            # held by the runtime before this callback; now acquire the
            # Python app-task permit (fail fast 503 on exhaustion — no
            # unbounded task creation, no waiting queue).
            sem: asyncio.Semaphore = outer._sem  # type: ignore[assignment]
            # `run_coroutine_threadsafe` needs the loop; blocking wait below
            # releases the GIL (condition wait) so the loop can run.
            fut: concurrent.futures.Future = asyncio.run_coroutine_threadsafe(
                outer._dispatch_one(sync_req, handler, sem, loop), loop
            )
            # Bound the blocking wait by handler timeout (honest: EggServe
            # stops waiting and closes the HTTP request on timeout, but
            # cannot kill Python code; the app task continues until it
            # observes cancellation/timeout itself).
            timeout = float(getattr(outer._config, "handler_timeout_secs", 30))
            try:
                result = fut.result(timeout=timeout)
            except concurrent.futures.TimeoutError:
                # Detach (permit released when the app task finishes);
                # runtime converts this sync raise to 504 via handler timeout.
                # Raise sync to trigger a generic 500 here if the runtime
                # hasn't already timed out (no second response after commit
                # — runtime owns commitment; this only covers pre-commit).
                fut.cancel()
                raise TimeoutError("async handler timed out")
            # `result` is a sync `Response` (buffered or bridged streaming)
            # or a handshake `Response` (tunnel accept). Tunnel handshakes
            # carry runtime acceptance via the native slot (post-return
            # check in Rust); here we just return the handshake copy.
            return result

        return shim

    async def _dispatch_one(self, sync_req, handler, sem: asyncio.Semaphore, loop) -> Any:
        # Try-acquire (no waiting): overload -> deterministic 503.
        acquired = False
        # When streaming, permit ownership transfers to the producer task
        # (released on completion, not here). Local per-dispatch flag (no
        # shared `self` state — concurrent dispatches must not race).
        transfer_permit = False
        try:
            # Try-acquire without waiting (deterministic 503 on exhaustion;
            # no unbounded queue). `locked()` + immediate `acquire()` is
            # atomic on the event loop (acquire does not yield when a permit
            # is available).
            try:
                if sem.locked():
                    raise asyncio.TimeoutError
                await sem.acquire()
                acquired = True
            except (asyncio.TimeoutError, asyncio.CancelledError):
                from eggserve._native import Response as _Resp

                return _Resp.text(503, "Service Unavailable")
            areq = AsyncRequest(sync_req)
            try:
                result = await handler(areq)
            except asyncio.CancelledError:
                raise
            except Exception:
                # App exception before response-start -> generic 500 (no
                # leak; Rust logs sanitized type-only). Streaming/tunnel
                # background failures truncate (handled in producers).
                from eggserve._native import Response as _Resp

                return _Resp.text(500, "Internal Server Error")
            if isinstance(result, _AsyncStreamMarker):
                transfer_permit = True
                return await self._bridge_streaming(result, areq, loop, sem)
            return result
        finally:
            if acquired and not transfer_permit:
                try:
                    sem.release()
                except ValueError:
                    pass

    async def _bridge_streaming(self, marker: _AsyncStreamMarker, areq: AsyncRequest, loop, sem) -> Any:
        from eggserve._native import Response as _Resp

        queue: asyncio.Queue = asyncio.Queue(maxsize=_ASYNC_BRIDGE_BOUND)
        trailers = marker.trailers
        # Producer task (tracked, holds the transferred permit until done).
        sentinel_error: list[Any] = []

        async def _produce():
            try:
                it = marker.async_iterable
                # Support async generators, async iterators, and sync
                # iterables of bytes (for convenience).
                if hasattr(it, "__aiter__"):
                    async for chunk in it:  # type: ignore[misc]
                        await self._put_chunk(queue, chunk, loop)
                elif hasattr(it, "__anext__"):
                    while True:
                        try:
                            chunk = await it.__anext__()  # type: ignore[attr-defined]
                        except StopAsyncIteration:
                            break
                        await self._put_chunk(queue, chunk, loop)
                else:
                    for chunk in it:  # sync iterable fallback
                        await self._put_chunk(queue, chunk, loop)
                await queue.put(None)
            except asyncio.CancelledError:
                # Producer cancelled (HEAD suppression, disconnect,
                # shutdown): unblock consumer with EOF (truncation handled
                # by Rust as suppressed/close).
                try:
                    queue.put_nowait(None)
                except asyncio.QueueFull:
                    pass
                raise
            except Exception as e:
                # Sanitized: queue the error type only (no content).
                sentinel_error.append(type(e).__name__)
                try:
                    queue.put_nowait(("__error__", type(e).__name__))
                except asyncio.QueueFull:
                    pass
                try:
                    queue.put_nowait(None)
                except asyncio.QueueFull:
                    pass

        producer = asyncio.current_task()  # placeholder; real task below
        # Spawn producer on the loop (tracked for shutdown/cancel).
        prod_task = loop.create_task(_produce())
        self._tasks.add(prod_task)

        def _release_permit(_t):
            self._tasks.discard(_t)
            try:
                sem.release()
            except ValueError:
                pass

        prod_task.add_done_callback(_release_permit)

        # Sync consumer generator for `Response.stream` (runs on Rust
        # producer thread, blocking on `run_coroutine_threadsafe(queue.get)`
        # with GIL released during wait). Validates bytes (non-bytes ->
        # raise -> Rust truncates with sanitized log). HEAD/body-forbidden
        # never call `__next__` (Rust drops without pulling) -> producer
        # `put` times out (see `_put_chunk`) and self-cancels (no orphan).
        # Disconnect -> Rust drops iterable -> generator GC -> `close()`
        # cancels producer via `call_soon_threadsafe`.
        q = queue
        pt = prod_task

        def sync_gen():
            try:
                while True:
                    fut = asyncio.run_coroutine_threadsafe(q.get(), loop)
                    # Bound the blocking wait by response-write timeout
                    # (no-progress guard; slow producer truncates).
                    timeout = float(getattr(self._config, "response_write_timeout_secs", 30))
                    try:
                        item = fut.result(timeout=timeout)
                    except concurrent.futures.TimeoutError:
                        raise TimeoutError("async response producer stalled")
                    if item is None:
                        # Check for queued error sentinel before EOF.
                        break
                    if isinstance(item, tuple) and item and item[0] == "__error__":
                        raise RuntimeError(f"async producer failed ({item[1]})")
                    if not isinstance(item, (bytes, bytearray, memoryview)):
                        raise TypeError("async response iterable must yield bytes-like")
                    if len(item) == 0:
                        continue
                    yield bytes(item)
            finally:
                # Consumer dropped (HEAD suppression, disconnect, shutdown,
                # error): cancel producer if still pending (no orphan).
                if not pt.done():
                    try:
                        loop.call_soon_threadsafe(pt.cancel)
                    except RuntimeError:
                        pass

        # Build the sync Response (buffered headers validated now;
        # framing owned by runtime; no second response after commitment —
        # producer errors after return truncate, never synthesize).
        if trailers:
            return _Resp.stream_with_trailers(
                marker.status, sync_gen(), marker.headers, marker.content_length, trailers
            )
        return _Resp.stream(marker.status, sync_gen(), marker.headers, marker.content_length)

    async def _put_chunk(self, queue: asyncio.Queue, chunk: Any, loop) -> None:
        if not isinstance(chunk, (bytes, bytearray, memoryview)):
            raise TypeError(f"async response iterable yielded {type(chunk).__name__}")
        data = bytes(chunk)
        if len(data) == 0:
            return
        # Bounded put with no-progress timeout (slow consumer applies
        # backpressure; stalled consumer truncates instead of unbounded
        # growth). Timeout from response-write policy.
        timeout = float(getattr(self._config, "response_write_timeout_secs", 30))
        try:
            await asyncio.wait_for(queue.put(data), timeout)
        except asyncio.TimeoutError as e:
            raise TimeoutError("async response consumer stalled") from e

    def track(self, task: asyncio.Task) -> asyncio.Task:
        """Register a long-lived app task (tunnel driver, SSE) for shutdown.

        Tracked tasks are cancelled on `shutdown` (deterministic, no
        orphans). Untracked background tasks may outlive shutdown (discouraged).
        """
        self._tasks.add(task)

        def _done(t):
            self._tasks.discard(t)

        task.add_done_callback(_done)
        return task

