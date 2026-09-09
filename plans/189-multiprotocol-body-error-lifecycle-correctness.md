# Plan 189 — Multiprotocol Request-Body, Error, and Lifecycle Correctness

## Status

**PLANNED — narrow corrective implementation after Plans 183–188.**

Baseline: `main` at or after the Plan 188 closure (`a96966bbd899cbc5ca4df722a4d38b8fd3eb3d05` when this plan was written). Re-read current code before implementation and preserve any later valid changes.

This plan does not reopen the protocol architecture. HTTP/1 remains the minimal/default supported baseline, native HTTP/2 and HTTP/3 remain opt-in experimental Rust features, and the Python `http.server` compatibility facade remains HTTP/1.1-shaped.

## Purpose

Correct the narrow semantic gaps found in the post-Plan-188 review without adding new server capabilities or redesigning the canonical service boundary.

The required end state is:

- request-body rejection is based on actual protocol body state rather than `Content-Length` alone;
- H2/H3 body-policy rejection suppresses service invocation whenever request DATA is present, including requests without `Content-Length`;
- HTTP/3 runtime-generated errors use the same canonical status/representation policy as H1/H2 instead of a second abbreviated error-body table;
- HTTP/3 request lifecycle cancellation has parity with the public transport-neutral lifecycle contract on peer loss, runtime shutdown, timeout, and stream failure;
- HTTP/2 response-stall behavior is described and tested according to what Hyper can actually observe, with no false claim that body polling is equivalent to per-stream wire progress;
- protocol-program documentation and release bookkeeping reflect the implementation that actually landed;
- no unsupported H2/H3 extension or broader framework/edge-server behavior enters scope.

## Findings that require correction

### 1. Reject-body policy can miss H2 DATA without Content-Length

The shared H1/H2 Hyper pipeline currently determines `has_body` from:

- a positive `Content-Length`; or
- `Transfer-Encoding`.

That is an HTTP/1 framing model. HTTP/2 has no Transfer-Encoding framing requirement and may carry DATA without `Content-Length`. A request with H2 DATA and no `Content-Length` can therefore reach `RequestBodyPolicy::Reject`, receive an empty canonical body, and still invoke the service.

This violates the policy contract: `Reject` means a request carrying content must be rejected without service invocation. It must not mean only “reject when a peer declared a body length.”

### 2. H3 has the equivalent body-presence ambiguity

The H3 adapter rejects a body under `Reject` when `Content-Length > 0`, but a request without Content-Length currently stops the receive direction, creates an empty canonical body, and proceeds to invoke the service.

HTTP/3 also allows DATA without Content-Length. The adapter must distinguish an end-of-request immediately after headers from a request that actually supplies DATA.

### 3. H3 owns a second, incomplete runtime error representation table

The H1/H2 runtime has a centralized error representation path that preserves the selected wire status and derives a generic body from the standard reason phrase when appropriate. The H3 adapter currently owns a separate `error_response()` status switch with only a subset of statuses.

That duplication can produce semantically misleading combinations, for example retaining one wire status while emitting the fallback `internal server error` representation because that status is absent from the H3-local table.

Runtime-generated error status and representation must have one canonical authority shared across transports.

### 4. H3 does not yet fully satisfy the public request-lifecycle cancellation contract

H1/H2 maintain a connection registry of live request lifecycles and cancel them on connection-level termination such as peer disconnect, server shutdown, and hard runtime timeout. This is important for downstream services that wait on `RequestLifecycle::cancelled()` without actively polling body or response I/O.

The H3 adapter has body timeout cancellation and stream control, but it does not currently maintain equivalent connection-wide lifecycle registration/cancellation for all accepted requests. A downstream H3 task must not remain unaware of a closed QUIC connection simply because it is not currently reading or writing the stream.

### 5. H2 “response progress” currently observes application-body polling, not guaranteed wire progress

