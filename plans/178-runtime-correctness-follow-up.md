# Plan 178 — Runtime Correctness Follow-up: Shutdown, Logging Failure, and Error Representation

## Status

**PLANNED — corrective; no product-surface expansion.**

Prerequisites: Plans 172–175 implemented/closed, Plan 176 closed/deferred, and Plan 177 closed. This plan should land before the structural work in Plans 179–181 so known correctness defects are fixed before code is moved or runtime context is expanded.

## Purpose

Close three narrow correctness defects found in the post-Plan-177 repository review:

1. caller-owned `ConnectionShutdown` is edge-triggered in practice and can miss a shutdown that occurs before a waiter registers;
2. `CompositeLogSink` can recursively re-enter the same failing global sink graph while attempting to report a child-sink panic;
3. `ServiceError::rejected(status, ..)` accepts arbitrary canonical status codes but the minimal error-body mapping can emit a body that claims `500 Internal Server Error` for a different status such as 429.

These are corrective changes to existing runtime behavior. Do not use this plan to add routes, middleware, protocol upgrades, ASGI/WSGI behavior, telemetry frameworks, new HTTP versions, or another product surface.

## Current-state findings

### 1. `ConnectionShutdown` can lose an already-issued shutdown

`crates/eggserve-core/src/server/connection.rs` currently stores a persistent atomic shutdown flag, but `ConnectionShutdown::cancelled()` does not consult that flag. `shutdown()` stores `true` and then calls `Notify::notify_waiters()`, while `cancelled()` only awaits `Notify::notified()`.

`notify_waiters()` wakes waiters already registered at that moment; it is not the persistent state. A caller can therefore signal shutdown before the connection driver registers its waiter, after which the driver may continue until another timeout/transport event despite `is_shutdown() == true`.

The caller-owned connection path reaches this directly through `serve_hyper_with_token()` and `drive_connection()`; there is no guaranteed pre-wait flag check that closes this race.

### 2. `CompositeLogSink` failure reporting can recurse

`CompositeLogSink::emit()` contains child sink panics and increments `dropped_log_events`, which is correct. The failure path then emits a `LogSinkFailure` event through `Logger::global()`.

If the global logger is the same composite and one child consistently panics, reporting the sink failure re-enters that same composite, invokes the same failing sink, and repeats. `catch_unwind` contains individual panics but does not make the recursive control flow safe.

Operational failure reporting must never depend on traversing the graph that just failed.

### 3. Rejected service status and representation can disagree

`ServiceError::rejected()` accepts statuses in the canonical HTTP range. The wire status is retained, but the current minimal representation table recognizes only a narrow set of statuses and falls back to the literal `500 Internal Server Error\n` body for other accepted statuses.

A 429 response with a body that says 500 is internally inconsistent and weakens the canonical error contract. The application-supplied error message must still remain private.

## Required invariants

### Shutdown invariants

- Shutdown is level-triggered: once signaled, every current and future waiter completes.
- `shutdown()` is idempotent.
- No polling loop or periodic timer is required to observe shutdown.
- A shutdown signaled before `serve_http1_connection()` begins is still observed promptly by the connection driver.
- Existing `ConnectionOutcome::Shutdown` semantics remain unchanged.
- No raw Tokio cancellation type is added to canonical request/response public types.

### Logging invariants

- A `LogSink` panic never escapes into request/connection execution.
- Failure reporting never recursively traverses the failing sink graph.
- A bad sink does not prevent healthy sibling sinks from receiving the original event where iteration can safely continue.
- `dropped_log_events` remains observable and increments deterministically for dropped sink emissions.
- Library code does not introduce an unconditional `println!`/`eprintln!` fallback that bypasses the repository logging policy.

### Error-representation invariants

- Runtime-generated status and body representation never contradict one another.
- Application-supplied `ServiceError` messages are not reflected to clients.
- `HEAD` suppression remains correct.
- `ErrorRepresentationPolicy::Empty` remains body-empty.
- Response normalization/framing remains the sole final authority.

## Track A — Make `ConnectionShutdown` persistent and race-safe

### A1. Implement a state-aware wait

Retain the existing public type unless a compelling implementation reason requires otherwise. Prefer the smallest change that makes `cancelled()` observe persistent state.

A correct implementation should follow a check/register/recheck discipline so both orderings are safe:

1. observe the atomic state;
2. register a `Notify` waiter;
3. re-check the atomic state before awaiting;
4. await only if shutdown is still false.

A loop is acceptable if required to defend against spurious or unrelated wakeups, but it must not busy-spin.

Using a different internal cancellation primitive is acceptable only if it reduces complexity without adding a broad dependency or changing public semantics.

### A2. Add deterministic regression tests

At minimum cover:

- `shutdown()` before `cancelled().await`;
- waiter registered before `shutdown()`;
- repeated/idempotent `shutdown()` calls;
- token pre-signaled before `serve_http1_connection()` starts over a `tokio::io::duplex` transport;
- a controlled registration race that proves no interleaving can leave a waiter pending after the flag is true.

