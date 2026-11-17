# Plan 322 — Streamed-response producer lifecycle and admission bounds

## 1. Status

- **State**: ready for handoff
- **Plan number**: 322
- **Subsystem**: Python facade
  (`plans/subsystems/python-facade-roadmap.md`, Milestone 322)
- **Source roadmap**: `plans/subsystems/python-facade-roadmap.md`
- **Long-term requirements**:
  - `plans/000-long-term-specification.md#2` (bounded resources;
    `http.server`-shaped facade)
  - `plans/001-terminology-and-domain-model.md` (`Request`, `Response`,
    `ResponseStream`)
  - `docs/python-api.md` and `docs/python-http-server-compatibility.md`
- **Primary class**: invariant (bounded resource use and cancellation)

## 2. Repository baseline

Repository baseline: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

`Response.stream(...)` with a sync Python iterable is bridged by a
dedicated `std::thread` per response:

- The backend is `spawn_python_stream_producer`
  (`crates/eggserve-python/src/server/body_bridge.rs:79-83`).
- Its own comment records the accepted gap: the `JoinHandle` is
  intentionally detached and untracked by `ServerHandle::wait()`, and an
  infinite `__next__` parks the thread past shutdown
  (`body_bridge.rs:71-74`).
- Empty chunks are counted and the thread yields after 32, but the empty
  branch does **not** check whether the channel receiver is closed
  (`body_bridge.rs:185-192`). The closed-channel check exists only after a
  send (`body_bridge.rs:201-211`).
- The Python callback permit is held only for the callback, not for the
  producer thread (`crates/eggserve-python/src/server/sync_handler.rs:664-692`).
- The two spawn sites are the plain and trailer stream conversions
  (`sync_handler.rs:488-500` and `547-566`).
- The public contract permits empty chunks and bounded buffering but does
  not state a producer-lifetime bound (`docs/python-api.md:152-158`).

## 3. Problem statement

A handler that returns a stream with a blocking or non-terminating
iterator can leave a thread behind after the client disconnects, the
response times out, or the server shuts down. An iterator that yields
`b""` forever is worse: it never reaches a send, never observes the closed
receiver, and keeps re-attaching to the GIL, so each request leaves a
live, busy producer thread.

Because the producer is detached and the callback permit is released when
the callback returns, repeated requests can accumulate producers without
bound. `max_in_flight_requests` and `max_connections` do not bound them.
The result is an application-dependent resource-exhaustion path: a remote
client can repeatedly request a route whose handler returns such a stream.

This is not a sandbox escape and does not affect the Rust static path. It
is a real lifetime/admission defect in the Python facade's streaming
contract.

## 4. Goal

Make every streamed response producer observe response/connection
teardown, bound the number of concurrent producer threads with an explicit
admission budget, and document the residual limitation that a blocking
Python `__next__` cannot be interrupted.

## 5. Non-goals

- No cancellation or termination of arbitrary Python code (impossible
  safely; the existing threat model records this).
- No change to the 16-chunk bridge, chunk splitting, backpressure
  semantics, or bytes-like acceptance.
- No async-producer support (`AsyncResponse.stream` is a separate path).
- No new public Python signature unless the existing
  `max_python_callbacks` cannot serve as the budget.
- No change to the Rust static response path.

## 6. Scope

### Work package A — Observe receiver closure before every pull

In `spawn_python_stream_producer`:

- Check `sender.is_closed()` at the top of the loop, before attaching to
  the GIL and calling `__next__`.
- Check it again in the `Pulled::Empty` branch and break when closed.
- Keep the existing post-send check; it remains the fast path for full
  channels.
- Drop the iterator and exit the thread promptly on closure; do not send a
  final error to a closed receiver.

Add a focused comment that the empty-yield branch exists to prevent a
GIL/CPU hot-spin and must not bypass teardown observation.

### Work package B — Bound concurrent producers