`TrackedBody` updates the H2 per-request activity timestamp when Hyper polls a response body and receives a frame. This correctly prevents sibling socket writes from refreshing an actively stalled application body producer. It does **not** prove that a frame already handed to Hyper has made stream-level flow-control or socket progress.

Once the body reaches end-of-stream, EggServe no longer owns a safe public H2 stream-reset/send-capacity handle. Hyper may still hold bounded buffered data. The implementation is therefore safer than aggregate socket-progress accounting, but the term “per-stream response write progress” can overstate the guarantee.

This plan must either use a current safe public Hyper/h2 capability that truly observes stream-local send progress or document/test the narrower producer/poll-progress guarantee. Do not depend on private Hyper internals.

### 6. Protocol-program bookkeeping is stale

Plan 183 still reads as planned even though its scope/product-contract gate was implemented before Plans 184–188. The roadmap also contains “until Plan 183 is implemented” language that is no longer current.

The repository remains versioned `0.1.2` while the migration guide correctly records a pre-1.0 stable API transition that must release as `0.2.0` or later. That is acceptable for an unreleased development branch only if release tooling/documentation prevents an accidental `0.1.x` publication of the breaking line.

## Design constraints

1. **Do not reopen the architecture.** Keep the canonical `Request`/`Service`/`Response` boundary and protocol adapters established by Plans 184–188.
2. **Do not add new protocol features.** No trailers, WebSockets, extended CONNECT, WebTransport, datagrams, push, 0-RTT application semantics, proxying, routing, middleware, ACME, or Python H2/H3 surface.
3. **Preserve H1 behavior.** Existing H1 parser/framing/body-policy semantics and Python compatibility behavior must remain regression-compatible except for an independently discovered bug.
4. **Reject based on actual body state, not header inference alone.** `Content-Length` remains useful for early size rejection and exact-length validation but is not proof that content is absent.
5. **Keep stream errors stream-scoped on multiplexed protocols.** An ordinary rejected body or body timeout must not terminate healthy sibling H2/H3 requests unless the transport library only exposes a safe connection-level fallback for the specific condition.
6. **One runtime error representation authority.** H3 must not maintain a parallel user-visible status/body table.
7. **Lifecycle cancellation is best-effort but prompt.** Preserve the public coarse reason taxonomy; do not add transport-specific public cancellation variants merely for QUIC.
8. **Do not claim unobservable wire semantics.** If Hyper does not expose a safe stream-local send-progress/reset capability, state that limitation explicitly and keep H2 experimental.
9. **Keep the minimal graph minimal.** No new dependency is justified by these corrections unless current Hyper/H3 APIs genuinely require it and it remains feature-gated.
10. **No CI matrix explosion.** Add focused deterministic coverage to existing H2/H3 feature jobs rather than new combinatorial jobs.

## Track A — Make request-body presence protocol-aware

### A1. Define the policy question precisely

Separate these concepts:

- declared length (`Content-Length`), used for early limit checks and exact-length validation;
- transfer framing, relevant to H1 only;
- whether the transport indicates the request is already end-of-stream at header completion;
- whether DATA is actually observed;
- the service's effective `RequestBodyPolicy`.

Do not introduce a public `BodyPresence` API unless a real downstream consumer needs it. An internal helper or protocol-specific decision is sufficient.

### A2. H1 behavior remains unchanged

For H1 preserve the existing hardened semantics:

- positive Content-Length means body present;
- Transfer-Encoding means a framed body is present/possible and `Reject` suppresses service invocation;
- unread rejected body bytes force connection close rather than risking request smuggling/reuse;
- duplicate/conflicting framing remains rejected as today.

Regression tests must prove no H1 request-reuse behavior changes.

### A3. H2 body-presence decision

Use Hyper/http-body state rather than H1 headers to decide whether an H2 request is already bodyless.

At implementation time inspect the pinned Hyper `Incoming`/`Body` API. Preferred behavior:

