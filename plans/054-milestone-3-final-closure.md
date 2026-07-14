# Phase 54 — Milestone 3 Final Closure

## Goal

Fully close Milestone 3 by consolidating the Python server bindings onto the reusable Rust runtime introduced in Plans 051–052, eliminating lifecycle and transport duplication, making callback timeout and forced-shutdown semantics truthful, proving custom-service parity across plaintext and TLS, and wiring the runtime guarantees into cross-platform release evidence.

This is a corrective integration phase. It must not add routing, middleware, request-body streaming, ASGI/WSGI adapters, HTTP/2, reverse proxying, authentication, or a general application framework.

## Starting state

The repository now has:

- a reusable Rust runtime in `eggserve_core::server`;
- public `Server`, `ServerBuilder`, `ServerHandle`, `RuntimeConfig`, and `Service` APIs;
- runtime-owned HTTP parsing, canonical request conversion, response normalization, file streaming, limits, and cancellation;
- caller-owned and runtime-created listener support;
- readiness, graceful shutdown, forced shutdown, task tracking, and TLS integration;
- CLI plaintext and TLS paths using the Rust runtime;
- Python lifecycle methods and tests;
- Python callback concurrency limits and coroutine rejection.

Remaining gaps:

1. Python still owns a parallel Tokio runtime, listener, accept loop, shutdown channel, and lifecycle state rather than wrapping `eggserve_core::server::Server` and `ServerHandle`;
2. Python lifecycle states are a reduced approximation of the Rust lifecycle;
3. `handler_timeout_secs` is documented more strongly than the implementation can enforce for blocking Python callbacks;
4. Python forced-shutdown behavior is not guaranteed to match Rust task cancellation and drain semantics;
5. TLS custom-service dispatch may still bypass the generic `Service` path;
6. Python lacks listener ownership parity and the capability matrix does not fully distinguish intentional gaps;
7. runtime/lifecycle guarantees are not represented by dedicated release gates;
8. installed-wheel lifecycle qualification is not yet explicit across Linux, macOS, and Windows;
9. no final-SHA evidence bundle has been inspected for the completed Milestone 3 implementation.

## Track A — Make the Rust runtime the single server implementation

Refactor `crates/eggserve-python/src/server.rs` so Python `Server` constructs and owns the actual Rust runtime types.

Required architecture:

- Python `Server` stores a configuration object that can build `eggserve_core::server::Server`;
- `start()` creates one Tokio runtime thread only as an async bridge, then calls `Server::start()`;
- the resulting real `ServerHandle` is retained in shared Rust state;
- `wait_ready()`, `shutdown()`, `force_shutdown()`, and `wait()` delegate to the real handle;
- Python no longer owns a separate accept loop, connection registry, graceful-drain implementation, or shutdown broadcast protocol;
- static serving and Python callback serving are expressed as Rust `Service` implementations supplied to `ServerBuilder`;
- all listener acceptance, connection limits, parsing, normalization, transport, TLS, and task cancellation remain in `eggserve_core::server`.

Remove or retire duplicate Python-side code for:

- listener binding;
- accept loops;
- per-connection spawning;
- transport timeouts already enforced by the Rust runtime;
- independent lifecycle transitions;
- independent forced-shutdown logic;
- independent connection/task tracking.

Temporary compatibility adapters are acceptable only if they are private, documented, and have explicit removal criteria.

Acceptance:

- Python and CLI/Rust embedding use the same server implementation;
- a Rust runtime lifecycle bug fix automatically applies to Python without duplicate patches;
- source-level checks show no second accept loop in the Python crate;
- Python static and callback modes are both implemented as services on the Rust runtime.

## Track B — Python service adapter and callback execution model

Implement a dedicated Rust `Service` adapter for Python callbacks.

The adapter must:

- receive canonical `RequestHead` and trustworthy `ConnectionInfo` from the runtime;
- build the existing Python `Request` projection without reparsing raw HTTP;
- execute callbacks in a bounded worker execution model;
- preserve `max_python_callbacks` as a hard upper bound;
- reject coroutine returns deterministically before attempting response conversion;
- validate the returned Python `Response` through the canonical response path;
- map callback exceptions to sanitized 500 responses without exposing tracebacks to clients;
- ensure callback failures do not poison the runtime or leak permits;
- keep socket and file I/O outside the GIL.