Add one explicit producer-admission budget and hold a permit for the
producer's lifetime:

- Preferred minimal design: reuse the existing callback capacity
  (`max_python_callbacks`, default 8) as the producer budget, owned by the
  sync handler beside `callback_semaphore`
  (`crates/eggserve-python/src/server/sync_handler.rs:41`,
  `runtime.rs:709`). Do not add a second independently tunable knob unless
  the implementation shows the callback budget is semantically wrong.
- Acquire with `try_acquire_owned()` in the response-conversion path,
  before spawning. On exhaustion, return a sanitized
  `ServiceError::rejected(503, "python stream producer budget exhausted")`
  (or the crate's equivalent) before response start; do not spawn a thread
  and do not silently render a truncated stream.
- Move the owned permit into the producer thread and let it drop when the
  thread exits. The permit must be released exactly once, including on
  iterator error, non-bytes item, channel closure, and panic containment.
- If the budget is exhausted, emit one structured ops event (severity
  warn) with no request content; increment a counter if the existing
  counters surface supports it.

If reusing `max_python_callbacks` would make ordinary handler concurrency
unacceptable (because long downloads hold producer permits), stop and
report the measured tradeoff; a dedicated `max_stream_producers` field is
a public-config addition and needs its own decision.

### Work package C — Document the residual blocking limitation

- `docs/python-api.md` — state that the producer is bounded by the
  producer budget, that disconnects/timeouts are observed before the next
  pull (and on empty yields), and that a Python `__next__` which blocks
  indefinitely cannot be interrupted; the permit stays held until it
  returns.
- `docs/python-http-server-compatibility.md` — if the 503-on-exhaustion
  behavior is visible to `http.server`-shaped users.
- `architecture/eggserve-python.md` — record the producer/admission
  ownership.
- `docs/threat-model.md` — update the "malicious or stalled Python
  callback" paragraph to include stream producers.

### Work package D — Regression tests

Rust/Python tests, whichever can observe the behavior reliably:

1. **Closed-receiver empty loop**: an iterator that records each
   `__next__` call and always returns `b""`. Open a streaming request,
   close the client socket, and assert the iterator stops advancing
   (the call count stabilizes) within a bounded wait. This is the direct
   regression for the missing closed check.
2. **Budget exhaustion**: hold N producers open (N = budget), issue
   request N+1, and assert a sanitized 503 (not a hang, not an unbounded
   thread). Release one and assert a later request succeeds.
3. **Normal streaming**: a finite iterator still delivers the same bytes,
   honors `content_length`, trailers, HEAD/body-forbidden suppression,
   and non-bytes/error truncation.
4. **Shutdown**: with a producer blocked in `__next__`, server shutdown
   still completes within the documented shutdown bound; the blocked
   producer may outlive the server (documented) but no new producer is
   admitted.

The first two tests need a real bound and an observable counter; expose a
test-only hook or count the iterator calls. Do not assert thread counts
from Python.

## 7. Invariants that must not regress

1. **HEAD/body-forbidden never advance the iterator.** Existing
   suppression stays exactly as is.
2. **Backpressure stays bounded.** The 16-slot channel and 64 KiB chunk
   splitting are unchanged; no unbounded buffering is introduced.
3. **Response framing stays runtime-owned.** A producer cannot set
   `Transfer-Encoding` or bypass normalization.
4. **No Python code is killed.** Timeouts still close the HTTP request,
   not the interpreter.
5. **Permit release is exactly once.** Every producer exit path releases
   its permit; no leak and no double release.
6. **No new public knob without a decision.** Reuse the callback budget
   unless evidence says otherwise.

## 8. Failure, cancellation, restart, and contention semantics

- **Client disconnect**: the next loop check observes the closed channel
  and exits; an in-progress `__next__` completes first.
- **Response timeout / body-suppression**: the receiver is dropped; the
  producer exits at the next check.
- **Budget exhausted**: the request fails with a sanitized 503 before
  response start; no producer thread is created.
- **Iterator exception / non-bytes**: the existing error path truncates
  the stream and exits; the permit is released.
- **Server shutdown**: no join of arbitrary Python producers (would block
  shutdown); admission stops, and the documented limitation remains.
- **Concurrent requests**: admission is a fair semaphore; no thread is
  spawned without a permit.

## 9. Compatibility and migration

- Normal finite streams are byte-identical.
- A handler that returns more concurrent streams than the budget now
  receives 503 instead of unbounded thread creation. Document this in the
  Python API contract and changelog.
- No Python signature change if the callback budget is reused. If a new
  config field is unavoidable, it is a public addition and must be
  classified in the plan/closure.

## 10. Required tests

### Focused

- Rust unit test for the closed-check helper if one is factored out.
- Permit acquisition/release unit test (exhaustion, release-on-drop,
  release-on-error).

### Integration

- Python streaming regression: finite stream bytes, content length,
  trailers, HEAD suppression.
- Closed-receiver empty-iterator regression.
- Budget-exhaustion 503 and recovery.
- Shutdown with a blocked producer.

### Security and negative

- A remote client cannot accumulate producers past the budget by opening
  and closing connections.
- Error paths never log request/response content or Python tracebacks.

## 11. Required verification commands

```bash
# Rust bridge
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings

# Python suite (needs Python 3.14 + maturin per AGENTS.md)
bash scripts/test-python-wheel.sh

# Repository gate
./scripts/verify.sh fast
```

The Python suite may not be runnable in a Rust-only environment. If it is
unavailable, record the exact missing prerequisite; do not claim Python
runtime evidence from a `cargo check`.

## 12. Documentation updates

- `docs/python-api.md` — producer budget, teardown observation, blocking
  limitation, 503 behavior.
- `docs/python-http-server-compatibility.md` — visible compatibility
  effect, if any.
- `architecture/eggserve-python.md` — producer/admission ownership.
- `docs/threat-model.md` — malicious/stalled Python callback paragraph.
- `CHANGELOG.md` — the behavior change.
- Roadmap/registry status.

## 13. Acceptance criteria

- A non-terminating `b""` iterator stops advancing after the client
  disconnects.
- Concurrent producers never exceed the configured budget; exhaustion is a
  sanitized 503, not a hang or an unbounded thread.
- Normal streaming, trailers, HEAD/body-forbidden suppression, and
  truncation behavior are unchanged.
- The blocking-`__next__` limitation is documented as a residual risk.
- No producer permit leaks on any exit path.

## 14. Stop conditions

Stop and report rather than improvise when:

- bounding requires a new public Python config field and the plan owner
  has not approved that addition;
- the permit cannot be transferred into the producer thread without
  changing callback-admission semantics;
- the Python test environment (3.14 + maturin) is unavailable;
- the fix would require joining arbitrary Python threads at shutdown;
- a proposed change would buffer the entire stream or break backpressure.

## 15. Closure evidence required

- Python/Rust test outputs for the closed-receiver and budget cases.
- Before/after demonstration of the empty-iterator call count after
  disconnect.
- Confirmation that normal streaming/trailer/HEAD tests are unchanged.
- Permit-lifecycle evidence for every exit path.
- Documentation and changelog diffs.
- Residual limitation: a blocking `__next__` still cannot be interrupted.

## 16. Handoff notes

- `spawn_python_stream_producer` is called from two conversion sites;
  both must use the same admission path.
- The producer thread is a `std::thread`, not a Tokio task; acquire the
  permit before spawning and move it into the closure.
- Do not hold a `std::sync::Mutex` across a blocking send or a GIL
  attach; the existing body bridge avoids that and must keep doing so.
- The 32-empty-yield `yield_now` is a CPU mitigation, not a teardown
  check; the new closed check is what lets the thread exit.
