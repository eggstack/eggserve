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
  --quiet                  Suppress startup banner
  --max-connections N      Max concurrent connections (default: 64)
  --max-file-streams N     Max concurrent file streams (default: 32)
  --header-timeout SECS    Header read timeout (default: 10)
  --write-timeout SECS     Response write timeout (default: 60)

TLS options (requires tls feature):
  --tls-cert PATH          PEM certificate chain (requires --tls-key)
  --tls-key PATH           PEM private key (requires --tls-cert)
```

See [docs/cli.md](docs/cli.md) for full details.

## Python API

eggserve provides a Python API with two layers: a native primitives library (PyO3-backed Rust bindings) and a server API for building HTTP servers with Rust-owned I/O.

**This is NOT an ASGI/WSGI server or a web framework.** It is a hardened static-serving primitive.

### Native primitives

Use these for path confinement, policy enforcement, and response planning without launching the server binary:

```python
from eggserve import SecureRoot, StaticPolicy

root = SecureRoot("public", policy=StaticPolicy())
resource = root.resolve_path("/assets/app.css")
if resource.is_file:
    plan = resource.file.plan_response("GET")
    print(plan.status, plan.body_kind)  # 200 file_full
```

### Server primitives

Build HTTP servers with Rust-owned I/O. Rust handles socket accept, HTTP parsing, file streaming, and timeout enforcement:

```python
from eggserve import Server, ServerSecureRoot

root = ServerSecureRoot(".")
with Server(root=root) as server:
    print(f"Serving on {server.addr}")
```

### Handler callbacks

Intercept requests with a Python handler:

```python
from eggserve import Server, ServerSecureRoot, Request, Response

root = ServerSecureRoot(".")

def handler(request: Request) -> Response:
    if request.path == "/health":
        return Response.text(200, "ok")
    return Response.empty(404)

with Server(root=root, handler=handler) as server:
    print(f"Serving on {server.addr}")
```

Handler callbacks support bounded request body consumption (`buffer` or `stream` mode) via constructor parameters. See [docs/body-migration.md](docs/body-migration.md) for details.

### Lifecycle control

Programmatic server lifecycle for tests and embedding:

```python
from eggserve import Server, ServerSecureRoot

root = ServerSecureRoot(".")
server = Server(root=root)
server.start()          # blocks until Running state
print(server.state)     # "running"
server.shutdown()       # non-blocking graceful shutdown
server.wait()           # blocks until stopped
print(server.state)     # "stopped"
```

### Subprocess API

Full HTTP serving via the Rust binary:

```python
from eggserve import serve_directory

# Serve current directory (blocking, safe defaults)
serve_directory(".")
```

For lifecycle control (tests, embedding):

```python
from eggserve import ServeConfig, ServerProcess

config = ServeConfig(directory="public", port=9000)
proc = ServerProcess(config)
proc.start()
proc.wait()
proc.stop()
```

Full API reference: [docs/python-api.md](docs/python-api.md)

## Security defaults

eggserve ships with secure defaults. Every option that weakens security requires explicit CLI flags.

- **Loopback only** — binds to 127.0.0.1 unless `--public` is passed
- **GET and HEAD only** — all other methods are rejected
- **No request bodies** — bodies on GET/HEAD are rejected (413 for size limits, 400 for malformed framing)
- **No symlink following** — denied unless `--follow-symlinks` is passed. On Unix, descriptor-relative traversal (`statat` + `openat`) prevents symlink swap attacks
- **No dotfiles served** — hidden files are excluded
- **No directory listing** — unless `--directory-listing` is passed
- **Unknown MIME as application/octet-stream** — safe fallback
- **Malformed request targets rejected** — invalid paths are not resolved
- **Logs sanitized** — paths/headers are sanitized before logging
- **Resource limits enabled** — connection and file stream limits are active

See [docs/security-policy.md](docs/security-policy.md) for the full security policy.

## Supported platforms

| Platform | Status |
|----------|--------|
| Linux x86_64 | Supported; hardened |
| Linux aarch64 | Supported; hardened |
| macOS arm64 (Apple Silicon) | Supported; hardened |
| macOS x86_64 | Supported; hardened |
| Windows x86_64 | Functional; handle-relative confinement (Plans 084–085). Adversarial qualification scaffold established (Plan 086, 113 tests). Awaiting independent safety review and profile promotion decision. |

Windows implements handle-relative confinement (Plans 084–085) with parser-level protections rejecting reserved names, ADS syntax, drive prefixes, and backslash. Plan 086 adversarial qualification test scaffold is established (reparse-point denial matrix, namespace normalization, race harness, root identity, file validators, ACL/sharing, resource stability, installed artifact parity, fuzz corpus replay). Independent safety review and profile promotion decision are awaited. Windows remains functional-only until those human gates complete.

## Production profiles

eggserve defines explicit production deployment profiles. Every production claim names a profile.

| Profile | Status | Description |
|---------|--------|-------------|
| unix-reverse-proxy | Hardened | Linux/macOS behind Caddy/nginx/Traefik (preferred public deployment). Plan 089 qualification: proxy interop, desync corpus, stateful fuzz, filesystem race, fault injection, 24h soak, installed artifacts, SBOM/provenance, independent review. |
| unix-direct-https | Candidate | Linux/macOS with native rustls (limited HTTP/1.1, not an edge platform). Plan 089 qualification: native TLS abuse, 24h soak, installed artifacts, SBOM/provenance, independent review. |
| windows-reverse-proxy | Candidate | Windows behind reverse proxy; adversarial qualification scaffold established (Plan 086), awaiting independent review and profile decision |
| windows-direct-https | Functional | Windows with native rustls (hardening in progress) |
| local-development | Hardened | Any platform, loopback, safe defaults |
| windows-functional | Functional | Windows SMB/non-NTFS/cloud filesystems |
| link-following-compat | Functional | Any platform with --follow-symlinks (weaker guarantee) |

See `release/support-profiles.toml` for the machine-readable definitions and `docs/threat-model.md` for profile-specific security notes.

**Production deployment recommendation:** Use a reverse proxy (Caddy, nginx, Traefik) for TLS termination. The `unix-reverse-proxy` profile is qualified for production use after Plan 089 gates pass. Native TLS is limited — no ACME, virtual hosting, HTTP/2, or edge platform features. See [docs/deployment.md](docs/deployment.md), [docs/tls.md](docs/tls.md), and [docs/release-runbook.md](docs/release-runbook.md).

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
