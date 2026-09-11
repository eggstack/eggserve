# HTTP/3 and QUIC transport boundary

EggServe's native HTTP/3 path is an opt-in Rust feature (`http3`). It remains
an experimental transport adapter after Plans 188, 190, 192, 193, 194, and
195 closure, not a change to the
Python compatibility surface or to the static service planner. The
implementation uses `h3` with `h3-quinn` and Quinn over Tokio; those
dependencies are absent from the default, HTTP/1, and HTTP/2 graphs.

## Adapter ownership (Plan 206 Track C)

The `server/http3/` directory is split into four submodules:

| Module | Owns |
|--------|------|
| `endpoint.rs` | `ActiveConnectionGuard`, close-reason classification |
| `request.rs` | H3→canonical request conversion, declared-length checks, trailers, `invoke_service` |
| `response.rs` | `runtime_error` construction, `response_write_timeout` watchdog, `send_*` trio (data, trailers, known-length) |
| `tunnel.rs` | `kind_string`, `H3ActiveTunnelGuard`, `send_h3_tunnel_handshake` |

The facade `server/http3.rs` owns `accept_loop` and tests, qualifying calls as
`endpoint::`/`request::`/`response::`/`tunnel::`. One shared kernel, no
H3-specific semantics.

## Ownership

| Concern | Owner | Boundary |
|---|---|---|
| QUIC packet handling, TLS handshake, stream flow control | Quinn/rustls | Internal adapter and explicit `Http3Config` projection |
| H3 control/QPACK/request stream state | h3 | Internal adapter; no H3 types in `Service` |
| Method, target, authority, ordinary headers, body policy | EggServe canonical adapter | `RequestHead`, `RequestBody`, and shared service kernel |
| Service admission, panic containment, timeout, error status | EggServe runtime | Shared `RuntimeState` and canonical invocation helper |
| Response privacy, `Date`, `Server`, `Alt-Svc` | EggServe finalization | Canonical response boundary plus Hyper adapter |
| Static files, ranges, conditionals, listings | Static service | Unchanged `StaticService`/planner |

## Startup and listener lifecycle

When `RuntimeConfig::http3.enabled` is true, `Server::start_with_service`:

1. binds or adopts the TCP listener and resolves its actual local address;
2. builds a separate QUIC rustls configuration from the PEM paths supplied by
   `ServerBuilder::http3_identity`;
3. binds the Quinn UDP endpoint to that same address and port; and
4. starts one supervisor owning both accept loops and one shutdown handle.

Port `0` therefore resolves to one origin port. A UDP bind failure is returned
before the server task is started, and the TCP listener is dropped as part of
the failed startup. `from_listener(TcpListener)` / `from_std_listener` uses the supplied listener's
resolved address for the UDP bind; `http3_socket(std UdpSocket)` (Plan 201) supplies a prebound
UDP socket instead (same-port TCP+UDP validated, Quinn-wrapped at startup, no
Quinn types in the public contract). H3 intentionally requires the accompanying TCP
listener so `ServerHandle::local_addr()` remains a truthful origin address.

The shared `max_connections` semaphore covers accepted TCP and QUIC
connections. QUIC handshakes additionally consume the H3
`max_pending_handshakes` budget until handshake completion. Shutdown closes
the endpoint, sends H3 GOAWAY through the h3 connection driver, drains active
request tasks up to the common graceful deadline, and then cancels remaining
request lifecycles with `ServerShutdown` before aborting tasks. A connection
that becomes unusable for peer or transport reasons cancels every still-live
request observer; a stream-local receive/send failure cancels only its own
observer.

## TLS and protocol policy

QUIC gets a fresh rustls `ServerConfig` rather than reusing TCP TLS state. It
offers only `h3`, restricts the protocol versions to TLS 1.3, and sets early
data to zero. The initial implementation does not accept application 0-RTT,
WebTransport, HTTP datagrams, extended CONNECT, server push, WebSockets, or
connection migration as an application identity mechanism.

## Bounded configuration

`Http3Config` pins the transport envelope:

| Field | Default | Purpose |
|---|---:|---|
| `max_concurrent_bidi_streams` | 100 | Peer-created request streams |
| `max_concurrent_uni_streams` | 16 | H3 control/QPACK headroom included |
| `stream_receive_window` | 256 KiB | Per-stream QUIC receive budget |
| `connection_receive_window` | 4 MiB | Per-connection receive budget |
| `send_window` | 4 MiB | QUIC outbound flow-control budget |
| `max_idle_timeout` | 60 s | QUIC idle timeout |
| `max_pending_handshakes` | 64 | Pending handshake task budget |
| `max_field_section_size` | 32 KiB | Decoded H3 field-section budget |
| `max_send_buf_size` | 256 KiB | H3 response send-chunk ceiling |
| `stateless_retry` | false | Optional retry-token policy |
| `advertise_alt_svc` | false | Runtime-owned TCP/H2 `Alt-Svc` advertisement |

