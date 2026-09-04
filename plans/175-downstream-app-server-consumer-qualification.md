# Plan 175 — Downstream Application-Server Consumer Contract and Qualification

## Status

**PLANNED.**

Prerequisites: Plans 172–174 implemented in substance. Plan 173 must have settled canonical metadata fidelity and Plan 174 must have settled deferred request-body ownership plus request lifecycle signaling before this plan can close.

## Goal

Prove, from outside `eggserve-core`, that the public EggServe API is sufficient to build the HTTP half of a real event-driven application server without importing Hyper internals, relying on the Python compatibility facade, buffering whole messages, or weakening EggServe's HTTP safety policy.

The deliverable is a small reference/fixture consumer plus deterministic integration tests and documentation. It is not an ASGI implementation and must not become a maintained second server product inside this repository.

## Why qualification belongs in a separate plan

Unit tests inside `eggserve-core` can prove individual primitives but can still accidentally depend on crate-private helpers or implementation knowledge. The purpose of this roadmap is downstream use, so the final gate must exercise the crate exactly as an external project would.

The consumer should model the architecture a Rust/PyO3 ASGI server would use:

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

The fixture does not need Python. Using pure Rust makes the EggServe substrate contract independently testable and keeps Python runtime behavior out of scope.

## Track A — External consumer fixture

Create a minimal consumer outside the `eggserve-core` module tree, preferably under an existing external-consumer/test-fixture convention. If none exists, use a clearly isolated fixture such as:

```text
tests/fixtures/app_server_consumer/
  Cargo.toml
  src/main.rs or src/lib.rs
```

It should depend only on documented/public EggServe APIs plus ordinary downstream dependencies needed to model bounded coordination (`tokio`, `futures-util`, `bytes` as appropriate).

It must not import:

- `eggserve_core::response` or other crate-private modules;
- Hyper request/response/service types;
- internal connection activity/state types;
- the Python package or `eggserve.lowlevel` facade;
- static-service internals.

Where the public API intentionally exposes a low-level conversion adapter that mentions Hyper, the fixture must not use it. The point is to prove the canonical `Service`/runtime path.

## Track B — Implement the hard HTTP bridge shape

The fixture service should implement an intentionally event-like internal protocol sufficient to exercise concurrency without copying ASGI names.

A useful structure is:

```rust
enum AppRequestEvent {
    Body(Bytes),
    End,
    Disconnected,
}

enum AppResponseEvent {
    Start { status: u16, headers: Vec<(Bytes, Bytes)> },
    Body(Bytes),
    End,
}
```

These names are local to the fixture only. Do not expose them from EggServe.

Use bounded channels with small capacities so tests exercise real backpressure rather than hiding ownership problems behind large buffers.

### Required bridge behavior

The service must:

1. choose streaming request-body policy;
2. move the `RequestBody` into a spawned application task or request-pump task;
3. retain/use the public request lifecycle observer;
4. wait only until the application produces response-start;
5. return a canonical `Response` with `ResponseBody::Stream`;
6. allow the application task to continue consuming request chunks and producing response chunks after `Service::call()` has returned;
7. stop promptly on response consumer drop, peer disconnect, timeout, or server shutdown.

Do not use `read_all()` in the main qualification path.

## Track C — Metadata fidelity contract

Prove the downstream consumer can construct the metadata expected by a byte-oriented application protocol.

The fixture should inspect and round-trip:

- method;
- HTTP version;
- scheme;
- path and query views;
- truthful request-target byte accessors from Plan 173, if provided;
- ordered duplicate request headers as byte values;
- local/remote addresses when present;
- absent addresses on caller-owned opaque transports;
- TLS metadata when the TLS path is used.

Response tests must prove duplicate and opaque byte header values can be emitted through canonical `Response` without importing `http::HeaderValue` or Hyper.

Do not require exact raw-path bytes if Plan 173 documented that the accepted parser boundary cannot truthfully provide them. The fixture should model a downstream server omitting optional metadata rather than synthesizing it.

## Track D — Bidirectional lifecycle cases

### D1. Early response while upload continues

Use a client that sends a request body in multiple delayed chunks. The application should produce response-start after the first chunk, return the EggServe response, and continue receiving the rest of the body while emitting response chunks.

Acceptance requires:

- response bytes arrive before request EOF;
- the remaining request body reaches the application task;
- body limits/timeouts remain active;
- after both sides complete, the HTTP/1 connection can handle another request when policy permits.

This is the key proof that Plan 174 solved the application-server seam rather than just exposing a cancellation token.

### D2. Application does not consume remaining body

Have the application produce a response and deliberately drop/abandon an incomplete request body.

Acceptance requires:

- the current response remains correctly framed if it can safely be sent;
- the connection is not reused;
- trailing upload bytes are never parsed as a second request;
- downstream task ownership terminates promptly.

### D3. Long-polling disconnect

Run an application that has consumed the request but is waiting indefinitely for an application event and is not currently polling body or response IO.

Disconnect the client.

Acceptance requires the public lifecycle observer to wake the application/bridge promptly and allow cleanup without a raw-socket probe.

### D4. Send-side disconnect

Run a streamed response, stop the client from reading/close the client, and prove:

- transport/stream failure or consumer drop reaches the downstream response producer;
- lifecycle cancellation is eventually observable;
- no deadlock depends on which signal occurs first;
- no second HTTP error response is attempted after commitment.

### D5. Shutdown

Initiate graceful shutdown while:

- request upload is active;
- the application is waiting before response-start;
- a streaming response is active;
- response has completed but delegated request-body consumption is still active.

Prove behavior matches the documented drain deadline and all fixture tasks exit.

## Track E — Timeout and admission composition

