"""Python API for eggserve.

Provides a programmatic interface to the eggserve static file server.
The Rust binary is the source of truth for all serving logic. This module
translates Python config objects to CLI arguments and manages the binary
process lifecycle.

The compatibility classes in this module are a narrow, bounded facade over
the Rust-owned runtime. They are not an ASGI/WSGI server or web framework.
"""

from __future__ import annotations

import ipaddress
import inspect
import io
import os
import socket
import subprocess
import sys
import threading
import time
from http import HTTPStatus
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal, Optional
from functools import partial

from eggserve._bin import _find_binary


__all__ = [
    "HTTPServer",
    "ThreadingHTTPServer",
    "HTTPSServer",
    "ThreadingHTTPSServer",
    "BaseHTTPRequestHandler",
    "SimpleHTTPRequestHandler",
]


class _HTTPMessage:
    """Small, immutable-ish, duplicate-preserving request header view."""

    def __init__(self, fields: list[tuple[str, str]]) -> None:
        self._fields = tuple((name, value) for name, value in fields)

    def get(self, name: str, default: str | None = None) -> str | None:
        values = self.get_all(name)
        return values[0] if values else default

    def get_all(self, name: str) -> list[str]:
        lowered = name.lower()
        return [value for key, value in self._fields if key.lower() == lowered]

    def items(self) -> list[tuple[str, str]]:
        return list(self._fields)

    def __contains__(self, name: object) -> bool:
        return isinstance(name, str) and bool(self.get_all(name))

    def __iter__(self):
        return (name for name, _ in self._fields)

    def __len__(self) -> int:
        return len(self._fields)


class _BodyReader(io.RawIOBase):
    def __init__(self, body: object | None) -> None:
        self._body = body
        self._data: bytes | None = None
        self._offset = 0

    def _read_all(self) -> bytes:
        if self._data is None:
            self._data = b"" if self._body is None else bytes(self._body.read())
        return self._data

    def read(self, size: int = -1) -> bytes:
        data = self._read_all()
        if size is None or size < 0:
            result = data[self._offset:]
            self._offset = len(data)
        else:
            result = data[self._offset:self._offset + size]
            self._offset += len(result)
        return result

    def readinto(self, buffer) -> int:
        data = self.read(len(buffer))
        buffer[:len(data)] = data
        return len(data)

    def readline(self, limit: int = -1) -> bytes:
        data = self._read_all()
        end = data.find(b"\n", self._offset)
        stop = len(data) if end < 0 else end + 1
        if limit >= 0:
            stop = min(stop, self._offset + limit)
        result = data[self._offset:stop]
        self._offset = stop
        return result

    def readable(self) -> bool:
        return True

    def __iter__(self):
        return self

    def __next__(self) -> bytes:
        line = self.readline()
        if not line:
            raise StopIteration
        return line


class _BodyWriter(io.RawIOBase):
    def __init__(self, limit: int) -> None:
        self._limit = limit
        self._data = bytearray()

    def write(self, data) -> int:
        value = bytes(data)
        if len(self._data) + len(value) > self._limit:
            raise ValueError("handler response exceeds max_handler_response_bytes")
        self._data.extend(value)
        return len(value)

    def writelines(self, lines) -> None:
        for line in lines:
            self.write(line)

    def flush(self) -> None:
        return None

    def writable(self) -> bool:
        return True

    def bytes(self) -> bytes:
        return bytes(self._data)


class _HandlerResponse:
    def __init__(self, status: int, headers: list[tuple[str, str]], body: bytes) -> None:
        self.status = status
        self.headers = headers
        self.body = body


