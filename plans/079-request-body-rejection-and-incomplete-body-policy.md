# Plan 079 — Request-Body Rejection and Incomplete-Body Policy

## Goal

Make request-body policy enforceable before user service invocation, implement truthful close/drain behavior, and ensure body framing, limits, timeouts, keep-alive reuse, cancellation, and shutdown compose without side effects or resource leaks.

## Preconditions

- Plan 077 provides structured connection ownership, cancellation, and terminal close hooks.
- Plan 078 provides one explicit service invocation boundary for static, custom Rust, and Python services.
- Plan 075 has registered body-policy findings and required evidence.

## Non-goals

Do not add:

- application middleware;
- multipart/form parsers;
- form or JSON decoding;
- unbounded request buffering;
- background body processing after response completion;
- automatic retries;
- new HTTP methods or framework semantics.

## Defect statement

`RequestBodyPolicy::Reject` is documented as rejecting a request before service invocation, but the current path can invoke a service with an empty body and only afterward attempt a short drain. Handler side effects can therefore occur for a request the runtime claims to reject.

The incomplete-body `Drain` policy is also advertised without a complete active bounded-drain implementation. Configuration parameters that do not affect behavior are misleading and unsafe for keep-alive decisions.

## Track A — Body/framing state model

Define a request body state model before service invocation.

At minimum classify:

- no declared body;
- positive `Content-Length`;
- zero `Content-Length`;
- transfer-encoded body;
- conflicting `Transfer-Encoding` and `Content-Length`;
- duplicate or inconsistent `Content-Length`;
- malformed framing;
- body forbidden by runtime policy;
- body allowed and unconsumed;
- body partially consumed;
- body completely consumed;
- body limit exceeded;
- body timeout or transport failure.

For each state define whether the service may be invoked, whether a response may be sent, whether draining is safe, and whether the connection may be reused.

## Track B — Pre-service rejection gate

Move body-policy enforcement ahead of service invocation.

For `Reject`:

1. parse and validate framing;
2. determine whether the request declares or begins a body according to the documented policy;
3. construct the rejection response;
4. choose close or bounded drain behavior;
5. never call static, custom Rust, or Python user code.

Instrument service invocation in tests so zero invocations is proven rather than inferred from response status.

Define stable response behavior for:

- positive content length;
- transfer encoding;
- conflicting framing;
- body larger than declared runtime maximum;
- unexpected body on methods commonly treated as bodyless;
- `Expect: 100-continue`.

Do not send `100 Continue` for a body that will be rejected.

## Track C — Body policy API review

Review all public policy types and fields.

Each advertised option must map to real behavior. Choose among:

- `RejectAndClose`;
- `RejectAndDrain { max_bytes, timeout }` where safely implementable;
- `Allow` with a bounded streaming body;
- incomplete-body close after service response;
- incomplete-body bounded drain after service response.

Avoid ambiguous combinations where one enum controls acceptance and another silently overrides connection reuse.

If active drain cannot be completed safely, remove the drain option and document close as the only supported behavior. Do not retain ignored fields.

## Track D — Bounded active drain

If drain remains supported, implement it against the real incoming body stream.

Required bounds:

- maximum bytes to drain;
- total or inactivity deadline, explicitly named;
- cancellation on server shutdown;
- no drain beyond graceful-shutdown deadline;
- no unbounded buffering;
- no service invocation during pre-service rejection drain;
- one owner for the body stream;
- connection reuse only after confirmed clean framing boundary.

A drain that exceeds byte/time limits, encounters malformed framing, or loses transport integrity must close the connection.

A drain task must remain owned by the connection task and cannot be detached.

## Track E — Post-service incomplete bodies

For allowed-body services, define what occurs when the service returns before consuming the entire body.

`Close` behavior:

- send response only when protocol state permits;
- set/force connection close;
- stop reading the body;
- release body resources;
- do not parse another request.

`Drain` behavior, if retained:

- drain within configured bounds;
- do not allow a second response;
- preserve the service's response unless drain failure makes it unsafe to continue;
- close on any uncertainty;
- permit next request only after complete successful drain.

