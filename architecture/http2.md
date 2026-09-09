# HTTP/2 Conformance and Release Boundary

EggServe's native HTTP/2 path is an opt-in Rust feature (`http2`). It uses
Hyper's HTTP/2 server driver; EggServe does not maintain a second H2 protocol
stack. The Python compatibility facade and the default CLI build remain
HTTP/1.1-shaped. HTTP/3 has its own QUIC boundary in
[`http3.md`](http3.md).

## Protocol ownership

| Behavior | Owner | EggServe qualification |
|---|---|---|
| Frame parsing, HPACK, stream state, pseudo-header ordering, protocol errors, stream windows, reset accounting | Hyper/h2 | H2 feature integration tests and release wire script |
| H2 stream/header/frame/window/send-buffer/reset budgets | `Http2Config`, projected to Hyper explicitly | Configuration validation tests |
| Canonical method, target, authority, scheme, version, and duplicate ordinary headers | EggServe request adapter | `http2_runtime.rs` |
| Aggregate decoded header bytes and request-target ceiling | EggServe request adapter | Pre-service rejection tests |
| Response normalization, privacy, body-forbidden statuses, content length, and no `Transfer-Encoding` | Canonical response pipeline | H2 response tests and shared normalization tests |
| Service admission, file-stream admission, body policy, timeouts, lifecycle, and shutdown | EggServe runtime | Shared lifecycle and H2 integration tests |
| TLS certificate handling and ALPN offer/selection | rustls plus EggServe TLS/accept loop | H2+TLS tests and wire script |

Hyper's H2 parser rejects malformed pseudo-header order, duplicate singleton
pseudo-fields, forbidden connection-specific fields, invalid content lengths,
and invalid frame/state transitions before service invocation. EggServe does
not log raw pseudo-header values or hostile targets. HTTP/1 transfer-framing
validation is deliberately skipped for H2; H2 has no `Transfer-Encoding` wire
framing.

## Implemented behavior checklist

- H2 over TLS is selected by ALPN `h2`; H1-only TLS advertises only
  `http/1.1`, and an H2-enabled listener offers `h2` before `http/1.1`.
- Cleartext H2 is prior-knowledge only. The bounded preface classifier does
  not implement HTTP/1 `Upgrade: h2c`.
- Multiple streams share one connection and one canonical service pipeline.
  An ordinary rejected request body is finalized without an H1
  `Connection` header, allowing Hyper to reset that H2 stream while siblings
  continue.
- Reject-body presence is protocol-aware: a positive declared length still
  rejects early, while a missing/zero `Content-Length` uses Hyper's public
  `Incoming::is_end_stream()` state. DATA without that header is rejected
  before service invocation; an already-ended stream is dispatched as an
  empty body. H2 has no `Transfer-Encoding` body-presence signal.
- `Http2Config` defaults are explicit: 100 concurrent streams, 32 KiB
  decoded header list, 16 KiB maximum frame, 256 KiB stream receive window,
  1 MiB connection receive window, 256 KiB per-stream send buffer, 1024 local
  reset states, 20 pending peer-reset states, fixed windows, and disabled
  keepalive PINGs. Each scalar is validated before runtime use.
- `max_in_flight_requests` and `max_file_streams` remain server-wide
  application/resource budgets; idle H2 streams do not acquire either permit.
- H2 response activity is tracked per request stream at the application-body
  poll boundary. Hyper's public server API does not expose a safe stream-reset
  or wire-progress handle from the response-body adapter, however. If a
  producer stalls beyond `response_write_timeout`, EggServe cancels live
  lifecycles and uses the conservative bounded connection shutdown fallback.
  A sibling's socket writes do not refresh the stalled producer timestamp;
  bytes already handed to Hyper are not claimed to have made wire progress.
- Graceful shutdown and `max_requests_per_connection` use the H2 driver's
  graceful shutdown/GOAWAY path. HTTP/1 alone receives `Connection: close`.
- Responses never originate server push, trailers, extended CONNECT, or
  WebSocket framing. A canonical `101` upgrade handoff is not available.
  HTTP/1 Upgrade headers remain ordinary unsupported application behavior and
  cannot bypass canonical response normalization.

## Release qualification status

Plans 186 and 190 close with H2 classified as **experimental**. The
deterministic feature suite, targeted DATA-without-`Content-Length` Reject
regression, local TLS ALPN/static/range/conditional/multiplexed checks, and
cleartext prior-knowledge checks pass on Linux x86_64. The manual script is:

```sh
bash scripts/qualify-http2.sh
```

It records the installed `curl`/nghttp availability through its output and
checks H2 and H1 negotiation, static responses, range and conditional
responses, parallel streams, forbidden response framing, cleartext prior
knowledge, and the absence of Upgrade-based h2c. Broad browser, second-client,
and macOS/Windows native-H2 runtime evidence remains release qualification
work; the response-stall fallback is also a deliberate hardening limitation.
Those gaps prevent a general “supported” declaration. See the original
[`release/plan-186-http2-qualification.md`](../release/plan-186-http2-qualification.md)
and the corrective
[`release/plan-190-multiprotocol-corrective-qualification.md`](../release/plan-190-multiprotocol-corrective-qualification.md)
for the captured evidence and exact decisions.

## Non-goals

This boundary does not add server push, proxying, middleware, routing,
upgrades, WebSockets, trailers, or HTTP/3. A reverse proxy may terminate H2
and forward H1 to EggServe; native H2 is available only when an operator
builds/enables the Rust feature and accepts its experimental status.
