# Downstream Application Servers on EggServe

EggServe is not an application server. It is a hardened HTTP/static-serving
runtime with a public canonical service/connection substrate suitable for
downstream application-server implementations. Plan 190 does not expand this
HTTP-only consumer boundary or add Python/H2/H3 adapter semantics; native H2/H3
remain transport qualification concerns for Rust consumers. This document
explains how to build the HTTP half of a real event-driven application server on
that substrate without importing Hyper internals, the Python compatibility facade,
or crate-private modules.

The reference qualification is
`crates/eggserve-core/tests/app_server_consumer.rs`: an external consumer
using only `eggserve_core::primitives` + `eggserve_core::server` plus
ordinary downstream dependencies (`tokio`, `bytes`, `futures-util`). It is a
consumer test, not a maintained second server product, and not an ASGI/WSGI
implementation.

## Canonical architecture

```text
EggServe Service::call(Request)
       |
       +--> app task owns RequestBody + RequestLifecycle
       |         |
       |         +--> bounded request/event adaptation
       |         +--> produces response-start
       |
       +<-- response-start
       |
       +--> return ResponseBody::Stream
                 |
                 +<-- bounded response chunks from app task
```

EggServe owns HTTP transport semantics and hardened runtime policy: parsing,
framing, limits, timeouts, connection reuse, response normalization, and the
final privacy boundary. The downstream server owns application protocol
adaptation, event-loop integration, worker strategy, routing, and language
FFI. ASGI lifespan, worker processes, reloaders, and framework loading are
downstream responsibility.

## Service / Request / Response ownership

- `Service: Send + Sync + 'static` receives a canonical `Request` by value
  and returns a canonical `Response`. No Hyper type appears in the trait.
  Plan 197 keeps this shape deliberately: no `ServiceOutcome` exists.
  Plan 198 implements trailers in the message-body abstraction
  (`ResponseStream::with_trailers`) and interim via the request-scoped
  `InterimSender`; accepted-tunnel outcome deferred to Plan 199 only if needed.
  Ordinary services convert via `Ok(Response)`; `service_fn` stays simple.
- `Request` bundles `RequestHead` (method, target, version, headers),
  `RequestBody` (one-shot, bounded, with terminal `trailers()` /
  `read_all_with_trailers()`), and a typed `RequestContext`
  (Plans 197–198). `Request::connection()` / `lifecycle()` forward to
  the context for the Plan 175 common path; new code should prefer
  `Request::context()`, `into_parts_with_context()`, and
  `Request::new_with_context()` when threading metadata + cancellation
  together. Cloning the context never clones the one-shot body.
- `RequestContext` is the single deliberate attachment point for
  transport-authenticated metadata and opaque capabilities. It owns
  `ConnectionInfo` + `RequestLifecycle` + bounded `InterimSender` (`interim()`);
  tunnel capabilities (Plan 199) attach there when that plan lands. There is no generic type map: downstream application state
  belongs in the service wrapper, and Tower/framework extension maps belong
  in the Plan 200 adapters. No raw socket, Hyper, H2/H3, rustls-session, or
  executor handle is exposed here.
- `ConnectionInfo` / `TlsInfo` come from the observed transport or the
  explicit caller-owned `ConnectionContext`. `Forwarded` / `X-Forwarded-*`
  stay ordinary untrusted headers and never populate the context.
- `RequestBodyPolicy::Stream { max_bytes }` selects deferred ownership.
  The runtime enforces the hard `max_request_body_bytes` ceiling; services
  may only lower it.
- Responses are built with `Response::builder()` and returned as
  `ResponseBody::Stream(ResponseStream::new(..))` (chunked) or
  `with_known_length(..)` (`Content-Length`). The runtime is the only
  framing authority: never emit `Transfer-Encoding` or framing headers from
  the service. `normalize_response` drops HEAD/body-forbidden streams
  without polling.

Streaming request and response example (shape, not a framework):

```rust,no_run
use bytes::Bytes;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::response_stream::ResponseStreamError;
use eggserve_core::primitives::ResponseStream;
use eggserve_core::server::{Service, ServiceError};

struct Bridge;

impl Service for Bridge {
    fn request_body_policy(
        &self,
        _head: &eggserve_core::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        // Never `read_all()` on this path; stream incrementally.
        RequestBodyPolicy::Stream { max_bytes: 1024 * 1024 }
    }

    fn call(
        &self,
        request: Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
    > {
        Box::pin(async move {
            let lifecycle = request.lifecycle_clone();
            let (_head, body) = request.into_head_and_body();
            let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
            tokio::spawn(async move {
                let mut body = body;
                // First chunk -> response-start would be signalled here;
                // remaining chunks continue after `Service::call` returns.
                while let Ok(Some(chunk)) = body.next_chunk().await {
                    tokio::select! {
                        _ = lifecycle.cancelled() => break,
                        res = tx.send(chunk) => {
                            if res.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            // In a full bridge, wait only for response-start, then return
            // the stream; the app task keeps producing after return.
            let stream = futures_util::stream::unfold(rx, |mut rx| async move {
                match rx.recv().await {
                    Some(chunk) => Some((Ok::<Bytes, ResponseStreamError>(chunk), rx)),
                    None => None,
                }
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(ResponseStream::new(stream)))
                .unwrap())
        })
    }
}
```

