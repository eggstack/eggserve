# Downstream Application Servers on EggServe

EggServe is not an application server. It is a hardened HTTP/static-serving
runtime with a public canonical service/connection substrate suitable for
downstream application-server implementations. This document explains how to
build the HTTP half of a real event-driven application server on that
substrate without importing Hyper internals, the Python compatibility facade,
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
- `Request` bundles `RequestHead` (method, target, version, headers),
  `RequestBody` (one-shot, bounded), `ConnectionInfo`, and a cloneable
  `RequestLifecycle` observer. Clone the lifecycle before moving the body.
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
`into_parts_with_lifecycle()`) is the transport-neutral observer. It fires
on peer disconnect, forced close, hard timeouts, shutdown past drain, and
body/transport failure — never merely on `Service::call` return, body EOF,
or normal response completion on keep-alive.

- Reasons are coarse and best-effort: `PeerDisconnected`,
  `ServerShutdown`, `ConnectionTimeout`, `TransportFailure`. The first
  reason wins. Downstream code must rely only on "no longer usable".
- A response producer may observe disconnect (stream poll/write failure or
  drop) before a waiter observes `cancelled()`; treat either path as
  cancellation. There is no second HTTP error response after commitment.
- A long-polling task that is not polling body/response IO must wait on
  `cancelled()` rather than probing a raw socket.

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
framing. WebSocket/upgraded protocols require Plan 176 or downstream
protocol support; no upgrade work is part of this HTTP contract. Python
FFI/asyncio architecture belongs in the downstream project's repository.
Downstream gateways build on the canonical `Service` boundary instead
(see [extension-contract.md](extension-contract.md) and
[non-goals.md](non-goals.md)).
