# Plan 281 — Direct runtime external admission ownership

## Status

**CLOSED — implementation, local qualification, and exact-SHA hosted CI passed. Registry artifact qualification is Plan 286.**

Hosted CI: run 36067050590, SHA `c62faf59b19913eb49b97d371435122c5a8fb6ac` (success).

## Purpose

Allow advanced direct-server embedders to make service-call and tunnel
admission externally owned, avoiding double semaphores and competing 503
authorities while preserving EggServe's current bounded admission by default.

This plan does not remove any default semaphore.

## Current authority

`RuntimeState` currently always constructs:

- `file_stream_semaphore`;
- `service_semaphore` from `max_in_flight_requests`;
- `tunnel_semaphore` from `max_active_tunnels`.

The direct H1 pipeline then acquires service admission before
`Service::call`, and tunnel acceptance acquires the tunnel semaphore before
launching the tunnel task.

This is correct for standalone/direct servers. It can be redundant for a host
that already performs request and tunnel admission before EggServe dispatch.

## Architecture decision

Add explicit admission ownership, separate from numeric limits.

Suggested vocabulary:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionOwner {
    EggServe,
    External,
}

#[derive(Debug, Clone)]
pub struct AdmissionOwnership {
    pub service_calls: AdmissionOwner,
    pub tunnels: AdmissionOwner,
}
```

Exact names may change. Requirements:

- default is EggServe-owned for both;
- ownership is selected explicitly;
- no `usize::MAX` sentinel;
- existing `RuntimeState::new/try_new/with_ops` preserve current behavior;
- new policy-aware construction is additive.

## Track A — RuntimeState representation

Refactor runtime admission so an externally-owned pool is represented
explicitly rather than by a giant semaphore.

Acceptable internal shapes include:

```rust
enum AdmissionGate {
    Bounded(Arc<Semaphore>),
    External,
}
```

or an equivalent non-allocating enum.

Do not expose raw semaphore mutation as the public ownership API.

Existing public accessors may remain for compatibility only if their semantics
stay truthful. If an accessor cannot return a semaphore in external mode,
prefer adding a new typed accessor and retaining the old accessor only on the
legacy/all-internal state rather than fabricating a semaphore.

## Track B — Service-call admission

When EggServe-owned:

- unchanged `try_acquire_owned` behavior;
- unchanged deterministic 503 on saturation;
- unchanged counters/events;
- permit remains scoped to pre-response `Service::call` as documented.

When External:

- no EggServe service permit is acquired;
- EggServe never synthesizes a service-admission 503;
- handler timeout/panic/body/framing policy remains independently controlled;
- the service is invoked immediately once protocol validation completes.

Document that the host is responsible for bounding application work in this
mode.

## Track C — Tunnel admission

When EggServe-owned:

- current `max_active_tunnels` behavior remains;
- rejected acceptance remains deterministic 503;
- active-tunnel accounting remains exact.

When External:

- EggServe does not acquire its tunnel semaphore;
- acceptance is not rejected solely by EggServe capacity;
- tunnel validation, single-accept commitment, upgrade ownership, lifecycle
  cancellation, and tracked task shutdown remain mandatory;
- active tunnel gauges may still count actual active tunnels, but
  `tunnels_rejected` must not increase for an admission decision EggServe did
  not make.

The external service/host is responsible for its own tunnel cap.

## Track D — Keep file-stream admission EggServe-owned

Do not externalize `max_file_streams` in this plan.

File-stream response execution is an EggServe-owned adapter/resource concern,
not a generic application-admission policy. The existing bounded file-stream
pool remains mandatory whenever file responses are used.

If later evidence shows a generic need, open a separate plan.

## Track E — High-level Server connection cap remains internal

`Server::start_with_service` owns accepted TCP connections and should keep
`max_connections` admission unchanged.

Caller-owned `serve_http1_connection` already does not acquire the
high-level server's connection permit. Do not add a fake "external
max_connections" switch where no duplicate authority exists.

## Track F — Composition with Plan 280

Admission ownership must compose independently with Plan 280 policy ownership.

Required combinations:

- all defaults;
- external service admission only;
- external tunnel admission only;
- both external;
- external deadlines but internal admission;
- internal deadlines but external admission.

No setting should implicitly change another.

## Track G — Failure and observability truth

Add/adjust typed events as needed so:

- internal saturation is distinguishable from service rejection;
- external mode produces no internal saturation event;
- active request/tunnel gauges remain truthful;
- shutdown always releases internal permits exactly once;
- cancellation cannot leak a permit.

Do not log application identifiers or payloads.

## Track H — Tests

Required direct-runtime tests:

- default service saturation still returns 503;
- external service admission allows concurrent calls beyond
  `max_in_flight_requests` without an EggServe 503;
- external mode still permits host/service-level rejection;
- default tunnel saturation still returns 503;
- external tunnel admission permits acceptance beyond the configured internal
  count without an EggServe admission rejection;
- tunnel tasks remain tracked/drained;
- mixed modes do not leak permits;
- immediate shutdown and shutdown-with-active-work remain bounded;
- two `RuntimeState` instances do not accidentally share gates;
- per-runtime ops counters remain isolated.

Use small deterministic local fixtures.

## API compatibility

Prefer additive construction such as:

```rust
RuntimeState::with_admission_policy(...)
```

or a policy-aware builder introduced by Plan 282.

Do not change existing constructor behavior.

If implementation discovers that preserving an existing public semaphore
accessor is impossible without lying, stop and record the API issue for Plan
285's version decision rather than silently changing semantics.

## Acceptance criteria

- [ ] Plan 279 is closed before production changes.
- [ ] service admission has explicit EggServe/External ownership.
- [ ] tunnel admission has explicit EggServe/External ownership.
- [ ] default behavior and limits are unchanged.
- [ ] no giant-semaphore sentinel represents external ownership.
- [ ] file-stream admission remains bounded and internal.
- [ ] high-level TCP connection admission remains unchanged.
- [ ] no internal 503/counter is produced for externally-owned admission.
- [ ] tunnel lifecycle/commitment/shutdown semantics remain unchanged.
- [ ] Plan 280 ownership settings compose independently.
- [ ] Rust 1.89 and repository CI remain green.

## Non-goals

- No queueing policy.
- No priority/fairness scheduler.
- No application worker pool.
- No downstream-specific admission callback.
- No publication in this plan.
