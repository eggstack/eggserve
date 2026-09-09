# HTTP/3 and QUIC transport boundary

EggServe's native HTTP/3 path is an opt-in Rust feature (`http3`). It is an
experimental transport adapter, not a change to the Python compatibility
surface or to the static service planner. The implementation uses `h3` with
`h3-quinn` and Quinn over Tokio; those dependencies are absent from the
default, HTTP/1, and HTTP/2 graphs.

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
the failed startup. `from_listener(TcpListener)` uses the supplied listener's
resolved address for the UDP bind; caller-owned Quinn endpoints are not part
of this initial surface. H3 intentionally requires the accompanying TCP
listener so `ServerHandle::local_addr()` remains a truthful origin address.

The shared `max_connections` semaphore covers accepted TCP and QUIC
connections. QUIC handshakes additionally consume the H3
`max_pending_handshakes` budget until handshake completion. Shutdown closes
the endpoint, sends H3 GOAWAY through the h3 connection driver, drains active
request tasks up to the common graceful deadline, and then aborts remaining
tasks.

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
policies preserve bounded memory and body lifecycle semantics. Reject and
body-limit failures send the canonical error, issue stream control, and leave
sibling QUIC streams alive.

Responses pass through canonical normalization and the shared privacy policy.
H3 omits HTTP/1 framing and connection fields, sends bytes/files/known-length
streams/unknown-length streams under QUIC backpressure, splits writes at the
configured H3 send bound, and validates known stream lengths. A producer
failure, stream-length mismatch, or per-stream write timeout resets only the
affected request stream after response commitment. Runtime-owned `Alt-Svc`
uses the actual same-port listener and is suppressed when the response policy
denylist contains `alt-svc`.

## Observability and qualification

The adapter uses the runtime `OpsContext` for listener readiness, admission,
handshake failures/timeouts, max-request drain, and shared counters. It does
not log QUIC connection IDs, tokens, TLS secrets, packets, or raw request
values. H3 is classified as experimental until Plan 188 supplies independent
client interoperability, adversarial QUIC/resource evidence, platform
results, and a support-tier decision.

Deterministic local coverage lives in the `http3` feature tests:

```sh
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
cargo tree -p eggserve-core --no-default-features
```

The no-feature tree must not contain h3, h3-quinn, or Quinn. Independent
wire-client interoperability and adversarial transport qualification are
deliberately deferred to Plan 188.