Do not rely on long sleeps. Use bounded test deadlines only as deadlock guards.

### A3. Requalify the downstream connection seam

Run the Plan 175 external-consumer tests because the caller-owned connection driver is part of that qualified downstream HTTP-only substrate.

## Track B — Make log-sink panic handling non-recursive

### B1. Remove recursive global emission from the failure path

The `CompositeLogSink` panic branch must not call a path that may lead back into the same composite. Prefer direct failure accounting and containment.

If a `LogSinkFailure` event is retained, it must be delivered through an explicitly non-recursive mechanism that cannot include the sink that just failed. If that introduces substantial state/complexity, dropping the synthetic event while preserving the dropped-event counter is preferable.

### B2. Define sibling-sink behavior

Continue iterating through other sinks after one child panics where safe. One broken optional sink should not disable unrelated healthy sinks for that event.

### B3. Add panic-path tests

Create test sinks such as an always-panicking sink and a recording sink. Verify:

- the original `emit()` call returns normally;
- the recording sibling receives the event;
- the dropped-event counter changes as specified;
- there is no recursion/stack growth or repeated synthetic failure loop;
- `flush()` panic containment remains safe.

The test must exercise a composite installed through the same global/default path that previously made recursion possible, not only a standalone local object.

## Track C — Centralize safe runtime error representation

### C1. Separate status selection from representation

`ServiceError` should determine the safe status class, while one response-layer helper should own runtime-generated representation for that status and the active error policy.

Avoid maintaining independent status/body tables in multiple modules.

### C2. Define uncommon-status behavior

For accepted statuses not having a dedicated phrase/body, choose a representation that cannot lie. Acceptable approaches include:

- deriving a safe standard phrase from the status when the canonical type can do so without broad dependencies; or
- returning a neutral empty/minimal representation for unknown/unlisted statuses while preserving the requested status.

Do not silently rewrite the status to 500 merely because a textual representation is unavailable unless the status itself is invalid under the canonical response rules.

### C3. Audit informational statuses

As part of implementation, explicitly audit the existing `100..=599` acceptance rule against canonical response normalization. If `ServiceError::rejected()` can currently construct statuses that cannot legally survive the runtime response path, document and correct that mismatch narrowly rather than preserving an impossible promise.

This audit must not create upgrade/101 support; Plan 176 remains deferred.

### C4. Tests

Cover at least common and uncommon rejected statuses, including 429 and another valid but currently unlisted status, plus:

- invalid status fallback behavior;
- `HEAD`;
- `ErrorRepresentationPolicy::Minimal`;
- `ErrorRepresentationPolicy::Empty`;
- no exposure of the application error message.

## Documentation

Update only documentation that describes the affected semantics. Likely targets include Rustdoc around `ConnectionShutdown`, `ServiceError::rejected`, and structured logging. If the implementation changes the documented set of accepted rejection statuses, update the relevant API/extension contract truthfully.

Do not reopen Plan 176 or alter the product-surface freeze.

## Verification

Run, at minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --features tls
cargo test --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Also run the Plan 175 downstream application-server consumer qualification/targeted test explicitly if it is not obvious from the workspace test output.

No new CI workflow is required for these regressions; they belong in the ordinary test suite.

## Acceptance criteria

- [ ] `ConnectionShutdown` cannot lose shutdown regardless of whether signaling occurs before, during, or after waiter registration.
- [ ] a pre-signaled token causes the caller-owned connection driver to terminate with the established shutdown semantics.
- [ ] shutdown remains idempotent and allocation/coordination remains bounded.
- [ ] a panicking `LogSink` cannot recursively re-enter the same failing sink graph.
- [ ] healthy sibling sinks remain usable after another sink panics.
- [ ] dropped log events remain counted without introducing unconditional stderr output from the library.
- [ ] `ServiceError::rejected()` never emits a body that claims a status different from the wire status.
- [ ] uncommon valid rejection statuses have a truthful, privacy-preserving representation.
- [ ] `HEAD`, empty-error policy, response normalization, and no-detail-leak invariants remain intact.
- [ ] Plan 175 downstream consumer tests remain green.
- [ ] no routes, middleware, protocol upgrades, new HTTP versions, app-server semantics, or telemetry framework dependencies are added.

## Suggested implementation order

1. Add failing shutdown-order regression tests and correct `ConnectionShutdown`.
2. Add the panicking-composite regression and make sink failure handling non-recursive.
3. Consolidate runtime error representation and add uncommon-status tests.
4. Run targeted runtime/consumer tests.
5. Run the ordinary repository verification commands.
6. Update narrow Rustdoc/current-state documentation and add a closure record to this plan.

## Handoff

After this plan closes, proceed to Plan 179 for configuration/limit authority consolidation. Do not begin the connection-pipeline module move in Plan 180 while these correctness fixes are still outstanding; otherwise the refactor obscures whether behavior changed because of a bug fix or file movement.