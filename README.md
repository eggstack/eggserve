# eggserve

[![CI](https://github.com/eggstack/eggserve/actions/workflows/ci.yml/badge.svg)](https://github.com/eggstack/eggserve/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/eggserve-core.svg)](https://crates.io/crates/eggserve-core)
[![PyPI](https://img.shields.io/pypi/v/eggserve.svg)](https://pypi.org/project/eggserve/)
[![PyPI Downloads](https://static.pepy.tech/personalized-badge/eggserve?period=total&units=INTERNATIONAL_SYSTEM&left_color=BLACK&right_color=GREEN&left_text=downloads)](https://pepy.tech/projects/eggserve)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/eggstack/eggserve/blob/main/LICENSE)

EggServe is a hardened, HTTP-correct static file server, plus a reusable
static-serving library for Python and Rust. It is a secure alternative to
`python -m http.server` with the same mental model and safe defaults.

Safe defaults: loopback bind, path confinement, no symlinks, no dotfiles, no
directory listing. Broader behavior is always an explicit opt-in. See
[security policy](docs/security-policy.md).

## Install

```sh
pip install eggserve          # Python library + CLI (CPython 3.11+)
cargo install --path crates/eggserve-bin   # CLI from source
```

## Quick start

### Python

Static files with the familiar `http.server` shape:

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), handler) as server:
    server.serve_forever()
```

A custom handler on the same runtime (bounded, synchronous):

```python
from eggserve.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"ok\n" if self.path == "/health" else b"not found\n"
        status = 200 if self.path == "/health" else 404
        self.send_response(status)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

A handler-only service without the `http.server` facade:

```python
from eggserve import lowlevel

def handler(request):
    if request.path == "/health":
        return lowlevel.Response.text(200, "ok\n")
    return lowlevel.Response.text(404, "not found\n")

server = lowlevel.Server(
    config=lowlevel.RuntimeConfig(bind="127.0.0.1", port=8000),
    handler=handler,
)
server.start()
server.wait_ready()
```

See [Python API](docs/python-api.md) and the
[compatibility contract](docs/python-http-server-compatibility.md) for
intentional `http.server` deviations.

### Rust

Choose the crate profile that matches the server you are building. Use
`eggserve-server` for a direct H1 application service, optionally with Tower;
use `eggserve-core` for compatibility, static, or multiprotocol composition.
Core intentionally retains the static-serving dependency closure.

For a composed/static server:

```toml
[dependencies]
eggserve-core = "0.3"
tokio = { version = "1", features = ["full"] }
```

For a generic supervised H1 daemon using native services, depend on
`eggserve-server`, `eggserve-primitives`, and Tokio. The direct `tower` feature
is published in `eggserve-server 0.3.0` with registry-only consumer proof
(see `release/plan-286-embedding-contract-publication-closure.md`); it adapts
Tower/Axum without depending on `eggserve-core` or `eggserve-static`:

```toml
[dependencies]
eggserve-server = { version = "0.3", default-features = false, features = ["tower"] }
tokio = { version = "1", features = ["full"] }
```

The direct handle can be
split into independent shutdown control and typed completion; setting
`connection_total_timeout(Duration::ZERO)` opts out of only the 60-second
total-lifetime ceiling. Other request, idle, write, admission, and shutdown
bounds remain. The combined leaf-only fixture is
[`downstream_embedding.rs`](crates/eggserve-server/tests/downstream_embedding.rs).

Serve a confined static directory:

```rust,no_run
use eggserve_core::server::{RuntimeConfig, Server};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let server = Server::builder()
    .runtime(RuntimeConfig::builder()
        .bind("127.0.0.1:8000".parse()?)
        .build()?)
    .static_service("public")?;
let handle = server.start().await?;
handle.ready().await?;
# Ok(())
# }
```

Serve a custom service instead:

```rust,no_run
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server, ServiceError};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let server = Server::builder()
    .runtime(RuntimeConfig::builder()
        .bind("127.0.0.1:8000".parse()?)
        .build()?)
    .build()?;
let service = service_fn(|request: Request| async move {
    let body = b"ok\n";
    Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Bytes(body.to_vec()))
        .map_err(|e| ServiceError::internal(e.to_string()))
});
let handle = server.start_with_service(service).await?;
handle.ready().await?;
# Ok(())
# }
```

H1 is the supported transport; H2/H3 are opt-in and experimental. See
[public API boundary](docs/public-api-boundary.md) and
[downstream app servers](docs/downstream-app-server.md).

### CLI

```sh
eggserve --directory ./public            # loopback, static only
eggserve --directory ./public --public --port 8080   # explicit public bind
```

See [CLI reference](docs/cli.md).

## Examples

Runnable starting points in [examples/](examples/README.md):

- Python: static facade, custom handler, `lowlevel` service, HTTPS
- Rust: static server, custom service, streaming service, caller-owned stream
- CLI: local static server, explicit public bind, directory listing opt-in

## Documentation

- [CLI reference](docs/cli.md) · [deployment](docs/deployment.md) · [timeouts](docs/timeout-reference.md) · [TLS](docs/tls.md)
- [Python API](docs/python-api.md) · [compatibility contract](docs/python-http-server-compatibility.md)
- [HTTP primitives](docs/http-primitives.md) · [migration guide](docs/migration-guide.md)
- [Threat model](docs/threat-model.md) · [non-goals](docs/non-goals.md)
- [Architecture overview](architecture/overview.md)
