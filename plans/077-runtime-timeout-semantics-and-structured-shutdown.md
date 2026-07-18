# Plan 077 — Runtime Timeout Semantics and Structured Shutdown

## Goal

Correct the runtime's timeout and task-ownership model so configuration names match actual enforcement, active progress is not mistaken for a timeout, persistent listener errors cannot spin, and shutdown does not report completion until all server-owned work has finished or been aborted and joined.

This plan replaces detached-task behavior with structured ownership shared by built-in static serving and custom services.

## Preconditions

- Plan 075 has established the baseline, finding registry, and release containment.
- Existing runtime configuration, connection executor, lifecycle state machine, and shutdown tests are identified.
- The built-in and custom-service accept loops are both in scope.

## Non-goals

Do not:

- redesign request-body policy beyond the hooks required by Plan 079;
- add a general scheduler or worker pool;
- implement per-IP quotas;
- add HTTP/2 or HTTP/3;
- add a metrics server;
- optimize file buffering;
- preserve misleading timeout names solely for compatibility.

## Defect statement

The current `response_write_timeout` wraps the entire Hyper connection future. This acts as a total connection lifetime limit and can terminate healthy keep-alive connections or large responses that continue making progress.

The current shutdown loop stores connection `JoinHandle` values in a vector, waits until a deadline, and can drop remaining handles. Dropping a Tokio `JoinHandle` detaches its task; it does not cancel it. The server can therefore report `Stopped` while server-owned work remains active.

Listener errors are also insufficiently classified, risking a hot loop under persistent resource exhaustion or fatal listener failure.

## Track A — Timeout taxonomy and compatibility decision

Inventory every timeout and deadline in the runtime, TLS adapters, static service, body handling, and Python callback path.

Define distinct semantics for:

- accept/listener backoff;
- TLS handshake total timeout;
- request-header total timeout;
- request-body total or inactivity timeout;
- handler execution timeout;
- response-write inactivity timeout;
- keep-alive idle timeout;
- optional maximum connection age;
- graceful-shutdown deadline.

For each field specify:

- when the clock starts;
- whether progress resets it;
- what constitutes progress;
- which task owns enforcement;
- terminal response/close behavior;
- permit and task cleanup;
- compatibility/migration impact.

If progress-aware response write enforcement cannot be safely implemented in this plan, remove or rename `response_write_timeout` so it accurately describes a connection-total deadline. Do not retain the current name with inaccurate behavior.

## Track B — Progress-aware response write enforcement

Preferred implementation: enforce a response-write inactivity deadline at the body/transport boundary.

Required behavior:

- the timer resets only after actual bytes are successfully written or meaningful transport progress is confirmed;
- a response that steadily streams longer than the configured duration remains valid;
- a client that stops reading is closed within the inactivity bound;
- timeout cancellation releases file handles, body streams, semaphores, and connection ownership;
- HEAD and body-forbidden responses do not create spurious write timers;
- graceful shutdown can shorten the remaining allowed lifetime without creating two competing terminal paths.

Avoid treating body production alone as write progress if bytes remain blocked in an unbounded internal buffer.

If Hyper does not expose a reliable per-write hook at the current abstraction level, document the limitation and choose an accurately named total-response or total-connection deadline until a transport wrapper is justified.

## Track C — Connection task ownership with `JoinSet`

Replace ad hoc `Vec<JoinHandle<_>>` management with `tokio::task::JoinSet` or an equivalent owner that supports:

- spawning accepted connection tasks;
- continuously reaping completed tasks;
- observing panics and task errors;
- graceful close signaling;
- aborting all remaining tasks;
- joining every aborted task;
- deterministic empty-state checks.

Do not duplicate this logic between static and custom-service paths. Extract a shared connection supervisor.

The supervisor must own every connection task from spawn until joined. No connection task may outlive the supervisor unless explicitly transferred to another documented owner, which is not expected in this scope.

## Track D — Shutdown state machine

Implement one explicit shutdown sequence:

1. transition from `Running` to `Stopping` exactly once;
2. stop accepting new sockets and close/drop listener ownership;
3. notify active connections to begin graceful shutdown;
4. wake or terminate idle keep-alive connections;
5. continue reaping completed tasks until the grace deadline;
6. abort all remaining connection tasks at the deadline;
7. join all aborted tasks and record join outcomes;
8. release runtime-owned permits and handles;
9. transition to `Stopped` only after the task set is empty;
10. return a structured shutdown result indicating graceful, forced, or failed completion.

Repeated shutdown calls must be idempotent. Concurrent callers must observe one terminal result.

## Track E — Panic and cancellation containment

Define behavior for:

- connection task panic;
- service future panic where panic boundaries exist;
- task cancellation during file streaming;
- cancellation during request-body handling;
- cancellation during Python callback wait;
- runtime thread shutdown while tasks are active.

A panic in one connection must not silently disappear. It should be observed, logged through the operational event interface, and reflected in counters or shutdown diagnostics without crashing unrelated connections unless policy requires process termination.

Plan 079 owns detailed body policy; this plan must provide cancellation hooks and task ownership that Plan 079 can use.