Recommended execution model:

- use a bounded pool or bounded `spawn_blocking` admission layer;
- create a one-shot completion channel per callback;
- stop awaiting the result after `handler_timeout_secs`;
- release request-side and connection-side resources after timeout;
- return a deterministic timeout response, preferably 504 or a documented 500 policy;
- allow the underlying Python function to finish later because Python code cannot be safely force-cancelled;
- track timed-out workers until they finish so abandoned work cannot bypass callback concurrency limits.

Do not spawn an unbounded OS thread per request.

Acceptance:

- callback concurrency cannot exceed configuration;
- timeout stops the request from waiting by the configured deadline;
- timed-out Python work does not retain socket, file-stream, or request permits;
- repeated timeout attempts cannot create unbounded worker growth;
- callback exceptions and invalid return values are isolated and deterministic.

## Track C — Define truthful handler-timeout semantics

Replace vague “best-effort transport-level timeout” language with a precise contract.

Document separately:

1. request handler wait deadline;
2. response write deadline;
3. inability to asynchronously interrupt arbitrary Python code;
4. lifetime of timed-out callback work;
5. callback-pool admission and saturation behavior;
6. shutdown behavior when timed-out callbacks remain active.

Required behavior:

- the HTTP request receives a deterministic timeout response or connection termination by the configured handler deadline;
- the runtime stops waiting for the Python result;
- the callback permit remains accounted for until the Python function returns;
- new callback requests are rejected or queued according to an explicit bounded policy;
- graceful shutdown waits for active callback work only within its configured deadline;
- forced shutdown stops runtime tasks even if Python worker code remains alive;
- process-exit limitations of non-interruptible Python worker threads are documented and tested.

Add typed internal outcomes such as:

- `Completed`;
- `TimedOut`;
- `RejectedAtCapacity`;
- `Raised`;
- `InvalidResponse`;
- `CoroutineReturned`.

Acceptance:

- API docs no longer claim cancellation that cannot be delivered;
- timeout behavior is covered by real-socket tests;
- callback saturation and shutdown interactions are deterministic.

## Track D — Lifecycle state parity

Expose lifecycle state from the actual Rust `LifecycleState` rather than maintaining a separate Python enum.

Python should represent at least:

- `created`;
- `starting`;
- `running`;
- `draining`;
- `stopped`;
- `failed`.

Requirements:

- `state` reads the actual runtime lifecycle state;
- `wait_ready()` blocks until `running` or terminal failure;
- `shutdown()` transitions to draining and is idempotent;
- `force_shutdown()` returns the actual Rust shutdown result;
- `wait()` returns or raises based on the real terminal state;
- calls before start, during start, during drain, and after stop have explicit typed behavior;
- context-manager exit uses the same graceful-shutdown path;
- double-start and restart-after-stop policies remain explicit.

Avoid stringly typed internal state. Python may expose strings or an enum publicly, but the source must be Rust lifecycle state.

Acceptance:

- lifecycle transitions observed from Python match Rust tests;
- no separate Python atomic state machine remains;
- race tests cover concurrent lifecycle calls.

## Track E — Forced shutdown and resource accounting

Prove Python forced shutdown delegates to the Rust task registry.

Tests must cover:

- slow client response reader;
- slow Python callback;
- callback blocked beyond timeout;
- connection-limit saturation;
- file-stream saturation;
- idle keep-alive connections;
- repeated `force_shutdown()` calls;
- `shutdown()` followed by `force_shutdown()`;
- context manager during active work;
- server object drop while running.

For every case verify:

- server thread terminates by the configured deadline;
- runtime connection tasks are drained or aborted as documented;
- semaphores return to baseline;
- listening socket closes;
- subsequent bind to the same address succeeds where platform semantics permit;
- no unbounded task or worker growth remains;
- Python exceptions are not raised from background threads without being captured.

