"""A hardened, Rust-backed static file server."""

from __future__ import annotations

__version__ = "0.1.0"

from eggserve.server import (
    BaseHTTPRequestHandler,
    HTTPServer,
    HTTPSServer,
    SimpleHTTPRequestHandler,
    ThreadingHTTPServer,
    ThreadingHTTPSServer,
)
from eggserve.server import serve_directory

try:
    import eggserve._native as _native
    NATIVE_AVAILABLE = True
except ImportError:
    _native = None
    NATIVE_AVAILABLE = False

__all__ = [
    "__version__", "serve_directory", "HTTPServer", "ThreadingHTTPServer",
    "HTTPSServer", "ThreadingHTTPSServer", "BaseHTTPRequestHandler",
    "SimpleHTTPRequestHandler",
]
