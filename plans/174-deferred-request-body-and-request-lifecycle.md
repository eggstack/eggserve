# Plan 174 — Deferred Request-Body Ownership and Request Lifecycle Signaling

## Status

**IMPLEMENTED / CLOSED.**

Prerequisites: Plan 172 present. Plan 173 closed (octet-preserving metadata).

## Closure record

Tracks A–F implemented on `main`:

- Phase 0 spike proved Hyper 1.11 supports overlapping request consumption/response production (early response before EOF, body completes later, keep-alive reuse; abandonment closes without parsing trailing bytes).
- Track A: `Arc<AtomicBool>` replaced by `RequestShared` (Active/Complete/Abandoned/Failed + cancellation, one allocation, `Notify` for body/cancel observers; EOF validates framing, transport error marks Failed, Drop marks Abandoned for network bodies only).
- Track B: ownership-derived delegation (move keeps Active, Drop marks Abandoned); reuse waits for both boundaries via Hyper-pinned behavior + `deferred` activity accounting (idle excludes deferred); abandonment forces `Connection: close` at return and Hyper close after.
- Track C: Stream `Service::call` stays collapsed as `min(body, handler)` for compat (disambiguated via lifecycle); remaining `body_read_timeout` continues after response-start via watchdog (Failed + cancel + driver close). Documented as compatibility-preserving split in `docs/timeout-reference.md`.
- Track D: public `RequestLifecycle` (`cancelled()`, `is_cancelled()`, `cancellation_reason()`; PeerDisconnected/ServerShutdown/ConnectionTimeout/TransportFailure, first wins) via `Request::lifecycle()`/`into_parts_with_lifecycle()` (additive). Driver cancels on ClientError/Shutdown/Timeouts; body failure cancels; send-side race preserved.
- Track E: response-stream drop + lifecycle compose (Hyper close wakes polls); EggServe owns no downstream tasks (reference consumer in Plan 175 proves composition).
- Track F: `max_in_flight_requests` bounds pre-response `Service::call` only (released at `finish`); downstream owns app-task budget (documented, tested).
- Observability: `deferred_body_delegated/completed/abandoned/timeout`, `request_lifecycle_peer_disconnect/runtime_cancel` counters + events (plus legacy `body_read_timeouts`).
- Verification: `tests/deferred_lifecycle.rs` (9 TCP + TLS parity + duplex parity + admission) pins all orderings, disconnect/shutdown/timeout, and permit recovery.

## Goal

Allow a downstream event-driven application server to safely return an HTTP response while a downstream-owned task still legitimately consumes the request body, and expose transport-neutral request cancellation/disconnect signaling for long-lived application work.

The design must retain EggServe's HTTP/1 safety invariant: unread request-body bytes must never be mistaken for a subsequent request. A response may begin before request-body EOF only when ownership is explicit and the runtime can prevent unsafe connection reuse.

This is generic HTTP lifecycle work. Do not add ASGI `receive()`/`send()` messages, Python asyncio primitives, WebSocket state, or framework-specific cancellation objects to EggServe.

## Current behavior and why it is insufficient

For `RequestBodyPolicy::Stream`, the connection pipeline currently:

1. wraps Hyper's incoming body in canonical `RequestBody`;
2. clones an internal `consumed_flag`;
3. invokes `Service::call(request)` under `min(body_read_timeout, handler_timeout)`;
4. when `Service::call()` returns, immediately checks the flag; and
5. if body EOF has not already occurred, adds `Connection: close`.

That behavior is conservative and correct for a conventional handler whose return means request processing is complete.

An event-driven app-server bridge has a different ownership shape. A generic implementation may need to:

```text
Service::call(Request)
  -> move RequestBody into app task
  -> app sends response-start
  -> Service returns ResponseBody::Stream
  -> app task continues:
       receive request body
       produce response body
       observe disconnect
  -> body/app task finishes
```

At the instant `Service::call()` returns, the request body can be incomplete but not abandoned. Treating those states as identical forces connection close and can race/cancel the continuing body consumer.

The public API also lacks a request-scoped signal that becomes ready when the peer disconnects or the runtime cancels the request/connection. A long-polling/SSE-style application should not have to poll the request body or infer cancellation solely from response-stream drop.

## Design invariants

The final design must distinguish at least these states:

```text
Unread/active request body owned by Service or delegated task
Fully consumed request body
Explicitly abandoned/dropped request body
Transport disconnected
Runtime cancelled/shutdown/timed out
Response completed
```

The implementation need not expose this exact enum publicly, but the runtime must have enough state to make correct decisions.

Core invariants:

- only one logical consumer may read a `RequestBody`;
- request-body byte/timeout policy continues to apply after ownership is delegated;
- `Service::call()` completion must not automatically imply request-body abandonment;
- connection reuse must not occur until the body framing boundary is known complete;
- explicit abandonment before EOF forces a safe HTTP/1 close;
- peer disconnect/runtime cancellation wakes downstream waiters even if they are not polling request/response IO;
- cancellation is idempotent and race-safe;
- no detached application task may retain an EggServe connection forever after response/shutdown;
- all permits/guards/resources recover on every terminal path.

## Phase 0 — Real transport concurrency spike

Before designing public API, prove what Hyper 1.11 permits on the current server path.

Build a focused internal experiment/test where a service:

1. receives a streaming POST body;
2. moves `RequestBody` to a spawned Tokio task;
3. returns a streaming response after only the first request chunk or before request EOF;
4. continues reading the request body while the response is written; and
5. completes both directions successfully.

Exercise:

- fixed `Content-Length` request bodies;
- chunked request bodies;
- client that uploads slowly while reading response;
- client that stops uploading after response-start;
- client disconnect during overlap;
- TLS path;
- caller-owned `AsyncRead + AsyncWrite` transport.

The purpose is not to guarantee arbitrary HTTP full-duplex behavior beyond Hyper's contract. It is to establish whether EggServe can safely support overlapping request consumption/response production without a custom parser/connection engine.

If Hyper cannot support the required ownership shape reliably, stop and revise this plan. Do not add a public deferred-body API that is not backed by real transport behavior.

## Track A — Replace the boolean consumption model with lifecycle state

The current `Arc<AtomicBool>` is sufficient only for “EOF happened before service return.” Replace or wrap it with an EggServe-owned internal state object capable of distinguishing completion from abandonment and active delegated ownership.

A possible internal shape:

```rust
enum BodyLifecycleState {
    Active,
    Complete,
    Abandoned,
    Failed,
}

struct BodyLifecycle {
    state: AtomicU8,
    notify: Notify,
}
```

Exact implementation is flexible. Requirements:

- one allocation/Arc per streaming request is acceptable; avoid multiple coordination objects if one can serve body completion and cancellation needs;
- transition ordering is explicit and tested;
- EOF marks `Complete` only after declared-length/framing validation succeeds;
- transport/body error marks a terminal failure state;
- dropping an incomplete body marks `Abandoned` unless ownership has been transferred into an explicit continuation object that still contains the same body;
- state observers do not require holding the `RequestBody` itself.

Do not infer abandonment merely from `Service::call()` returning.

## Track B — Define delegated request ownership

### B1. Prefer ownership-derived semantics over manual flags

The best API is one where moving `RequestBody` into a task naturally keeps it active and dropping it naturally marks abandonment. Avoid an unsafe-looking `request.keep_body_alive(true)` flag that can outlive the actual consumer.

Investigate whether lifecycle state embedded in `RequestBody` plus Drop is sufficient:

- runtime retains a lifecycle observer;
- service owns/moves the actual `RequestBody`;
- returning the `Response` does not force a decision while the body state is still `Active`;
- runtime/connection keeps the request framing lifecycle associated until `Complete`, `Abandoned`, or transport termination.

If an explicit guard is required, use an EggServe-owned RAII type such as `RequestBodyLease`/`RequestContinuation` whose ownership is coupled to the body consumer. Do not expose internal Hyper `Incoming`.

### B2. Safe connection reuse

For HTTP/1.1, body completion and response completion may occur in either order. Connection reuse must wait for both the request message boundary and response message boundary.

Test all orderings:

```text
request body complete -> response complete
response complete -> request body complete
request abandoned -> response completes -> connection closes
peer disconnect -> both sides cancelled
shutdown -> both sides cancelled/drained according to policy
```

Do not add `Connection: close` merely because response-start became available first. Add/force close when body ownership is abandoned/failed or another runtime policy requires close.

