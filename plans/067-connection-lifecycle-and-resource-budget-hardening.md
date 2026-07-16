# Phase 67 — Connection Lifecycle and Resource-Budget Hardening

## Goal

Make every remotely controlled connection phase explicitly bounded, cancellation-safe, observable, and testable. Separate admission budgets for TCP acceptance, TLS handshakes, active HTTP connections, request bodies, file streams, and Python callbacks; add keep-alive and shutdown controls required for internet-facing operation.

## Preconditions

- Plan 066 defines public deployment profiles and proxy timeout layering.
- Existing connection/file-stream semaphores, header/write timeouts, body limits, and lifecycle state machine are present.

## Non-goals

Do not add:

- per-IP rate limiting or banning;
- distributed quotas;
- authentication or tenant accounting;
- a general metrics/admin HTTP endpoint;
- middleware;
- proxy behavior;
- application scheduling;
- HTTP/2 connection management.

## Resource model

Define distinct budgets for:

1. accepted sockets awaiting protocol setup;
2. TLS handshakes;
3. active HTTP connections;
4. active request-body ingestion/streaming;
5. active static file bodies;
6. active directory listing generation;
7. active Python callbacks;
8. graceful-shutdown work.

Do not reuse one semaphore where it obscures which resource is exhausted or permits one phase to monopolize unrelated capacity.

## Track A — Lifecycle state inventory

Document the exact state machine from accept through close:

- listener ready;
- socket accepted;
- admission permit acquired;
- optional TLS handshake;
- HTTP connection established;
- request header read;
- service invocation;
- optional body ingestion/drain;
- response planning;
- response streaming;
- keep-alive idle;
- next request;
- graceful close;
- forced cancellation.

For each transition identify:

- owner task;
- permit(s) held;
- timeout/deadline;
- cancellation token;
- error category;
- logging/observer event;
- cleanup action.

The architecture note must expose any state with no deadline or ambiguous permit ownership.

## Track B — Accepted socket and connection admission

Clarify when connection permits are acquired.

Required behavior:

- no unbounded accepted-socket queue in user space;
- admission failure closes promptly without allocating request state;
- listener errors do not spin;
- accepted socket options are deliberate;
- permits release on TLS failure, parser failure, client disconnect, timeout, panic boundary, and shutdown;
- saturation behavior is documented: silent close or minimal response only when protocol state permits.

Add separate counters/events for admission rejection and active connection saturation.

## Track C — TLS handshake budget interface

Introduce a separate handshake budget even if full TLS hardening lands in Plan 069.

This phase should provide:

- configurable maximum concurrent handshakes;
- handshake deadline distinct from header deadline;
- cancellation on shutdown;
- transfer from handshake permit to HTTP connection permit without double counting or gaps;
- plaintext builds with no unused complexity exposed to users;
- stable configuration validation.

Plan 069 owns adversarial TLS behavior and policy defaults.

## Track D — Header and request-line limits

Ensure explicit configuration and enforcement for:

- maximum request-line bytes;
- maximum request-target bytes;
- maximum header field count;
- maximum aggregate header bytes;
- header-read total deadline;
- optional per-progress inactivity policy only if clearly separated from total deadline.

Limits must be enforced before service invocation and before large allocations. Error behavior should be deterministic and connection closure should follow uncertain parse/framing states.

Add boundary tests at limit minus one, exact limit, and limit plus one.

## Track E — Keep-alive controls

Add:

- keep-alive idle timeout;
- maximum requests per connection;
- explicit connection-close behavior after the maximum;
- reset rules for per-request deadlines;
- no reset of connection lifetime counters due to malformed requests;
- correct HTTP/1.0 and HTTP/1.1 persistence semantics;
- graceful-shutdown behavior for idle keep-alive connections.

Defaults should favor bounded utility/static origin operation rather than unlimited reuse. Document compatibility impact.

Tests must use multi-request raw TCP connections, not only separate client requests.

## Track F — Body and drain budget integration

Confirm body policies from Milestone 4 compose with lifecycle budgets:

- body read deadline is bounded;
- byte limit is enforced during both handler consumption and runtime drain;
- incomplete body `Close` is the safe default;
- bounded drain cannot outlive graceful-shutdown deadline;
- body tasks cannot retain connection permits after terminal failure;
- a timed-out Python callback does not silently free callback capacity while code continues running;
- body error and service error do not cause two responses.

Add permit baseline assertions after every error path.

