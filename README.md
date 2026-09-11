# eggserve

[![CI](https://github.com/eggstack/eggserve/actions/workflows/ci.yml/badge.svg)](https://github.com/eggstack/eggserve/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/eggserve-core.svg)](https://crates.io/crates/eggserve-core)
[![PyPI](https://img.shields.io/pypi/v/eggserve.svg)](https://pypi.org/project/eggserve/)
[![PyPI Downloads](https://static.pepy.tech/personalized-badge/eggserve?period=total&units=INTERNATIONAL_SYSTEM&left_color=BLACK&right_color=GREEN&left_text=downloads)](https://pepy.tech/projects/eggserve)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/eggstack/eggserve/blob/main/LICENSE)

EggServe is a hardened, HTTP-correct static file server and reusable Rust HTTP/static-serving library, with a Python `http.server`-shaped api.

The CLI serves static files only. The Python package provides hardened static
serving plus a bounded, synchronous custom-handler path shaped like
`http.server`, and a public `eggserve.lowlevel` handler-only runtime/service
substrate for downstream bounded application servers. The Rust crate exposes a
low-level, embeddable HTTP runtime and service boundary. EggServe itself is not
an application framework, ASGI/WSGI runtime, CGI executor, FastCGI gateway,
proxy, or general-purpose `socketserver` replacement.

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
[examples/python_custom_handler.py](https://github.com/eggstack/eggserve/blob/main/examples/python_custom_handler.py). For the
handler-only runtime/service substrate without the facade, use
[examples/python_lowlevel_service.py](https://github.com/eggstack/eggserve/blob/main/examples/python_lowlevel_service.py)
(buffered plus bounded streamed responses over the shared native runtime).
The optional subprocess lifecycle example is
[examples/python_subprocess.py](https://github.com/eggstack/eggserve/blob/main/examples/python_subprocess.py); it is not the
canonical `http.server` replacement.

When the `tls` feature is available, HTTPS serving uses
`HTTPSServer` / `ThreadingHTTPSServer` — see
[examples/python_https_server.py](https://github.com/eggstack/eggserve/blob/main/examples/python_https_server.py).

Custom handlers are synchronous and receive bounded in-memory `rfile`/`wfile`
facades. They do not receive raw sockets and do not turn EggServe itself into
an application framework. For a downstream bounded application server, use the
public `eggserve.lowlevel` runtime/service substrate: handler-only
`Server(config, handler)` with no static root, frozen `RuntimeConfig` (admission,
parser, timeout, and safe privacy controls, projected via the single
`_native_kwargs()` helper), bounded `Response.stream` over a
16-chunk backpressured bridge (HEAD/body-forbidden never advance the iterator;
async producers rejected), and caller-owned `StaticResponder` composition. The
optional subprocess helpers are canonically owned by `eggserve.subprocess`
(`eggserve.server` retains compatibility re-exports; top-level
`eggserve.serve_directory` re-exports the subprocess implementation); the primary API is
`eggserve.server`. See the [Python API reference](https://github.com/eggstack/eggserve/blob/main/docs/python-api.md) for the full six-class
surface and [the compatibility contract](https://github.com/eggstack/eggserve/blob/main/docs/python-http-server-compatibility.md)
for intentional deviations from the stdlib.

## Rust library

`eggserve-core` is the intended Rust library crate for the 0.x line. It
exposes `primitives` as the semver-considered public facade and `server` as an
experimental transport-owning runtime; there is no additional `eggserve`
facade crate. Canonical response/request types, `Service`, and the
caller-owned connection driver do not require a direct Hyper dependency.
`primitives::to_hyper_response()` is an explicit opt-in outbound transport
adapter; its returned body type is opaque, so consumers should rely on the
`http_body::Body` contract rather than naming `BoxBody`. This adapter change is
classified as the intentional `0.1.x` → `0.2.0` pre-1.0 transition documented
in the [migration guide](https://github.com/eggstack/eggserve/blob/main/docs/migration-guide.md).

Optional ecosystem adapters (never in default builds) connect the canonical
model to standard types: `http-interop` (`primitives::interop` — loss-aware
`http`/`http-body` conversions, `RequestBody: http_body::Body`,
`response_from_http_body`) and `tower` (`server::tower` —
`TowerToEggserve` per-request clones plus `EggserveToTower`); see the
[interop guide](https://github.com/eggstack/eggserve/blob/main/docs/http-interop.md).
Native `Service` remains the maximum-fidelity path.

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
[the application service](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/application_service.rs),
[the caller-owned stream](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/caller_owned_stream.rs),
and [the primitives demo](https://github.com/eggstack/eggserve/blob/main/crates/eggserve-core/examples/primitives.rs).
They use public EggServe modules only, include readiness plus graceful
shutdown, and are the recommended starting points for custom services.

The runtime owns listeners, protocol parsing, framing, timeouts, and lifecycle.
Default builds and the Python facade remain HTTP/1.1-shaped. Rust builds with
the opt-in `http2` feature add cleartext prior-knowledge HTTP/2 and, when
combined with `tls`, ALPN selection (`h2` before `http/1.1`) through the same
canonical service pipeline. H2 resource limits are owned by `Http2Config` and
remain separate from server-wide service admission. The feature remains
experimental after Plans 186, 190, and 191: deterministic tests, targeted H2
body-policy regressions, two-family interop (curl/libnghttp2 plus python-h2),
h2spec classification, and Linux wire/flow-control/load qualification pass, but
browser/platform evidence, trailer-scope determinism, and a
public safe per-stream reset hook are still release gaps. See the
[HTTP/2 architecture boundary](https://github.com/eggstack/eggserve/blob/main/architecture/http2.md)
and [qualification records](https://github.com/eggstack/eggserve/blob/main/release/plan-186-http2-qualification.md) plus the
[Plan 191 promotion attempt](https://github.com/eggstack/eggserve/blob/main/release/plan-191-http2-supported-tier-qualification.md).
H2's response no-progress guard observes per-response application-body
polling, not guaranteed stream-level wire progress after Hyper accepts a
frame; a stall therefore uses the conservative connection-shutdown fallback.
H3's guard is per-stream instead: the `ResponseStream` producer wait runs
under an absolute `response_write_timeout` no-progress deadline that only
non-empty production plus successful send re-arms (Plan 194; empty chunks are
not progress), each send call keeps its own bound, and a stall resets
only the affected stream while siblings survive. The opt-in `http3` feature
adds an experimental native QUIC/HTTP/3 endpoint
beside that TCP listener. It requires `tls`, a certificate/key identity passed
to `ServerBuilder::http3_identity`, and binds UDP to the resolved TCP port;
`--http3` enables the CLI endpoint and its runtime-owned `Alt-Svc` response
advertisement. QUIC uses TLS 1.3 with `h3` ALPN and rejects application 0-RTT.
HTTP/3 remains Rust-only and experimental after Plans 188, 190, 192, 193, 194, and 195 closure:
deterministic bounded implementation checks and in-process corrective
regressions pass, but independent-client, adversarial-wire, and cross-platform
runtime evidence is incomplete, and the Plan 192 dependency-readiness gate
closed `BLOCKED` on the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 /
Quinn 0.11.11): upstream `hyperium/h3#338` has no released fix and the
`hyperium/h3#262` stream-drop remainder is unresolved. Plan 193 closed at preflight
without entering promotion qualification (unmet Plan 192 prerequisite) after
re-checking both issues and inventorying the missing evidence; Plan 194
bounds the H3 response-producer wait without changing
the tier, and Plan 195 correctively qualifies that bound (stalled,
progress-then-stall, slow-progress, empty-chunk, and sibling evidence plus
shutdown-race and write-stall observability regressions) without changing
the tier. See the
[HTTP/3 architecture boundary](https://github.com/eggstack/eggserve/blob/main/architecture/http3.md)
and [qualification records](https://github.com/eggstack/eggserve/blob/main/release/plan-188-http3-qualification.md) plus the
[Plan 190 corrective record](https://github.com/eggstack/eggserve/blob/main/release/plan-190-multiprotocol-corrective-qualification.md), the
[Plan 192 readiness record](https://github.com/eggstack/eggserve/blob/main/release/plan-192-http3-dependency-readiness.md), the
[Plan 193 promotion record](https://github.com/eggstack/eggserve/blob/main/release/plan-193-http3-supported-tier-qualification.md), the
[Plan 194 correction record](https://github.com/eggstack/eggserve/blob/main/release/plan-194-http3-producer-timeout-correction.md), and the
[Plan 195 corrective qualification record](https://github.com/eggstack/eggserve/blob/main/release/plan-195-http3-response-timeout-corrective-qualification.md).
Reject-body handling is protocol-aware: H2 uses Hyper's end-stream state and
H3 performs one bounded receive probe when headers do not establish an empty
request. H3 request lifecycles are registered for peer-loss, timeout, stream,
and forced-shutdown cancellation, while sibling streams remain isolated.
`Service` owns request handling and response construction. Connections,
in-flight service executions, and file streams have independent observable
budgets; parser ceilings, keep-alive idle, per-connection request counts, and
response write no-progress timeouts are configured via CLI flags, `Limits`, or
`RuntimeConfig` (see the [per-profile defaults](https://github.com/eggstack/eggserve/blob/main/docs/deployment.md)
and [timeout reference](https://github.com/eggstack/eggserve/blob/main/docs/timeout-reference.md)).
Shared runtime defaults and validation live once in the canonical
`eggserve_core::runtime_limits` authority consumed by `Limits`,
`RuntimeConfig`, and the `ServeConfig` bridge; static listing/extra-header
budgets stay service-owned, and hand-constructed `RuntimeConfig` values are
rejected at `ServerBuilder`, `RuntimeState::try_new`, and the caller-owned
connection boundary before semaphore/Hyper use. Each runtime also owns a
per-runtime observability context (`OpsContext`: sink, counters, connection
correlation IDs) attached via `ServerBuilder::ops_context` or
`RuntimeState::with_ops`, with bounded snapshots via
`RuntimeState::ops_snapshot` / `ServerHandle::ops_snapshot`; default
construction clones the process-global default so CLI behavior is unchanged
(see the [operations logging guide](https://github.com/eggstack/eggserve/blob/main/docs/ops-logging.md)). Services may lower
request-body ceilings but cannot raise the runtime hard ceiling.
Canonical `HttpVersion` metadata is non-exhaustive and represents HTTP/1.0,
HTTP/1.1, HTTP/2, and HTTP/3 without silently relabeling an unsupported
transport. `RequestHead::authority()` exposes validated effective host
authority independently of HTTP/1 `Host` or HTTP/2/3 pseudo-header spelling;
forwarded headers remain untrusted. `serve_http1_connection` remains a strict
HTTP/1 entry point; `serve_http_connection` is the opt-in Rust H1/H2 entry
point when the `http2` feature is enabled. The opt-in `http3` feature also
provides a native experimental QUIC/H3 server path; the Python facade and
default builds remain HTTP/1.1-shaped.
The `server` module
is experimental before 1.0. For caller-owned byte streams (for example an
anonymity-network transport), `server::connection::serve_http1_connection`
drives the same canonical pipeline over any `AsyncRead + AsyncWrite` stream
with an explicit `ConnectionContext` (no fabricated socket addresses) and
shared `RuntimeState` admission. See the [Rust architecture overview](https://github.com/eggstack/eggserve/blob/main/architecture/eggserve-core.md),
[primitives facade](https://github.com/eggstack/eggserve/blob/main/architecture/primitives-api.md), and
[runtime contract](https://github.com/eggstack/eggserve/blob/main/architecture/runtime.md).

Downstream application servers build on the same canonical `Service`
boundary. The currently qualified path is HTTP-only: its builder-facing HTTP-half contract (bounded full-duplex
bridging, deferred body ownership, lifecycle cancellation, timeout and
admission splits, byte metadata, plus the Plan 197 stabilized `RequestContext`
single attachment point, `Response`-only final return, 7-stage
commitment/cancellation contract, `Send + Sync` sharing with no `poll_ready`,
and `#[non_exhaustive]` error tolerance) is documented in
[downstream-app-server.md](https://github.com/eggstack/eggserve/blob/main/docs/downstream-app-server.md)
and qualified externally by `crates/eggserve-core/tests/app_server_consumer.rs`
plus the Hyper-free `crates/eggserve-core/tests/application_service_contract.rs`.
The minimal native demonstration is
`crates/eggserve-core/examples/application_service.rs` (buffered echo,
bounded streamed pipe, lifecycle long-poll, no static filesystem).
EggServe itself remains a static server and library, not an application
framework or ASGI/WSGI runtime. Qualification of this substrate does not make
the experimental `server` module a stable 1.0 API.

## Security and compatibility boundaries

- Loopback bind, no symlinks, no dotfiles, and no directory listing are the
  safe defaults for static serving.
- Static serving is GET/HEAD only and rejects request bodies; custom services
  may opt into bounded bodies under the runtime ceiling and return
  known/unknown-length streaming responses (`ResponseBody::Stream`) without
  importing Hyper. `ResponseStream` producers must be `Send` and are one-shot;
  they do not need to be `Sync` because one connection task owns polling.
  Stream services may return response-start while a downstream task still owns
  an Active `RequestBody`; reuse waits for body Complete and abandonment still
  forces safe close, with a transport-neutral `RequestLifecycle` for
  disconnect/cancel observation.
- Path traversal and symlink escape are denied at library level. Unix safe
  defaults use descriptor-relative resolution; Windows is qualified for the
  executed handle-relative classes but remains trusted/local-content only.
- HTTP/1.1, optional Rust HTTP/2, ranges, conditional requests, canonical response normalization
  (including known/unknown-length streaming bodies with runtime-owned
  framing, terminal trailers, and bounded interim 1xx), and bounded resource admission are part of the implemented contract.
  Request trailers arrive as distinct terminal metadata (`trailers()` /
  `read_all_with_trailers()`, denylist + count/byte limits, one validator for
  H1/H2/H3); response trailers stream as one terminal block
  (`ResponseStream::with_trailers`, no data after, `HEAD`/body-forbidden never
  poll); interim 1xx are bounded request-scoped (`InterimSender`, no 101/body,
  no post-commit, HTTP/1.0 suppressed, single 100); `100 Continue` follows body
  policy (`Reject` → 413 without inviting, `Buffer`/`Stream` → Hyper owns wire
  `100`, unknown `Expect` → 417). H1 trailers require `TE: trailers`;
  HTTP/1.0 carries none. See [HTTP primitives](https://github.com/eggstack/eggserve/blob/main/docs/http-primitives.md).
- Final-boundary response privacy: `Server` suppressed by default (optional
  fixed value, never versions), EggServe-owned `Date` (system clock by default,
  caller-supplied provider or explicit suppression), validated header denylist,
  generic errors, and configurable static `ETag`/`Last-Modified`. The
  minimal-fingerprint profile minimizes gratuitous signals without claiming
  un-fingerprintability; see [deployment](https://github.com/eggstack/eggserve/blob/main/docs/deployment.md).
- The CLI accepts hostnames in `--bind`, repeatable safe `-H/--header` static
  metadata, and `--content-type`; TLS accepts a combined cert/key PEM when
  `--tls-key` is omitted.
- Raw socket ownership, `translate_path()`, arbitrary `SSLContext` handling,
  async Python handlers, unbounded response generators, ASGI/WSGI, and
  CGI (`CGIHTTPRequestHandler`/`--cgi`, removed Python 3.15 surface) / FastCGI
  gateways are intentionally unavailable. Generic tunnel handoff (Plan 199,
  superseding deferred Plan 176) **is** available: validated H1 `Upgrade`,
  `CONNECT`, and H2/H3 Extended `CONNECT` (H3 generic `:protocol` blocked by
  `h3` 0.0.8, see tunnel docs) yield a one-shot, transport-backed
  `TunnelCapability` on `RequestContext` (`take_tunnel()`, double-take `None`,
  `AfterCommit` after final commitment). `accept(headers, handler)` returns a
  handshake `Response` (`101` for H1, `200` otherwise; runtime owns
  transition/framing bytes, no raw socket) and a bounded, single-owner
  `TunnelIo` (`AsyncRead + AsyncWrite`, 32 KiB backpressure, lifecycle-aware,
  no payload logged) for the downstream codec. Denial stays ordinary HTTP;
  WebSocket framing itself remains downstream (see `tunnel_upgrade.rs`
  echo + `tokio-tungstenite` interop fixture, no WS in core). Downstream
  gateways build on the canonical `Service` boundary instead.

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
- [Downstream application servers](https://github.com/eggstack/eggserve/blob/main/docs/downstream-app-server.md) — builder-facing HTTP-half contract for downstream app servers (not an EggServe feature)
- [Migration guide](https://github.com/eggstack/eggserve/blob/main/docs/migration-guide.md) — legacy → canonical type mappings, breaking-change policy
- [Library capability matrix](https://github.com/eggstack/eggserve/blob/main/docs/library-capability-matrix.md) — cross-surface feature inventory

**Operations:**
- [Operations logging guide](https://github.com/eggstack/eggserve/blob/main/docs/ops-logging.md) — JSON Lines schema, event reference, counters, troubleshooting
- [Benchmark methodology](https://github.com/eggstack/eggserve/blob/main/benchmarks/README.md) — measurement method, regression policy, claims policy, machine-readable results

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
bash scripts/qualify-http2.sh  # manual H2 wire/ALPN qualification
bash scripts/qualify-http3.sh  # manual H3/QUIC qualification; external clients required for direct H3
```

The routine CI workflow has separate Rust and Python jobs. Platform
qualification and release certification are manual workflows; see
[the release process](https://github.com/eggstack/eggserve/blob/main/docs/release-process.md).

Performance evidence is profile-specific and manual: the final Linux x86_64
same-machine matrix is recorded in
[`benchmarks/170-closure/results.json`](benchmarks/170-closure/results.json).
It documents representative scaling, streaming, low-level Python, TLS,
caller-owned transport, and CPython substitution behavior; absolute timings
are not CI gates or universal performance claims.
