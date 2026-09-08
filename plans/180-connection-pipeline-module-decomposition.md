# Plan 180 — Connection Pipeline Module Decomposition

## Status

**PLANNED — maintainability refactor; behavior-preserving.**

Prerequisites: Plans 178 and 179 closed. This plan is intentionally ordered after the runtime correctness and configuration-authority work so file movement cannot obscure known bug fixes or perpetuate duplicated validation.

## Purpose

Decompose the oversized `crates/eggserve-core/src/server/connection.rs` implementation along existing invariant boundaries without redesigning the public connection/service contract.

The current module correctly centralizes the canonical HTTP/1 request pipeline, but it has accumulated substantially different responsibilities: public connection context and shutdown capability, lifecycle registries, activity/deadline bookkeeping, admission guards, response-body tracking, transport progress instrumentation, Hyper adaptation, canonical service dispatch, deferred-body watchdog/tracking, and caller-owned connection entry points.

The objective is auditable ownership and lower change coupling, not a line-count contest. No protocol behavior, timeout semantics, public feature, or downstream application-server contract should change because of this plan.

## Design constraints

- Preserve the public paths under `eggserve_core::server::connection` unless a pre-1.0 change is unavoidable and demonstrably simpler.
- Preserve the Plan 175 downstream-consumer contract exactly.
- Preserve the single canonical pipeline shared by TCP/TLS `Server` and caller-owned transports.
- Do not create a second server/driver implementation while splitting the file.
- Do not expose Hyper types through new public signatures.
- Do not add middleware, routing, upgrade/WebSocket, HTTP/2/3, or framework abstractions.
- Do not change timeout/admission semantics while moving code.
- Prefer mechanical moves and narrow visibility adjustments over opportunistic rewrites.

## Track A — Build a dependency and invariant map before moving code

Document the internal responsibility graph in the plan closure notes or a short architecture note before the first extraction.

Classify at least these groups:

1. **Public connection facade** — `ConnectionContext`, `ConnectionShutdown`, `ConnectionOutcome`, and public caller-owned entry points.
2. **Request lifecycle registry** — live request registration and abnormal-connection cancellation propagation.
3. **Activity/deadline state** — connection start/activity/write timestamps, in-flight/outstanding/deferred counters, body-timeout trigger, idle/write/total deadline derivation.
4. **Admission/response tracking** — in-flight service permit guard and tracked response body completion.
5. **Transport instrumentation** — `ProgressIo` and related read/write progress observation.
6. **HTTP/1 driver** — Hyper connection builder, graceful close, completion classification, deadline/select loop, TCP broadcast and caller-token adapters.
7. **Canonical request pipeline** — Hyper request conversion, header/target/body framing validation, service body policy, service invocation/panic containment, normalization and conversion.
8. **Deferred request-body lifecycle** — watchdog and terminal-state tracker.

Record cross-group dependencies so extraction order follows dependency direction instead of creating circular module imports.

## Track B — Introduce a connection-module facade

Prefer a directory module such as:

```text
server/connection/
    mod.rs
    context.rs
    activity.rs
    transport.rs
    driver.rs
    pipeline.rs
    deferred_body.rs
```

This exact file list is not mandatory; Phase A may show a smaller split is cleaner. The required property is that each module owns a coherent invariant and the top-level `connection` facade remains the canonical import point.

### B1. Preserve public names and documentation

`mod.rs` should re-export the existing public connection types/functions needed by external consumers. External code using the Plan 175 path should not need to know which internal source file owns the implementation.

### B2. Keep implementation details crate-private

Do not respond to extraction friction by making Hyper adapters, activity state, lifecycle registries, or transport wrappers public. Narrow visibility where possible.

## Track C — Extract public context/shutdown and lifecycle state

Move `ConnectionContext`, `ConnectionShutdown`, `ConnectionOutcome`, and narrowly associated helpers into a coherent facade/context module.

Move connection request lifecycle registration/cancellation into its own internal unit if that makes the abnormal-termination invariant easier to audit.

Plan 178's persistent shutdown semantics are immutable input to this plan. Do not redesign them here.

Tests for shutdown ordering and lifecycle cancellation should move with the owning code where practical while retaining integration coverage through the real driver.

## Track D — Extract activity, admission, and response tracking

Group the state that answers the driver's core questions:

- is a service invocation in flight?
- is a response body outstanding?
- is a deferred request body still active?
- when was the last inbound/outbound progress?
- has the deferred-body watchdog requested closure?
- how many requests have completed on this connection?

Keep atomic/mutex ordering semantics unchanged. Preserve exactly-once counter/permit release behavior through RAII guards and tracked-body completion.

Avoid introducing a generic state-machine framework. The existing state is small and explicit; the improvement is source ownership, not abstraction depth.

## Track E — Extract transport progress and driver/deadline logic

### E1. Transport instrumentation

