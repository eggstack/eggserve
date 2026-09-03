# eggserve

[![CI](https://github.com/eggstack/eggserve/actions/workflows/ci.yml/badge.svg)](https://github.com/eggstack/eggserve/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/eggserve-core.svg)](https://crates.io/crates/eggserve-core)
[![PyPI](https://img.shields.io/pypi/v/eggserve.svg)](https://pypi.org/project/eggserve/)
[![PyPI Downloads](https://static.pepy.tech/personalized-badge/eggserve?period=total&units=INTERNATIONAL_SYSTEM&left_color=BLACK&right_color=GREEN&left_text=downloads)](https://pepy.tech/projects/eggserve)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/eggstack/eggserve/blob/main/LICENSE)

EggServe is a hardened, HTTP-correct static file server and reusable Rust HTTP/static-serving library, with a Python `http.server`-shaped api.

The CLI serves static files only. The Python package provides hardened static
serving plus a bounded, synchronous custom-handler path shaped like
`http.server`. The Rust crate exposes a low-level, embeddable HTTP runtime and
service boundary. EggServe itself is not an application framework, ASGI/WSGI
runtime, proxy, or general-purpose `socketserver` replacement.

## Secure alternative to `python -m http.server`

`python -m http.server` is a useful local-development tool with a
well-understood interface. EggServe provides a secure alternative built on
the same mental model: loopback binding, path confinement, dotfile denial,
and disabled directory listings are the defaults; broader behavior requires
an explicit opt-in. It also adds native range and conditional responses,
bounded resource limits, and the same hardened static service behind its
CLI, Python, and Rust surfaces.

The concise surface comparison is in the
[Python compatibility contract](https://github.com/eggstack/eggserve/blob/main/docs/python-http-server-compatibility.md).

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

The positional form is `eggserve [OPTIONS] [PORT] [DIRECTORY]`. Explicit port
sources occupy the PORT slot, so a numeric directory remains unambiguous after
them—for example, `eggserve --port 9000 1234` serves directory `1234`. Use
`--directory 1234` when selecting a numeric directory without a positional
port; a single positional numeric token continues to mean PORT.

The CLI is a static file server. Directory listings, symlink following, and
dotfile serving are separate explicit flags. Static metadata can be set with
`--content-type` and repeatable `-H/--header`; see the [CLI reference](https://github.com/eggstack/eggserve/blob/main/docs/cli.md)
and [security policy](https://github.com/eggstack/eggserve/blob/main/docs/security-policy.md).

## Python `http.server` facade

The canonical Python static-serving example is
[examples/python_http_server_static.py](https://github.com/eggstack/eggserve/blob/main/examples/python_http_server_static.py).
Run it with `python examples/python_http_server_static.py`; it is source-
familiar while keeping the filesystem and transport in Rust.

Stock `SimpleHTTPRequestHandler` with the documented default eligibility uses
the native static fast path. Directory listings, dotfiles, and symlinks remain
denied unless explicitly enabled through the supported facade settings.
The Python 3.15-shaped static metadata hooks are supported: set
`default_content_type` for unknown suffixes and pass ordered
`extra_response_headers` through a stock handler or `functools.partial`.
Extra headers are emitted only on final `200` static responses and cannot
override runtime-owned metadata. See
[examples/python_custom_headers.py](https://github.com/eggstack/eggserve/blob/main/examples/python_custom_headers.py) for a
working demonstration.

For bounded synchronous custom responses, use the complete
[examples/python_custom_handler.py](https://github.com/eggstack/eggserve/blob/main/examples/python_custom_handler.py). The
optional subprocess lifecycle example is
[examples/python_subprocess.py](https://github.com/eggstack/eggserve/blob/main/examples/python_subprocess.py); it is not the
canonical `http.server` replacement.

When the `tls` feature is available, HTTPS serving uses
`HTTPSServer` / `ThreadingHTTPSServer` — see
[examples/python_https_server.py](https://github.com/eggstack/eggserve/blob/main/examples/python_https_server.py).

Custom handlers are synchronous and receive bounded in-memory `rfile`/`wfile`
facades. They do not receive raw sockets, do not provide unbounded streaming,
and do not turn EggServe into an application server. The optional subprocess
helpers are under `eggserve.subprocess`; the primary API is `eggserve.server`.
See the [Python API reference](https://github.com/eggstack/eggserve/blob/main/docs/python-api.md) for the full six-class
surface and [the compatibility contract](https://github.com/eggstack/eggserve/blob/main/docs/python-http-server-compatibility.md)
for intentional deviations from the stdlib.

## Rust library

`eggserve-core` is the intended Rust library crate for the 0.x line. It
exposes `primitives` as the semver-considered public facade and `server` as an
experimental transport-owning runtime; there is no additional `eggserve`
facade crate. Rust applications do not need a direct Hyper dependency.

The concise static-server flow is:

```rust,no_run
use eggserve_core::server::{RuntimeConfig, Server};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let server = Server::builder()
    .runtime(RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse()?)
        .build()?)
    .static_service("public")?;
let handle = server.start().await?;
handle.ready().await?;
println!("listening on {}", handle.local_addr());
// ... make requests ...
handle.shutdown();
handle.wait().await?;
# Ok(())
# }
```

The executable, mechanically checked examples are [the static server](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/static_server.rs),
[the custom service](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/custom_service.rs),
[the streaming service](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/streaming_service.rs),
and [the primitives demo](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/primitives.rs).
They use public EggServe modules only, include readiness plus graceful
shutdown, and are the recommended starting points for custom services.

The runtime owns listeners, HTTP/1 parsing, framing, timeouts, and lifecycle;
`Service` owns request handling and response construction. The `server` module
is experimental before 1.0. For caller-owned byte streams (for example an
anonymity-network transport), `server::connection::serve_http1_connection`
drives the same canonical pipeline over any `AsyncRead + AsyncWrite` stream
with an explicit `ConnectionContext` (no fabricated socket addresses) and
shared `RuntimeState` admission. See the [Rust architecture overview](https://github.com/eggstack/eggserve/blob/main/architecture/eggserve-core.md),
[primitives facade](https://github.com/eggstack/eggserve/blob/main/architecture/primitives-api.md), and
[runtime contract](https://github.com/eggstack/eggserve/blob/main/architecture/runtime.md).

## Security and compatibility boundaries

- Loopback bind, no symlinks, no dotfiles, and no directory listing are the
  safe defaults for static serving.
- Static serving is GET/HEAD only and rejects request bodies; custom services
  may opt into bounded bodies under the runtime ceiling and return
  known/unknown-length streaming responses (`ResponseBody::Stream`) without
  importing Hyper.
- Path traversal and symlink escape are denied at library level. Unix safe
  defaults use descriptor-relative resolution; Windows is qualified for the
  executed handle-relative classes but remains trusted/local-content only.
- HTTP/1.1, ranges, conditional requests, canonical response normalization
  (including known/unknown-length streaming bodies with runtime-owned
  framing), and bounded resource admission are part of the implemented contract.
- The CLI accepts hostnames in `--bind`, repeatable safe `-H/--header` static
  metadata, and `--content-type`; TLS accepts a combined cert/key PEM when
  `--tls-key` is omitted.
- Raw socket ownership, `translate_path()`, arbitrary `SSLContext` handling,
  async Python handlers, unbounded Python response streaming, and ASGI/WSGI are
  intentionally unavailable.

See the [security policy](https://github.com/eggstack/eggserve/blob/main/docs/security-policy.md),
[threat model](https://github.com/eggstack/eggserve/blob/main/docs/threat-model.md),
[Python compatibility matrix](https://github.com/eggstack/eggserve/blob/main/docs/python-http-server-compatibility.md), and
[non-goals](https://github.com/eggstack/eggserve/blob/main/docs/non-goals.md).

## Installation

```sh
# Python wheel: CPython 3.11+ with prebuilt wheels for 9 platforms
# including Linux (manylinux/musllinux, x86_64/aarch64/armv7),
# macOS (x86_64/arm64), and Windows (x86_64/arm64).
# Covers Raspberry Pi/SBC via aarch64/armv7 wheels and Alpine via musllinux.
pip install eggserve

pipx run eggserve

# From source (requires a Rust toolchain)
cargo install --path crates/eggserve-bin
```

The source-checkout command installs the `eggserve-bin` package's `eggserve`
binary. Rust embedders should add `eggserve-core` as their library dependency;
the executable crate is intentionally a thin CLI surface.

The Python wheel includes the native extension and extension-backed CLI entry
point; it does not bundle a second standalone CLI binary. See
[toolchain and wheel support](https://github.com/eggstack/eggserve/blob/main/docs/toolchain-support.md).

## Deeper references

**CLI and installation:**
- [CLI reference](https://github.com/eggstack/eggserve/blob/main/docs/cli.md) — all flags, positional parsing, and examples
- [Timeout reference](https://github.com/eggstack/eggserve/blob/main/docs/timeout-reference.md) — every runtime timeout, semantics, and precedence
- [TLS support](https://github.com/eggstack/eggserve/blob/main/docs/tls.md) — building with `--features tls`, certificate requirements
- [Toolchain and wheel support](https://github.com/eggstack/eggserve/blob/main/docs/toolchain-support.md) — platform matrix, Python versions
- [Deployment guidance](https://github.com/eggstack/eggserve/blob/main/docs/deployment.md) — production profiles, reverse-proxy patterns

**Python:**
- [Python API reference](https://github.com/eggstack/eggserve/blob/main/docs/python-api.md) — `HTTPServer`, `ThreadingHTTPServer`, `HTTPSServer`, handler classes
- [Python compatibility contract](https://github.com/eggstack/eggserve/blob/main/docs/python-http-server-compatibility.md) — deviations from `http.server`
- [Python packaging](https://github.com/eggstack/eggserve/blob/main/docs/python-packaging.md) — wheel architecture, build from source
- [Request body migration](https://github.com/eggstack/eggserve/blob/main/docs/body-migration.md) — body modes, one-shot enforcement, error hierarchy

**Rust library:**
- [Rust HTTP primitives](https://github.com/eggstack/eggserve/blob/main/docs/http-primitives.md) — HTTP/1.1 primitive contract
- [Public API boundary](https://github.com/eggstack/eggserve/blob/main/docs/public-api-boundary.md) — stability tiers, semver policy
- [Migration guide](https://github.com/eggstack/eggserve/blob/main/docs/migration-guide.md) — legacy → canonical type mappings, breaking-change policy
- [Library capability matrix](https://github.com/eggstack/eggserve/blob/main/docs/library-capability-matrix.md) — cross-surface feature inventory

**Operations:**
- [Operations logging guide](https://github.com/eggstack/eggserve/blob/main/docs/ops-logging.md) — JSON Lines schema, event reference, counters, troubleshooting

**Security:**
- [Security policy](https://github.com/eggstack/eggserve/blob/main/docs/security-policy.md) — safe defaults and enforcement
- [Threat model](https://github.com/eggstack/eggserve/blob/main/docs/threat-model.md) — attacker profiles, trust boundaries
- [Security review](https://github.com/eggstack/eggserve/blob/main/docs/security-review.md) — posture summary for adopters
- [Non-goals](https://github.com/eggstack/eggserve/blob/main/docs/non-goals.md) — explicit exclusions

**Architecture:**
- [Architecture overview](https://github.com/eggstack/eggserve/blob/main/architecture/overview.md) — workspace layout, module map, data flow
- [Examples](https://github.com/eggstack/eggserve/tree/main/examples) — all runnable demonstrations

## Local verification

```sh
./scripts/verify.sh fast    # format, clippy, and workspace tests
./scripts/verify.sh full    # fast + examples + TLS + installed Python wheel checks
./scripts/verify.sh deep    # expensive suites selected for release risk
```

The routine CI workflow has separate Rust and Python jobs. Platform
qualification and release certification are manual workflows; see
[the release process](https://github.com/eggstack/eggserve/blob/main/docs/release-process.md).
