# Plan 204 — Async Python Application-Server Substrate

## Status

**PLANNED.** Prerequisites: Plan 197; Plan 198 for trailers/interim semantics; Plan 199 for WebSocket/tunnel capability. Plan 200 is useful but not required.

## Purpose

Provide a low-level asynchronous Python bridge over EggServe's canonical runtime that is sufficient to implement an ASGI-class application server downstream without routing requests through the synchronous `http.server` compatibility callback machinery.

EggServe does **not** become the maintained ASGI server in this plan. The implementation target is a safe native/Python boundary with bounded event transfer, cancellation, streaming, and tunnel ownership. A small ASGI fixture is required for qualification because it is the most useful concrete consumer of the boundary.

The existing `eggserve.server` six-class compatibility facade remains synchronous and HTTP/1.1-shaped. Do not mutate it into an async API.

## Reference consumer requirements

ASGI 3 uses one async application callable and its HTTP/WebSocket sub-specification represents incremental request content, response-start/body events, disconnects, WebSocket connection/data/close events, and optional response trailers. Those event names belong to the downstream adapter, but EggServe's Python primitives must be expressive enough to map them without buffering or fabricating transport state.

The bridge must support:

- async request dispatch without holding the GIL across Rust network waits;
- incremental request body with bounded backpressure;
- early final response while a delegated request body may still be consumed;
- incremental response body;
- request/response trailers;
- peer disconnect/reset/server-shutdown notification;
- generic tunnel/WebSocket handoff from Plan 199;
- H1/H2/H3 request metadata truthfully where the native runtime supports each protocol;
- verified TLS/proxy metadata from Plans 202–203 when enabled.

## Track A — Choose the Python/Rust async integration model

Do not hand-roll an asyncio executor inside PyO3 callbacks without first evaluating current PyO3 async integration options and compatibility with the package's ABI baseline.

The bridge needs two directions:

1. Rust runtime schedules/invokes a Python coroutine on the intended Python event loop;
2. Python awaits bounded native receive/send operations whose progress is driven by the Rust/Tokio runtime.

Selection criteria:

- no GIL held during socket/body waits;
- no unbounded cross-runtime queue;
- cancellation propagates both ways;
- shutdown can join/cancel bridge tasks deterministically;
- Python event-loop object ownership is explicit and tied to server lifetime;
- no assumption that arbitrary Python callbacks may be invoked safely from any Tokio worker thread;
- CPython ABI/wheel support remains compatible with project policy.

If an async PyO3 helper crate is needed, feature-gate it within the Python package and pin/audit it deliberately. Do not add Python async dependencies to `eggserve-core`.

## Track B — New low-level namespace/API

Keep async server primitives under `eggserve.lowlevel` or a clearly separate advanced namespace, not `eggserve.server` compatibility classes.

A conceptual API:

```python
server = eggserve.lowlevel.AsyncServer(config, app_handler)
await server.start()
...
await server.shutdown()
```

The handler receives a request object exposing metadata plus async body/cancellation primitives and returns/uses a response sender. Exact API may be callback-oriented or event-channel-oriented, but it must map one-to-one onto the canonical native ownership model.

Do not expose raw Python sockets for ordinary HTTP requests.

## Track C — Request metadata projection

Expose immutable/frozen request metadata with byte fidelity:

- method;
- raw target/path/query views where canonical runtime can provide them truthfully;
- ordered duplicate-preserving headers as bytes;
- HTTP version;
- canonical authority/scheme;
- immediate/effective connection metadata with provenance;
- TLS metadata;
- per-request lifecycle/disconnect observer.

Avoid eagerly decoding headers/paths to Python `str`. Provide explicit convenience decoding only where semantics are defined.

## Track D — Bounded request-body bridge

Implement async incremental receive over the native one-shot `RequestBody`.

Requirements:

- small bounded channel or direct future bridge; capacity justified and tested;
- Rust stops reading when Python is not consuming, preserving transport-level backpressure;
- runtime hard byte limit remains authoritative;
- Python cannot request a larger limit;
- body errors/cancellation map to stable Python exception/categories;
- request trailers become available after terminal body state;
- early response does not incorrectly mark a body abandoned when the Python app task legitimately retains ownership;
- application abandonment triggers existing safe close/reset semantics.

Never use `read_all()` as the hidden implementation for the streaming API.

## Track E — Response-start/body/trailer bridge

Python must be able to commit a final response and then stream body chunks without requiring the coroutine to finish first.

Use bounded native channels/awaitables. Required semantics:

- response status and ordered byte headers validated before commitment;
- runtime owns transfer framing/Content-Length reconciliation;
- after final response commitment, Python exception/error cannot cause a second HTTP response;
- response-body producer backpressure reflects Rust transport polling;
- disconnect/reset/shutdown wakes blocked Python send operations with a stable exception;
- body close optionally supplies terminal trailers under Plan 198;
- HEAD/body-forbidden responses never advance Python body iterators/producers unnecessarily;
- unknown-length response is allowed without Python setting transfer coding.

## Track F — Interim responses