## Track G — File and directory work budgets

Retain or refine:

- maximum concurrent file streams;
- separate or shared directory-listing generation budget with explicit rationale;
- maximum listing entries;
- maximum generated listing bytes;
- bounded metadata work per request;
- cancellation during listing and file streaming;
- write deadline for slow readers.

Do not read entire large files into memory. Do not generate unbounded directory HTML before applying limits.

## Track H — Python callback budget

Define exact semantics for Python callbacks:

- maximum concurrent callbacks;
- queueing behavior, preferably no unbounded queue;
- callback wait timeout;
- execution timeout behavior;
- timed-out callback continues until Python returns because unsafe termination is not possible;
- callback permit remains held until actual completion;
- shutdown does not wait indefinitely for unkillable Python code after forced deadline;
- listener/runtime resources are released independently.

Expose these semantics in Python docs without presenting callbacks as an application-server runtime.

## Track I — Graceful and forced shutdown

Add a configurable graceful-shutdown deadline.

Required sequence:

1. stop accepting new sockets;
2. cancel pending handshakes;
3. reject or stop beginning new requests according to connection state;
4. allow eligible active responses to finish within deadline;
5. wake idle keep-alive connections;
6. cancel body drain/listing/file tasks at deadline;
7. close transport resources;
8. transition lifecycle state exactly once;
9. allow `wait()` to return deterministically.

Test repeated shutdown calls and shutdown from callbacks/other threads. Avoid deadlock when shutdown is initiated by work owned by the runtime.

## Track J — Observability hooks

Define stable internal events/counters for:

- admission rejected;
- handshake active/timeout/failure;
- connection active/closed;
- header timeout/limit rejection;
- keep-alive idle timeout;
- requests-per-connection exhaustion;
- body timeout/limit/drain failure;
- file-stream saturation;
- listing saturation/limit;
- callback saturation/timeout;
- graceful deadline exceeded;
- forced task cancellation.

Events may feed structured logs or library callbacks. Do not add a network metrics server.

## Required tests

### Saturation

- exceed each budget independently;
- verify unrelated budgets retain capacity;
- verify deterministic rejection/close behavior;
- verify permits return to baseline.

### Timeouts

- no TLS progress;
- partial headers;
- slow but progressing headers exceeding total deadline;
- keep-alive idle;
- body stall;
- slow response reader;
- listing stall/cancellation;
- callback timeout;
- graceful deadline.

### Connection sequencing

- multiple valid requests up to maximum;
- maximum plus one;
- malformed request before maximum;
- successful bounded body drain then next request;
- failed drain prevents next request;
- shutdown during idle, header read, body read, service, and response.

### Resource stability

- repeated saturation cycles;
- repeated start/stop;
- file descriptor/handle count;
- task count or test instrumentation;
- semaphore counts;
- memory trend smoke tests.

Run plaintext, TLS-feature, Rust service, Python static server, and callback paths where applicable.

## Configuration and compatibility

Add configuration fields with:

- validated nonzero/range semantics;
- conservative defaults;
- CLI flags only where operators need control;
- Python constructor support only where part of server primitives;
- stable names and documentation;
- no silent use of zero as unlimited unless explicitly designed.

Update API snapshots and migration documentation for any signature changes.

## Release criteria

Add required gates for:

- each resource budget;
- keep-alive idle and request-count behavior;
- total header deadline and limits;
- graceful/forced shutdown;
- permit recovery;
- Python callback lifecycle;
- TLS-feature handshake budget integration.

Invalidate on runtime, config, TLS adapter, body, static response, Python server, and shutdown changes.

## Acceptance criteria

- Every connection phase has a bound or explicit finite lifecycle.
- Budgets are independently observable.
- No error/cancellation path leaks permits, tasks, sockets, files, or handles.
- Keep-alive idle and request count are bounded.
- Graceful shutdown completes or transitions to deterministic forced shutdown.
- Python timeout semantics are honest and bounded around unkillable code.
- All limits apply before unbounded allocation or work.

## Stop conditions

Stop and document if:

- a proposed quota requires per-user/authentication scope;
- cancellation would require unsafe Python thread termination;
- an unlimited queue is introduced to preserve compatibility;
- timeout layering with proxies cannot be explained deterministically;
- one shared semaphore makes resource ownership unprovable.

## Handoff

Plan 068 uses these limits in hostile edge/origin desynchronization tests. Plan 069 uses the handshake budget and shutdown model for direct TLS qualification.
