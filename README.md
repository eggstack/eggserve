# eggserve

> EggServe is a hardened, HTTP-correct static file server and reusable Rust HTTP/static-serving library, with a Python `http.server`-shaped facade.

The CLI serves static files only. The Python package provides hardened static
serving plus a bounded, synchronous custom-handler path shaped like
`http.server`. The Rust crate exposes a low-level, embeddable HTTP runtime and
service boundary. EggServe itself is not an application framework, ASGI/WSGI
runtime, proxy, or general-purpose `socketserver` replacement.

## Why EggServe instead of `python -m http.server`?

`python -m http.server` is a useful local-development tool, but its ordinary
defaults bind broadly, follow symlinks, serve dotfiles, and list directories.
EggServe makes loopback binding, path confinement, dotfile denial, and disabled
directory listings the defaults; weaker behavior requires an explicit opt-in.
It also provides native range and conditional responses, bounded resource
limits, and the same hardened static service behind its CLI, Python, and Rust
surfaces.

The concise surface comparison is in the
[Python compatibility contract](docs/python-http-server-compatibility.md).

## CLI quickstart

Serve the small example fixture on loopback:

```sh
eggserve --directory ./examples/site
```

For a source checkout, the equivalent is:

```sh
cargo run -p eggserve-bin -- --directory ./examples/site
```

Make a public bind explicit when serving beyond the local machine:

```sh
eggserve --directory ./examples/site --public --port 8080
```

The CLI is a static file server. Directory listings, symlink following, and
dotfile serving are separate explicit flags; see the [CLI reference](docs/cli.md)
and [security policy](docs/security-policy.md).

## Python `http.server` facade

The canonical Python static-serving example is
[examples/python_http_server_static.py](examples/python_http_server_static.py).
Run it with `python examples/python_http_server_static.py`; it is source-
familiar while keeping the filesystem and transport in Rust.

Stock `SimpleHTTPRequestHandler` with the documented default eligibility uses
the native static fast path. Directory listings, dotfiles, and symlinks remain
denied unless explicitly enabled through the supported facade settings.

For bounded synchronous custom responses, use the complete
[examples/python_custom_handler.py](examples/python_custom_handler.py). The
optional subprocess lifecycle example is
[examples/python_subprocess.py](examples/python_subprocess.py); it is not the
canonical `http.server` replacement.

Custom handlers are synchronous and receive bounded in-memory `rfile`/`wfile`
facades. They do not receive raw sockets, do not provide unbounded streaming,
and do not turn EggServe into an application server. The optional subprocess
helpers are under `eggserve.subprocess`; the primary API is `eggserve.server`.
See the [Python API reference](docs/python-api.md) for the full six-class
surface and [the compatibility contract](docs/python-http-server-compatibility.md)
for intentional deviations from the stdlib.

## Rust library

`eggserve-core` exposes `primitives` as the intended public facade and
`server` as an experimental transport-owning runtime. The executable,
mechanically checked examples are [the static server](crates/eggserve-core/examples/static_server.rs),
[the custom service](crates/eggserve-core/examples/custom_service.rs),
and [the primitives demo](crates/eggserve-core/examples/primitives.rs).
They use no direct Hyper imports and include readiness plus graceful shutdown.

The runtime owns listeners, HTTP/1 parsing, framing, timeouts, and lifecycle;
`Service` owns request handling and response construction. The `server` module
is experimental before 1.0. See the [Rust architecture overview](architecture/eggserve-core.md),
[primitives facade](architecture/primitives-api.md), and
[runtime contract](architecture/runtime.md).

## Security and compatibility boundaries

- Loopback bind, no symlinks, no dotfiles, and no directory listing are the
  safe defaults for static serving.
- Static serving is GET/HEAD only and rejects request bodies; custom services
  may opt into bounded bodies under the runtime ceiling.
- Path traversal and symlink escape are denied at library level. Unix safe
  defaults use descriptor-relative resolution; Windows is qualified for the
  executed handle-relative classes but remains trusted/local-content only.
- HTTP/1.1, ranges, conditional requests, canonical response normalization,
  and bounded resource admission are part of the implemented contract.
- Raw socket ownership, `translate_path()`, arbitrary `SSLContext` handling,
  async Python handlers, unbounded Python response streaming, and ASGI/WSGI are
  intentionally unavailable.

See the [security policy](docs/security-policy.md),
[threat model](docs/threat-model.md),
[Python compatibility matrix](docs/python-http-server-compatibility.md), and
[non-goals](docs/non-goals.md).

## Installation

```sh
# Python wheel: CPython 3.11+; Linux, macOS, and Windows wheels are built
# according to the support matrix.
pip install eggserve

pipx run eggserve

# From source (requires a Rust toolchain)
cargo install --path crates/eggserve-bin
```

The Python wheel includes the native extension and extension-backed CLI entry
point; it does not bundle a second standalone CLI binary. See
[toolchain and wheel support](docs/toolchain-support.md).

## Deeper references

- [CLI reference](docs/cli.md)
- [Python API reference](docs/python-api.md)
- [Python compatibility contract](docs/python-http-server-compatibility.md)
- [Rust HTTP primitives](docs/http-primitives.md)
- [Security policy](docs/security-policy.md)
- [Deployment guidance](docs/deployment.md)
- [TLS constraints](docs/tls.md)
- [Library capability matrix](docs/library-capability-matrix.md)
- [Architecture overview](architecture/overview.md)
- [Examples](examples/)

## Local verification

```sh
./scripts/verify.sh fast    # format, clippy, and workspace tests
./scripts/verify.sh full    # fast + examples + TLS + installed Python wheel checks
./scripts/verify.sh deep    # expensive suites selected for release risk
```

The routine CI workflow has separate Rust and Python jobs. Platform
qualification and release certification are manual workflows; see
[the release process](docs/release-process.md).