Move `ProgressIo` and response write-progress observation into a transport-focused internal module. Preserve support for TCP, TLS, and arbitrary `AsyncRead + AsyncWrite` caller-owned streams through the same wrapper point.

### E2. HTTP/1 driver

Move Hyper builder construction, graceful-close behavior, result classification, and `drive_connection()` deadline selection into a driver module.

The driver remains the sole authority for:

- total connection lifetime;
- keep-alive idle timeout;
- response write no-progress timeout;
- deferred-body timeout closure;
- server/caller shutdown;
- final `ConnectionOutcome` classification.

Do not duplicate deadline computation in transport-specific wrappers.

### E3. Preserve upgrade non-capability

The internal Hyper connection may continue using `.with_upgrades()` as currently required by implementation details, but no public upgrade vocabulary or escape hatch may appear. Plan 176 remains deferred.

## Track F — Extract canonical request/service pipeline

Move `CanonicalHyperService`, request conversion, framing/header/target checks, service admission, body-policy selection, service panic containment, canonical response normalization/conversion, and related request-level helpers into a pipeline module.

This module must remain the single request-processing source of truth for TCP/TLS and caller-owned transports.

Do not allow `Server`, `serve_http1_connection`, or static-service code to grow alternate request validation paths during extraction.

## Track G — Extract deferred-body supervision

Move deferred body timeout watchdog and terminal-state tracking into a narrowly named module tied to the existing request lifecycle primitive.

Preserve:

- Active → Complete/Abandoned/Failed semantics;
- total body-read deadline behavior after response-start;
- lifecycle cancellation reason handling;
- connection deferred-count accounting;
- no retention of the service admission permit after response-start;
- bounded spawned-task behavior.

Do not generalize this into a task supervisor framework.

## Track H — Verification against behavioral drift

### H1. Public API compile fixture

The Plan 175 external-consumer test must continue importing only the documented public modules/types and compile without path changes if possible.

If rustdoc links or re-exports change internally, add a compile/API assertion that protects the facade.

### H2. Existing behavior suites

Run all connection-heavy tests, especially:

- parser/framing rejection;
- request-body policies and deferred-body reuse;
- request lifecycle cancellation;
- keep-alive and max-requests behavior;
- header/body/handler/total/write timeouts;
- graceful/forced shutdown;
- service panic containment;
- streaming responses;
- TCP/TLS/caller-owned parity;
- Plan 178 shutdown race regression.

### H3. Avoid synthetic size gates

Do not add CI that fails because a source file exceeds an arbitrary line count. The maintainability result should be judged by responsibility ownership, visibility, and testability, not a mechanical size threshold.

## Documentation

Update `architecture/runtime.md`, `architecture/eggserve-core.md`, or equivalent current architecture documentation only where source ownership/module diagrams are now inaccurate.

Public user documentation should not change materially because behavior and product surface are unchanged.

## Verification

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features tls
cargo test -p eggserve-bin --features tls
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Also run the Plan 175 consumer qualification explicitly and any targeted connection tests that are not ordinary workspace members.

A full wheel rebuild is required only if Rust public paths/types used by the Python crate are moved in a way that could affect its build; at minimum the excluded Python crate must compile.

## Acceptance criteria

- [ ] `server/connection.rs` is replaced or substantially reduced by modules with coherent invariant ownership.
- [ ] there remains exactly one canonical HTTP/1 request/service pipeline.
- [ ] TCP/TLS and caller-owned transports still use that same pipeline.
- [ ] public Plan 175 connection/service imports remain source-compatible unless a separately documented pre-1.0 correction is unavoidable.
- [ ] Hyper types remain absent from the canonical downstream connection signature.
- [ ] timeout, admission, lifecycle, response tracking, shutdown, and body-delegation semantics are unchanged.
- [ ] no new public implementation details are exposed merely to solve module visibility.
- [ ] all Plan 178 correctness regressions remain fixed.
- [ ] all ordinary, TLS, and downstream-consumer tests remain green.
- [ ] no arbitrary source-size CI gate or abstraction framework is added.
- [ ] no routing, middleware, upgrades, HTTP/2/3, ASGI/WSGI, proxy, or application-server feature is introduced.

## Suggested implementation order

1. Produce the dependency/invariant map.
2. Create the `connection` facade and extract public context/shutdown types with re-exports.
3. Extract activity/admission/response tracking.
4. Extract transport progress and driver/deadline logic.
5. Extract deferred-body supervision.
6. Extract canonical request/service pipeline last, once its dependencies are stable.
7. Run targeted tests after every extraction rather than waiting until the end.
8. Run full verification and update architecture docs.
9. Add a closure record listing the resulting module ownership and confirming zero intentional wire/API behavior change.

## Handoff

After closure, proceed to Plan 181. Per-runtime observability will be easier to plumb once request/driver/activity responsibilities are explicit; do not combine that semantic/context change into this mechanical decomposition.