# Plan 195 — HTTP/3 Response-Timeout Corrective Qualification and Closure

## Status

**PLANNED — evidence/closure pass after Plan 194.**

Prerequisite: Plan 194 is implemented on the candidate commit and its targeted H3 tests are green. If Plan 194 is incomplete, stop; do not close this plan by inspection.

This plan does not add HTTP capability or attempt support-tier promotion. It proves that the Plan 194 H3 streaming-response timeout correction behaves as documented, that the Plan 193 historical trace is internally consistent, and that the broader H1/H2/H3 support claims remain conservative.

Expected support tiers after closure:

- HTTP/1.1: supported default/baseline;
- HTTP/2: experimental, opt-in, with Plan 191 blockers unchanged unless separately resolved;
- HTTP/3: experimental, opt-in, with Plan 192 dependency/readiness blockers unchanged unless separately resolved.

## Purpose

Close the post-Plan-193 corrective pass with reproducible evidence rather than relying on the implementation diff.

Plan 195 must answer:

1. Is a non-yielding H3 application response producer actually bounded by `response_write_timeout` after response commitment?
2. Does meaningful progress re-arm the deadline without converting the timeout into a total response-duration limit?
3. Can empty chunks falsely refresh the deadline?
4. Does producer timeout remain request/stream scoped so healthy siblings survive?
5. Are known-length stream accounting, normal EOS, producer-error, QUIC send timeout, and graceful shutdown semantics unchanged?
6. Do H1/H2 timeout contracts remain unchanged?
7. Does documentation now distinguish application producer progress from QUIC send progress accurately?
8. Does Plan 193's historical record accurately say promotion was preflight-blocked because Plan 192 was `BLOCKED`, while preserving the useful evidence inventory performed that day?
9. Are H3 promotion blockers still explicitly recorded, with no accidental support-tier promotion?

## Candidate freeze

Before running qualification, record in `release/plan-195-http3-response-timeout-corrective-qualification.md`:

- candidate commit SHA;
- Rust stable version and workspace MSRV;
- `h3`, `h3-quinn`, Quinn, Tokio, and rustls versions;
- operating system/architecture used for deterministic execution;
- effective `response_write_timeout` values used by targeted tests;
- whether Tokio paused time or real time is used;
- feature set under test;
- current Plan 192 upstream blocker status as a factual snapshot only.

Do not update H3 dependencies merely to execute this plan.

## Track A — Static implementation inspection

Inspect the exact `ResponseBody::Stream` branch in `send_canonical_response()` and record the implementation contract.

Qualification must confirm:

- `stream.send_response(...)` remains bounded by `response_write_timeout`;
- the application producer wait no longer uses an unbounded raw `response_stream.next().await` path;
- the producer budget uses a monotonic absolute deadline or an equivalent mechanism that cannot be refreshed by empty chunks;
- a non-empty chunk is sent through the existing bounded `send_bytes()` flow-control path;
- the producer deadline is re-armed only after meaningful forward progress according to the Plan 194 contract;
- `stream.finish()` remains bounded;
- producer timeout reaches the existing post-commit failure/reset path;
- no secondary HTTP status/body is attempted after response commitment;
- no new public H3/timeout type leaked into the stable canonical API.

A code pattern that wraps each `next()` call in a fresh relative timeout, including after empty chunks, does **not** pass this track.

## Track B — Stalled-before-first-item test

Run the focused H3 test where the service returns a `ResponseBody::Stream` promptly and the producer then remains pending without yielding its first item.

Required evidence:

- handler response-start succeeds before `handler_timeout`;
- H3 response headers may be committed normally;
- no DATA is produced;
- the request/response task terminates after the configured no-progress budget rather than remaining parked indefinitely;
- the send direction is reset/terminated through the post-commit H3 failure path;
- `RequestLifecycle` is cancelled according to the existing failure taxonomy;
- no second HTTP response is emitted;
- relevant counters/events reflect a timeout/failure rather than normal completion;
- the connection remains usable when the underlying transport is healthy.

Record the timeout configuration and observed ordering. Do not use an exact millisecond equality as an acceptance criterion.

## Track C — Progress-then-stall test

Run the case where the producer yields at least one non-empty body chunk, the chunk is successfully sent/received, and the producer then parks.

Verify:

- the first real chunk reaches the client;
- the subsequent no-progress budget begins/re-arms from meaningful progress rather than original response construction;
- the stalled stream terminates after the re-armed deadline;
- a known-length declaration, if used, does not cause the runtime to wait forever for missing bytes;
- post-timeout cleanup is prompt and bounded.

This test distinguishes the correction from a one-time “first chunk” timeout.

## Track D — Slow-but-progressing stream

Use a response stream that yields several non-empty chunks at intervals safely below `response_write_timeout`, with total duration greater than one timeout interval.

Acceptance:

- entire response succeeds;
- bytes arrive in order;
- known-length accounting, when used, remains exact;
- no timeout is emitted merely because total response duration exceeds the configured no-progress interval;
- the test demonstrates reset-on-progress semantics.

Run a large/unknown-length streaming variant if the existing fixture makes it cheap; do not add a broad performance harness.

## Track E — Empty-chunk semantics

Exercise at least these two shapes:

### E1. Empty then data

- producer yields one or more empty chunks;
- real data arrives before the original no-progress deadline;
- response succeeds;
- empty chunks do not alter body-length accounting.

### E2. Empty then pending

- producer yields empty chunks and then remains pending;
- timeout is measured against the last meaningful progress point, not refreshed per empty item;
- stream terminates as a no-progress failure.

If testing reveals that an always-ready infinite empty-chunk producer can starve the timer because it never yields to the runtime, document the reproducer. Add a minimal cooperative scheduling guard only if Plan 194 already authorizes it or create a new narrow plan; do not silently introduce an arbitrary chunk-count limit in Plan 195.

## Track F — Sibling-stream isolation

On one H3 connection:

- start stream A with a producer that will stall;
- while A is stalled, execute stream B and verify it completes successfully;
- allow A to hit its producer timeout;
- execute or complete another healthy sibling after A's timeout;
- verify the H3 connection remains usable unless the dependency reports a genuine connection-wide failure.

Also verify:

- A's `RequestLifecycle` cancellation does not cancel B;
- service/file/request admission counters for A return to baseline;
- connection-level counters are not double-decremented;
- no connection-wide `cancel_all()` is caused solely by A's application producer timeout.

This is a mandatory correctness gate.

## Track G — Existing response semantics regression

Re-run existing H3 tests covering:

- buffered `ResponseBody::Bytes`;
- file responses;
- known-length `ResponseBody::Stream` success;
- known-length underrun/overrun or mismatch handling;
- producer `Err`;
- client/peer close;
- response send failure;
- graceful shutdown;
- response/body lifecycle cleanup;
- max-request drain;
- body Reject/Buffer/Stream behavior;
- DATA without `Content-Length` correction from Plans 189–190;
- early-error stream scoping from Plan 192.

The Plan 194 timeout correction must not alter ordinary content, HEAD, range, conditional, error, or canonical response semantics.

## Track H — Transport flow-control separation

The corrective record must explicitly distinguish producer no-progress from QUIC/H3 send no-progress.

At minimum prove by inspection and existing tests that:

- waiting for application data is governed by the new producer deadline;
- once non-empty bytes exist, `send_bytes()` still bounds each awaited H3 `send_data()` operation under QUIC flow control;
- a producer timeout does not claim to measure actual packet transmission;
- a QUIC send timeout does not require the producer to be stalled;
- both failure paths remain bounded and stream-scoped through the same post-commit response failure boundary.

If a cheap deterministic receive-credit withholding test already exists, rerun it. Do not make external H3 tooling a prerequisite for this corrective plan.

## Track I — Shutdown races

Exercise at least one race between a stalled producer and server shutdown.

Verify:

- graceful shutdown can begin while the producer is parked;
- the configured graceful deadline remains authoritative for whole-server shutdown;
- no task survives forced drain/abort;
- lifecycle cancellation is first-reason-wins according to existing semantics;
- response timeout and shutdown do not double-release permits/counters.

There is no requirement that the producer timeout win a race with shutdown; either ordering is acceptable if cleanup and first-reason-wins semantics are deterministic and documented.

## Track J — Observability

Capture or assert the relevant `OpsContext` behavior for producer timeout.

Required properties:

- timeout is not counted as `ResponseStreamCompleted`;
- explicit producer `Err` remains distinguishable from no-progress timeout if the existing schema makes that distinction;
- reuse of `WriteStallTimeout` / existing write-stall counter is consistent with the timeout-reference definition if Plan 194 chose that path;
- event/counter emission occurs no more than once per timed-out response;
- no raw response data, QUIC connection identifiers, TLS secrets, or internal dependency errors are exposed in normal logs.

If Plan 194 deliberately leaves H3 send/producer timeout observability generic, record that exact boundary rather than inventing a stronger claim.

## Track K — H1/H2 non-regression

Run the existing H1/H2 deterministic suites and inspect timeout docs.

Acceptance:

- H1 still defines response-write progress through socket writes/`ProgressIo`;
- H2 still defines response progress through observable application-body polling with conservative connection fallback, not guaranteed stream-level wire progress;
- no H2 timeout behavior was silently changed to match H3;
- `connection_total_timeout` still applies to TCP/H1/H2 and not H3 under the Plan 192 lifetime decision;
- no Python behavior changes;
- minimal/default build remains free of H3 dependencies.

## Track L — Plan-history consistency audit

Search the repository for all references to Plan 193 and classify them.

Correct state:

- Plan 192: `BLOCKED` readiness gate;
- Plan 193: **closed at preflight because the prerequisite was blocked**;
- work actually performed under the Plan 193 closure commit: candidate freeze, upstream re-check, environment/tool availability inventory, deterministic gate rerun, and fail-closed promotion-gate probes;
- supported-tier H3 qualification itself was not eligible to proceed;
- H3 remained experimental;
- a future attempt requires a new scoped plan after readiness blockers resolve.

Audit at minimum:

- `plans/193-http3-supported-tier-promotion-qualification.md`;
- `release/plan-193-http3-supported-tier-qualification.md`;
- `plans/ROADMAP.md`;
- `README.md`;
- `architecture/http3.md`;
- `docs/release-contract.md`;
- `docs/api-stability.md`;
- `docs/dependency-policy.md`;
- `docs/http-primitives.md`;
- `docs/library-capability-matrix.md`;
- `docs/threat-model.md`;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`.

Do not edit immutable commit history. The goal is truthful tracked documentation.

## Track M — Timeout documentation audit

Search for all `response_write_timeout` claims and make sure H3 is described consistently.

The final docs must make clear:

- H3 response HEADERS send is bounded;
- H3 canonical stream producer wait is now bounded after Plan 194;
- empty chunks are not progress;
- each H3 DATA send is bounded under flow control;
- H3 finish is bounded;
- a slowly progressing response can exceed one timeout interval in total;
- `connection_total_timeout` does not apply to H3;
- QUIC idle timeout remains separate;
- H2 semantics remain intentionally different because Hyper exposes different observability.

Add a short Plan 194/195 corrective note to the Plan 192 readiness record rather than rewriting history to imply its original producer-timeout statement was accurate at execution time.

## Track N — Support-tier preservation

Before closure, verify no live document accidentally says:

- H2 is supported because Plan 191 ran;
- H3 is supported because Plan 193 exists;
- Plan 192 was READY;
- `h3#338` has a released fix when it does not;
- all `h3#262` termination paths are closed when residual paths remain;
- H3 has browser/two-family/adversarial/network/platform evidence when it does not.

Plan 195 must preserve the experimental H2/H3 tiers. A future support-promotion effort needs a new plan and fresh evidence.

## Track O — Verification matrix

Run at minimum:

```text
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
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
cargo audit
cargo deny check
bash scripts/qualify-http2.sh
bash scripts/qualify-http3.sh
```

The normal qualification scripts may still report unavailable external promotion evidence. Do not use Plan 195 to bypass their strict support-tier gates.

## Track P — Corrective qualification record

Create:

- `release/plan-195-http3-response-timeout-corrective-qualification.md`

The record must contain:

- candidate SHA and dependency versions;
- exact implementation semantic observed;
- targeted producer-timeout test results;
- sibling-isolation result;
- empty-chunk result;
- slow-progress result;
- resource/lifecycle/observability result;
- H1/H2 non-regression result;
- Plan 193 history-audit result;
- timeout-documentation audit result;
- full deterministic/MSRV/supply-chain commands and outcomes;
- final support tiers;
- remaining H2/H3 promotion blockers.

Do not call this an H3 support qualification record; it is a **corrective timeout/history qualification** record.

## Completion criteria

Plan 195 is CLOSED only when:

- [ ] Plan 194 implementation is present on the candidate.
- [ ] A producer that never yields is bounded after response commitment.
- [ ] A producer that stalls after real progress is bounded from its last meaningful progress point.
- [ ] Slow meaningful progress re-arms the no-progress budget and permits a total response duration longer than one timeout interval.
- [ ] Empty chunks cannot indefinitely refresh the deadline.
- [ ] Healthy H3 sibling streams survive one producer timeout.
- [ ] No request/service/file/connection permit leak is observed.
- [ ] Existing H3 body/response/shutdown tests remain green.
- [ ] H1/H2 timeout semantics and tests are unchanged.
- [ ] Plan 193 tracked status is corrected to preflight-blocked/non-executed promotion qualification.
- [ ] Plan 193's useful evidence inventory is preserved.
- [ ] Timeout docs accurately distinguish H3 producer and send progress.
- [ ] The Plan 192 historical overstatement receives a transparent corrective note.
- [ ] H2 and H3 remain experimental.
- [ ] `h3#338` and residual `h3#262` promotion blockers remain explicit unless a later authorized change genuinely resolves them.
- [ ] Full deterministic/MSRV/supply-chain matrix is green.
- [ ] A release qualification record is committed.

## Stop condition

After Plan 195 closes successfully, stop this corrective line. Do not create another general H2/H3 implementation plan merely because promotion evidence remains unavailable. Future work should be triggered by concrete conditions:

- H2: availability of browser + macOS/Windows runtime evidence and cleanup of the remaining trailer-scope standards item;
- H3: a maintained disposition for `h3#338`, closure/mitigation of the residual `h3#262` stream-termination paths, and an environment capable of the independent-client/browser/adversarial/network/platform promotion matrix.

Those are future support-tier efforts, not defects in the Plan 194/195 correction.