# ADR-003: Custom-Service Ownership Model

## Status

**Accepted** (implemented — Plan 078)

## Context

The `ServerBuilder` exposed a `build_with_service()` method that accepted a service value but silently discarded it. The service had to be supplied again at `start_with_service()`, making the builder contract misleading. Connection metadata was also synthesized with placeholder loopback addresses.

## Decision

Remove all builder methods that accept a service. Require the service only at `start_with_service()`.

## Rationale

### Considered designs

1. **Type-state builder** (`ServerBuilder<NoService>` → `ServerBuilder<S>`): Provides compile-time guarantees but adds generics to a public API that is already experimental. Increases complexity for Python bindings and embedding consumers.

2. **ServiceMode enum with boxed erasure**: Internal enum dispatching between static and custom service. Hides the service type behind dynamic dispatch, complicating drop semantics and ownership tracking.

3. **Remove builder service methods** (selected): The simplest approach. `ServerBuilder` configures the runtime (bind, limits, timeouts). `start()` uses the built-in static service. `start_with_service()` accepts a custom service. No silent discard, no ambiguous ownership.

### Why the fallback was chosen

- The `server` module is experimental — the API is subject to change.
- Type-state adds complexity without proportional benefit at this stage.
- The custom service path already requires `start_with_service()` — the builder method was redundant.
- Python bindings use `start_with_service()` directly, so the builder service method was never used in practice.

## Consequences

- `ServerBuilder` has no `.service()` method.
- A built custom-service server has an unambiguous service owner: the service is passed to `start_with_service()` and owned by the accept loop.
- The service is wrapped in `Arc` for shared ownership across connections.
- Drop occurs exactly once on shutdown (verified by lifecycle tests).
- Static and custom service modes use the same `accept_loop` / `JoinSet` supervisor.

## Service Lifecycle

- The service is wrapped in `Arc<S>` at `start_with_service()` entry.
- Each connection clones the `Arc` and passes it to the `Service::call` method.
- On shutdown, the `JoinSet` aborts all connection tasks, dropping the `Arc` clones.
- The original `Arc` is dropped when the accept loop exits.
- Service state persists across keep-alive requests on the same connection (service is cloned per-connection, not per-request).

## Related

- Plan 078: Custom-Service Ownership and Real Connection Metadata
- ADR-002: Windows Handle-Relative Filesystem Confinement