If Hyper already prevents next-request parsing until incoming body completion, rely on that behavior only after pinning it with deterministic regression tests and documenting the assumption. EggServe should not duplicate Hyper's internal state machine unnecessarily.

## Track C — Separate response-start deadline from request-body deadline

Current Stream mode runs the whole `Service::call()` under `min(body_read_timeout, handler_timeout)`. Once request-body consumption may continue after `Service::call()` returns, the deadlines must be explicit.

Target semantics:

- `handler_timeout` (or a renamed/deprecated successor if needed) bounds time until the service produces an HTTP response object/response-start capability;
- `body_read_timeout` continues to bound request-body progress/consumption according to the existing documented semantics, even after response-start;
- response stream production is governed by connection/write progress and downstream policy, not retroactively by `handler_timeout`;
- hard connection lifetime/shutdown remain outer bounds.

Do not collapse body and handler timeouts merely for implementation convenience once their lifetimes can diverge.

If preserving the old Stream timeout behavior is required for compatibility, introduce an explicit runtime mode or deprecation path rather than silently changing the existing field semantics.

## Track D — Request-scoped disconnect/cancellation observer

Expose a small transport-neutral public primitive associated with each canonical `Request`.

Possible shape:

```rust
pub struct RequestLifecycle { /* opaque */ }

impl RequestLifecycle {
    pub async fn cancelled(&self);
    pub fn is_cancelled(&self) -> bool;
    pub fn cancellation_reason(&self) -> Option<RequestCancellationReason>;
}
```

or an equivalent cloneable token/future.

`Request` could expose:

```rust
pub fn lifecycle(&self) -> &RequestLifecycle;
```

and include it in `into_parts()`.

### D1. Reason taxonomy

Keep reasons small and transport-neutral. Candidate public reasons:

- `PeerDisconnected`;
- `ServerShutdown`;
- `ConnectionTimeout`;
- `RequestCancelled`/`TransportFailure` if a generic remainder is needed.

Do not promise overly precise classification if Hyper/IO cannot reliably distinguish TCP reset, EOF, TLS close, timeout, and local cancellation. It is better to expose `Cancelled` plus best-effort reason than a misleading taxonomy.

ASGI-style adapters need only know that the request/connection is no longer usable; detailed WebSocket close codes are outside this plan.

### D2. Trigger semantics

The lifecycle token must become cancelled on:

- peer transport loss;
- runtime-forced connection close;
- hard connection timeout;
- shutdown cancellation after graceful-drain policy reaches the relevant terminal point;
- body/transport failure that makes further application IO impossible.

It must not fire merely because:

- `Service::call()` returned a streaming response;
- the request body reached EOF normally;
- the response body completed normally while the keep-alive connection remains valid.

If applications need a distinct “response completed” signal, keep that separate; do not overload disconnect.

### D3. Send-side failure race

A response producer may discover disconnect before a waiting application task observes `RequestLifecycle::cancelled()`. Preserve this race rather than imposing expensive synchronization.

Document that:

- response stream poll/write failure/drop can occur first;
- lifecycle cancellation follows promptly;
- downstream adapters should make send operations fail once either path establishes cancellation.

This maps naturally to event-driven server semantics without naming ASGI.

## Track E — Response-stream/application-task ownership

A downstream app server will commonly back `ResponseStream` with a bounded channel fed by an application task. EggServe must make teardown deterministic.

Required behavior:

- dropping the transport response stream on client disconnect drops/closes the consumer end promptly;
- downstream producer observes that closure without unbounded queueing;
- request lifecycle cancellation is also available to stop work that is blocked somewhere other than response send;
- a delegated `RequestBody` observes transport cancellation and exits;
- shutdown cannot leave a detached producer/body task indefinitely alive solely because it retains canonical objects.

EggServe should not own or join arbitrary downstream application tasks. Plan 175's reference consumer should prove that a downstream project can compose the primitives into leak-free task ownership.

## Track F — Service admission semantics

The existing in-flight-service semaphore bounds active `Service::call()` executions. Once an app-server bridge returns at response-start while an application task continues, that permit will naturally be released before the application's full lifetime ends.

Keep that behavior unless there is a compelling generic reason to change it.

Document the distinction:

- EggServe `max_in_flight_requests` / service admission bounds pre-response `Service::call()` execution;
- downstream app servers should own a separate bounded application-task/concurrency budget if their task outlives `Service::call()`;
- open connection and response write bounds remain EggServe-owned.

