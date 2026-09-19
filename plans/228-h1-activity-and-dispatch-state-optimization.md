# Plan 228 — H1 activity tracking and dispatch-state optimization

## Prerequisite

Plan 227 current-HEAD profiling must be complete enough to characterize H1
response-frame bookkeeping and small-response dispatch overhead.

## Purpose

Reduce H1 per-frame and per-request coordination overhead without weakening
write-stall detection, keep-alive/lifecycle accounting, service admission, or
the public `Service` contract.

The planning review found that `TrackedBody::poll_frame()` currently records
producer-frame progress through `ConnectionActivity::response_poll_progress`.
That path takes a mutex, calls `Instant::now()`, searches a per-response
vector, and notifies the driver for each emitted frame. The H1 driver currently
bases `response_write_timeout` on `state.last_write`, which is updated by
`ProgressIo` only after actual forward socket writes. If the Plan 227 audit
confirms no H1 decision consumes producer-progress timestamps, maintaining that
vector on every frame is redundant.

The same H1 service wrapper also clones numerous independent `Arc`/context
handles and dispatches through an internal `Arc<dyn Fn> -> Pin<Box<dyn Future>>`
layer per request.

## Goals

- Make actual transport write progress the explicit H1 write-stall authority.
- Remove H1 producer-frame bookkeeping that has no timeout/lifecycle consumer.
- Preserve response start/end accounting exactly once.
- Collapse request-pipeline shared state behind one connection-level state
  object so each request performs fewer independent refcount operations.
- Remove unnecessary internal dynamic dispatch where it can be done without a
  public API change.
- Keep `ServiceFuture` and the external `Service` trait source-compatible in
  this plan.

## Work

### 1. Prove timeout ownership before deleting state

Audit all references to:

- `ConnectionActivity::response_poll_progress`;
- the `response_poll_progress` vector;
- `TrackedBody::poll_frame`;
- `ProgressIo::record_write`;
- H1 write-stall deadline computation;
- H2/H3 stream-progress implementations.

Record the result in the plan implementation notes/tests.

Deletion is allowed only if:

- H1 stall expiry depends solely on actual forward transport write progress;
- no deferred-body, tunnel, shutdown, or cancellation path reads the producer
  timestamp;
- H2/H3 do not rely on this H1-private storage through a shared hook.

If any consumer exists, replace the per-frame mutex/vector design with the
minimum data structure required by that consumer instead of deleting it.

### 2. Remove redundant per-frame work

When the ownership proof permits:

- remove `response_poll_progress: Mutex<Vec<(request_id, Instant)>>`;
- remove the per-frame `response_poll_progress(request_id)` call from
  `TrackedBody::poll_frame`;
- stop waking the connection driver merely because the body producer yielded
  another frame;
- retain `response_started`, `response_finished`, outstanding counters,
  response-body drop handling, and driver notification for state transitions
  that can create/clear deadlines.

Do not replace the mutex vector with an atomic timestamp unless a real consumer
requires producer progress. Eliminating dead state is preferable to optimizing
dead state.

### 3. Preserve write-stall semantics with explicit tests

Add deterministic regressions proving:

- producer yields data but peer does not read -> timeout still fires from lack
  of socket progress;
- peer reads slowly but forward writes continue inside the timeout budget ->
  no false timeout;
- producer itself stalls before yielding another frame -> behavior remains
  consistent with the documented H1 write-timeout contract;
- body finishes/drops/errors -> outstanding state is released once;
- HEAD/empty/error responses do not leave outstanding state behind;
- keep-alive idle timing starts only after the prior response is terminal;
- shutdown and connection-total timeout still cancel and drain correctly.

Tests must distinguish application-producer progress from socket-write progress
in their names and assertions.

### 4. Collapse per-connection pipeline state

Introduce one internal connection/pipeline state structure containing the
shared values currently cloned separately into each request future, for example:

```text
PipelineState<S>
  service
  config
  RuntimeState/admission handles
  activity
  request registry
  ConnectionContext
  connection id
  ops context
```

Exact field placement may differ, but:

- one `Arc<PipelineState<S>>` should be the normal per-request shared clone;
- do not duplicate semaphores outside `RuntimeState` merely to satisfy this
  structure;
- keep immutable configuration shared;
- keep request-local lifecycle/tunnel/body state request-local.

### 5. Simplify the internal Hyper service wrapper

Where Rust/Hyper type bounds permit, replace the
`Arc<dyn Fn(Request) -> Pin<Box<dyn Future>>>` wrapper with a named generic
internal service type whose `call` method captures/clones the single pipeline
state handle.

Do not change the public `eggserve_server::Service` trait or require
downstream services to use a new future type under this plan.

If the internal future still must be boxed for Hyper trait compatibility,
retain the one necessary box and document why; the target is removing
avoidable layers, not forcing an unstable type-system redesign.

### 6. Optional validated-connection fast path

Only if Plan 227 connection-churn profiling shows configuration validation is
material:

- add a crate-private server-owned entry point that accepts an already
  validated `RuntimeConfig`;
- keep the public caller-owned connection entry point validating
  hand-constructed/untrusted configuration;
- prove both paths reach the same connection driver.

Do not bypass validation for public embedders.

## Measurement requirements

A/B against the Plan 227 source on the same machine/session:

- 1 KiB custom bytes response at concurrency 1/16/64;
- 1 KiB static response at concurrency 1/16/64;
- 1 MiB file response;
- 1 MiB known/unknown application streams;
- one slow-reader/write-stall scenario.

Record CPU, p50/p95/p99, throughput, allocation/lock evidence where available,
and errors.

Retain a change when it either:

- shows a reproducible improvement in its targeted workload without a
  meaningful tail-latency/resource regression; or
- removes demonstrably dead synchronization/dynamic-dispatch complexity while
  remaining performance-neutral within measurement noise.

Do not retain a more complex implementation for a statistically noisy or
unreproducible microbenchmark win.

## Non-goals

- No public `Service` API redesign.
- No removal of write-stall protection.
- No relaxed semaphore/admission limits.
- No protocol-tier changes.
- No custom executor/runtime.
- No change to H2/H3 timeout semantics unless required for shared-code
  correctness; protocol-specific work needs separate qualification.

## Acceptance criteria

- [ ] H1 producer-progress ownership is explicitly audited.
- [ ] Redundant per-frame locks/notifies are removed or reduced to the proven
      minimum required by semantics.
- [ ] Actual forward socket writes remain the H1 stall-progress authority.
- [ ] Response start/end/outstanding accounting remains exactly-once.
- [ ] Per-request shared-state cloning is reduced behind one pipeline state
      handle.
- [ ] No public service API break is introduced.
- [ ] Deterministic timeout/lifecycle regressions pass.
- [ ] Same-machine A/B evidence is recorded for the affected workloads.
- [ ] Full H1/direct-vs-compatibility parity tests pass.