- if the incoming H2 body is already end-of-stream at header completion, `Reject` may invoke the service with an empty canonical body;
- if it is not end-of-stream, treat the request as potentially carrying DATA and reject/cancel that request stream without invoking the service;
- a positive Content-Length continues to allow immediate early rejection before waiting for DATA;
- H2 `Transfer-Encoding` must not become the body-presence mechanism.

If Hyper cannot reliably expose header-time end-of-stream state, use the safest bounded alternative and document it. Do not read an unbounded body merely to determine presence.

### A4. H3 body-presence decision

For H3, a missing Content-Length is ambiguous. Under `RequestBodyPolicy::Reject`, determine whether the request ends immediately or produces DATA before invoking the service.

Preferred bounded algorithm:

1. keep the receive stream alive after request headers;
2. if `Content-Length > 0`, reject immediately, send the canonical 413 representation, and issue request-stream cancellation/STOP_SENDING;
3. when Content-Length is absent or zero, perform the minimum protocol-level receive needed to distinguish immediate end-of-stream from DATA, under `body_read_timeout` and existing QUIC flow-control limits;
4. if DATA is observed, reject with canonical 413, cancel receive, and do not invoke the service;
5. if the request ends without DATA, invoke the service with an empty canonical body;
6. if a transport/body error occurs during this presence check, map it through the canonical runtime error/lifecycle path and do not invoke the service.

Do not buffer an entire request for the Reject policy. At most retain the bounded first DATA chunk long enough to make the rejection decision, then discard/cancel it.

### A5. Content-Length exactness

For Buffer and Stream policies, preserve current declared-length limit checks and add/retain exactness validation:

- premature EOF before declared length is an error;
- receiving more bytes than declared length must also be rejected if the transport library does not already enforce it before the adapter;
- request-body byte limit remains independent of Content-Length presence.

Audit both H2 and H3 paths for “more than declared” behavior rather than only “less than declared.” Delegate to Hyper/h3 when the library guarantees the invariant, but record that ownership in tests/docs.

## Track B — Add regression tests for body rejection without Content-Length

### B1. H2 deterministic integration test

Add an H2 test that sends a request with DATA but no Content-Length under the default/static Reject policy.

Acceptance:

- response is the configured canonical body-rejection status (currently 413);
- service invocation count remains zero for that request;
- no `Connection`/`Transfer-Encoding` response field appears;
- a sibling H2 stream completes successfully;
- request/service admission permits return to baseline.

Also test a genuinely bodyless H2 request with no Content-Length still reaches the service normally.

### B2. H3 deterministic adapter/integration test

Add the equivalent H3 case with no Content-Length:

- DATA present -> reject, service not invoked, stream cancelled at request scope;
- no DATA -> service invoked normally;
- sibling request remains usable where the in-process H3 harness can prove it;
- timeout/error during the body-presence probe releases the request task and does not leak permits.

Use the existing in-tree H3 stack for deterministic correctness. Independent-client evidence remains Plan 190/release qualification work and is not required to implement the bug fix.

### B3. Declared zero plus DATA

Explicitly test `Content-Length: 0` followed by DATA for H2/H3 if the client stack allows constructing it. Expected behavior is protocol rejection by the dependency stack or EggServe rejection before service invocation; never silently treat the DATA as an empty request.

## Track C — Create one canonical runtime-error representation path

### C1. Extract a transport-neutral error constructor

Add an internal canonical helper that builds a `crate::primitives::canonical::Response` from:

- selected wire status;
- HEAD/body suppression state;
- `ErrorRepresentationPolicy`;
- standard `Allow` metadata where required by current runtime policy.

The helper must:

- preserve the selected wire status;
- use the standard reason phrase when one exists for the generic Minimal representation;
- emit an empty body when the status has no standard phrase rather than claiming another status;
- obey body-forbidden response semantics;
- obey HEAD semantics;
- avoid reflecting internal/service error text;
- leave Date/Server/denylist application to the existing final response policy boundary.

