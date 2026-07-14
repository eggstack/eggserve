# Phase 52 — Milestone 3B: Listener Ownership, Lifecycle, and Shutdown

## Goal

Complete the reusable runtime lifecycle by giving embedded consumers explicit control over listener ownership, startup readiness, graceful shutdown, forced shutdown, task cancellation, and runtime state.

This phase builds on the service/runtime boundary from Phase 51. It must preserve Rust ownership of sockets and HTTP transport while allowing downstream process managers, test harnesses, socket-activation systems, and Python bindings to control lifecycle predictably.

## Starting state

Eggserve already supports CLI binding, context-managed Python server use, subprocess lifecycle helpers, connection limits, graceful-shutdown timeout configuration, and production-path tests. The new runtime service boundary needs a single lifecycle model that all frontends share.

Known risks to close:

- multiple bind/start/stop implementations;
- ambiguous ownership when a pre-bound listener is supplied;
- readiness races;
- double-start or double-shutdown behavior;
- permit/task leakage during cancellation;
- indefinite shutdown while handlers or clients are stalled;
- inconsistent CLI, Rust, and Python lifecycle semantics.

## Track A — Lifecycle state machine

Define an explicit lifecycle state machine, for example:

```text
Created -> Starting -> Running -> Draining -> Stopped
                \-> Failed
```

Specify allowed operations and errors for each state:

- build/configure;
- start;
- obtain bound address;
- wait for readiness;
- request graceful shutdown;
- request forced shutdown;
- wait for completion;
- inspect terminal result.

Requirements:

- no implicit restart unless intentionally designed;
- double start returns a typed lifecycle error;
- shutdown before start has deterministic behavior;
- multiple shutdown callers are idempotent or return a documented typed result;
- terminal failure is retained for inspection;
- state transitions are race-safe.

Add model-based or exhaustive state-transition tests.

## Track B — Listener abstraction and ownership

Support both runtime-created and caller-supplied listeners.

Suggested APIs:

```rust
ServerBuilder::bind(addr)
ServerBuilder::from_listener(listener)
```

Requirements:

- accept a pre-bound TCP listener without rebinding;
- document blocking/nonblocking requirements and normalize safely;
- expose the actual local address, including port-zero assignment;
- define ownership transfer: after successful build/start, the runtime owns the listener;
- return the listener or a recoverable error when setup fails before ownership transfer, if feasible;
- prevent use-after-transfer ambiguity;
- preserve socket options already configured by the caller unless eggserve must change them for correctness;
- document unsupported listener types.

Consider systemd/socket-activation compatibility as a downstream use case, but do not add platform-specific activation code in this phase.

Tests:

- bind by address;
- port zero;
- pre-bound listener;
- occupied address;
- listener dropped before start;
- invalid/non-TCP listener rejection where applicable;
- local address consistency.

## Track C — Server and handle split

Adopt a clean separation between the configured server and the running handle.

Possible shape:

```rust
let server = ServerBuilder::new(service).bind(addr).build()?;
let handle = server.start().await?;
handle.ready().await?;
handle.shutdown().await?;
handle.wait().await?;
```

Define:

- whether `start()` consumes `Server`;
- whether `ServerHandle` is cloneable;
- which handle methods are thread-safe;
- how readiness is signaled;
- how terminal errors are surfaced;
- how the bound address is queried;
- whether dropping the last handle requests shutdown or merely detaches.

Preferred safety behavior: dropping handles should not silently abandon an uncontrolled server. Choose explicit ownership semantics and test them.

## Track D — Readiness and startup failure

Readiness must mean the server can accept connections, not merely that a task was spawned.

Startup sequence should validate:

- configuration;
- listener readiness;
- TLS configuration where enabled;
- runtime resources/semaphores;
- service construction;
- observer/logging initialization where applicable.

Expose startup failures synchronously or through a readiness future before reporting success.

Tests:

- valid startup;
- invalid TLS material;
- invalid configuration;
- bind failure;
- service initialization failure if services have initialization;
- no connection accepted before readiness is signaled;
- readiness cannot hang indefinitely after fatal startup failure.

## Track E — Graceful shutdown semantics

Define graceful shutdown precisely:

1. stop accepting new connections;
2. signal active connections to stop accepting new requests where possible;
3. allow in-flight requests and response streams to complete;
4. wait until the configured deadline;
5. cancel remaining tasks and close connections;
6. release all permits/resources;
7. return a terminal shutdown result.