Do not stretch the existing service permit across arbitrary response streams; doing so would change resource semantics for native/static services and conflate protocol/runtime admission with application-worker policy.

## Observability

Add narrowly useful counters/events if the current ops model can represent them without exposing application details:

- request body delegated past service return;
- incomplete body abandoned -> connection close;
- deferred body completed after response-start;
- request lifecycle peer disconnect;
- request lifecycle runtime cancellation;
- deferred-body timeout.

Avoid high-cardinality task IDs or application-provided labels.

## Verification

Run the standard Rust/package suite plus focused lifecycle tests. At minimum:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test api_stability
cargo test -p eggserve-core --test response_streaming
cargo test -p eggserve-core --test transport_driver
cargo test -p eggserve-core --test lifecycle
cargo check -p eggserve-core --examples
cargo test --doc -p eggserve-core
bash scripts/verify-cargo-packages.sh --mode all
```

Use actual existing target names where they differ.

New deterministic integration cases must cover:

- response begins before fixed-length request EOF and body completes later;
- response begins before chunked request EOF and body completes later;
- successful keep-alive request after deferred body completion;
- body abandoned after response-start forces close and never parses trailing bytes as a request;
- peer disconnect wakes lifecycle waiter with no body polling;
- peer disconnect while request body task is blocked;
- peer disconnect while response stream producer is active;
- body timeout after response-start;
- response completion before request completion;
- request completion before response completion;
- shutdown during overlap;
- TLS parity;
- caller-owned IO parity;
- service/application admission permits recovered on all terminal paths.

A small loom/model test for lifecycle state transitions is appropriate if the repository already uses concurrency-model tooling; do not add a large new dependency solely for this plan unless ordinary deterministic tests cannot cover the race state machine.

## Compatibility and migration

`Request` and `RequestBody` are semver-considered primitives; `server` APIs are experimental.

Prefer additive changes:

- add lifecycle access to `Request` without changing normal handler usage;
- keep existing `RequestBody::read_all()` and `next_chunk()` behavior;
- change internal Drop/completion tracking transparently where possible.

If `Request::into_parts()` must gain a lifecycle value, avoid a silent tuple-arity source break. Prefer either:

- a new `into_parts_with_lifecycle()` while preserving the old method; or
- embedding lifecycle state in `RequestBody` plus a cloneable accessor obtained before deconstruction.

Any timeout semantic change requires explicit migration documentation and the appropriate pre-1.0 release classification.

## Acceptance criteria

- [ ] a real HTTP/1 integration test proves a service can return response-start while a downstream task continues consuming the request body;
- [ ] request-body limits and body-read timeout remain enforced during deferred consumption;
- [ ] service completion no longer automatically means incomplete-body abandonment;
- [ ] dropping/abandoning an incomplete body still forces safe connection close;
- [ ] a keep-alive connection is reusable after deferred body completion and response completion;
- [ ] no next request is dispatched before the previous request framing boundary is complete;
- [ ] a public transport-neutral request cancellation/disconnect observer exists;
- [ ] peer disconnect wakes an idle downstream waiter without requiring body polling;
- [ ] shutdown/timeouts propagate cancellation deterministically;
- [ ] response-stream drop and lifecycle cancellation compose without deadlock;
- [ ] service admission remains distinct from downstream application-task admission and is documented;
- [ ] TLS and caller-owned transport behavior matches TCP;
- [ ] all state/permit/task ownership tests pass under cancellation races;
- [ ] no Hyper/socket/ASGI/Python runtime type enters the canonical public contract.

## Non-goals

Do not add:

- ASGI message channels;
- Python event-loop integration;
- application-task executors or worker pools;
- WebSocket upgrades or framing;
- HTTP trailers;
- HTTP/2 multiplexing;
- request replay/body cloning;
- multiple simultaneous consumers of one request body;
- arbitrary server-side push;
- a public raw TCP stream on ordinary HTTP requests.

## Handoff

Plan 175 must validate this API from a separate consumer's point of view. The reference consumer should deliberately use the difficult ownership shape—request body moved to an application task, response returned at response-start, bounded response channel, independent disconnect waiter—so closure proves the abstraction rather than merely exercising conventional handlers.