"""eggserve: a hardened, Rust-backed static file server."""

from __future__ import annotations

from collections import namedtuple
from typing import Any, List, Tuple, Optional

__version__ = "0.1.0"

from eggserve.server import (
    ServeConfig,
    ServerProcess,
    serve_directory,
)

ResponsePlan = namedtuple("ResponsePlan", ["status", "headers", "body_kind", "range"])

try:
    from eggserve._native import (
        EggserveError,
        PathPolicy,
        PathPolicyError,
        RequestTarget,
        RequestTargetError,
        RequestValidationError,
        ResolvedDirectory,
        ResolvedFile,
        ResolvedResource,
        SecureRoot,
        SecureRootError,
        StaticPolicy,
        generate_etag,
        validate_method,
        validate_request_body,
        validate_request_target,
        BodySource,
        BodySourceError,
        ResponseConstructionError,
        LifecycleError,
        Request,
        Response,
        Server,
        ServerBodySource,
        ServerRequestError,
        ServerSecureRoot,
        StaticResponder,
        StaticPolicyWrapper,
        HttpClient,
        ClientConfig,
        ClientRequest,
        ClientResponse,
        ClientError,
        Method,
    )

    NATIVE_AVAILABLE = True
except ImportError:
    NATIVE_AVAILABLE = False

__all__ = [
    "__version__",
    "ServeConfig",
    "ServerProcess",
    "serve_directory",
    "ResponsePlan",
    "NATIVE_AVAILABLE",
]

if NATIVE_AVAILABLE:
    __all__ += [
        "EggserveError",
        "PathPolicy",
        "PathPolicyError",
        "RequestTarget",
        "RequestTargetError",
        "RequestValidationError",
        "ResolvedDirectory",
        "ResolvedFile",
        "ResolvedResource",
        "SecureRoot",
        "SecureRootError",
        "StaticPolicy",
        "generate_etag",
        "validate_method",
        "validate_request_body",
        "validate_request_target",
        "BodySource",
        "BodySourceError",
        "ResponseConstructionError",
        "LifecycleError",
        "Request",
        "Response",
        "Server",
        "ServerBodySource",
        "ServerRequestError",
        "ServerSecureRoot",
        "StaticResponder",
        "StaticPolicyWrapper",
        "HttpClient",
        "ClientConfig",
        "ClientRequest",
        "ClientResponse",
        "ClientError",
        "Method",
    ]
