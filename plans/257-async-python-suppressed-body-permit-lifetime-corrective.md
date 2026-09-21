# Plan 257 — Async-Python suppressed-body permit lifetime corrective

## Purpose

Correct one narrow resource-lifetime defect discovered after the Plans 251–256
campaign was closed.

Current planning baseline:

```text
cd6061a97f6538d013f0fac2adc1653a96097dd0
docs: clarify metadata-SHA record in Plan 256 closure (metadata-only, final)
```

The exact Plan 256 implementation candidate
`4c145421c851fffa5e1f6762a7ef742c5db1e5d8` passed remote CI run
`35653232800`; the current metadata-only head also passed normal CI. This is
therefore a post-closure corrective, not a reopening of the Rust authority,
typing, topology, or broad async-parity work.

Plan 254 correctly fixed eager application-iterator advancement for HEAD and
body-forbidden responses by making the async producer wait for a first native
body pull. However, source review of the resulting bridge found that an
unpulled response can retain its async application permit until
`response_write_timeout_secs` expires.

The defect is narrow but real because the default Python async admission bound
is finite (`max_async_tasks` defaults to `max_python_callbacks`, currently
8). A burst of suppressed streaming responses can therefore temporarily consume
all async permits and cause unrelated otherwise-valid requests to fail fast with
503 until the first-pull timeout expires.

Plan 258 is the mandatory qualification/closure pass.

## Confirmed mechanism

The current Python bridge in
`crates/eggserve-python/python/eggserve/lowlevel.py` does the following for an
async streaming response:

1. the request acquires one `AsyncServer` application semaphore permit;
2. returning `_AsyncStreamMarker` transfers permit ownership away from the
   dispatch coroutine;
3. `_bridge_streaming` immediately creates a producer task and registers a
   done callback that releases the permit;
4. the producer task waits on `first_pull.wait()`, bounded by
   `response_write_timeout_secs`;
5. the sync consumer is implemented as a Python generator `sync_gen()`;
6. the generator's `finally` cancels the producer when the consumer is
   dropped.

For ordinary streamed responses, the first `next()` enters the generator and
the cleanup `finally` is active.

For HEAD and body-forbidden responses, the Rust/PyO3 conversion correctly drops
the Python iterable without advancing it. A Python generator that has never
been entered does **not** execute its body or its `finally` when closed or
destroyed. Therefore the producer-cancellation path in `sync_gen()` is never
armed. The producer remains parked on `first_pull` until its timeout or server
shutdown, and the permit remains owned until the producer task completes.

The Plan 254 HEAD/204 tests prove that application iteration does not occur, but
they do not prove immediate permit/task release after suppression.

## Scope

This plan changes only internal async-Python streaming lifetime ownership.

It must preserve:

- every existing public Python import, class, method, property, argument, and
  return shape;
- every Rust public item and crate boundary;
- the H1-only status of the Python async substrate;
- the current bounded queue and backpressure model;
- the current application admission limit and fail-fast 503 semantics;
- native/canonical ownership of HTTP body-suppression policy;
- HEAD/body-forbidden "do not advance application iterable" semantics;
- stream truncation/error privacy behavior;
- response-write timeout semantics for genuinely stalled active producers;
- shutdown/task tracking;
- sync Python response streaming behavior;
- H2/H3 support tiers.

This plan must **not** duplicate the canonical HTTP payload-permission table in
Python merely to pre-classify HEAD/1xx/204/205/304 locally.

## Track A — add a deterministic failing regression first

Before changing production code, add a regression that demonstrates the
retained permit.

Use the installed-wheel async test harness and explicit synchronization; do not
rely on waiting for the 30-second default timeout.

Minimum reproducer:

1. configure `max_async_tasks=1`;
2. set `response_write_timeout_secs` to a comfortably large value so expiry
   cannot make the test pass accidentally;
3. handler returns an async streamed response for a HEAD request;
4. the producer records if its application iterable is ever advanced;
5. after the HEAD response completes, immediately issue an ordinary buffered
   GET through the same `AsyncServer`;
6. require the GET to succeed normally rather than receive bridge overload
   503;
7. require the streamed application iterable to remain unadvanced.

The baseline should fail at step 6 because the suppressed response still owns
the sole async permit.

Add equivalent coverage for a body-forbidden status (at least 204). If the
canonical fixture makes 304 easy to express, cover that too.

Do not use `time.sleep` to wait for the first-pull timeout.

## Track B — prove the Python generator lifetime premise

Add a small unit-level regression or explanatory fixture showing that the
current cleanup assumption is invalid:

```python
def g():
    try:
        yield ...
    finally:
        cleanup()

it = g()
it.close()  # before first next()
# cleanup() has not run
```

The repository does not need a generic Python-language test if the corrected
bridge test already captures the same behavior structurally, but the final code
comment must not continue claiming that an unentered generator's `finally`
will cancel the producer.

## Track C — introduce explicit stream-lifetime ownership

Replace the cleanup mechanism that depends on entering the generator frame with
an explicit lifetime owner whose cleanup is valid even when the iterable is
never pulled.

Preferred implementation shape is a private iterator/lifetime object, for
example conceptually:

```text
_AsyncStreamBridgeIterator
    queue
    producer task
    event loop
    first-pull signal
    response-write timeout
    closed / signalled state

__iter__ -> self
__next__:
    signal first pull exactly once
    obtain next queue item with bounded wait
    validate bytes / EOF / producer error
close / finalization:
    idempotently cancel producer if pending
```

The exact class/name is not prescribed. Equivalent internal designs are
acceptable if they prove the same ownership properties.