Acceptance:

- forced shutdown has the same core result semantics in Rust and Python;
- resource accounting is testable and returns to baseline after each cycle.

## Track F — TLS and generic service parity

Audit the TLS accept path so transport setup is independent of service dispatch.

Required invariant:

> plaintext and TLS connections pass through the same generic `Service` dispatch and response normalization path after the transport handshake.

Remove any TLS branch that directly invokes legacy static handling or `crate::service::handle_request()` instead of the configured service.

Add focused tests:

- custom `service_fn` over plaintext;
- the same service over TLS;
- `StaticService` over plaintext and TLS;
- callback service over TLS where Python TLS exposure is supported;
- connection metadata reports `http` versus `https` accurately;
- TLS handshake timeout does not consume a connection permit indefinitely;
- TLS handshake failure does not invoke the service;
- graceful and forced shutdown work during TLS handshakes and active TLS responses.

Acceptance:

- service selection is identical across transports;
- no static-only TLS fallback exists;
- TLS remains feature-gated and does not alter plaintext behavior.

## Track G — Listener ownership and Python capability policy

Decide whether Python will support pre-bound listeners in this milestone.

Preferred approach if technically clean:

- accept a Python socket object or duplicated file descriptor;
- validate it is a bound TCP listening socket;
- transfer or duplicate ownership explicitly;
- construct Rust `TcpListener` through the supported platform path;
- document ownership after `start()` and after server shutdown.

If deferred:

- state explicitly that `from_listener` is Rust-only;
- update `docs/library-capability-matrix.md`, `docs/python-api.md`, and the release contract;
- avoid using “full parity” for listener ownership;
- add a future-work marker without blocking Milestone 3 closure.

Do not implement fragile raw-handle ownership without platform-specific tests.

Acceptance:

- listener ownership capability is either implemented and tested or explicitly classified as an intentional Rust-only capability.

## Track H — Cross-platform installed-wheel conformance

Extend installed-wheel qualification on Linux, macOS, and Windows.

The wheel test suite must run outside the source tree with `PYTHONPATH` cleared and cover:

- import and typing surface;
- port-zero startup;
- `wait_ready()`;
- state transitions;
- static response;
- callback response;
- callback exception;
- callback timeout;
- callback-capacity saturation;
- graceful shutdown;
- forced shutdown;
- context manager;
- repeated start/stop cycles using new instances;
- address reuse after stop where supported;
- no source-tree binary or module fallback.

Platform-specific assertions:

- Windows tests must not assume Unix signal semantics;
- socket-reuse expectations must match platform behavior;
- timeout thresholds must allow runner variance while remaining bounded;
- macOS and Windows must exercise the native wheel, not an emulated/source build.

Acceptance:

- all advertised wheel platforms execute lifecycle tests;
- platform exclusions are explicit and narrow;
- installed-wheel behavior matches source-tree Rust semantics.

## Track I — Dedicated runtime and lifecycle release gates

Add stable gate identities to `release/criteria.toml` and CI.

Recommended gates:

- `runtime.public-rust-consumer`;
- `runtime.service-dispatch`;
- `runtime.listener-lifecycle`;
- `runtime.graceful-shutdown`;
- `runtime.forced-shutdown`;
- `runtime.tls-service-parity`;
- `python.runtime-parity`;
- `python.callback-timeout`;
- `python.lifecycle-linux`;
- `python.lifecycle-macos`;
- `python.lifecycle-windows`.

Each gate must define:

- trigger policy;
- platform applicability;
- evidence class;
- command;
- invalidation paths;
- freshness;
- required artifacts where applicable;
- whether it is required for release approval.

Use structured evidence wrappers for each dedicated command. Do not rely only on `cargo test --workspace` or aggregate Python counts.

Update:

- `.github/workflows/ci.yml`;
- `release/criteria.toml`;
- `docs/ci-gate-inventory.md`;
- `docs/release-process.md`;
- generated checklist;
- contract-consistency validation.

Acceptance:

