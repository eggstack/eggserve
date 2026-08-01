"""Advanced, security-sensitive primitives for embedding integrations."""

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
]