Critical requirement: cleanup must be attached to the iterable object lifetime
itself (or an equivalent native drop owner), not to a generator frame that may
never start.

### Acceptable alternatives

A Rust/PyO3-side drop/close owner is acceptable if it can guarantee the same
behavior without adding public API or invoking application iteration.

Deferring producer task creation until the first native pull is also acceptable
if the pre-commit admission reservation remains bounded and can be released
immediately when the unpulled body is discarded. Do not reacquire an async
permit after the response is already committed unless the design proves that
this cannot turn ordinary admission pressure into post-commit truncation.

## Track D — exactly-once permit ownership

Make permit ownership explicit across all stream states.

Required state transitions:

```text
dispatch acquired
  -> buffered/non-stream response
       dispatch releases permit
  -> stream response
       bridge lifetime owns permit
          -> normal producer EOF
               release once
          -> producer error
               release once
          -> active consumer drop/disconnect
               cancel producer -> release once
          -> HEAD/body-forbidden drop before first pull
               cancel/finish producer -> release once immediately
          -> server shutdown
               cancel producer -> release once
          -> first-pull/no-progress timeout
               producer finishes -> release once
```

Do not add a second semaphore or hidden queue.

The permit-release callback may remain task-owned if the unpulled-body lifetime
owner reliably cancels/completes that task immediately.

## Track E — retain single HTTP suppression authority

The Rust conversion in
`crates/eggserve-python/src/server/sync_handler.rs` already owns suppression
through canonical status/body rules and request method information. Preserve
that architecture.

The corrective should not introduce a Python copy such as:

```python
if request.method == "HEAD" or status in (...):
    ...
```

unless the value is used only as a defensive optimization and canonical Rust
remains the authoritative decision. The correctness path must work when Rust
drops an iterable without ever polling it.

This also protects future canonical policy changes from Python drift.

## Track F — normal-stream behavior regression

The new lifetime owner must not regress ordinary streaming.

Cover:

- first pull starts/permits application iteration;
- ordered multiple chunks;
- empty chunks ignored as today;
- bounded queue backpressure;
- non-bytes item truncation;
- producer exception after commitment;
- exact known length;
- under/overrun behavior;
- trailers;
- client disconnect during streaming;
- producer stall/no-progress timeout;
- server shutdown;
- synchronous iterable convenience path used by `AsyncResponse.stream`.

The existing Plan 254 lifecycle matrix should remain the base; add only the
cases needed to prove the new ownership mechanism.

## Track G — suppressed-response repetition / resource closure

Add a deterministic repeated test stronger than the single-response reproducer.

With `max_async_tasks=1` or another deliberately small bound:

- issue repeated streaming HEAD responses and verify each next request can be
  admitted immediately;
- repeat for 204/body-forbidden responses;
- interleave suppressed stream responses with ordinary buffered GETs;
- prove the producer/application iterator was never advanced for suppressed
  bodies;
- prove bridge-owned task state returns to baseline after each request or
  bounded batch without server shutdown and without waiting for
  `response_write_timeout_secs`.

Tests may inspect private `AsyncServer` task/semaphore state because this is an
internal bridge invariant; do not expose a new production inspection API.

## Track H — shutdown and race cases

Qualify the lifetime owner when:

- server shutdown races with an unpulled suppressed body;
- disconnect occurs before first body pull;
- producer task is scheduled but has not yet observed `first_pull`;
- cancellation and timeout race;
- cleanup is requested twice.

All cleanup must be idempotent and the semaphore must never over-release.

## Track I — comments and documentation truthfulness

Correct the current `lowlevel.py` comments that state an unpulled generator's
`finally` cancels the producer.

Document the actual owner after the fix.

User-facing docs should not need changes unless they currently promise a
different externally observable lifetime. The existing semantic promise
remains:

- HEAD/body-forbidden responses do not consume application stream state;
- streaming producers hold bounded application admission only while actually
  owned/active;
- shutdown/disconnect cancels bridge-owned work.

## Required qualification

At minimum run:

```sh
python3 scripts/check-python-release-metadata.py
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
```

Focused Python tests must include:

- the new `max_async_tasks=1` HEAD permit-release reproducer;
- body-forbidden permit release;
- repeated suppressed-response resource closure;
- the full Plan 254 async lifecycle matrix;
- low-level runtime and public typing fixtures;
- ASGI sufficiency fixture.

If Rust/PyO3 response conversion changes, also run:

```sh
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
```

and the relevant Python callback/response-stream Rust tests.

## Acceptance criteria

- [ ] baseline regression deterministically demonstrates that a suppressed
      async stream can retain the sole async permit.
- [ ] corrected HEAD streaming response does not advance the application
      iterable.
- [ ] corrected HEAD response releases its async permit/task ownership without
      waiting for `response_write_timeout_secs`.
- [ ] 204/body-forbidden streaming response has the same property.
- [ ] a normal request immediately following a suppressed stream is admitted
      with `max_async_tasks=1`.
- [ ] repeated suppressed streams do not accumulate bridge-owned tasks or
      permits.
- [ ] ordinary streamed responses retain bounded admission for their actual
      producer lifetime.
- [ ] disconnect/shutdown/error/timeout paths release exactly once.
- [ ] no Python copy of canonical HTTP suppression policy becomes the
      correctness authority.
- [ ] no public Python/Rust API, capability, support tier, queue bound, or
      security behavior changes.
- [ ] full installed-wheel and structural qualification is green.

## Stop conditions

If the only robust solution requires a new public Python API, a new Rust public
callback/drop contract, or duplicating canonical HTTP status policy into the
Python facade, stop and write a separate architecture plan.

A narrow private PyO3 helper or private Python iterator/lifetime object is
within scope.
