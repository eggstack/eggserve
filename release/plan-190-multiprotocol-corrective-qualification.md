# Plan 190 Multiprotocol Corrective Qualification

Date: 2026-09-09  
Candidate: `279392cdb6e8c5dbd659e8cebd84291ef2c93603`  
Environment: Linux x86_64, Rust/Cargo 1.98.1, MSRV target Rust 1.88, loopback
qualification  
Decision: **closed; HTTP/1.1 supported baseline, native H2/H3 experimental**

## Scope and inventory

Plan 190 qualified the narrow Plan 189 corrections. No routing, middleware,
proxy, upload, WebSocket, application-server, Python protocol, or dependency
scope was added.

| Item | Observed value |
|---|---|
| Hyper / http-body | Hyper 1.11.1 / http-body 1.0.1 |
| H3 / h3-quinn | 0.0.8 / 0.0.10 |
| Quinn / rustls | 0.11.11 / 0.23.41 |
| Enabled deterministic feature sets | default; `http2,tls`; `http3,tls` |
| Independent clients | curl 8.5.0 with libnghttp2 1.59.0 (HTTP/2) |
| Unavailable clients | `nghttp`; direct HTTP/3 clients; browsers |
| Platform evidence | Linux x86_64 only |
| Minimal graph | no `h3`, `h3-quinn`, or `quinn` |

The Plan 189 implementation is narrow: request-body policy branches, the
canonical runtime-error constructor, H3 lifecycle registration/cancellation,
H2 producer-progress naming, and focused tests/docs.

## Deterministic corrective evidence

The focused tests passed:

```text
cargo test -p eggserve-core --features http2,tls --test http2_runtime
cargo test -p eggserve-core --features http3,tls --test http3_runtime -- --nocapture
cargo test -p eggserve-core --features http3,tls --lib
```

The H2 harness covers DATA without `Content-Length` under Reject with zero
service invocation, bodyless dispatch without `Content-Length`, no H1
hop-by-hop response fields, stream-scoped rejection, and a surviving sibling
stream. The H3 in-process Quinn/h3 client covers DATA without
`Content-Length`, `Content-Length: 0` plus DATA, bodyless dispatch, bounded
presence-probe timeout, sibling survival, and a detached lifecycle waiter
waking after peer close. Shared `RequestBody` tests cover premature EOF and
over-declared data for buffered and streaming consumption.

Canonical runtime-error tests cover 400, 405 (including `Allow`), 408, 413,
414, 431, 500, 503, an unassigned status, `HEAD`, body-forbidden responses,
and `ErrorRepresentationPolicy::Empty`. H3's timeout response is asserted to
use the canonical 408 representation, so it cannot carry an internal-error
fallback body. Response producer failures remain post-commit stream failures;
the H2 public API limitation is documented rather than overstated.

The full local deterministic gate was also run:

```text
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo +1.88 check --workspace --all-targets --features http2,tls
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
bash scripts/test-python-wheel.sh
```

The supply-chain checks also passed:

```text
cargo audit
cargo deny check
```

## External qualification

`bash scripts/qualify-http2.sh` passed. Curl verified TLS ALPN H2 and H1
fallback, static GET/HEAD-compatible framing, range and conditional responses,
parallel H2 requests, cleartext prior knowledge, and the absence of Upgrade
h2c switching. `nghttp` was unavailable.

The following H3 commands were run without weakening their evidence gates:

```text
bash scripts/qualify-http3.sh                         # baseline passed
EGGSERVE_REQUIRE_H3_CLIENTS=1 bash scripts/qualify-http3.sh       # exit 2
EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1 bash scripts/qualify-http3.sh   # exit 2
```

The baseline H3 script verified the minimal dependency graph, H3 build,
same-port Alt-Svc, and TCP fallback, then reported that no direct H3 client was
installed. Direct H3 GET/HEAD/range/conditional wire behavior, second-client
agreement, browser Alt-Svc discovery, packet impairment, and non-Linux runtime
behavior remain unqualified.

The H3 script's strict-client gates both returned the expected exit code 2 on
this host; the two-client gate was corrected during this qualification so that
zero available clients cannot be mistaken for satisfying a two-client
requirement.

## Lifecycle, resources, and security

H3 peer-close cancellation wakes detached `RequestLifecycle` observers with
`PeerDisconnected`; body-presence timeout uses `ConnectionTimeout`, stops only
the affected receive direction, and does not invoke the service. The shared
registry preserves first-reason-wins semantics and stream-scoped response
failure handling. No QUIC connection IDs, tokens, keys, raw body/path values,
or internal error details are reflected in responses or operational logs.

H2 response timeout wording now matches the public Hyper guarantee: EggServe
tracks application-body producer/poll progress per response, while bytes
already accepted by Hyper are bounded by the configured send buffer and hard
connection lifetime. Hyper exposes no safe public stream-local wire-progress or
reset hook, so a stalled H2 response uses conservative connection shutdown and
H2 remains experimental.

## Release decision

| Surface | Tier | Remaining gate |
|---|---|---|
| HTTP/1.1 and Python facade | Supported baseline | Normal platform/release gates |
| Native HTTP/2 | Experimental | Second-client, browser/platform, and safe stream-reset/wire-progress evidence |
| Native HTTP/3 | Experimental | Independent clients, adversarial QUIC/network evidence, and non-Linux runtime |
| Python wheels | HTTP/1.1-shaped | No H2/H3 API or version bump from this plan |

The repository remains development version `0.1.2`. The documented stable Rust
API transition must be released as `0.2.0` or later, not as a `0.1.x` patch.
Future H2/H3 promotion requires a new narrow evidence plan; Plan 190 does not
promote either protocol merely because the deterministic Plan 189 bugs are
closed.

Historical records remain authoritative for their original evidence:
[Plan 186 H2](plan-186-http2-qualification.md) and
[Plan 188 H3](plan-188-http3-qualification.md). This record carries the
post-correction qualification delta.
