"""Type stubs for the eggserve package."""

from typing import List, NamedTuple, Optional, Tuple

from eggserve._native import (
    CanonicalRequest,
    ConnectionInfo,
    DuplicateHeaderError,
    EggserveError,
    HeaderBlock,
    HeaderError,
    HttpVersion,
    HttpVersionError,
    Method,
    MethodError,
    BodySource,
    BodySourceError,
    BodyChunkIterator,
    LifecycleError,
    PathPolicy,
    PathPolicyError,
    RequestBody,
    RequestBodyCancelledError,
    RequestBodyConsumedError,
    RequestBodyDisconnectedError,
    RequestBodyError,
    RequestBodyIncompleteError,
    RequestBodyRejectedError,
    RequestBodyTimeoutError,
    RequestBodyTooLargeError,
    RequestTarget,
    RequestTargetError,
    RequestValidationError,
    ResolvedDirectory,
    ResolvedFile,
    ResolvedResource,
    ResponseConstructionError,
    SecureRoot,
    SecureRootError,
    StaticPolicy,
)

__version__: str

class ResponsePlan(NamedTuple):
    status: int
    headers: list
    body_kind: str
    range: Optional[Tuple[int, int]]

NATIVE_AVAILABLE: bool

def generate_etag(path: str) -> str: ...
def validate_method(method: str) -> str: ...
def validate_request_body(...) -> None: ...
def validate_request_target(...) -> None: ...
def parse_method(value: str) -> Method: ...
def parse_http_version(value: str) -> HttpVersion: ...