Document whether response transmission may overlap with draining. Prefer a simple ordering that keeps framing provable.

## Track F — Limits and timeouts

Enforce body limits during every path:

- service consumption;
- pre-service rejection drain;
- post-service incomplete-body drain;
- Python iteration/adaptation;
- any eager buffering path.

Boundary tests must cover limit minus one, exact limit, and limit plus one.

Timeouts must be distinct from handler and response-write timeouts. A slow body cannot hold a connection permit indefinitely.

## Track G — Error and terminal-response ownership

Guarantee one terminal response/close decision.

Cover races between:

- body limit and handler error;
- handler completion and body transport error;
- drain timeout and shutdown;
- client disconnect and response start;
- Python callback timeout and body ownership;
- malformed framing discovered before or during body read.

The connection supervisor must observe body task failures. No body task may emit a second response after the main service path commits one.

## Track H — Rust and Python body parity

Ensure Rust and Python handlers observe the same acceptance, limits, and terminal connection semantics.

Python-specific requirements:

- rejected bodies do not invoke callbacks;
- iterator/read errors map to stable Python exceptions;
- callback timeout does not falsely release body/callback permits while Python code still runs;
- installed-wheel tests cover partial consumption and drain/close behavior;
- no unbounded cross-thread queue is introduced.

## Track I — Observability

Add internal events/counters for:

- body rejected before service;
- framing rejected;
- body limit exceeded;
- body read timeout;
- drain started/completed/limited/timed out/cancelled;
- connection closed due to incomplete body;
- service returned with unread body;
- service invocation suppressed.

Do not log body contents.

## Required raw-wire tests

Use raw HTTP/1 connections for:

- rejected positive Content-Length with bytes sent;
- rejected body headers without bytes sent;
- `Expect: 100-continue` rejection;
- transfer-encoded rejection;
- TE+CL ambiguity;
- duplicate inconsistent Content-Length;
- allowed body fully consumed then next keep-alive request;
- allowed body partially consumed with Close;
- allowed body partially consumed with successful bounded Drain;
- drain byte limit exceeded;
- drain timeout;
- client disconnect during drain;
- shutdown during drain;
- body limit boundaries;
- malformed chunking;
- smuggled second-request attempts after failed/incomplete drain.

Tests must assert both response bytes and service invocation counts.

## Configuration and migration

- Remove or rename options that do not map to implemented behavior.
- Validate nonzero drain limits and durations.
- Keep safe close behavior as the default.
- Add Rust, CLI, and Python parity only for options genuinely supported by each frontend.
- Update snapshots and migration notes.
- Plan 080 will consolidate the final owner of these fields; avoid duplicate definitions.

## Documentation changes

Update:

- request-body policy reference;
- runtime state machine;
- connection reuse rules;
- security/threat model framing section;
- Rust and Python examples;
- reverse-proxy deployment guidance where body handling affects upstream reuse;
- finding registry and release criteria.

## Acceptance criteria

- `Reject` is enforced before any static, Rust custom, or Python service invocation.
- Rejected `Expect: 100-continue` requests do not receive an invitation to send the body.
- Every advertised drain/close option is operational and tested; unimplemented options are removed.
- Draining is bounded by bytes, time, shutdown, and connection ownership.
- A failed or incomplete drain cannot permit keep-alive reuse.
- Body limits apply identically during service consumption and runtime drain.
- No race path produces two responses or a detached body task.
- Rust and installed Python paths have parity for documented behavior.
- Raw-wire tests demonstrate that hidden second requests cannot cross a rejected or failed drain boundary.
- Exact-SHA evidence is recorded and blocking findings are closed independently.

## Stop conditions

Stop and default to close-only behavior if:

- Hyper body ownership cannot support a provably bounded drain without invoking the service;
- response/drain overlap makes the framing boundary ambiguous;
- a drain task cannot remain owned by the connection supervisor;
- Python adaptation requires unbounded buffering;
- any desynchronization case reaches user code unexpectedly.

## Handoff

Plan 080 consolidates the body-policy configuration fields and validates frontend parity. Plan 083 later reruns stateful/raw-wire conformance after Release C response-planner changes.