class BaseHTTPRequestHandler:
    """Bounded ``http.server``-shaped handler over EggServe's Rust runtime."""

    server_version = "eggserve"
    sys_version = ""
    protocol_version = "HTTP/1.1"
    error_message_format = "%(code)d - %(message)s"
    error_content_type = "text/plain; charset=utf-8"
    responses = {status.value: (status.phrase, status.description) for status in HTTPStatus}

    def __init__(self, request, client_address, server) -> None:
        self.request = request
        self.client_address = client_address
        self.server = server
        self.command = request.method
        self.path = request.path + (f"?{request.query}" if request.query else "")
        self.request_version = request.http_version
        self.headers = _HTTPMessage(getattr(request, "header_items", []))
        self.rfile = _BodyReader(request.body)
        self.wfile = _BodyWriter(server.max_handler_response_bytes)
        self.close_connection = False
        self.requestline = f"{self.command} {self.path} {self.request_version}"
        self._response_status: int | None = None
        self._response_headers: list[tuple[str, str]] = []
        self._headers_ended = False
        self._dispatch()

    def _dispatch(self) -> None:
        method = self.command
        if not method.isascii() or not method.replace("_", "A").isalnum():
            self.send_error(501, "Unsupported method")
            return
        callback = getattr(self, f"do_{method}", None)
        if callback is None:
            self.send_error(501, "Unsupported method")
            return
        try:
            result = callback()
            if inspect.isawaitable(result):
                raise TypeError("coroutine handlers are not supported")
            if self._response_status is None and not hasattr(self, "_native_response"):
                raise RuntimeError("handler did not send a response")
        except Exception:
            self.log_error("handler failed")
            self._response_status = 500
            self._response_headers = [("Content-Type", self.error_content_type)]
            self._headers_ended = True
            self.wfile = _BodyWriter(self.server.max_handler_response_bytes)
            self.wfile.write(b"Internal Server Error")

    def send_response(self, code: int, message: str | None = None) -> None:
        self.send_response_only(code, message)
        self.log_request(code, "-")

    def send_response_only(self, code: int, message: str | None = None) -> None:
        if not isinstance(code, int) or not 100 <= code <= 599:
            raise ValueError("status code must be between 100 and 599")
        self._response_status = code

    def send_header(self, keyword: str, value: str) -> None:
        if self._headers_ended:
            raise ValueError("response headers are already ended")
        if keyword.lower() in {"connection", "keep-alive", "upgrade", "transfer-encoding"}:
            raise ValueError(f"runtime-owned header is not permitted: {keyword}")
        if any(c in keyword or c in value for c in ("\x00", "\r", "\n")):
            raise ValueError("header contains prohibited control characters")
        self._response_headers.append((keyword, value))

    def end_headers(self) -> None:
        if self._response_status is None:
            raise ValueError("send_response must precede end_headers")
        self._headers_ended = True

    def flush_headers(self) -> None:
        self.end_headers()

    def send_error(self, code: int, message: str | None = None, explain: str | None = None) -> None:
        reason = message or HTTPStatus(code).phrase if code in HTTPStatus._value2member_map_ else "Error"
        self.send_response_only(code, reason)
        self.send_header("Content-Type", self.error_content_type)
        self.end_headers()
        self.wfile.write(f"{code} {reason}\n".encode("utf-8"))

    def version_string(self) -> str:
        return self.server_version if not self.sys_version else f"{self.server_version} {self.sys_version}"

    def date_time_string(self, timestamp: float | None = None) -> str:
        from email.utils import formatdate
        return formatdate(timestamp, usegmt=True)

    def address_string(self) -> str:
        return str(self.client_address[0])

    def log_request(self, code="-", size="-") -> None:
        return None

    def log_error(self, format, *args) -> None:
        self.log_message(format, *args)

    def log_message(self, format, *args) -> None:
        return None

    def _result(self) -> _HandlerResponse:
        native_response = getattr(self, "_native_response", None)
        if native_response is not None:
            return native_response
        if self._response_status is None or not self._headers_ended:
            raise RuntimeError("handler returned without a complete response")
        return _HandlerResponse(self._response_status, self._response_headers, self.wfile.bytes())


