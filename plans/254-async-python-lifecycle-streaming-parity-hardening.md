# Plan 254 — Async-Python lifecycle, admission, and streaming parity hardening

## Purpose

Strengthen deterministic correctness evidence for the existing
`eggserve.lowlevel.AsyncServer` bridge without redesigning it, changing its
public API, or turning EggServe into an ASGI runtime.

Planning baseline:

```text
0ee02acd69f1c63d32134f8265283fff04e4630c
```

The current async bridge is intentionally H1-only and implemented primarily in
Python over the synchronous native callback server:

```text
Rust/Tokio HTTP runtime
  -> synchronous PyO3 callback
     -> run_coroutine_threadsafe(...)
        -> asyncio application task
           -> optional bounded async response producer
              -> bounded asyncio queue
                 -> synchronous iterable bridge
                    -> native Response.stream(...)
```

The bridge already includes bounded application-task admission, producer
backpressure, timeout handling, task tracking, disconnect observation, interim
responses, and generic tunnel handoff. The maintenance risk is that these
semantics are partly orchestrated in Python and partly in Rust, so later
runtime changes can create subtle parity drift even when both layers remain
individually tested.

This plan makes that seam a first-class conformance boundary.

## Constraints

- Preserve every existing public `eggserve.lowlevel` class, method,
  constructor argument, property, and return behavior.
- Keep the async surface experimental and H1-only.
- Do not add a native async server API in this plan.
- Do not add ASGI/WSGI framework semantics, lifespan management, worker
  processes, reloaders, routing, middleware, or WebSocket framing.
- Do not expose raw sockets or Tokio/PyO3 runtime handles to Python.
- Keep network I/O in Rust/Tokio and application coroutine ownership in the
  user's asyncio loop.
- Preserve bounded queues/semaphores and fail-closed timeout/cancellation
  behavior.
- Production changes are permitted only for defects reproduced by this plan's
  deterministic tests.
- No performance redesign is authorized merely because a bridge uses
  `asyncio.to_thread` or `run_coroutine_threadsafe`.

## Contract to qualify

### Application-task admission

The native runtime acquires its pre-response in-flight request admission before
invoking Python. `AsyncServer` then uses `max_async_tasks` as a separate
application-task bound.

The async bridge must preserve:

- no unbounded queued app-task creation;
- fail-fast 503 when async application admission is exhausted;
- exactly one application permit per admitted async request;
- ordinary buffered responses release the permit after handler completion;
- streaming responses transfer permit ownership to the producer and release it
  only when the producer terminates;
- tunnel/SSE-style long-lived work retains ownership according to the existing
  contract and explicit `track()` semantics.

### Handler timeout

The synchronous shim waits for the coroutine result only within the configured
handler timeout.

The contract is:

- the HTTP runtime stops waiting and closes/maps the request according to the
  existing timeout policy;
- Python code cannot be forcibly killed by EggServe;
- detached/cancelled coroutine cleanup must not leak the application permit;
- timeout must not create a second response after commitment;
- timeout handling must not expose application exception text.

### Request cancellation/disconnect

`AsyncRequest` must observe the native `RequestLifecycle` consistently:

- peer disconnect;
- server shutdown;
- connection timeout;
- transport failure;
- unknown future cancellation categories remain safely represented.

`wait_disconnected()` must terminate promptly when the native lifecycle is
cancelled and must respect its optional timeout.

### Async response streaming

The existing bridge uses a bounded queue and a synchronous iterable consumed by
the Rust response stream.

Required semantics:

- at most the configured queue bound is buffered;
- slow client/native consumer applies backpressure to the Python producer;
- producer no-progress is bounded by the existing response-write timeout;
- empty chunks are not progress/data;
- producer failure after commitment truncates/closes rather than synthesizing a
  second HTTP error;
- cancellation/drop of the native consumer cancels the Python producer;
- HEAD and body-forbidden responses do not consume an application body stream;
- known content-length under/overrun keeps existing native behavior;
- trailers are emitted only after successful data completion and remain
  canonical-validator controlled.

### Shutdown/task ownership

`AsyncServer.shutdown()` must cancel and account for tracked tasks without
leaving bridge-owned producer tasks behind.

User-created untracked background tasks remain outside EggServe ownership by
contract; do not attempt to discover/cancel arbitrary loop tasks.

## Track A — deterministic admission tests

Add tests using explicit events/barriers instead of timing sleeps wherever
possible.

Cover at minimum:

1. `max_async_tasks=1`, one handler held before response, second request
   receives deterministic overload behavior;
2. first buffered handler completes and permit returns;
3. first handler returns a streaming response whose producer is held; permit
   remains unavailable until producer termination;
4. producer completes and permit returns exactly once;
5. producer errors and permit returns exactly once;
6. producer is cancelled by disconnect/shutdown and permit returns exactly
   once.

Assert the semaphore/task state through observable request results or
test-owned counters. Do not expose new production inspection APIs.

## Track B — timeout race matrix

Qualify:

- handler completes just before timeout;
- handler remains blocked past timeout;
- handler is cancelled while timeout fires;
- handler returns a streaming marker before timeout but producer later stalls;
- server shutdown occurs while handler is awaiting;
- peer disconnect occurs while handler is awaiting.

Use generous deterministic synchronization so CI does not depend on
millisecond scheduler races.