The H3 adapter also applies shared aggregate header bytes, header count,
request-target bytes, request-body, handler/body/response-write timeouts,
service admission, and file-stream admission. A rough receive-memory ceiling
from the explicit transport windows is
`max_concurrent_bidi_streams * stream_receive_window`, capped by the
connection receive window for a single connection, plus the configured H3
unidirectional/control state. The separate connection semaphore bounds how
many such envelopes may exist concurrently.

## Canonical request and response adaptation

H3 pseudo-fields are consumed by h3 and become canonical method, target,
scheme, authority, and `HttpVersion::Http3`; pseudo-header names never reach a
service. `https` is required, connection-specific headers are rejected, `TE`
is accepted only for `trailers`, and `Content-Length` is strict and unique.
Request DATA is a pull-driven canonical `RequestBody`, so Buffer and Stream
policies preserve bounded memory and body lifecycle semantics. Under Reject,
H3 performs at most one bounded receive to distinguish immediate end-of-stream
from DATA when `Content-Length` is absent or zero; DATA is discarded and the
receive direction is cancelled without invoking the service. Reject and
body-limit failures send the canonical error, issue stream control, and leave
sibling QUIC streams alive. Declared-length under/overrun checks are owned by
the shared `RequestBody` consumer.

Responses pass through canonical normalization and the shared privacy policy.
H3 omits HTTP/1 framing and connection fields, sends bytes/files/known-length
streams/unknown-length streams under QUIC backpressure, splits writes at the
configured H3 send bound, and validates known stream lengths. Response trailers
(Plan 198) use stream-local terminal field sections (`send_trailers`) after data
completion under the same no-progress deadline, validated by the single canonical
`Trailers` validator; failures reset only the affected stream (siblings survive).
Producer waits
use a `response_write_timeout` absolute no-progress deadline and each send
call keeps its own bound (Plan 194): only non-empty production followed by
successful send re-arms the producer deadline, so slow-but-progressing
producers never spuriously time out while empty chunks cannot refresh the
budget. Producer silence or a stalled send resets only the affected request
stream with `H3_INTERNAL_ERROR` (siblings survive) and observes
`WriteStallTimeout`. A producer/trailer failure or stream-length mismatch uses the
same stream-scoped reset path. Request trailers (Plan 198) use a bounded
`recv_trailers` probe after DATA EOF with the same canonical validator
(stream-local, siblings survive). Interim 1xx are validated/recorded via the
shared `InterimSender` core; wire emission follows current `h3` server-API
support (no raw fallback). Runtime-owned `Alt-Svc`
uses the actual same-port listener and is suppressed when the response policy
denylist contains `alt-svc`.

## Observability and qualification

The adapter uses the runtime `OpsContext` for listener readiness, admission,
handshake failures/timeouts, max-request drain, body timeouts, response
write-stall timeouts, and shared
counters. Runtime-generated H3 errors use the same canonical status/reason/body
constructor as H1/H2, including HEAD, `Allow`, empty privacy policy, and
unassigned-status behavior. It does not log QUIC connection IDs, tokens, TLS
 secrets, packets, or raw request values. Plans 188, 190, 192, 193, 194, and
195 keep H3
experimental because the available qualification host had no direct H3 client,
second independent client, adversarial network environment, or non-Linux H3
runtime. The in-process H3 qualification now directly covers DATA without
`Content-Length`, zero-length declarations followed by DATA, bodyless
dispatch, bounded presence-probe timeouts, sibling survival, detached
 lifecycle wake-up after peer close, early-error stream scoping,
complete-response survival across an immediate peer close, stalled
response-producer timeout with sibling survival (Plan 194), and the Plan 195
shutdown-race drain plus write-stall observability/permit-release
regressions (H3 suite now 16 tests).

Deterministic local coverage lives in the `http3` feature tests:

```sh
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
cargo tree -p eggserve-core --no-default-features
bash scripts/qualify-http3.sh
```

The no-feature tree must not contain h3, h3-quinn, or Quinn. The manual
qualification script records dependency/client versions, proves same-port
Alt-Svc and TCP fallback, and runs direct H3 semantic checks when curl has
HTTP/3 support. Set `EGGSERVE_REQUIRE_H3_CLIENTS=1` or
`EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1` when a release environment must fail on
missing independent-client evidence. The complete decision and evidence
 boundary is in [`release/plan-188-http3-qualification.md`](../release/plan-188-http3-qualification.md);
the post-Plan-189 corrective evidence is in
[`release/plan-190-multiprotocol-corrective-qualification.md`](../release/plan-190-multiprotocol-corrective-qualification.md),
the dependency-readiness gate is in
 [`release/plan-192-http3-dependency-readiness.md`](../release/plan-192-http3-dependency-readiness.md),
 and the promotion-attempt outcome is in
