"""eggserve: a hardened, Rust-backed static file server."""

__version__ = "0.1.0"

from eggserve.server import (
    StaticPolicy,
    ServeConfig,
    ServerProcess,
    serve_directory,
)

__all__ = [
    "__version__",
    "StaticPolicy",
    "ServeConfig",
    "ServerProcess",
    "serve_directory",
]