Expose Plan 198's bounded interim-response capability to the low-level handler.

Do not use ASGI names in the native extension API. A downstream adapter can map `http.response.early_hint`/other extensions if appropriate.

The bridge enforces 1xx-only status and commitment ordering natively; Python cannot bypass it.

## Track G — Generic tunnel/WebSocket bridge

Expose Plan 199 tunnel acceptance as a low-level async duplex object.

Requirements:

- one-shot acceptance;
- async `recv`/`send` or stream-like methods with bounded backpressure;
- close/cancel semantics map to native tunnel lifecycle;
- H1 Upgrade and H2/H3 Extended CONNECT present the same downstream Python capability where semantically possible;
- Python never receives Hyper/h2/h3/Quinn objects;
- no WebSocket frame parsing in EggServe production code.

A downstream/test ASGI adapter may use a maintained Python or Rust WebSocket codec, but it lives outside the core bridge.

## Track H — Application concurrency/admission

Do not equate EggServe's pre-response `max_in_flight_requests` permit with Python application lifetime. Async app tasks may outlive response-start.

Add an explicit bounded Python application-task admission limit owned by the Python async server adapter, with deterministic overload behavior. This may default to a safe value derived from existing Python runtime configuration but must be separately named/documented.

Requirements:

- no unbounded task creation;
- permit held for actual app task lifetime;
- disconnect/cancel/shutdown returns permit exactly once;
- multiplexed H2/H3 requests can execute concurrently up to configured application/runtime ceilings;
- one slow Python request does not serialize all requests behind the GIL or a global mutex.

## Track I — Lifecycle and server ownership

A Python `AsyncServer` instance owns:

- its native server/runtime handle or a clearly documented shared runtime;
- a specific asyncio event loop association;
- active app task registry;
- bounded bridge channels/resources;
- startup/readiness and graceful shutdown.

Define behavior for:

- server constructed outside a running loop;
- `start` called on the wrong loop;
- event loop closed while server is running;
- Python cancellation of the server task;
- interpreter shutdown/finalization;
- object garbage collection without explicit shutdown.

Prefer explicit errors and deterministic shutdown over destructor-driven network cleanup.

## Track J — ASGI qualification fixture

Build a **test/example adapter**, not the production product, that maps the low-level bridge to ASGI 3 HTTP/WebSocket semantics.

Qualification cases:

- GET/POST buffered and streaming;
- large streamed upload under backpressure;
- early response while upload continues;
- duplicate/opaque header bytes;
- H1/H2 requests through the same ASGI callable;
- H3 HTTP where Python bridge protocol support is enabled;
- response streaming/SSE-like long poll;
- disconnect notification while app is idle;
- request/response trailers extension where supported;
- WebSocket echo over H1 and Extended CONNECT protocols available from Plan 199;
- app exception before and after response-start;
- app cancellation/server shutdown;
- concurrency saturation/recovery.

Do not implement worker processes, reloaders, lifespan ownership, router/framework loading, or Gunicorn integration here. A downstream real ASGI server owns those.

## Track K — GIL/performance evidence

Add measurements to catch pathological bridge behavior, not absolute CI thresholds:

- requests/sec/latency for trivial async handler compared with synchronous callback and native Rust fixture;
- streaming throughput with small/large chunks;
- CPU/GIL contention with concurrent H2 streams;
- bounded memory under slow Python consumer/producer;
- task/thread count per server.

The acceptance requirement is architectural: GIL is not held across network waits and memory remains bounded. Do not chase native-Rust throughput parity at the cost of correctness.

## Security verification

- malicious Python response headers cannot bypass canonical framing/denylist;
- Python cannot increase native body/resource ceilings;
- dropped Python awaitables/tasks release native ownership;
- exceptions cannot leak Rust/Python internal text to clients;
- tunnel capability cannot be cloned/reused;
- no untrusted request metadata is mislabeled as verified TLS/proxy data;
- cross-loop/cross-thread misuse fails safely;
- shutdown cannot leave orphan Python app tasks or native body streams.

## Acceptance criteria

- [ ] an async Python handler can process requests without the synchronous `http.server` callback path;
- [ ] no GIL is held across ordinary network/body waits;
- [ ] request and response streams are genuinely incremental and bounded;
- [ ] disconnect/cancellation propagates both Rust->Python and Python->Rust;
- [ ] trailers/interim responses/tunnels project without raw transport access;
- [ ] Python application-task concurrency is separately bounded from EggServe pre-response admission;
- [ ] one event loop/server ownership model is documented and misuse is deterministic;
- [ ] a test/example ASGI adapter demonstrates HTTP and WebSocket-class sufficiency without making EggServe the maintained ASGI server;
- [ ] H1/H2/H3 metadata semantics are truthful and protocol limitations explicit;
- [ ] `eggserve.server` synchronous compatibility surface remains intact.

## Handoff

Plan 207 treats the async Python/ASGI fixture as a qualification consumer. A separate downstream repository can then implement a maintained ASGI server with lifespan/workers/reload/framework integration.