from eggserve.server import (
    BaseHTTPRequestHandler, HTTPServer, HTTPSServer, SimpleHTTPRequestHandler,
    ThreadingHTTPServer, ThreadingHTTPSServer,
)
from eggserve.subprocess import serve_directory as serve_directory

__version__: str
NATIVE_AVAILABLE: bool