## Track F — Listener error classification and backoff

Replace silent `accept` retries with explicit classification.

Required behavior:

- interrupted/transient errors retry immediately or with minimal yielding;
- resource exhaustion such as descriptor/handle pressure uses bounded exponential or capped backoff;
- fatal listener errors transition lifecycle to `Failed` and stop the accept loop;
- shutdown interrupts any backoff promptly;
- repeated errors are rate-limited in logs;
- the listener error result is observable to `wait()`/server owner.

Tests may use an injected listener abstraction or fault hook rather than depending on difficult OS-level exhaustion.

## Track G — Shared static/custom runtime path

Audit `start`, `start_with_service`, builder output, TLS/plaintext paths, and Python-owned server startup.

Required architecture:

- one listener supervisor;
- one connection task supervisor;
- one shutdown state machine;
- service-specific request dispatch supplied as a parameter/type;
- no copy-pasted timeout or drain loop with divergent behavior.

Plan 078 may change custom-service ownership, but this plan should leave a clear insertion point rather than preserving duplicated runtime code.

## Track H — Lifecycle observability

Define internal events or structured results for:

- listener started;
- listener transient error;
- listener backoff;
- listener fatal failure;
- connection task spawned/completed/panicked;
- graceful shutdown started;
- grace deadline exceeded;
- forced abort count;
- abort/join failure;
- shutdown completed with mode and duration.

Do not add a network metrics endpoint. Existing logging or observer hooks are sufficient.

## Required tests

### Timeout semantics

- keep-alive connection remains usable beyond the configured write inactivity duration while idle policy permits it;
- steadily progressing large response may exceed the duration;
- stalled client triggers response-write inactivity timeout;
- explicit maximum connection age, if retained, is tested separately;
- header, body, handler, write, idle, and shutdown deadlines do not alias one another;
- timeout cancellation releases file-stream and connection permits.

### Structured shutdown

- zero active connections;
- one active completing connection;
- many active connections, some completing before deadline;
- tasks remaining after deadline are all aborted and joined;
- no callback/request code executes after `Stopped` is observed, except explicitly documented unkillable external code that is no longer server-owned;
- repeated and concurrent shutdown calls;
- shutdown during accept, header read, body read, service execution, response stream, and keep-alive idle;
- static and custom-service modes produce equivalent lifecycle results;
- plaintext and TLS-feature builds use the same supervisor behavior.

### Listener faults

- transient error retries;
- resource exhaustion backs off without spinning;
- fatal error transitions to `Failed`;
- shutdown interrupts backoff;
- log/event rate limiting prevents amplification.

### Stability

- repeated start/stop cycles;
- repeated forced-shutdown cycles;
- task count returns to baseline;
- file descriptor/handle count returns to baseline within bounded platform noise;
- semaphores return to full capacity;
- no detached connection tasks remain according to test instrumentation.

## Configuration and migration

Update Rust, CLI, and Python-facing timeout names only where they currently expose the incorrect field.

Requirements:

- deprecated aliases may emit warnings for one transition period only if they can map without preserving incorrect semantics;
- generated help and API docs state total versus inactivity behavior;
- zero durations and invalid combinations return configuration errors;
- shutdown deadline remains distinct from response deadlines;
- migration notes include before/after examples.

Plan 080 will unify final configuration ownership. Avoid creating a second temporary authority that Plan 080 must later unwind.

## Documentation changes

Update:

- runtime architecture/lifecycle state machine;
- timeout reference;
- graceful shutdown contract;
- Python callback/runtime lifecycle docs where affected;
- operational error behavior;
- release criteria and evidence invalidation mapping;
- finding registry and corrective status.

## Acceptance criteria

- No timeout field named as a response-write timeout is implemented as an undocumented whole-connection lifetime.
- A steadily progressing response is not terminated solely because total duration exceeds the write inactivity bound.
- A stalled writer is terminated within the documented bound.
- Every accepted connection task remains owned until completion or abort and join.
- The server does not transition to `Stopped` while its connection task set is non-empty.
- Static and custom-service modes use one shared supervisor and shutdown implementation.
- Listener resource errors back off and fatal errors become observable lifecycle failures.
- Repeated start/stop and forced-shutdown tests show no unbounded task, socket, handle, or permit growth.
- Rust, CLI, and Python documentation accurately describe timeout and shutdown semantics.
- Exact-SHA evidence is recorded for all required platforms and feature paths.

## Stop conditions

Stop and record a blocking architecture finding if:

- current Hyper integration cannot distinguish write progress from body production and no accurate interim semantic can be exposed;
- any server-owned task cannot be aborted or joined due to ownership loss;
- custom-service and static runtime paths cannot be unified without first changing the public service API;
- shutdown completion depends on unkillable Python execution while still claiming that work is server-owned;
- dedicated lifecycle evidence cannot be produced.

## Handoff

Release A closes after Plans 075–077 pass their gates and receive independent review for the Windows ownership and detached-task findings. Plan 078 then adopts the shared supervisor for custom-service ownership and connection metadata. Plan 079 uses the cancellation and termination hooks for body close/drain semantics.