# eggserve-python — Deep Dive

The Python wheel is built by maturin and contains a PyO3 extension plus the
Python façade and bundled CLI. The supported programming surface is
`eggserve.server`; advanced primitives and CLI lifecycle helpers are kept in
separate namespaces.

## Supported modules

`eggserve.server` exports exactly:

- `HTTPServer` and `ThreadingHTTPServer`;
- `HTTPSServer` and `ThreadingHTTPSServer`;
- `BaseHTTPRequestHandler` and `SimpleHTTPRequestHandler`.

The classes use the Rust runtime for socket ownership, HTTP/1.1 parsing,
timeouts, response framing, static path resolution, and file streaming.
Handlers are synchronous and receive bounded `rfile`/`wfile` adapters, never
raw sockets. HTTPS uses the shared core rustls PEM loader, requires certificate
and key paths, and restricts ALPN to `http/1.1`.

`eggserve.lowlevel` contains the advanced PyO3 wrappers (`SecureRoot`,
`StaticPolicy`, `RequestTarget`, canonical HTTP types, and body/response
primitives). `eggserve.subprocess` contains `ServeConfig`, `ServerProcess`,
`StaticPolicy`, and the `serve_directory` convenience. The top-level package
only re-exports the version, `serve_directory`, and the six façade classes.

The native callback `Server`, `StaticResponder`, `ServerSecureRoot`, and
`ServerBodySource` remain internal implementation/test types. The Python
extension does not compile the experimental HTTP client; the Rust client is
still an opt-in core feature.

## Structure

```
crates/eggserve-python/
├── Cargo.toml          # cdylib and feature-scoped Rust dependencies
├── pyproject.toml      # maturin metadata and bundled CLI files
├── src/
│   ├── lib.rs          # PyO3 module registration
│   └── server.rs       # internal runtime bridge and response primitives
└── python/eggserve/
    ├── __init__.py     # small supported top-level namespace
    ├── server.py       # compatibility façade and internal subprocess code
    ├── lowlevel.py     # advanced native exports
    ├── subprocess.py   # optional CLI lifecycle exports
    └── bin/             # staged platform-native CLI in wheels
```

## Security boundary

Python cannot choose a filesystem path per request, reopen translated paths,
provide a raw socket, or override runtime-owned framing. Static handler roots
and policy flags are captured during server construction. Invalid TLS
configuration fails before the native server reports readiness; key material
is never logged. The platform qualifications in `docs/security-review.md`,
especially the incomplete independent Windows adversarial review, continue to
apply.

## Verification

The installed-wheel harness is `scripts/test-python-wheel.sh`. It builds the
wheel, installs it into a clean CPython 3.14 environment, checks the import
boundary, and runs the focused compatibility, TLS, low-level, lifecycle, and
boundary tests with `unittest`. No Python client test suite is shipped.