The fixture should explicitly demonstrate the intended split between EggServe runtime admission and downstream application admission.

Use a small application-task semaphore owned by the fixture and a separate EggServe `max_in_flight_requests` setting.

Verify:

- EggServe service admission protects pre-response `Service::call()` work;
- the downstream semaphore bounds application tasks that continue after response-start;
- neither queue is unbounded;
- cancellation returns both classes of permit;
- application saturation can be mapped by the downstream service to a deterministic response without changing EggServe core policy.

Test response-start/handler timeout independently from request-body read timeout if Plan 174 split their lifetimes.

## Track F — Transport parity

Run representative bridge cases over:

1. normal TCP through `Server`;
2. TLS when the feature/test infrastructure is available;
3. caller-owned `AsyncRead + AsyncWrite` using `serve_http1_connection` and shared `RuntimeState`.

The caller-owned test is important because downstream server projects may embed EggServe behind another transport and because lifecycle signaling must not secretly depend on `TcpStream::peer_addr()` or socket-specific APIs.

The application-visible request model should be identical except for truthful connection metadata (`Some` socket endpoints vs `None`).

## Track G — Reference documentation

Add one current-state document or example explaining how to build a downstream application server on EggServe without implying that EggServe itself implements one.

Recommended content:

- canonical architecture diagram;
- Service/Request/Response ownership;
- streaming request and response example;
- deferred body ownership rule;
- disconnect/cancellation semantics;
- timeout split;
- service admission vs downstream application-task admission;
- bounded-channel requirement for FFI/event-loop adapters;
- lifecycle/shutdown ordering;
- byte-oriented header handling;
- explicit statement that WebSocket/upgrades require Plan 176 or downstream protocol support.

Keep Python-specific PyO3/asyncio recommendations high level. Detailed FFI architecture belongs in the downstream project's repository.

## Track H — Public API stability inventory

Update the public API inventory after Plans 173/174.

Classify:

- byte-native canonical header APIs;
- request-target byte accessors/limitations;
- `RequestLifecycle` or equivalent;
- any request-body lifecycle/continuation types;
- relevant `RuntimeConfig` timeout semantics;
- `Service`, `ResponseStream`, `RuntimeState`, and caller-owned connection API stability status.

The consumer fixture should be compiled as part of the relevant API/packaging verification so accidental removal of a required public method is caught without creating a large CI matrix.

## Performance sanity check

This plan is correctness-first. Nevertheless, the reference bridge introduces the same kinds of coordination a real app server will use, so record a small non-gating sanity measurement for:

- direct trivial native `Service` baseline;
- reference bounded-channel bridge with one response chunk;
- streamed bridge with multiple chunks.

The purpose is to detect accidental orders-of-magnitude overhead or pathological allocation, not to claim Uvicorn/Granian parity and not to introduce latency/RPS thresholds into CI.

If the bridge overhead is dominated by fixture scheduling/channel choices, record that fact; do not contort EggServe's API around an artificial microbenchmark.

## Verification

Run at minimum:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test api_stability
cargo test -p eggserve-core --test response_streaming
cargo test -p eggserve-core --test transport_driver
cargo check -p eggserve-core --examples
cargo test --doc -p eggserve-core
bash scripts/verify-cargo-packages.sh --mode all
```

Add the external consumer build/test invocation to the repository's focused verification script if one already exists. Do not add a new always-on multi-version dependency matrix.

Run TLS and installed Python suites if Plans 173/174 changed shared native primitives/runtime behavior even though the fixture itself is Rust-only.

## Acceptance criteria

- [ ] an isolated external consumer compiles using only documented public EggServe APIs;
- [ ] it does not import Hyper or crate-private modules;
- [ ] it uses bounded coordination and does not buffer entire request/response bodies by design;
- [ ] response-start can be returned while request-body consumption continues in a downstream-owned task;
- [ ] a subsequent keep-alive request succeeds after deferred request-body completion;
- [ ] abandoning an incomplete request body prevents connection reuse safely;
- [ ] ordered duplicate and opaque-byte headers cross the consumer boundary correctly;
- [ ] request-target raw-byte capability/limitations are represented truthfully;
- [ ] peer disconnect wakes an idle/long-polling application task;
- [ ] response-stream disconnect/cancellation races terminate without deadlock;
- [ ] graceful shutdown drains/cancels downstream-owned bridge tasks deterministically;
- [ ] EggServe service admission and downstream application admission remain distinct and bounded;
- [ ] TCP, TLS, and caller-owned transports preserve the same canonical application contract where applicable;
- [ ] current docs explicitly state that EggServe can underpin downstream app servers but does not implement ASGI/WSGI/framework/process semantics;
- [ ] the public API stability inventory and migration guidance match the implementation;
- [ ] no optional WebSocket/upgrade work is smuggled into this HTTP qualification plan.

## Non-goals

Do not add:

- a production ASGI server package;
- PyO3, pyo3-async-runtimes, uvloop, or Python event-loop code to `eggserve-core`;
- framework compatibility tests for Django/FastAPI/Starlette inside EggServe;
- worker processes, reloaders, signal supervisors, or app import strings;
- HTTP/2/HTTP/3;
- WebSocket framing;
- benchmark competition claims;
- unbounded channels;
- Tower/Axum adapters without a separate concrete need.

## Handoff

After this plan closes, an HTTP-only downstream application-server project should be able to begin implementation against EggServe without requesting privileged internal access.

If the external consumer still needs crate-private/Hyper behavior, do not patch the fixture around it. Record the missing generic capability and reopen the narrow child plan responsible for that boundary.

Plan 176 is independent and should be implemented only if the downstream project needs upgraded protocols such as WebSockets.