#!/usr/bin/env python3
"""Compact stable-ABI fixture (Plan 264).

Exercises representative native classes/functions through the installed
wheel — more than an import check, less than the full test suite. Used by
the build-once/test-many ABI proof on every supported CPython minor and by
`scripts/test-python-wheel.sh --mode abi-smoke`.
"""

from __future__ import annotations

import platform


def main() -> int:
    import eggserve
    from eggserve._native import (
        CanonicalRequest,
        ConnectionInfo,
        HeaderBlock,
        HttpVersion,
        Method,
        PathPolicy,
        RequestTarget,
        Response,
        SecureRoot,
        StaticPolicy,
        parse_http_version,
        parse_method,
    )

    print(f"  eggserve version: {eggserve.__version__}")
    print(f"  python: {platform.python_version()} ({platform.python_implementation()})")
    print(f"  machine: {platform.machine()}")

    # Representative native surface: parsing functions plus frozen value types.
    method = parse_method("GET")
    assert isinstance(method, Method), repr(method)
    version = parse_http_version("HTTP/1.1")
    assert isinstance(version, HttpVersion), repr(version)
    block = HeaderBlock([("x-abi-smoke", "1")])
    assert block is not None
    head = CanonicalRequest(method="GET", path="/smoke.txt")
    assert head is not None
    for cls in (ConnectionInfo, RequestTarget, Response, SecureRoot, StaticPolicy, PathPolicy):
        assert cls is not None, cls
    policy = StaticPolicy()
    assert policy is not None
    print("  native surface: parse_method/parse_http_version/HeaderBlock/"
          "RequestTarget/CanonicalRequest/ConnectionInfo/Response/"
          "SecureRoot/StaticPolicy/PathPolicy present")

    print("  ABI smoke passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