[`release/plan-193-http3-supported-tier-qualification.md`](../release/plan-193-http3-supported-tier-qualification.md),
the producer-timeout correction is in
[`release/plan-194-http3-producer-timeout-correction.md`](../release/plan-194-http3-producer-timeout-correction.md),
and the corrective timeout/history qualification is in
[`release/plan-195-http3-response-timeout-corrective-qualification.md`](../release/plan-195-http3-response-timeout-corrective-qualification.md).

## Standards boundary

The ownership review uses [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html)
for common HTTP semantics, [RFC 9114](https://www.rfc-editor.org/rfc/rfc9114.html)
for HTTP/3 mapping and stream/control rules,
[RFC 9000](https://www.rfc-editor.org/rfc/rfc9000.html) for QUIC transport,
[RFC 9001](https://www.rfc-editor.org/rfc/rfc9001.html) for TLS 1.3/QUIC,
[RFC 9204](https://www.rfc-editor.org/rfc/rfc9204.html) for QPACK, and
[RFC 7838](https://www.rfc-editor.org/rfc/rfc7838.html) for Alt-Svc. EggServe
owns canonical semantics, aggregate limits, admission, body lifecycle,
response privacy, and shutdown policy. `h3`, Quinn, rustls, and the operating
system own wire parsing, QPACK encoding/decoding, packet recovery, TLS key
schedule, retry/amplification behavior, and path/MTU mechanics.

## Plan 192 dependency readiness (BLOCKED)

Plan 192 re-evaluated the frozen candidate (`h3` 0.0.8 / `h3-quinn` 0.0.10 /
Quinn 0.11.11 / rustls 0.23.x — the latest released versions; generic tunnel via plain `CONNECT` + h3-crate Extended `CONNECT` (`webtransport`/`connect-udp`); generic `:protocol` (e.g. `websocket`) blocked by `h3` 0.0.8
candidate exists) and closed with `BLOCKED`, leaving H3 experimental. The
full matrices and dispositions live in
[`release/plan-192-http3-dependency-readiness.md`](../release/plan-192-http3-dependency-readiness.md);
the durable contract points are:

- **Upstream blockers**: `hyperium/h3#338` (open; server HEADERS/DATA paths
  traverse the affected frame layer, no released fix) and the `hyperium/h3#262`
  remainder (three early-error paths now abort receive explicitly with
  `H3_REQUEST_CANCELLED`; the 503-admission, Buffer-error, and
  post-service unconsumed-body paths still drop receive without an explicit
  public-API abort). No fork or vendored patch was introduced.
- **QPACK/resources**: the request path uses stateless decode capped by
  `max_field_section_size` and responses use stateless encode, so no dynamic-table
  knob is owed; control-stream uniqueness and frame-parser correctness stay
  dependency-owned with adversarial proof deferred to Plan 193.
- **Lifetime**: QUIC idle (`max_idle_timeout`) plus per-operation/request
  deadlines is the documented H3 contract; `connection_total_timeout` does not
  bound QUIC connection lifetime (see `docs/timeout-reference.md`).
- **Tooling**: `scripts/qualify-http3.sh` reports every Plan 193 evidence
  class as PASS/SKIP (never mistaking unavailable for passed) and fails closed
  under `EGGSERVE_REQUIRE_ADVERSARIAL_H3`, `EGGSERVE_REQUIRE_H3_BROWSER`,
  `EGGSERVE_REQUIRE_H3_IMPAIRMENT`, and `EGGSERVE_REQUIRE_H3_PLATFORM`.
- **Plan 193 gate**: Plan 193 closed at preflight on 2026-09-10 without
  entering promotion qualification (unmet Plan 192 prerequisite). The pass
  inventoried the unchanged frozen candidate, re-checked `#338`/`#262` as
  still open, and recorded every mandatory external evidence class as
  unavailable on the execution host. See
  [`release/plan-193-http3-supported-tier-qualification.md`](../release/plan-193-http3-supported-tier-qualification.md).
  A future promotion requires a new scoped plan; Plan 193 is no longer an open
  promotion authority.
- **Plan 194 correction**: Plan 194 bounds the H3 `ResponseStream` producer
  poll with a `response_write_timeout` absolute no-progress deadline (only
  non-empty production followed by successful send re-arms it; empty chunks
   preserve the deadline; silence resets only the affected stream and observes
   `WriteStallTimeout`), correcting the Plan 192 Track F disposition that
   claimed existing bounds already covered the stall. H3 stays experimental;
   Plans 192/193 blockers stand. See
   [`release/plan-194-http3-producer-timeout-correction.md`](../release/plan-194-http3-producer-timeout-correction.md).
- **Plan 195 corrective qualification**: Plan 195 qualifies the Plan 194 bound
  without source change (stalled, progress-then-stall, slow-progress, and
  empty-chunk evidence plus new shutdown-race drain and write-stall
  observability/permit-release regressions; H3 suite 14 → 16). Tier and
  blockers unchanged. See
  [`release/plan-195-http3-response-timeout-corrective-qualification.md`](../release/plan-195-http3-response-timeout-corrective-qualification.md).
