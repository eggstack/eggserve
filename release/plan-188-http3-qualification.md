# Plan 188 HTTP/3 Qualification Record

Date: 2026-09-09  
Environment: Linux x86_64, Rust stable 1.98.1, loopback-only local checks  
Decision: **experimental**

## Decision

EggServe's opt-in native HTTP/3 implementation remains experimental. Plan 187
deterministic startup and adapter coverage is extended by this closure with
QUIC-specific request context metadata, runtime-owned Alt-Svc suppression,
stream-local response reset behavior, and deferred request-body timeout
cancellation. The evidence is not sufficient for a supported-protocol claim:
this host has no direct HTTP/3 client, no second independent client, no
representative non-Linux runtime, and no safe packet-impairment environment.

H1 remains the minimal/default protocol. H2 remains experimental under
[Plan 186](plan-186-http2-qualification.md). The Python compatibility facade
and wheel remain HTTP/1.1-shaped and do not expose H3.

Plan 190 re-ran the corrected H3 body-presence and lifecycle cases and
preserves this experimental decision. See
[`plan-190-multiprotocol-corrective-qualification.md`](plan-190-multiprotocol-corrective-qualification.md)
for the in-process regression evidence and current independent-client gap.

## Standards and ownership checklist

The standards baseline checked for this closure is:

| Area | Reference | EggServe ownership | Delegated ownership |
|---|---|---|---|
| HTTP semantics, methods, status, content, authority | [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html) | canonical request/response types, static planner, normalization, privacy | — |
| HTTP/3 message mapping, control streams, stream errors | [RFC 9114](https://www.rfc-editor.org/rfc/rfc9114.html) | H3 adapter policy and service boundary | `h3` wire state and protocol validation |
| QUIC transport, flow control, idle, retry, amplification | [RFC 9000](https://www.rfc-editor.org/rfc/rfc9000.html) | bounded `Http3Config`, connection/admission budgets | Quinn packet transport and recovery |
| TLS 1.3 over QUIC and ALPN | [RFC 9001](https://www.rfc-editor.org/rfc/rfc9001.html) | TLS 1.3-only config, `h3` ALPN, 0-RTT disabled | rustls/Quinn handshake and key schedule |
| QPACK field compression | [RFC 9204](https://www.rfc-editor.org/rfc/rfc9204.html) | decoded field/header aggregate ceilings | `h3` QPACK implementation |
| Alternative services | [RFC 7838](https://www.rfc-editor.org/rfc/rfc7838.html) | same-port `Alt-Svc`, suppression, resolved port | client discovery/cache behavior |

The implementation deliberately does not expose H3/Quinn types in `Service`,
does not implement a second QPACK parser, and does not implement custom retry
tokens or congestion/loss recovery. The dependency stack owns wire parsing,
transport state, and recovery; EggServe owns the canonical service, admission,
body, response, privacy, and shutdown boundaries.

## Deterministic evidence

The following repository checks passed after the Plan 188 corrections:

```text
cargo fmt --all -- --check
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
```

The focused tests cover same-port TCP/UDP startup, missing QUIC identity,
canonical H3 request metadata, QUIC HTTPS context endpoints, H3 configuration
validation, runtime-owned Alt-Svc port selection and suppression, H3 body
timeouts, and stream/error adapter behavior. Routine CI now compiles and runs
the H3/TLS feature representative on Linux and checks it with MSRV Rust 1.88.

The reproducible manual path is:

```sh
bash scripts/qualify-http3.sh
EGGSERVE_REQUIRE_H3_CLIENTS=1 bash scripts/qualify-http3.sh
EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1 bash scripts/qualify-http3.sh
```

Without a direct client the first command still records the feature graph,
build, same-port Alt-Svc, and TCP fallback baseline, then reports that direct
H3 is unavailable. The latter commands intentionally fail with exit 2 until
the requested client evidence exists.

## Independent-client and network evidence

Observed client inventory:

```text
curl 8.5.0, libnghttp2/1.59.0, HTTP/2 enabled, HTTP/3 unavailable
nghttp3-client: unavailable
quiche-client: unavailable
Chromium/Chrome/Firefox: unavailable
network namespace/traffic-control qualification: unavailable
```

Therefore the following are explicitly **not claimed**: direct H3 GET/HEAD,
range/conditional/streaming wire interoperability, two-client agreement,
packet-loss/reordering/MTU recovery, QUIC connection-pressure behavior under
incomplete handshakes, browser Alt-Svc discovery, or macOS/Windows H3 runtime
support. The manual script is ready to rerun when an independent client and a
representative platform are available.

The code-level review confirms that H3 accepts only TLS 1.3 with `h3` ALPN,
disables application 0-RTT, disables migration in the Quinn configuration,
uses bounded stream/connection windows, bounds pending handshakes, and keeps
TCP fallback independent. Transport identifiers, tokens, key material, raw
paths, and QPACK details are not emitted by the H3 adapter's operational log
messages.

## Resource, lifecycle, and shutdown review

`Http3Config` defaults remain bounded and opt-in: 100 bidirectional streams,
16 unidirectional streams, 256 KiB per-stream receive window, 4 MiB per-
connection receive/send windows, 60-second QUIC idle timeout, 64 pending
handshakes, 32 KiB decoded field sections, and 256 KiB H3 response send
chunks. The shared connection, service, file-stream, header, target, body,
handler, and response-write limits remain active.

The adapter now cancels a deferred incoming body at the shared body deadline
and resets only the affected request direction. A response write/producer
failure resets only the affected H3 stream after commitment. H3 shutdown
closes the endpoint, sends H3 shutdown/GOAWAY through `h3`, drains accepted
request tasks to the common deadline, and aborts only after that deadline.

The remaining unsupported evidence is adversarial wire qualification of mixed
stream cancellation, QPACK pressure, slow-peer flow control, pending
handshake pressure, GOAWAY races, and exactly-once permit release under a
real independent client. These are follow-up qualification work, not a reason
to expand the H3 feature surface.

## Footprint and platform decision

The measured normal dependency graph sizes on this host were:

```text
eggserve-bin default:       88 lines
eggserve-bin http2,tls:    131 lines
eggserve-bin http3:        182 lines
```

The H3 graph is optional and absent from the minimal graph. H3 is not default-
enabled. The project MSRV remains Rust 1.88; minimal, H2/TLS, and H3/TLS
checks are part of the release verification commands, with H3 MSRV checking
added to routine CI.

Only Linux x86_64 H3 runtime startup evidence is available. Compilation and
runtime support must not be extrapolated to macOS or Windows; those platforms
remain unqualified for H3. Python wheels intentionally do not expose or
advertise H3.

## Final tier and follow-up

| Protocol/surface | Final tier | Reason |
|---|---|---|
| HTTP/1.1 default and Python facade | supported baseline | Existing deterministic and release evidence |
| Native HTTP/2 (`http2`) | experimental | Plan 186 evidence boundary and no safe public per-stream reset |
| Native HTTP/3 (`http3`) | experimental | Deterministic bounded implementation passes; independent-client, adversarial-wire, and cross-platform evidence incomplete |
| H3 extensions (0-RTT app requests, WebTransport, datagrams, CONNECT/WebSocket, push) | intentionally unsupported | Outside Plans 183–188 scope |

Promotion of H3 requires a new narrow qualification update with at least two
current independent clients, one not sharing Quinn/h3 code with the server,
direct semantic requests, Alt-Svc discovery/fallback, mixed-stream and
resource-pressure evidence, graceful drain, representative platform runtime
evidence, and refreshed dependency/security review. This record does not make
those claims by inference from local startup or compilation.
