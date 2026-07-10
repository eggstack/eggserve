# Phase 32 — Deterministic Concurrency and Timeout Verification

## Goal

Turn existing runtime smoke tests into deterministic evidence that EggServe enforces configured connection, callback, file-stream, timeout, and shutdown limits under success and failure.

## Scope

Applies to the Python `Server` path and shared Rust serving machinery. This is verification and corrective polish only.

## Workstream A — Connection semaphore

Create barrier-based tests that hold exactly `max_connections` requests open, then prove an additional connection cannot enter request service until a permit is released.

Cover permit release after:

- normal completion;
- malformed request;
- header timeout;
- write timeout;
- peer disconnect;
- service error;
- shutdown.

Instrument test-only counters or hooks rather than relying solely on sleeps.

## Workstream B — Python callback semaphore

Use a handler with an atomic active counter, maximum-observed counter, and barriers.

Prove:

- observed concurrency never exceeds `max_python_callbacks`;
- queued callbacks proceed after release;
- exceptions release permits;
- invalid return types release permits;
- shutdown does not deadlock while callbacks are queued;
- no Rust mutex is held while arbitrary Python executes.

## Workstream C — File-stream semaphore

Use large files and controlled readers to hold stream permits.

Prove:

- no more than `max_file_streams` are active;
- a queued stream begins only after release;
- HEAD and empty-body responses do not consume stream permits;
- disconnect, I/O failure, range completion, and shutdown release permits;
- handler-returned file bodies use the same limit as static responses.

## Workstream D — Timeout boundaries

Verify exact documented coverage:

- header timeout covers incomplete headers;
- write timeout covers stalled response delivery, including file bodies;
- client request timeout covers handshake, headers, and full body collection;
- connect timeout remains distinct and documented;
- timeout values reject zero, negative, NaN, infinity, and invalid types as appropriate.

If Python callbacks have no execution timeout, state this explicitly and ensure bounded shutdown behavior is honest. Do not claim cancellation of arbitrary Python code.

## Workstream E — Graceful shutdown

Define a shutdown contract:

- listener stops accepting immediately;
- idle/incomplete connections are signaled;
- active responses receive a bounded grace period if supported;
- blocked callbacks cannot cause silent indefinite joins;
- repeated `stop()` is safe;
- context-manager exit is safe after partial startup failure.

Add deterministic tests for shutdown during each resource state.

## Workstream F — Test reliability

Remove assertions that merely collect results without proving the configured bound. Avoid broad timing thresholds except where testing a timeout itself. Use channels, events, barriers, atomic counters, and bounded joins.

Mark platform-specific timing tests carefully; do not solve flakiness by making assertions meaningless.

## Likely files

- `crates/eggserve-python/src/server.rs`
- `crates/eggserve-python/python/eggserve/test_server_integration.py`
- shared response/streaming modules in `eggserve-core`
- CI workflow timeout configuration
- server architecture and Python API docs

## Acceptance criteria

- Each semaphore has deterministic saturation and release tests.
- Success, error, timeout, disconnect, and shutdown exits release permits.
- Maximum observed callback and stream concurrency is asserted.
- Header/write/client timeout coverage matches documentation.
- Shutdown behavior is bounded and documented.
- No test proves correctness only through arbitrary sleep duration.
- Full Rust/Python/feature CI remains green.

## Bugs found and fixed

### Bug 1: GIL deadlock in `stop()` (server.rs)

The `stop()` method is a `#[pymethod]` that holds the Python GIL while calling `handle.join()`. The tokio runtime's `Drop` impl waits for all spawned tasks to complete. Spawned connection tasks call `Python::with_gil` to invoke handlers. If a handler is sleeping (e.g., `time.sleep`), the GIL is released during sleep, but the handler needs the GIL again to complete. Since `stop()` holds the GIL while waiting for the runtime to shut down, the handler can never finish → deadlock.

**Fix**: Wrap `handle.join()` in `py.allow_threads(|| { ... })` to release the GIL during the blocking join. Also update `__exit__` to pass `py` to `stop()`.

### Bug 2: File-stream semaphore permit not held during streaming (server.rs)

The file-stream semaphore permit was acquired in `convert_to_hyper_response` but stored as a local variable that was dropped when the function returned. The permit was NOT passed into `stream_file()`, so the semaphore was effectively a no-op — permits were acquired and immediately released.

**Fix**:
1. Add `OwnedSemaphorePermit` import
2. Change `stream_file` to accept `permit: Option<OwnedSemaphorePermit>` and hold it in the stream state tuple
3. Pass the permit from `convert_to_hyper_response` to `stream_file`

## Non-goals

- No new scheduler or worker pool.
- No Python callback cancellation mechanism unless it can be implemented safely and narrowly.
- No load-balancer or multi-process design.
