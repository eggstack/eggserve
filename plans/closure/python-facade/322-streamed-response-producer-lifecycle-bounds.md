# Python Facade Milestone 322 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/python-facade/322-streamed-response-producer-lifecycle-bounds.md`

Source subsystem roadmap:

- `plans/subsystems/python-facade-roadmap.md`

Repository baseline reviewed: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

Implementation state: the change is in the working tree. No commit was created
by this task.

## 1. Executive finding

`Response.stream` producer threads now observe teardown and are admitted under
an explicit budget.

- **Teardown observation.** The producer checks `sender.is_closed()` before
  every pull (before attaching to the GIL and calling `__next__`) and again in
  the empty-chunk branch. The existing post-send check remains the fast path
  for a full channel. A `b""`-forever iterator therefore exits instead of
  re-attaching to the GIL indefinitely.
- **Bounded producers.** `PythonCallbackService` gains
  `stream_producer_semaphore`, sized with `max_python_callbacks` (default 8).
  The budget is independent of `callback_semaphore`, so a long download holds
  a producer permit, not a callback slot. Conversion acquires with
  `try_acquire_owned()` before spawning; exhaustion returns a sanitized
  `ServiceError::rejected(503, …)` before response start and emits one
  `ServiceAdmissionRejected` event at `Warn` with no request content.
- **Permit lifetime.** The owned permit moves into the producer thread and is
  dropped on every exit path, including EOF, iterator error, non-bytes,
  channel closure, and panic. Release is exactly once by construction
  (`_producer_permit` is never sent anywhere else).
- **Residual limitation.** A Python `__next__` that blocks indefinitely cannot
  be interrupted; it may outlive the request and server shutdown and keeps its
  permit until it returns. No new producer is admitted. This is documented as
  residual risk.

No Python signature changed and no new public config field was added.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| Closed check before every pull | met | `body_bridge.rs` loop-top `sender.is_closed()` |
| Closed check in the empty-chunk branch | met | `Pulled::Empty` branch |
| Post-send check preserved | met | unchanged |
| Iterator dropped promptly on closure | met | loop break; thread returns; permit drops |
| No final error sent to a closed receiver | met | closed-check precedes the error sends |
| Explicit producer budget | met | `stream_producer_semaphore` in `PythonCallbackService` |
| Acquire before spawn, 503 on exhaustion | met | `acquire_stream_producer_permit` at both conversion sites |
| Permit held for producer lifetime, released exactly once | met | owned permit moved into the thread closure |
| One warn event on exhaustion, no request content | met | `ServiceAdmissionRejected` event |
| No new public knob | met | capacity reuses `max_python_callbacks`; no Python field |
| Documentation of the blocking limitation | met | §6 |
| Regression tests | met | §4 |

## 3. Implementation evidence

### 3.1 Bridge

```rust
// crates/eggserve-python/src/server/body_bridge.rs
pub(super) fn spawn_python_stream_producer(
    iterable: Py<PyAny>,
    sender: mpsc::Sender<Result<Bytes, ResponseStreamError>>,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    std::thread::spawn(move || {
        let _producer_permit = permit;   // dropped on every exit path
        …
        loop {
            if sender.is_closed() { break; }   // before GIL attach / __next__
            …
            Pulled::Empty => {
                if sender.is_closed() { break; }
                …
            }
        }
    });
}
```

### 3.2 Admission

`PythonCallbackService` owns `stream_producer_semaphore: Arc<Semaphore>`,
initialized with `max_python_callbacks` beside `callback_semaphore`. The
response conversion acquires one permit before creating the channel or
spawning, at both the plain-stream and stream-with-trailers sites. Exhaustion
returns 503 without spawning.

Capacity choice: reusing the callback capacity keeps the public configuration
surface unchanged. The budget is a separate semaphore so the plan's warned
tradeoff (long downloads holding callback permits) does not occur; a dedicated
`max_stream_producers` knob remains a separate public-config decision if an
operator needs to tune the two independently.

### 3.3 HEAD/body-forbidden suppression

The permit is acquired only in the non-suppressed branches; HEAD and
1xx/204/205/304 still drop the iterator without advancing it and without
consuming a producer permit.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | PASS (standalone checkout) |
| `cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings` | PASS |
| `bash scripts/test-python-wheel.sh` | PASS — **853 tests, OK**, including the two new regressions |
| `cargo clippy --workspace --lib --bins --tests -- -D warnings` | PASS |
| `cargo fmt --all -- --check` / `rustfmt` on the Python crate | PASS |

New regressions in `test_lowlevel_runtime.py`:

1. `test_empty_iterator_stops_after_teardown` — a recording `b""`-forever
   generator; the write-progress deadline (1 s) plus a 1 s graceful drain
   closes the connection; the call count must then stop advancing. The first
   verification run used the default 10 s drain, so the receiver was still
   alive when the assertion fired (14,427,690 vs 10,894,824 calls) and the
   count kept climbing; the test now pins the drain to 1 s. With the closed
   checks in place the count stabilizes after teardown. The pre-fix behavior
   is established by code review: the empty branch had no `is_closed` check,
   so it could never observe closure.
2. `test_stream_producer_budget_rejects_with_503` — four blocked producers
   (`max_python_callbacks=4`), the fifth request gets a sanitized 503 with no
   traceback, and after the gate is released a later request succeeds (permit
   release evidence).

Existing streaming tests (unknown length, known length, trailers, HEAD
suppression, non-bytes, iterator exception, length mismatch, backpressure,
shutdown with an active stream) stay green.

## 5. Invariant review

| Invariant | Result |
|---|---|
| HEAD/body-forbidden never advance the iterator | Held; unchanged |
| Backpressure stays bounded (16 slots, 64 KiB split) | Held; unchanged |
| Response framing stays runtime-owned | Held; no framing change |
| No Python code is killed | Held; timeouts still close the request only |
| Permit release exactly once | Held; single owned permit per thread |
| No new public knob | Held; no Python-visible field added |

## 6. Documentation and operations

| Surface | Change |
|---|---|
| `docs/python-api.md` | Producer budget, teardown observation, 503, blocking limitation |
| `docs/python-http-server-compatibility.md` | Visible deviation row |
| `architecture/eggserve-python.md` | Producer/admission ownership |
| `docs/threat-model.md` | Malicious/stalled Python callback paragraph includes producers |
| `CHANGELOG.md` | Unreleased entry |

## 7. Residual risk

A blocking `__next__` remains uninterruptible and keeps its permit until it
returns. The budget bounds how many such threads exist, and shutdown does not
join them (joining arbitrary Python could hang shutdown). Operators sizing
`max_python_callbacks` now also size concurrent streamed responses' producer
threads.

## 8. Unresolved findings

- The first full Python run in this environment failed
  `test_native_handler_only_without_root` with `Address already in use` because
  a leftover test server held port 8000; the rerun with the port free passed.
  This is an environment/test-isolation artifact, not a product defect, and is
  recorded rather than fixed here.

## 9. Roadmap disposition

`plans/subsystems/python-facade-roadmap.md` Milestone 322 is closed.

## 10. Registry updates

`plans/registry.md` row 322 moves to `closed` with a pointer to this record;
the Python roadmap milestone table and status rows are updated to match.