class HTTPServer:
    """Serial ``http.server.HTTPServer``-compatible facade."""

    def __init__(self, server_address, RequestHandlerClass, bind_and_activate=True,
                 *, max_request_body_bytes=1024 * 1024, max_handler_response_bytes=16 * 1024 * 1024):
        self._init_compat(server_address, RequestHandlerClass, bind_and_activate,
                          max_workers=1, max_request_body_bytes=max_request_body_bytes,
                          max_handler_response_bytes=max_handler_response_bytes,
                          tls_certfile=None, tls_keyfile=None)

    def _init_compat(self, server_address, handler_class, bind_and_activate, *, max_workers,
                     max_request_body_bytes, max_handler_response_bytes,
                     tls_certfile, tls_keyfile):
        if not isinstance(server_address, tuple) or len(server_address) != 2:
            raise OSError("server_address must be a (host, port) tuple")
        host, port = server_address
        if not isinstance(host, str) or not isinstance(port, int) or not 0 <= port <= 65535:
            raise OSError("invalid server address")
        factory = handler_class.func if isinstance(handler_class, partial) else handler_class
        if not isinstance(factory, type) or not issubclass(factory, BaseHTTPRequestHandler):
            raise TypeError("RequestHandlerClass must subclass BaseHTTPRequestHandler")
        if max_request_body_bytes <= 0 or max_handler_response_bytes <= 0:
            raise ValueError("body and response limits must be greater than zero")
        self.RequestHandlerClass = handler_class
        self._handler_type = factory
        self._static_config = self._static_handler_config(handler_class, factory)
        self.allow_reuse_address = False
        self.max_handler_response_bytes = max_handler_response_bytes
        self._closed = False
        self._stop_event = threading.Event()
        self._serve_done = threading.Event()
        self._serve_done.set()
        self._native = None
        self._bind = host
        self._requested_port = port
        self._max_workers = max_workers
        self._max_request_body_bytes = max_request_body_bytes
        self._tls_certfile = tls_certfile
        self._tls_keyfile = tls_keyfile
        self.server_address = (host, port)
        self.server_name = socket.getfqdn(host)
        self.server_port = port
        if bind_and_activate:
            self.server_bind()
            self.server_activate()

    @staticmethod
    def _static_handler_config(handler_class, handler_type):
        if not issubclass(handler_type, SimpleHTTPRequestHandler):
            return None
        keywords = handler_class.keywords if isinstance(handler_class, partial) else {}
        directory = keywords.get("directory", getattr(handler_type, "directory", None))
        if directory is None:
            directory = os.getcwd()
        root = os.fspath(directory)
        if not os.path.isdir(root):
            raise NotADirectoryError(root)
        return {
            "root": root,
            "directory_listing": bool(getattr(handler_type, "directory_listing", False)),
            "follow_symlinks": bool(getattr(handler_type, "follow_symlinks", False)),
            "allow_dotfiles": bool(getattr(handler_type, "allow_dotfiles", False)),
            "index_pages": tuple(getattr(handler_type, "index_pages", ("index.html", "index.htm"))),
            "extensions_map": dict(getattr(handler_type, "extensions_map", {})),
        }

    def server_bind(self):
        return None

    def server_activate(self):
        if self._native is None:
            from eggserve._native import Server as _NativeServer
            root = "."
            if self._static_config is not None:
                from eggserve._native import (
                    ServerSecureRoot,
                    StaticPolicyWrapper,
                    StaticResponder,
                )
                config = self._static_config
                secure_root = ServerSecureRoot(
                    config["root"],
                    policy=StaticPolicyWrapper(
                        directory_listing=config["directory_listing"],
                        follow_symlinks=config["follow_symlinks"],
                        allow_dotfiles=config["allow_dotfiles"],
                    ),
                )
                self._static_responder = StaticResponder(secure_root)
                root = config["root"]
            callback = self._handle_request
            self._native = _NativeServer(
                root, bind=self._bind, port=self._requested_port, handler=callback,
                public=ipaddress.ip_address(self._bind).is_unspecified,
                max_python_callbacks=self._max_workers,
                request_body_mode="reject" if self._static_config is not None else "buffer",
                max_request_body_bytes=0 if self._static_config is not None else self._max_request_body_bytes,
                tls_certfile=self._tls_certfile,
                tls_keyfile=self._tls_keyfile,
            )

    def _handle_request(self, request):
        client = (request.remote_addr or "", 0)
        handler = self.RequestHandlerClass(request, client, self)
        result = handler._result()
        return result

    def _start(self):
        if self._closed:
            raise RuntimeError("server is closed")
        self.server_activate()
        if self._native.state == "created":
            self._native.start()
            self._native.wait_ready()
            host, port = self._native.addr.rsplit(":", 1)
            self.server_address = (host.strip("[]"), int(port))
            self.server_name = socket.getfqdn(self.server_address[0])
            self.server_port = self.server_address[1]

    def serve_forever(self, poll_interval=0.5):
        del poll_interval
        self._start()
        native = self._native
        stop_event = self._stop_event
        self._serve_done.clear()
        try:
            stop_event.wait()
            if native is not None:
                native.wait()
        finally:
            self._serve_done.set()

    def shutdown(self):
        if self._native is not None:
            self._native.shutdown()
        self._stop_event.set()

    def server_close(self):
        if self._native is not None:
            self._native.shutdown()
            self._stop_event.set()
            self._serve_done.wait(5)
            self._native = None
        self._stop_event.set()
        self._closed = True

    def handle_request(self):
        raise NotImplementedError("one-request mode is not exposed by the Rust runtime")

    def fileno(self):
        raise OSError("the native listener descriptor is not exposed")

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.server_close()
        return False