### C2. Make H1/H2 wrappers consume the canonical owner where practical

Prefer making the existing Hyper error helpers thin wrappers around the new canonical constructor so there is exactly one representation table/algorithm.

Do not force a broad response-pipeline rewrite. A narrow canonical constructor plus Hyper conversion wrapper is sufficient.

### C3. Remove the H3-local abbreviated status switch

Replace H3 `error_response()` or equivalent local status/body switch with the canonical runtime error constructor.

Audit all H3 error paths, including:

- malformed request metadata;
- body-policy rejection;
- body read timeout;
- body limit/framing error;
- service admission exhaustion;
- service timeout/panic/error;
- internal response-construction failure before commitment.

After response commitment, preserve stream-reset behavior rather than attempting to send a second HTTP error response.

### C4. Cross-protocol golden tests

For representative statuses, compare canonical semantic results across H1/H2/H3:

- 400;
- 405 including `Allow` when applicable;
- 408;
- 413;
- 414;
- 431;
- 500;
- 503;
- at least one nonstandard/unassigned 4xx/5xx status supported by `StatusCode`.

The protocols may differ only in framing/hop-by-hop fields that are forbidden or irrelevant on H2/H3. Status, generic representation, content metadata, Date/Server/privacy policy, and HEAD/body suppression must agree.

## Track D — Give H3 full RequestLifecycle cancellation parity

### D1. Reuse the existing lifecycle registry concept

Make `ConnectionRequests` or a narrowly renamed protocol-neutral equivalent reusable by H3. Do not create a second QUIC-only registry with different semantics.

Update comments that assume HTTP/1 has at most one live request; H2/H3 may retain multiple weak lifecycle observers concurrently. Prune dead weak references on registration and/or bounded maintenance so the registry remains proportional to live/recent streams.

### D2. Register every H3 request lifecycle

Every accepted H3 request must have a runtime-retained weak observer of its `RequestShared`, including requests whose body is immediately complete.

A downstream service may clone `RequestLifecycle` and retain it after `Service::call` returns; connection termination must still wake that observer.

### D3. Connection-level cancellation

When an H3 connection becomes unusable, cancel all live request lifecycles promptly with the best available existing reason:

- explicit server shutdown/drain forced termination -> `ServerShutdown`;
- peer/QUIC connection disappearance -> `PeerDisconnected` when clearly peer-originated;
- protocol/transport failure -> `TransportFailure` where classification is available;
- hard runtime/body timeout -> `ConnectionTimeout` as already used by the public taxonomy.

First reason wins. Do not leak raw QUIC errors into the public reason taxonomy or client response body.

### D4. Stream-local cancellation

Retain a lifecycle handle in each H3 request task long enough to cancel it when:

- request receive fails;
- body limit/timeout makes further application I/O impossible;
- response send/reset fails after commitment;
- the request task is aborted by drain deadline.

An ordinary stream failure must not cancel sibling lifecycles.

### D5. Shutdown ordering

During graceful server shutdown:

1. stop admitting new H3 connections/requests according to H3 GOAWAY semantics;
2. allow accepted request tasks to complete within the shared grace period;
3. do **not** pre-cancel healthy request lifecycles merely because drain started if they are still allowed to finish;
4. when the grace deadline forces task/connection termination, cancel remaining lifecycles with `ServerShutdown` before/with abort so downstream waiters wake promptly.

Document the exact point at which cancellation fires.

## Track E — Qualify H3 lifecycle behavior deterministically

Add focused tests using a custom service that clones `RequestLifecycle` and waits independently of request-body polling.

Cover:

- peer closes QUIC connection while service task is waiting -> lifecycle resolves;
- server shutdown with an accepted long-lived request -> lifecycle resolves if/when the request is forcibly terminated;
- one H3 request stream fails -> that lifecycle resolves while sibling request/lifecycle remains usable;
- body timeout -> request lifecycle reports `ConnectionTimeout` under current taxonomy;
- normal request/response completion on a still-live connection does not spuriously cancel lifecycle.

