# eggserve

> A hardened, Rust-backed static file server with safe-by-default behavior.

**eggserve is not a general web server, framework, ASGI/WSGI runtime, or Granian replacement.** It serves static files from a directory with secure-by-default behavior. That is all.

## Why not `python -m http.server`?

`python -m http.server` is convenient but unsafe by default:

- Binds to all interfaces (0.0.0.0) unless explicitly told otherwise
- Follows symlinks without restriction
- Serves dotfiles
- Enables directory listing
- Uses a slow, single-threaded Python implementation

eggserve fixes these by making the safe choice the only default. Every unsafe behavior is available but requires explicit opt-in.

For subclass-based custom handlers, eggserve also provides a bounded,
Rust-backed `http.server`-shaped facade. Secure static serving uses the
source-familiar `SimpleHTTPRequestHandler` form:

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

Directory listing is disabled, dotfiles and symlinks are denied, and the
default index order is `index.html`, then `index.htm`. Rust pins the root,
resolves paths, and streams files; Python never reopens a translated path.
The compatibility server accepts stdlib-shaped `(host, port)` tuples: an empty
host is normalized to the explicit IPv4 wildcard `0.0.0.0`, literal wildcard
addresses are accepted by this façade, and port `0` publishes the actual native
port. The CLI continues to require `--public` for wildcard binds.

For subclass-based custom responses:

```python
from eggserve.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"ok\n"
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

with HTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

This facade uses the existing Rust runtime; it does not expose raw sockets or
Python's thread-per-connection implementation. See [the compatibility contract](docs/python-http-server-compatibility.md).

Static MIME customization is bounded to response metadata. `extensions_map`
applies to direct files and native-selected index files; subclass
`guess_type()` applies to direct file targets. GET, HEAD, range, and conditional
responses retain the selected type, while invalid values fail closed. Handler
response conversion is also fail-closed: malformed bodies, invalid headers, and
one-shot body reuse produce a generic 500 without logging untrusted exception
text or response data. Every native file-backed response uses the shared
`max_file_streams` admission limit for its transport lifetime; byte, empty, and
HEAD responses do not consume a file-stream permit.

## Installation

```sh
# Via Python wheel (CPython 3.14 on Linux, macOS, or Windows)
pip install eggserve

# Or run directly with pipx
pipx run eggserve

# From source (requires Rust toolchain)
cargo install --path crates/eggserve-bin
```

## Quick start

**Serve the current directory:**

```sh
eggserve
# Serves on http://127.0.0.1:8000 with safe defaults
```

**Serve a specific directory on a custom port:**

```sh
eggserve --directory public --port 9000
```

**Enable directory listing and follow symlinks:**

```sh
eggserve --directory-listing --follow-symlinks
```

**Bind to all interfaces (requires --public):**

```sh
eggserve --public --port 8080
```

## CLI reference

```
eggserve [OPTIONS] [PORT] [--directory DIR]

Options:
  --directory DIR          Root directory to serve (default: .)
  --addr HOST:PORT         Bind address (default: 127.0.0.1:8000)
  --bind HOST              Bind host (host:port or bare host)
  --port PORT              Port to listen on
  --public                 Bind to all interfaces (required for 0.0.0.0)
  --directory-listing      Enable directory listing
  --follow-symlinks        Follow symlinks
  --allow-dotfiles         Serve dotfiles
  --log-format FORMAT      text, json, or none (default: text)
  --quiet                  Suppress routine informational output (warn/error only)
  --max-connections N      Max concurrent connections (default: 64)
  --max-file-streams N     Max concurrent file streams (default: 32)
  --header-timeout SECS    Header read timeout (default: 10)
  --connection-total-timeout SECS
                            Total connection lifetime timeout (default: 60)
  --handler-timeout SECS   Handler invocation timeout (default: 30)
  --body-read-timeout SECS Request body read timeout (default: 30)

TLS options (requires tls feature):
  --tls-cert PATH          PEM certificate chain (requires --tls-key)
  --tls-key PATH           PEM private key (requires --tls-cert)
```

See [docs/cli.md](docs/cli.md) for full details.

## Python API

The canonical API is a narrow, synchronous `http.server`-shaped façade. Rust
owns sockets, parsing, framing, timeouts, and file streaming; handlers never
receive raw sockets and their in-memory bodies are bounded.

```python
from eggserve.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok\n")

with HTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

Use `SimpleHTTPRequestHandler` for secure static files. `HTTPSServer` and
`ThreadingHTTPSServer` use the same rustls runtime with PEM certificate/key
paths, HTTP/1.1 ALPN only, no SNI certificate selection, and no client
certificates:

```python
from functools import partial
from eggserve.server import HTTPSServer, SimpleHTTPRequestHandler

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with HTTPSServer(("127.0.0.1", 8443), Handler,
                 certfile="cert.pem", keyfile="key.pem") as server:
    server.serve_forever()
```

The advanced primitives are under `eggserve.lowlevel`; the optional bundled
CLI lifecycle helpers are under `eggserve.subprocess`. `serve_directory()`
remains available at the package root. EggServe is not an ASGI/WSGI runtime,
framework, proxy, or HTTP client library.