class ThreadingHTTPServer(HTTPServer):
    """Concurrent bounded-handler variant of :class:`HTTPServer`."""

    def __init__(self, server_address, RequestHandlerClass, bind_and_activate=True,
                 *, max_workers=8, max_request_body_bytes=1024 * 1024,
                 max_handler_response_bytes=16 * 1024 * 1024):
        self._init_compat(server_address, RequestHandlerClass, bind_and_activate,
                          max_workers=max_workers, max_request_body_bytes=max_request_body_bytes,
                          max_handler_response_bytes=max_handler_response_bytes,
                          tls_certfile=None, tls_keyfile=None)


class HTTPSServer(HTTPServer):
    """HTTP/1.1 TLS server using EggServe's rustls runtime."""

    def __init__(self, server_address, RequestHandlerClass, bind_and_activate=True,
                 *, certfile, keyfile=None, password=None, alpn_protocols=None,
                 max_request_body_bytes=1024 * 1024,
                 max_handler_response_bytes=16 * 1024 * 1024):
        if password is not None:
            raise ValueError("encrypted private-key passwords are not supported")
        protocols = ["http/1.1"] if alpn_protocols is None else list(alpn_protocols)
        if protocols != ["http/1.1"]:
            raise ValueError("only the 'http/1.1' ALPN protocol is supported")
        if not isinstance(certfile, (str, os.PathLike)):
            raise TypeError("certfile must be a path")
        key_path = certfile if keyfile is None else keyfile
        if not isinstance(key_path, (str, os.PathLike)):
            raise TypeError("keyfile must be a path")
        self._init_compat(server_address, RequestHandlerClass, bind_and_activate,
                          max_workers=1, max_request_body_bytes=max_request_body_bytes,
                          max_handler_response_bytes=max_handler_response_bytes,
                          tls_certfile=os.fspath(certfile), tls_keyfile=os.fspath(key_path))


