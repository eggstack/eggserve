from eggserve.server import (
    BaseHTTPRequestHandler, HTTPServer, HTTPSServer, SimpleHTTPRequestHandler,
    ThreadingHTTPServer, ThreadingHTTPSServer,
)

__version__: str
NATIVE_AVAILABLE: bool

def serve_directory(
    directory: str = ..., *, bind: str = ..., port: int = ..., public: bool = ...,
    policy: object | None = ..., log_format: str = ...,
) -> None: ...