Where exact peer-vs-transport classification is unstable across Quinn versions, assert cancellation and a documented allowed reason set rather than brittle private error matching.

## Track F — Correct the H2 response-progress contract

### F1. Re-check current Hyper/h2 public APIs

Before changing behavior, inspect the pinned/current Hyper and h2 server APIs for a safe supported way to observe one stream's outbound flow-control/send progress or reset that stream after response commitment.

Use such a capability only if it is public, maintained, and can be integrated without exposing Hyper/h2 types through EggServe's public `Service` boundary.

Do not use private fields, version-fragile downcasts, or dependency forks merely to make the timeout label stronger.

### F2. If true stream send progress is available

If a safe hook exists:

- arm `response_write_timeout` per H2 stream when response emission begins;
- refresh only on actual stream-local forward send progress;
- timeout/reset only the stalled stream where the API supports it;
- sibling stream traffic must not refresh the timer;
- preserve global connection lifetime as a defense-in-depth ceiling.

Add flow-control tests that deliberately stall one stream while a sibling progresses.

### F3. If true stream send progress is not available

If the current public stack still cannot expose that information safely:

- retain the current per-request body producer/poll progress tracking because it prevents sibling masking while EggServe still owns response production;
- rename internal helpers/comments away from `h2_response_stalled`/“write progress” if needed to reflect producer/poll progress accurately;
- document that bytes already handed to Hyper are bounded by Hyper's explicitly configured per-stream send buffer and the hard connection lifetime, not by a guaranteed EggServe per-stream wire no-progress timer;
- keep H2 experimental;
- ensure release/capability docs do not claim a safe public per-stream reset hook or full wire-progress timeout.

### F4. Preserve H1 semantics

Do not weaken H1 `response_write_timeout`, which does observe forward socket write progress through `ProgressIo` while a response body is outstanding.

Docs must distinguish H1 socket progress, H2 producer/poll progress or true stream progress depending on available API, and H3 stream send-call progress.

## Track G — Documentation and protocol-program bookkeeping

### G1. Close Plan 183 bookkeeping

After the corrective implementation is complete, update Plan 183 status from PLANNED to an executed/implemented state that matches what actually happened. Do not rewrite historical design text except where a stale status creates contradiction.

Update `plans/ROADMAP.md` language that says the live contract remains authoritative “until Plan 183 is implemented.” The live contract was already changed before H2/H3 implementation.

### G2. Record Plans 189–190 in the roadmap

Register this corrective implementation plan and Plan 190's qualification closure as a narrow post-188 sequence. Make clear that they do not authorize any new protocol surface.

### G3. Keep support tiers truthful

Until Plan 190 closes:

- H1 = supported baseline;
- H2 = experimental;
- H3 = experimental;
- Python compatibility = H1.1-shaped.

The corrections alone do not promote H2 or H3.

### G4. Preserve the 0.2 release boundary

Do not publish the current stable-Rust API changes as a `0.1.x` release.

Implementation may leave development metadata at the repository's current version if that is the existing workflow, but release preparation must use the Plan 182 synchronized version path and publish this breaking pre-1.0 line as `0.2.0` or later.

Add/update release checklist language or a cheap existing metadata guard if necessary so a future release agent cannot interpret the current `0.1.2` workspace value as authorization to ship a patch containing the documented breaking changes.

Do not create an elaborate new versioning subsystem for this single transition.

## Track H — Observability and security review

The corrections must not add untrusted values to logs.

For new body-presence/lifecycle paths:

- count/emit low-cardinality rejection/cancellation events using existing event kinds where appropriate;
- do not log raw request DATA, authority, path, QUIC IDs, reset tokens, or dependency error strings at warning/error severity without sanitization;
- keep service invocation suppression observable for rejected bodies;
- make permit/gauge transitions exactly once under timeout/cancellation races.