class ThreadingHTTPSServer(HTTPSServer):
    """Bounded-concurrency TLS variant of :class:`HTTPSServer`."""

    def __init__(self, server_address, RequestHandlerClass, bind_and_activate=True,
                 *, certfile, keyfile=None, password=None, alpn_protocols=None,
                 max_workers=8, max_request_body_bytes=1024 * 1024,
                 max_handler_response_bytes=16 * 1024 * 1024):
        if password is not None:
            raise ValueError("encrypted private-key passwords are not supported")
        protocols = ["http/1.1"] if alpn_protocols is None else list(alpn_protocols)
        if protocols != ["http/1.1"]:
            raise ValueError("only the 'http/1.1' ALPN protocol is supported")
        if not isinstance(certfile, (str, os.PathLike)):
            raise TypeError("certfile must be a path")
        key_path = certfile if keyfile is None else keyfile
        if not isinstance(key_path, (str, os.PathLike)):
            raise TypeError("keyfile must be a path")
        self._init_compat(server_address, RequestHandlerClass, bind_and_activate,
                          max_workers=max_workers, max_request_body_bytes=max_request_body_bytes,
                          max_handler_response_bytes=max_handler_response_bytes,
                          tls_certfile=os.fspath(certfile), tls_keyfile=os.fspath(key_path))


class SimpleHTTPRequestHandler(BaseHTTPRequestHandler):
    """Secure static handler with a familiar stdlib-compatible shape.

    Filesystem resolution and response construction are delegated to the Rust
    ``SecureRoot``/``StaticResponder``. Policies are captured when the server
    is configured; later class-attribute mutation does not affect serving.
    """

    directory = None
    index_pages = ("index.html", "index.htm")
    directory_listing = False
    follow_symlinks = False
    allow_dotfiles = False
    extensions_map = {}

    def __init__(self, request, client_address, server, directory=None):
        # ``directory`` is captured by the server configuration. Accepting it
        # here preserves the standard constructor shape without allowing a
        # request to choose a root.
        del directory
        super().__init__(request, client_address, server)

    def _static_response(self):
        headers = {name.lower(): value for name, value in self.headers.items()}
        return self.server._static_responder.respond(
            self.command,
            self.path,
            headers=headers,
            has_body=bool(getattr(self.request, "has_body", False)),
            index_pages=list(self.server._static_config["index_pages"]),
            mime_overrides=self.server._static_config["extensions_map"],
        )

    def do_GET(self):
        self._native_response = self._static_response()

    def do_HEAD(self):
        self._native_response = self._static_response()

    def send_head(self):
        return self._static_response()

    def guess_type(self, path):
        suffix = os.fspath(path).rsplit("/", 1)[-1].rsplit(".", 1)[-1]
        key = f".{suffix.lower()}" if suffix else ""
        if key in self.extensions_map:
            return self.extensions_map[key]
        return {
            ".html": "text/html; charset=utf-8",
            ".htm": "text/html; charset=utf-8",
            ".css": "text/css; charset=utf-8",
            ".js": "application/javascript; charset=utf-8",
            ".json": "application/json; charset=utf-8",
            ".txt": "text/plain; charset=utf-8",
            ".png": "image/png",
            ".jpg": "image/jpeg",
            ".jpeg": "image/jpeg",
            ".svg": "image/svg+xml",
        }.get(key, "application/octet-stream")

    def list_directory(self, path):
        raise NotImplementedError(
            "EggServe listings are generated from resolver-filtered entries; "
            "raw filesystem paths are not exposed"
        )

    def translate_path(self, path):
        raise NotImplementedError(
            "EggServe does not expose an authoritative translated filesystem path"
        )


@dataclass(frozen=True)
class StaticPolicy:
    """Filesystem access policy for the server.

    All defaults are safe. Unsafe behaviors require explicit opt-in.
    """

    directory_listing: bool = False
    follow_symlinks: bool = False
    allow_dotfiles: bool = False


_VALID_LOG_FORMATS = frozenset({"text", "json", "none"})