Full API reference: [docs/python-api.md](docs/python-api.md)

## Security defaults

eggserve ships with secure defaults. Every option that weakens security requires explicit CLI flags.

- **Loopback only** — binds to 127.0.0.1 unless `--public` is passed
- **GET and HEAD only** — all other methods are rejected
- **Static service rejects request bodies** — custom services may opt into
  buffered/streamed bodies within the runtime ceiling
- **No symlink following** — denied unless `--follow-symlinks` is passed. On Unix, descriptor-relative traversal (`statat` + `openat`) prevents symlink swap attacks
- **No dotfiles served** — hidden files are excluded
- **No directory listing** — unless `--directory-listing` is passed
- **Unknown MIME as application/octet-stream** — safe fallback
- **Malformed request targets rejected** — invalid paths are not resolved
- **Logs sanitized** — paths/headers are sanitized before logging
- **Resource limits enabled** — connection and file stream limits are active

Responses follow the shared RFC 9110 response rules: status codes are limited
to 100–599, 205 responses carry no content, weak metadata ETags are not valid
`If-Range` validators, and the runtime adds one authoritative `Date` header.
HEAD responses preserve the equivalent GET representation metadata, including
directory-listing `Content-Length`, while sending no body.

Static files and ranges remain canonical, opened-handle-backed bodies until the
runtime transport boundary. One file-stream admission pool is created per
running server and is shared by static, Rust custom, and Python custom file
responses. Custom services have no implicit filesystem root. Their declared
request-body policy controls GET/HEAD/DELETE/OPTIONS/extension content within
the runtime ceiling; TRACE content is rejected, and incomplete streamed bodies
close the connection.

Plan 108 is retained as a historical corrective implementation and hosted-CI
record. Verified Plan 109 completed the final admission ownership, build-time
static-service consumption, exact Stream wire closure, and truthful
distribution evidence. The pre-runtime Rust `service` module is a
deprecated compatibility adapter requiring an explicit caller-owned runtime
context; production servers use a single runtime-owned file-stream admission
pool.

See [docs/security-policy.md](docs/security-policy.md) for the full security policy.

## Supported platforms

| Platform | Status |
|----------|--------|
| Linux x86_64 | Supported; hardened |
| Linux aarch64 | Supported; hardened |
| macOS arm64 (Apple Silicon) | Supported; hardened |
| macOS x86_64 | Supported; hardened |
| Windows x86_64 | Functional; handle-relative confinement (Plans 084–085). Adversarial qualification test scaffold established (Plan 086, 114 tests). Independent adversarial review is incomplete. Do not use with untrusted public content until that review is completed. |

## Deployment

**Production recommendation:** Use a reverse proxy (Caddy, nginx, Traefik) for TLS termination. Native TLS is limited — no ACME, virtual hosting, HTTP/2, or edge platform features. See [docs/deployment.md](docs/deployment.md) and [docs/tls.md](docs/tls.md).

## Verification

```sh
./scripts/verify.sh fast    # routine dev check: format, clippy, tests
./scripts/verify.sh full    # pre-release: features, Python wheel, package dry-run
./scripts/verify.sh deep    # expensive suites (manual): corpus replay, fault injection, etc.
```

## Examples

See the [examples/](examples/) directory:

- `examples/python_basic.py` — minimal subprocess API usage
- `examples/python_dynamic_static.py` — dynamic health endpoint + static assets using primitives
- `examples/python_safe_download.py` — safe file download handler with user-provided names

Rust examples in `crates/eggserve-core/examples/`:

```sh
cargo run --example rust_primitives -p eggserve-core
cargo run --example server_embedding -p eggserve-core
```

## Scope

eggserve is deliberately narrow. For the full list of non-goals, see [docs/non-goals.md](docs/non-goals.md).

**This is not:** an ASGI/WSGI runtime, a reverse proxy, a web framework, a template engine, a plugin host, a dynamic request execution environment, a production edge platform, or a replacement for nginx/Caddy.

**This is:** a hardened static file server with safe defaults, a hardened static file server for controlled environments and reverse-proxy origins, a small reusable library for path confinement and policy enforcement, and a Python-packaged tool that feels like `python -m http.server`.

Downstream projects may build ASGI/WSGI adapters, application servers, or HTTP clients on eggserve primitives, but those projects are not release deliverables or supported application-serving modes of eggserve.

## Documentation

- [docs/python-api.md](docs/python-api.md) — full Python API reference
- [docs/cli.md](docs/cli.md) — CLI usage reference
- [docs/http-primitives.md](docs/http-primitives.md) — HTTP primitive contract
- [docs/secure-root.md](docs/secure-root.md) — SecureRoot API
- [docs/body-migration.md](docs/body-migration.md) — request body support guide
- [docs/deployment.md](docs/deployment.md) — deployment patterns
- [docs/tls.md](docs/tls.md) — TLS configuration
- [docs/security-policy.md](docs/security-policy.md) — security defaults and opt-in behaviors
- [docs/threat-model.md](docs/threat-model.md) — threat model
- [CONTRIBUTING.md](CONTRIBUTING.md) — contribution guidelines