- runtime guarantees have explicit evidence records;
- missing cross-platform lifecycle evidence blocks release approval where required.

## Track J — API stability and documentation reconciliation

Keep `eggserve_core::server` experimental through this closure phase.

Audit and reconcile:

- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- `docs/python-api.md`;
- `architecture/runtime.md`;
- `architecture/eggserve-python.md`;
- `README.md`;
- `AGENTS.md`;
- `.pyi` files;
- Rust crate documentation and examples.

Required documentation truths:

- Python uses the actual Rust runtime after this phase;
- callback timeout does not interrupt arbitrary Python execution;
- callback concurrency and timeout accounting are explicit;
- lifecycle states and methods map to the real handle;
- listener ownership differences are explicit;
- TLS service behavior is shared across transports;
- `server` APIs remain experimental;
- no ASGI/WSGI or async Python callback support is implied.

Remove stale statements that describe “the same pattern” instead of the same runtime.

Acceptance:

- contract consistency tests detect lifecycle, parity, listener, and timeout claim drift;
- public docs match actual behavior.

## Track K — Final-SHA qualification

After all corrections, run the complete CI and wheel matrix on a clean `main` commit.

Inspect and record:

- Rust runtime consumer evidence;
- listener/lifecycle evidence;
- graceful and forced shutdown evidence;
- TLS custom-service evidence;
- Python timeout/saturation evidence;
- Linux/macOS/Windows installed-wheel lifecycle evidence;
- exact commit SHA;
- aggregate evidence manifest;
- generated release checklist;
- absence of secrets or credentials;
- deterministic regeneration from downloaded evidence.

Perform one controlled negative test in the release-safety suite proving that missing or wrong-SHA runtime evidence prevents a release-ready checklist.

Acceptance:

- one exact final SHA has complete Milestone 3 evidence;
- no runtime/lifecycle gate is silently absent or represented only by a broad test job;
- downloaded evidence regenerates the same checklist.

## Required testing

Rust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test server_integration
cargo test -p eggserve-core --test lifecycle_integration
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --features tls --test lifecycle_integration
```

Python, from an installed wheel with `PYTHONPATH` cleared:

```sh
python -m unittest eggserve.test_server_primitives -v
python -m unittest eggserve.test_server_integration -v
python -m unittest eggserve.test_runtime_parity -v
python -m unittest eggserve.test_callback_timeout -v
```

Release infrastructure:

```sh
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
python3 scripts/release_criteria.py generate-checklist --check
python3 -m unittest scripts.test_release_criteria -v
python3 -m unittest scripts.test_check_contract_consistency -v
python3 -m unittest scripts.test_release_safety -v
bash scripts/release-validate.sh metadata
bash scripts/release-validate.sh fast
```

Add focused tests for:

- no parallel Python accept loop;
- Python owning a real Rust `ServerHandle`;
- callback timeout and permit accounting;
- TLS custom service;
- forced shutdown during callback and TLS work;
- cross-platform installed-wheel lifecycle.

## Completion criteria

Milestone 3 is complete only when:

- Python `Server` uses the actual Rust runtime and `ServerHandle`;
- no duplicate Python listener/accept/lifecycle implementation remains;
- callback timeout semantics are precise, bounded, and tested;
- lifecycle states come from the Rust runtime;
- graceful and forced shutdown are equivalent at the core runtime level;
- custom services work identically over plaintext and TLS;
- listener ownership parity or intentional difference is documented;
- Linux, macOS, and Windows wheels pass installed lifecycle qualification;
- dedicated runtime/lifecycle release gates produce structured evidence;
- one final-SHA evidence bundle has been inspected;
- `eggserve_core::server` remains accurately classified as experimental;
- no Milestone 3 blocker remains before service composition and request-body work.

## Non-goals

- No async Python callback or coroutine execution support.
- No safe cancellation claim for arbitrary Python code.
- No request-body streaming.
- No routing or middleware framework.
- No ASGI/WSGI adapter.
- No HTTP/2 or WebSocket support.
- No reverse proxying.
- No promotion of the runtime API to stable in this phase.