def _parse_bind(bind: str) -> tuple[str, Optional[int]]:
    """Parse ``bind`` into ``(host, port)``.

    Accepts either a bare IP address (host-only; port carried separately
    via ``ServeConfig.port``) or a ``HOST:PORT`` socket address. IPv6
    host:port forms must use brackets (e.g. ``[::1]:8000``) to disambiguate
    colons in the address. Returns ``(host, port_or_none)``. Raises
    ``ValueError`` for unparseable values.
    """
    if not isinstance(bind, str):
        raise ValueError(
            f"bind must be a str, got {type(bind).__name__}: {bind!r}"
        )
    # Bracketed IPv6 socket form: [host]:port
    if bind.startswith("["):
        end = bind.find("]")
        if end == -1:
            raise ValueError(f"invalid bind address {bind!r}: missing ']'")
        host = bind[1:end]
        rest = bind[end + 1:]
        if not rest.startswith(":"):
            raise ValueError(f"invalid bind address {bind!r}: expected ':' after ']'")
        try:
            port = int(rest[1:])
        except ValueError as exc:
            raise ValueError(
                f"invalid bind address {bind!r}: bad port: {exc}"
            ) from None
        if not (0 <= port <= 65535):
            raise ValueError(f"invalid bind address {bind!r}: port out of range")
        try:
            ipaddress.ip_address(host)
        except ValueError as exc:
            raise ValueError(
                f"invalid bind address {bind!r}: {exc}"
            ) from None
        return (host, port)
    # Bare IPv6 (no brackets): multiple colons mean it can't carry a port.
    if bind.count(":") > 1:
        try:
            ipaddress.ip_address(bind)
        except ValueError as exc:
            raise ValueError(
                f"invalid bind address {bind!r}: {exc}"
            ) from None
        return (bind, None)
    # HOST:PORT for IPv4 or hostname. We require an IP literal here so
    # the public-bind guard is unambiguous.
    if ":" in bind:
        host, port_str = bind.rsplit(":", 1)
        try:
            port = int(port_str)
        except ValueError as exc:
            raise ValueError(
                f"invalid bind address {bind!r}: bad port: {exc}"
            ) from None
        if not (0 <= port <= 65535):
            raise ValueError(f"invalid bind address {bind!r}: port out of range")
        try:
            ipaddress.ip_address(host)
        except ValueError as exc:
            raise ValueError(
                f"invalid bind address {bind!r}: {exc}"
            ) from None
        return (host, port)
    # Bare IPv4.
    try:
        ipaddress.ip_address(bind)
    except ValueError as exc:
        raise ValueError(
            f"invalid bind address {bind!r}: {exc}"
        ) from None
    return (bind, None)


@dataclass(frozen=True)
class ServeConfig:
    """Configuration for the eggserve static file server.

    Defaults match the CLI and Rust core safe-by-default behavior:
    loopback bind, no directory listing, no symlinks, no dotfiles.

    Validation runs in ``__post_init__``: an invalid bind, port,
    ``log_format``, or public-bind combination raises ``ValueError``
    before any subprocess is spawned. The Rust CLI performs the same
    checks independently as defense in depth.
    """

    directory: str | Path = "."
    bind: str = "127.0.0.1"
    port: int = 8000
    public: bool = False
    policy: StaticPolicy = field(default_factory=StaticPolicy)
    log_format: Literal["text", "json", "none"] = "text"

    def __post_init__(self) -> None:
        if not isinstance(self.port, int) or isinstance(self.port, bool):
            raise ValueError(
                f"port must be an int, got {type(self.port).__name__}: {self.port!r}"
            )
        if not (1 <= self.port <= 65535):
            raise ValueError(
                f"port must be between 1 and 65535, got {self.port}"
            )
        host, embedded_port = _parse_bind(self.bind)
        if embedded_port is not None and embedded_port != self.port:
            raise ValueError(
                f"bind={self.bind!r} carries port {embedded_port} but "
                f"port={self.port}; omit the port from bind or use the same value"
            )
        if self.log_format not in _VALID_LOG_FORMATS:
            raise ValueError(
                f"log_format must be one of {sorted(_VALID_LOG_FORMATS)}, "
                f"got {self.log_format!r}"
            )
        if not self.public and ipaddress.ip_address(host).is_unspecified:
            raise ValueError(
                f"binding to {self.bind} requires public=True "
                "to acknowledge public exposure intent"
            )