Verify no double permit release, no orphan bridge task, and no second response.

## Track C — response-producer lifecycle matrix

Test async generator/iterator and supported synchronous iterable convenience
paths.

Cases:

- multiple small chunks under backpressure;
- queue fills and later drains;
- producer stalls before first chunk;
- progress then stall;
- empty chunks then progress;
- non-bytes item;
- producer exception;
- known-length exact;
- known-length underrun;
- known-length overrun;
- trailers after data;
- producer cancelled while queue is full;
- consumer/native response dropped before producer completes.

Where possible reuse the same semantic cases as the Rust `ResponseStream`
qualification so Python and Rust are tested against equivalent expectations.

## Track D — HEAD/body-forbidden suppression

Prove that an async response producer is not advanced for:

- HEAD;
- 204;
- 304;
- any other existing body-forbidden status under the canonical policy.

A test producer should fail or increment a counter on first poll so accidental
consumption is unambiguous.

Also prove producer/task cleanup occurs without leaking an application permit.

## Track E — disconnect and lifecycle parity

Use real local H1 connections or the existing async fixture to reproduce:

- client disconnect before handler returns;
- disconnect during streamed response;
- server shutdown during handler;
- server shutdown during stream producer;
- connection timeout while handler/producer is active.

Assert `AsyncRequest.is_disconnected()`,
`cancellation_reason()`, and `wait_disconnected()` agree with native
lifecycle behavior.

Do not promise ordering stronger than the underlying cancellation contract.

## Track F — tunnel/long-lived task ownership

Exercise the existing generic H1 Upgrade/CONNECT bridge:

- denial remains ordinary HTTP and does not create a tunnel task;
- accepted tunnel returns the validated handshake;
- tracked tunnel driver is cancelled by AsyncServer shutdown;
- tunnel close releases bridge-owned resources;
- app exception before acceptance remains a generic HTTP error;
- post-accept tunnel errors do not attempt another HTTP response.

Do not add WebSocket framing. Existing ASGI fixture WebSocket echo remains a
sufficiency fixture, not a product API.

## Track G — task-registry closure proof

At the end of each lifecycle scenario, verify the bridge-owned task registry
returns to baseline.

Qualify repeated sequential streaming requests so task count does not grow with
historical requests.

A moderate repeated loop is sufficient; this is structural resource-lifetime
evidence, not a throughput benchmark.

## Track H — inspect bridge implementation only after tests

After the tests exist, review:

- `_make_sync_shim`;
- `_dispatch_one`;
- `_bridge_streaming`;
- `_put_chunk`;
- `track`;
- `shutdown`.

If tests reproduce a defect, fix it with the smallest internal change.

Preferred properties:

- single explicit permit owner at each phase;
- one cleanup path per producer/task;
- no shared mutable per-request state on `AsyncServer`;
- no unbounded queue/task creation;
- no blocking network I/O on the asyncio loop;
- no exception body/message leak.

Do not rewrite working code solely for stylistic symmetry with Rust.

## Track I — ASGI sufficiency regression

Run the existing `asgi_fixture.py` coverage and retain:

- HTTP request/response path;
- streamed body/response behavior currently covered;
- generic tunnel-backed WebSocket echo fixture;
- cancellation/shutdown behavior already represented.

Do not expand the fixture into a maintained ASGI implementation.

## Documentation

Update `docs/python-api.md` only if tests clarify behavior that is currently
ambiguous.

Keep the key product statement explicit:

- `eggserve.lowlevel.AsyncServer` is an experimental bounded substrate;
- EggServe itself is not ASGI/WSGI;
- framework lifecycle/process semantics remain downstream-owned;
- H2/H3 remain Rust-only experimental.

## Required qualification

At minimum:

```sh
python3 scripts/check-python-release-metadata.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
```

Run focused:

- async bridge tests;
- body conformance/wire tests;
- boundary hardening;
- low-level runtime tests;
- server integration tests;
- ASGI fixture tests;
- tunnel tests;
- Plan 252 typing fixture after that plan lands.

If Rust bridge code is changed, also run normal workspace format/check/clippy
and the relevant `eggserve-core`/`eggserve-server` suites.

## Acceptance criteria

- [ ] async app admission is deterministically bounded and fail-fast.
- [ ] permit ownership is proven across buffered, streaming, error, timeout,
      disconnect, and shutdown paths.
- [ ] no double release or historical task accumulation is observed.
- [ ] handler timeout races cannot create a second response or leaked permit.
- [ ] async stream producer backpressure and no-progress timeout are bounded.
- [ ] producer error/drop/cancellation semantics match the native stream
      contract.
- [ ] HEAD/body-forbidden responses do not consume producers.
- [ ] request lifecycle observations match native disconnect/shutdown reasons.
- [ ] tracked tunnel/long-lived tasks terminate under shutdown.
- [ ] existing ASGI sufficiency fixture remains green.
- [ ] no public Python API or support-tier change.
- [ ] no new native async API, framework semantics, or unbounded resource path.
- [ ] installed-wheel focused qualification passes.

## Stop conditions

If a failing test demonstrates that the existing documented behavior itself is
internally contradictory, record the contradiction and open a narrow
corrective rather than silently redefining the public contract.

If resolving a defect requires a new Python public method/property,
H2/H3 support, raw socket access, or native asyncio runtime redesign, stop and
DEFER that item outside Plans 251–256.