## Deferred body ownership rule

Moving `RequestBody` into a spawned task keeps it `Active`; dropping an
incomplete network-backed body marks it `Abandoned`. The runtime
distinguishes the two at `Service::call` return:

- `Active` (delegated): no forced close. Connection reuse waits for both
  the request framing boundary (body `Complete`) and the response boundary.
- `Abandoned` / `Failed`: safe `Connection: close`. Trailing upload bytes
  are never parsed as a subsequent request.
- In-memory (`Fixed`/`Empty`) bodies never force close.

A service may therefore return response-start while a downstream task still
legitimately consumes the request body. After both sides complete, the
HTTP/1 connection can handle another request when policy permits.

## Disconnect and cancellation semantics

`RequestLifecycle` (`Request::lifecycle()`, `lifecycle_clone()`,
`into_parts_with_lifecycle()`, `Request::context().lifecycle()`) is the
transport-neutral observer. It fires
on peer disconnect, forced close, hard timeouts, shutdown past drain, and
body/transport failure — never merely on `Service::call` return, body EOF,
or normal response completion on keep-alive.

- Reasons are coarse and best-effort: `PeerDisconnected`,
  `ServerShutdown`, `ConnectionTimeout`, `TransportFailure`. The first
  reason wins. The enum is `#[non_exhaustive]` (Plan 197 Track F):
  downstream code must match with a wildcard and rely only on "no longer
  usable".
- A response producer may observe disconnect (stream poll/write failure or
  drop) before a waiter observes `cancelled()`; treat either path as
  cancellation. There is no second HTTP error response after commitment.
- A long-polling task that is not polling body/response IO must wait on
  `cancelled()` rather than probing a raw socket.

## Commitment contract (Plan 197 Track D, normative)

A request passes through these stages in order; later stages never revisit
earlier ones:

1. **not started** — admission (`max_in_flight_requests`, 503 on
   exhaustion), body-policy selection, pre-service ceilings (414 target,
   431 header, 417 unknown `Expect`). No service code has run.
2. **interim metadata emitted** — bounded via `request.context().interim()`
   (`InterimSender`): only 1xx (no 101/body/trailers), bounded count/bytes,
   HTTP/1.0 suppressed, single 100, no post-commit. `ServiceError::rejected(1xx)`
   still collapses to 500; final responses never carry 1xx.
3. **final response head committed** — the service returned
   `Ok(Response)` and the runtime normalized it (hop-by-hop stripping,
   framing, Plan 165 privacy) and marked interim committed. This is the single commitment point.
4. **body streaming** — the runtime polls the `ResponseStream` producer
   with backpressure; `response_write_timeout` (no-progress) and the hard
   connection lifetime bound it. Empty chunks are skipped, not progress.
5. **terminal metadata/trailers emitted** — one terminal `Trailers` block via
   `ResponseStream::with_trailers` after data completion (no data after,
   `HEAD`/body-forbidden never poll, known length counts data only).
6. **complete / cancelled / failed** — normal completion releases permits
   and may keep the connection reusable; cancellation (peer/shutdown/
   timeout/transport) drops producers promptly; failure after commitment
   (including trailer producer failure) closes (H1) or resets the stream
   (H2/H3, siblings survive) with sanitized diagnostics only.
7. **transitioned into a non-HTTP tunnel where applicable** — deferred
   (Plan 176 deferred, Plan 199 owns the design). No tunnel outcome exists
   today; 101 handshakes cannot survive normalization.

What happens on races:

- service errors/panics **before** final commitment → sanitized runtime
  error response (`Minimal` fixed body or `Empty`; `HEAD`/body-forbidden
  empty; no detail leak);
- interim attempt **after** final commitment → `InterimError::AfterCommit`
  (fail closed, no wire bytes);
- response/trailer producer errors **after** commitment → transport close/reset,
  never a second HTTP error; `ResponseStreamError` display stays generic;
- request body still delegated (`Active`) after response-start → reuse
  waits for body `Complete`; `Abandoned`/`Failed` forces safe close;
  in-memory bodies never force close;
- peer disconnect/reset at any stage → lifecycle cancels (first reason
  wins); send-side failure may precede `cancelled()` — treat either as
  cancellation;
- shutdown/timeout racing service completion → `ServerShutdown` /
  `ConnectionTimeout` cancels the lifecycle; permits return on drop;
  graceful drain waits up to `graceful_shutdown_timeout`, then aborts.

There is never an attempt to synthesize a second HTTP error response
after final commitment.

## Concurrency and readiness (Plan 197 Track E, normative)

Native `Service` stays `Send + Sync + 'static` and is shared across
connection tasks. There is no `poll_ready` on the native trait: Tower
readiness belongs in the Plan 200 adapters. Native admission stays
runtime-owned and deterministic (`max_in_flight_requests` held across
`Service::call`, 503 on exhaustion, permit released at response-start).

