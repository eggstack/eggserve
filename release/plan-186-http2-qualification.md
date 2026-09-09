# Plan 186 HTTP/2 Qualification Record

Date: 2026-09-09
Decision: **experimental**
Environment: Linux x86_64, Rust stable, loopback-only qualification

## Decision

EggServe's opt-in native Rust HTTP/2 implementation remains experimental.
Plan 185's deterministic implementation suite is green, and the available
Linux wire checks pass, but the evidence is not sufficient for a general
supported-protocol claim. In particular, this environment has no `nghttp`,
Chromium-family browser, or non-Chromium browser; direct macOS/Windows H2
runtime qualification was not available. Hyper's public server API also does
not expose a safe stream-reset handle from EggServe's response-body adapter,
so a stalled H2 response uses the documented bounded connection-shutdown
fallback. That limitation is conservative and explicit, but it is not the
stream-scoped behavior required for a supported tier.

This record does not claim HTTP/3 support.

Plan 190 re-ran the H2 correction cases and preserves this experimental
decision. See [`plan-190-multiprotocol-corrective-qualification.md`](plan-190-multiprotocol-corrective-qualification.md)
for the DATA-without-`Content-Length` regression and current evidence
inventory.

## Deterministic repository evidence

The following passed locally:

```text
cargo fmt --all -- --check
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo +1.88 check --workspace --all-targets --features http2,tls
```

The focused H2 tests cover cleartext prior knowledge and multiplexing,
canonical H2 metadata, strict HTTP/1 behavior, TLS ALPN H2 selection and H1
fallback, and stream-scoped rejected-body handling without H2 hop-by-hop
headers. The complete workspace and packaging checks were also run as the
release gate before this closure was pushed.

## Available independent-client evidence

Tool versions observed during qualification:

```text
curl 8.5.0 (Ubuntu build), libnghttp2/1.59.0, HTTP2 enabled
nghttp: unavailable
h2load: unavailable
Chromium/Chrome: unavailable
```

The reproducible command is:

```sh
bash scripts/qualify-http2.sh
```

Against a temporary self-signed loopback certificate and the committed
`examples/site` fixture, `curl` verified:

- TLS ALPN selected HTTP/2 with `--http2` and HTTP/1.1 with `--http1.1`;
- static GET, HEAD-compatible response framing, range 206, and conditional
  304 behavior;
- no `Connection` or `Transfer-Encoding` response field on H2;
- parallel requests over one H2 connection;
- cleartext prior-knowledge H2;
- HTTP/1 `Upgrade: h2c` did not switch the connection to H2.

The script runs `nghttp` when installed, but its absence is reported rather
than counted as a pass.

## RFC/ownership summary

RFC 9110 semantics and response normalization remain canonical across H1/H2.
RFC 9113 frame/state, HPACK, pseudo-header ordering, forbidden connection
fields, stream resets, and flow-control mechanics are delegated to Hyper/h2;
EggServe explicitly configures the bounded H2 resource envelope and validates
its canonical post-decode header/target limits. TLS ALPN is owned by rustls and
EggServe's protocol selector. The detailed ownership checklist is in
[`architecture/http2.md`](../architecture/http2.md).

Unsupported capabilities are explicit: no server push, trailers, extended
CONNECT, WebSocket-over-H2, HTTP/1 Upgrade-based h2c, or generic upgrade
handoff. Python's `eggserve.server` facade remains HTTP/1.1-only.

## Resource and lifecycle qualification

Repository tests exercise bounded H2 stream configuration, shared service
admission, response normalization, request-body policy, lifecycle cancellation,
graceful shutdown, and request-count drain through the common runtime kernel.
The H2 response-progress clock is stream-keyed, so sibling writes do not
refresh it. Because the public Hyper API cannot safely reset only the stalled
stream from the response-body path, the runtime closes the connection after
the bounded fallback and records the limitation above.

## Measurements

The feature remains opt-in because H2 adds the `h2`/`tracing` dependency path
and is not needed by default local static serving. The exact local binary-size
and dependency measurements are captured by the release verification commands
below and should accompany any future decision to change the default feature
policy:

```sh
cargo tree -e normal --prefix none -p eggserve-bin --no-default-features   # 88 lines
cargo tree -e normal --prefix none -p eggserve-bin --no-default-features --features http2   # 112 lines
cargo tree -e normal -p eggserve-bin --no-default-features --features http2,tls   # h2 0.4.19, rustls 0.23.41
cargo build --profile dist --locked -p eggserve-bin
cargo build --profile dist --locked -p eggserve-bin --features http2
cargo build --profile dist --locked -p eggserve-bin --features http2,tls
stat -c '%n %s bytes' target/dist/eggserve   # run after each build; see values below
```

The H2-only dist binary measured 1,270,016 bytes, versus 1,002,264 bytes for
the default binary (+267,752 bytes, +26.7%). The H2+TLS binary measured
2,214,512 bytes; this includes the separate TLS footprint. The H2 feature is
not default-enabled by this plan.

## Platform and release placement

Routine CI compiles and tests the H2+TLS feature combination on Linux and
checks it with MSRV Rust 1.88. External-client, browser, and platform runtime
qualification stays manual and targeted. Native H2 is not advertised as
generally supported until those evidence gaps and the stream-reset limitation
are resolved in a later scoped plan.