def _config_to_argv(config: ServeConfig) -> list[str]:
    """Translate a ServeConfig into CLI arguments for the eggserve binary."""
    argv: list[str] = []

    argv.extend(["--directory", str(config.directory)])
    host, _ = _parse_bind(config.bind)
    argv.extend(["--bind", host])
    argv.extend(["--port", str(config.port)])

    if config.public:
        argv.append("--public")

    if config.policy.directory_listing:
        argv.append("--directory-listing")
    if config.policy.follow_symlinks:
        argv.append("--follow-symlinks")
    if config.policy.allow_dotfiles:
        argv.append("--allow-dotfiles")

    if config.log_format != "text":
        argv.extend(["--log-format", config.log_format])

    return argv


def serve_directory(
    directory: str | Path = ".",
    *,
    bind: str = "127.0.0.1",
    port: int = 8000,
    public: bool = False,
    policy: Optional[StaticPolicy] = None,
    log_format: Literal["text", "json", "none"] = "text",
) -> None:
    """Start a blocking static file server.

    Runs until interrupted (KeyboardInterrupt) or the process exits.
    This is a programmatic equivalent of ``eggserve`` on the command line.

    Args:
        directory: Root directory to serve (default: current directory).
        bind: Bind address (default: 127.0.0.1).
        port: Listen port (default: 8000).
        public: Acknowledge public exposure intent (required for 0.0.0.0).
        policy: Filesystem access policy (safe defaults if omitted).
        log_format: Log output format: "text", "json", or "none".

    Raises:
        ValueError: If configuration is invalid (port, log_format, or
            public-bind combination).
        FileNotFoundError: If the eggserve binary is not found.
    """
    config = ServeConfig(
        directory=directory,
        bind=bind,
        port=port,
        public=public,
        policy=policy or StaticPolicy(),
        log_format=log_format,
    )
    proc = ServerProcess(config)
    proc.start()
    try:
        proc.wait()
    except KeyboardInterrupt:
        proc.stop()


class ServerProcess:
    """Manage an eggserve subprocess.

    Wraps the eggserve binary for use in tests and simple embedding.
    This is a subprocess lifecycle manager, not a Python server object.
    """

    def __init__(self, config: ServeConfig) -> None:
        self._config = config
        self._process: Optional[subprocess.Popen] = None

    def start(self) -> None:
        """Start the server subprocess.

        Raises:
            FileNotFoundError: If the eggserve binary is not found.
            RuntimeError: If the server is already running.
        """
        if self._process is not None:
            raise RuntimeError("server is already running")

        config = self._config

        binary = _find_binary()
        argv = [binary] + _config_to_argv(config)

        self._process = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE if config.log_format == "none" else None,
            stderr=subprocess.PIPE if config.log_format == "none" else None,
        )

    def stop(self, timeout: float | None = None) -> None:
        """Stop the server subprocess.

        Args:
            timeout: Seconds to wait for graceful shutdown before killing.
        """
        if self._process is None:
            return

        self._process.terminate()
        try:
            self._process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait()
        self._process = None

    def wait(self) -> int:
        """Wait for the server to exit. Returns the exit code."""
        if self._process is None:
            raise RuntimeError("server is not running")
        returncode = self._process.wait()
        self._process = None
        return returncode

    @property
    def is_running(self) -> bool:
        """Check if the server subprocess is still running."""
        if self._process is None:
            return False
        return self._process.poll() is None

    @property
    def pid(self) -> Optional[int]:
        """The PID of the server subprocess, or None if not started."""
        if self._process is None:
            return None
        return self._process.pid
