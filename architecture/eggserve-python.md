# eggserve-python — Deep Dive

The Python wheel is built by maturin and contains a PyO3 extension plus the
Python façade. The supported programming surface is `eggserve.server`; advanced
primitives and subprocess lifecycle helpers are kept in separate namespaces.
The wheel includes an `eggserve` console script backed by the native extension
(no separate bundled binary).

Plan 108 correction: custom-handler startup is runtime-only: it constructs a Python callback
service and `RuntimeConfig` without a `ServeConfig`, responder root, or pinned
filesystem state. Its compatibility `root` argument is inactive and is not
opened or validated. `SimpleHTTPRequestHandler` is the separate static branch and
constructs one confined `StaticService`. Both branches use the server-wide
runtime file-stream semaphore for canonical file responses.

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

Compatibility addresses are normalized in the Python façade only at the API
boundary: `""` becomes `0.0.0.0`, and literal unspecified IPv4/IPv6 addresses
carry explicit wildcard intent to the existing native server. Hostname and
literal resolution remains native, and the published `server_address` is the
actual native `(host, port)` tuple.

`eggserve.lowlevel` contains the advanced PyO3 wrappers (`SecureRoot`,
`StaticPolicy`, `RequestTarget`, canonical HTTP types, and body/response
primitives). `eggserve.subprocess` contains `ServeConfig`, `ServerProcess`,
`StaticPolicy`, and the `serve_directory` convenience. The top-level package
only re-exports the version, `serve_directory`, and the six façade classes.

The native callback `Server`, `StaticResponder`, `ServerSecureRoot`, and
`ServerBodySource` remain internal implementation/test types.

## Structure

```
crates/eggserve-python/
├── Cargo.toml          # cdylib and feature-scoped Rust dependencies
├── pyproject.toml      # maturin metadata and entry points
├── src/
│   ├── lib.rs          # PyO3 module registration
│   └── server.rs       # internal runtime bridge and response primitives
└── python/eggserve/
    ├── __init__.py     # small supported top-level namespace
    ├── _bin.py         # CLI entry point via native _run_cli
    ├── __main__.py     # python -m eggserve support
    ├── server.py       # six-class Rust-runtime compatibility façade
    ├── lowlevel.py     # advanced native exports
    └── subprocess.py   # optional subprocess lifecycle exports
```

## Security boundary

Python cannot choose a filesystem path per request, reopen translated paths,
provide a raw socket, or override runtime-owned framing. Static handler roots
and policy flags are captured during server construction. Invalid TLS
configuration fails before the native server reports readiness; key material
is never logged. The platform qualifications in `docs/security-review.md`,
especially the incomplete independent Windows adversarial review, continue to
apply.

The callback bridge stages status, ordered headers, body ownership, and
`Content-Length` validation before constructing a canonical response. Native
file and byte bodies are one-shot; consumed or malformed structural bodies are
errors rather than empty-body fallbacks. Handler failures are logged with
fixed categories only. MIME hooks provide metadata to the native responder;
they never perform Python path translation, `stat`, open, or reopen operations.

## Verification

The installed-wheel harness is `scripts/test-python-wheel.sh`. It builds the
wheel, installs it into a clean CPython environment (default 3.14), checks the
import boundary, and runs the focused compatibility, TLS, low-level, lifecycle,
and boundary tests with `unittest`. Subprocess helpers are isolated in
`eggserve.subprocess`.