## Timeout split

- `handler_timeout` bounds time until `Service::call` produces the
  response object (response-start), not downstream work after return.
- `body_read_timeout` continues to bound deferred request-body progress
  after response-start via a watchdog (failure cancels the lifecycle and
  closes the transport so pending polls wake).
- Response production is bounded by `response_write_timeout`
  (no-progress) and the connection by `connection_total_timeout` (hard
  ceiling). Do not reinterpret `handler_timeout` as a total
  application-coroutine deadline. Full semantics are in
  [timeout-reference.md](timeout-reference.md).

## Error taxonomy at the boundary (Plan 197 Track F, normative)

- Client-facing errors stay sanitized: fixed `<status> <reason>` or empty
  bodies, `HEAD`/body-forbidden empty, no application detail reflected.
- Service/library callers distinguish rejection (`ServiceError::rejected`
  preserves `200..=599`, `1xx`/out-of-range → 500), internal failure
  (`ServiceError::internal`), panic (`is_panic`), timeout (`is_timeout`,
  504), cancellation (`RequestLifecycle`, first reason wins), and
  committed-stream failure (transport close/reset, never a second HTTP
  error) where useful. `ServiceError` is a struct with a private kind so
  future categories do not break construction.
- `RequestBodyError`, `ServerError`, `RequestCancellationReason`, and
  `ConnectionOutcome` are `#[non_exhaustive]`: match with a wildcard arm.
  Transport-specific H2/H3 reset codes are not exposed to ordinary
  applications; any future tunnel API exposes a small generic
  close/cancellation reason without leaking implementation enums.

## Service admission vs downstream application admission

`max_in_flight_requests` (default 64, 503 on exhaustion) bounds concurrent
pre-response `Service::call` executions only; the permit releases at
response-start. A downstream server whose application task outlives
`Service::call` must own a separate bounded application-task semaphore.
Saturation maps deterministically in the downstream service (for example a
fixture 503) without changing core policy. Cancellation returns both
classes of permit. Neither queue may be unbounded.

## Bounded-channel requirement

Cross-thread/event-loop adapters must use bounded channels with small
capacities (the qualification fixture uses capacity 2) so backpressure is
real. Every send that could block must also watch `lifecycle.cancelled()`
so disconnect/shutdown promptly unblocks the bridge. Never hide an
unbounded queue in the adapter; EggServe itself contains none.

## Lifecycle and shutdown ordering

Graceful shutdown stops accepting, drains in-flight connections up to
`graceful_shutdown_timeout`, then aborts the remainder. Bridge tasks must
exit on `cancelled()` (reason `ServerShutdown`) and on response-stream
drop; EggServe does not join arbitrary downstream tasks. The qualification
covers shutdown while an upload is active, while waiting before
response-start, while streaming, and after response completion with
deferred body consumption still active.

## Byte-oriented header and target handling

- `HeaderValue` preserves validated field-value octets
  (`from_bytes`/`as_bytes()`; fallible `to_str()`). `HeaderBlock` preserves
  order and duplicates; `push_bytes(..)` forwards opaque values.
  `Display` is lossy diagnostic only.
- Responses emit duplicate and opaque byte headers through the same
  canonical `Response` without importing `http::HeaderValue` or Hyper.
- `RequestTarget::raw_bytes()` / `path_bytes()` / `query_bytes()` expose
  the accepted origin-form bytes. `/path` and `/path?` deliberately
  canonicalize identically (`query() == None`): omit optional downstream
  metadata rather than fabricating bytes the parser cannot truthfully
  provide.
- `ConnectionInfo.local_addr` / `remote_addr` are `Option<SocketAddr>`;
  caller-owned transports expose `None` (`without_socket_addrs`). TLS
  session metadata is caller-asserted `TlsInfo`, not a raw session object.

## What EggServe does not do

EggServe does not implement ASGI/WSGI/framework/process semantics:
application protocol adaptation, event loops, routing, middleware, worker
supervision, lifespan state machines, HTTP/2/3, trailers, or WebSocket
framing. Plan 176 closed as deferred: no generic HTTP upgrade handoff is
exposed (`Request` / `RequestContext` have no upgrade/tunnel capability,
`Service` returns `Response` only — Plan 197 Track C keeps this shape —
101 handshakes cannot survive normalization), so upgraded protocols
are not currently buildable on the canonical boundary and raw Hyper
`OnUpgrade`/`Upgraded` bypass is unsupported. No Tower `Service`,
`poll_ready`, routing, middleware, or worker semantics enter the native
contract (Plan 197 Track E); those belong in downstream adapters
(Plan 200). Python
FFI/asyncio architecture belongs in the downstream project's repository.
Downstream gateways build on the canonical `Service` boundary instead
(see [extension-contract.md](extension-contract.md) and
[non-goals.md](non-goals.md)).

The minimal native application-service demonstration is
`crates/eggserve-core/examples/application_service.rs` (buffered echo,
bounded streamed pipe, lifecycle long-poll, no static filesystem).