Do not add new public metrics types solely for these fixes unless existing counters cannot represent a security-relevant invariant.

## Verification

Run the existing deterministic project gates plus both protocol feature representatives. Expected minimum:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo +1.88 check --workspace --all-targets --features http2,tls
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
bash scripts/test-python-wheel.sh
```

Also run the focused new body-policy, lifecycle, and error-parity tests directly so failures are easy to diagnose.

Do not require external H3 clients or privileged network emulation to implement this plan; those belong to Plan 190 qualification.

## Acceptance criteria

- [ ] H1 request-body Reject behavior is unchanged and regression-tested.
- [ ] H2 DATA without Content-Length under Reject is rejected before service invocation.
- [ ] a bodyless H2 request without Content-Length still reaches the service.
- [ ] H2 body rejection remains stream-scoped and a sibling stream survives.
- [ ] H3 DATA without Content-Length under Reject is rejected before service invocation.
- [ ] a bodyless H3 request without Content-Length still reaches the service.
- [ ] H3 Reject presence detection is bounded by existing timeout/flow-control policy and does not buffer the whole body.
- [ ] declared-length exactness is owned/tested for both multiplexed protocols, including over-declaration/under-declaration behavior as applicable.
- [ ] H3 no longer owns an abbreviated runtime error status/body switch.
- [ ] one canonical runtime-error representation path supplies H1/H2/H3 semantic status/body policy.
- [ ] cross-protocol error golden tests cover 400/405/408/413/414/431/500/503 and an unassigned status case.
- [ ] every H3 request can expose a lifecycle that is cancelled promptly on unusable connection/stream termination.
- [ ] server shutdown, peer disconnect, stream failure, and body timeout H3 lifecycle cases are covered without cancelling healthy siblings.
- [ ] H3 graceful drain does not pre-cancel requests that are still allowed to finish, but forced shutdown wakes remaining lifecycle waiters.
- [ ] H2 response-progress implementation or documentation matches the actual safe public Hyper capability; no body-poll event is mislabeled as guaranteed wire progress.
- [ ] H1 socket-write no-progress semantics remain unchanged.
- [ ] Plan 183/roadmap status text is synchronized and Plans 189–190 are registered.
- [ ] release documentation still requires the documented stable API break to ship as `0.2.0` or later rather than a `0.1.x` patch.
- [ ] H2 and H3 remain experimental pending Plan 190 evidence.
- [ ] minimal/default builds remain free of H3/QUIC dependencies and Python compatibility remains H1.1-shaped.
- [ ] no WebSocket, WebTransport, datagram, extended CONNECT, 0-RTT application request, push, proxy, routing, middleware, ACME, upload, or app-server feature is added.

## Suggested implementation order

1. Add failing H2/H3 regression tests for DATA-without-Content-Length under Reject.
2. Implement protocol-aware body-presence handling while preserving H1 behavior.
3. Add declared-length exactness tests and close any discovered H2/H3 gap.
4. Extract the canonical runtime-error response constructor and switch H3 to it.
5. Add cross-protocol error semantic golden tests.
6. Reuse/generalize the lifecycle registry for H3 and register every request lifecycle.
7. Wire H3 connection/stream/shutdown cancellation and add detached-waiter tests.
8. Audit current Hyper/h2 stream-progress/reset capabilities; improve implementation only through safe public APIs, otherwise narrow naming/docs.
9. Run the full deterministic/MSRV/Python/supply-chain-compatible verification set.
10. Synchronize Plan 183/roadmap/release-boundary bookkeeping.
11. Hand the corrected implementation to Plan 190 for evidence-based closure.

## Handoff

Plan 189 is complete when the known post-188 correctness gaps are fixed without changing EggServe's product scope or protocol support tiers.

Do not use completion of this plan as evidence that HTTP/2 or HTTP/3 is production-supported. Plan 190 owns corrected-regression qualification, external protocol checks where available, documentation synchronization, and the final post-correction support-tier statement.