Specify behavior for:

- idle keep-alive connections;
- slow header readers;
- handlers still running;
- slow response readers;
- file streams;
- TLS handshakes;
- Python callbacks;
- clients disconnected during drain.

Graceful shutdown should not wait forever on a peer that is not reading.

## Track F — Forced shutdown and cancellation safety

Provide an explicit forced-shutdown mechanism or deadline escalation.

Requirements:

- every spawned connection/task is tracked;
- cancellation cannot leak connection or stream permits;
- open file handles are dropped;
- service futures are cancelled according to documented semantics;
- Python callback cancellation limitations are documented honestly;
- forced shutdown is idempotent;
- runtime waiters are awakened;
- no detached tasks survive terminal shutdown.

Use RAII guards for all permits and task registrations.

Add stress tests with repeated cancellation at each pipeline stage:

- before TLS handshake;
- during headers;
- before service call;
- during service call;
- during byte response;
- during full-file response;
- during range response;
- during keep-alive idle.

## Track G — Connection and task registry

Implement a bounded internal registry or task set sufficient to:

- track active connections;
- broadcast drain/force signals;
- await task completion;
- collect terminal task failures where useful;
- avoid unbounded bookkeeping growth;
- expose aggregate counts for tests/observation without exposing task internals.

Do not expose Tokio task handles as stable public API.

Define whether connection-local errors are logged/observed and ignored, or can fail the entire server. Normal peer resets must not terminate the server.

## Track H — Runtime ownership and Tokio integration

Decide how the runtime interacts with Tokio:

- require an existing Tokio runtime for async Rust APIs;
- optionally provide a blocking convenience wrapper elsewhere;
- do not create hidden nested runtimes inside async contexts;
- document Send/Sync and runtime-thread assumptions;
- support multi-threaded and current-thread runtimes where feasible.

Tests should cover:

- current-thread runtime;
- multi-thread runtime;
- start from one task, shutdown from another;
- handle use across threads;
- no blocking operations on core async threads beyond known filesystem constraints.

## Track I — CLI integration

Refactor the CLI to use the same `ServerBuilder`/`ServerHandle` lifecycle.

Requirements:

- signal handling requests graceful shutdown;
- second signal may force shutdown if that is existing/desired policy;
- startup banner prints only after readiness;
- startup errors retain current exit-code/error behavior;
- quiet and JSON logging behavior remain intact;
- no separate accept loop remains in the binary crate.

Add CLI integration tests for startup, SIGTERM/SIGINT where portable, graceful drain, forced timeout, and exit status.

## Track J — Resource and soak tests

Add bounded but adversarial tests:

- repeated start/shutdown cycles;
- 100+ concurrent idle connections;
- connection-limit saturation;
- slowloris headers during shutdown;
- slow readers during shutdown;
- file-stream-limit saturation;
- handler timeout plus shutdown;
- abrupt force shutdown;
- no file-descriptor growth across cycles;
- no permit-count drift;
- shutdown duration within configured bound.

Where platform-specific resource inspection is needed, keep portable invariants in normal CI and deeper FD/memory checks in Linux qualification jobs.

## Error and API contract

Add or refine typed errors:

- `LifecycleError`;
- `StartupError`;
- `BindError`;
- `ShutdownError`;
- `ShutdownTimeout`;
- `AlreadyRunning`;
- `NotRunning`;
- `TerminalRuntimeError`.

Avoid excessive type proliferation if existing variants can represent these precisely.

Classify public lifecycle APIs as experimental until Phase 53 closure.

## Documentation

Update:

- runtime architecture docs;
- lifecycle state diagram;
- Rust API examples;
- CLI shutdown behavior;
- Python lifecycle mapping notes;
- release contract;
- API stability inventory;
- platform-specific signal limitations.

## Completion criteria

Phase 52 is complete only when:

- callers can bind or provide a listener;
- readiness has a precise meaning;
- lifecycle transitions are race-safe and typed;
- graceful shutdown drains within a configured deadline;
- forced shutdown cancels all tracked work;
- no permit, task, socket, or file-handle leakage is observed;
- CLI uses the same runtime lifecycle as embedded Rust;
- lifecycle behavior is validated under real sockets and stress.

## Non-goals

- Socket activation implementation.
- Multi-listener or multi-protocol serving.
- Hot restart or zero-downtime process replacement.
- Runtime config reload.
- Request-body streaming.
- HTTP/2 or WebSockets.