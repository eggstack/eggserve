# Python Facade Milestone 331 — Free-threading concurrency contract hardening

Status: ready for handoff (hard dependency: Milestone 330)

Repository baseline: the Milestone 330 merge commit (free-threaded family
landed as Beta). The evidence below was taken at `origin/main` `5fdd7da`
before that merge; re-confirm the line numbers after 330 lands.

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md#7` (milestones)

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md` (`RequestBody`, `Response`)
- `plans/002-long-term-roadmap.md` (phase 4)

Applicable ADRs: none.

Primary class: invariant (concurrent use must never return silent wrong data;
it either works or raises a typed error).

## 1. Objective

Make every Python-visible concurrency contract in the affected surface either
safe or explicitly failing when the GIL is absent. The target is the class of
defect where two OS threads race a take/restore slot and one caller receives a
plausible but wrong result instead of an error.

This milestone does not add parallelism, change admission limits, or promote
the free-threaded tier. It closes the audit findings that Milestone 330's
branch does not address.

## 2. Why this milestone is ready

- Milestone 330 lands the explicit `gil_used = false` declaration and the
  free-threaded test module, so the runtime this milestone hardens is a
  supported (Beta) configuration with an owning test file.
- The audit reproduced GIL-free execution on CPython 3.14.5t (GIL disabled
  after import; 8 concurrent callbacks; 856-test suite passes), so the races
  below are reachable today, not hypothetical.
- The branch already fixes concurrent `Tunnel.recv`; this milestone must not
  duplicate that guard.

## 3. Current implementation evidence

All paths are in `crates/eggserve-python/`.

### 3.1 `BodySource` silent lost read (highest severity)

`src/lib.rs:283` `read_all` and `src/lib.rs:302` `read_range` use a
take-restore idiom:

- lock `Mutex<BodySource>`, `std::mem::replace(&mut *guard, Empty)`, unlock;
- `py.detach(|| body.read_all())` (or `read_range`);
- restore with `if let Ok(mut guard) = self.inner.lock() { *guard = body; }`.

Two threads calling concurrently can interleave as: T1 takes the real body,
T2 takes `Empty`, T2 reads zero bytes and restores `Empty`, T1 restores the
real body last. T2 returns `Ok(vec![])` with no error. The other ordering
loses T1's restore. The result is silent short/empty data, not a crash.
Sequential repeated reads are intended to work today because the body seeks
explicitly.

### 3.2 Request-body bridge (typed, but inconsistent)

`src/server/body_bridge.rs` uses the same take/restore shape in
`read_chunk` (~line 470) and `trailers` (~line 535), but the slot is
`Mutex<Option<RequestBody>>`, so a losing concurrent caller gets
`RequestBodyConsumedError` instead of silent data. That is acceptable
behavior, but the restore is still `if let Ok(...)`, and `read_chunk` sets
`incremental=true` before the take, so a losing call mutates mode before it
errors. Align both sites on one helper and one tested contract.

### 3.3 Single-consumer iterator

`src/server/body_bridge.rs:585` `BodyChunkIterator` is the only non-frozen
`#[pyclass]` and is documented "not thread-safe; concurrent __next__ calls
are not supported". PyO3's runtime borrow check turns a concurrent
`__next__` into a typed `Already borrowed` error, not UB. The contract is
fine; it lacks a regression test and is now load-bearing.

### 3.4 Compatibility facade lifecycle state

`python/eggserve/server.py`: `_closed` is set at `:486` and read at `:648`,
and `server_close` clears `_native` at `:685` before setting `_closed` at
`:687`, with no lock. A concurrent `serve_forever` and `server_close` can
start a native server after the close decided to stop (the audit saw
`RuntimeError: server is closed` raised from a `serve_forever` thread during
teardown). No memory-safety issue; a lifecycle race that should be
deterministic.

### 3.5 Async bridge loop affinity

`python/eggserve/lowlevel.py:1051` `track()` mutates the `_tasks` set
directly. Every other mutation path runs on the loop thread
(`:999`, `:1002`, `:820`, `:829`). Calling `track()` from a foreign
thread is unprotected.

### 3.6 GIL-shaped tests

`tests/test_server_integration.py:574` and
`tests/test_boundary_hardening.py:386` increment handler counters without a
lock. `tests/test_server_integration.py:640` states "The GIL is the only
synchronization during Python callback execution". The suite passes on 3.14t
today, but those tests would not detect a real callback-parallelism defect.

### 3.7 Runtime worker shape

`src/server/runtime.rs:701-707` builds a single 2-worker Tokio runtime for
listeners, body producers, and `block_on` waits; the B86 note records the
contention. No public knob exists and this milestone must not add one.

## 4. Invariants that must not regress

- Sequential repeated `BodySource.read_all`/`read_range` behavior stays as
  it is today (reads seek explicitly and restore the body).
- `RequestBody.read()` / `iter_chunks()` / `read_chunk()` / `trailers()`
  one-shot and mode-exclusivity semantics are unchanged.
- `max_python_callbacks`, stream-producer admission, and the 16-chunk bridge
  are unchanged.
- No Rust lock is held across `Python::attach`, `Py::call`, or
  `block_on`; no `unsafe`.
- The GIL `cp311-abi3` family and the free-threaded Beta family behave
  identically for these contracts.
- No public worker-count API and no new dependency.

## 5. Scope

### In scope

- `PyBodySource` concurrent-entry contract and its tests.
- One shared take/restore helper for the request-body bridge, with typed
  failure on concurrent entry.
- Regression tests for `BodyChunkIterator`, tunnel `recv` (landed by 330),
  facade close/serve, and async `track()`.
- Test-suite atomicity and the GIL-free assertion.
- Measurement and documentation of the 2-worker runtime envelope.

### Explicitly out of scope

- A public worker-count knob or runtime API change.
- The Plan 238 async producer redesign.
- Tier promotion (Milestone 332) or new platforms.
- Any change to static serving, HTTP framing, TLS, or confinement.
- Rewriting the tunnel guard from Milestone 330.

## 6. Required production changes

### Crates and ownership

`crates/eggserve-python` only. No workspace crate changes. The excluded
crate's lint lane must stay green.

### Config and policy

None.

### Protocol and compatibility

No wire change. Error codes/messages for concurrent misuse are additive; do
not reuse an unrelated code.

### Runtime and concurrency

- Replace `Mutex<BodySource>` + replace/restore with an explicit state:
  `Ready(BodySource)`, `Busy`, `Consumed` (or equivalent). `Ready` enters
  `Busy`; a second entrant gets a typed
  `BodySourceError` (new stable code, e.g. `body_in_use`); the owner
  restores `Ready` on success and on every error path. Use a drop guard so an
  unwind cannot strand `Busy`.
- Apply the same helper shape to `read_chunk`/`trailers`; preserve the
  existing `RequestBodyConsumedError` for a genuinely completed/consumed
  body.
- Guard the compatibility facade's close/serve transition with a lock or a
  single atomic state machine so exactly one start wins and a close after
  start is deterministic.
- Route `AsyncServer.track()` through the loop thread or document and enforce
  loop-thread affinity with a clear error.

### Frontend or operator surface (CLI / Python)

No CLI change. `BodySource` gains one documented error condition; the stubs
and `docs/python-api.md` describe it.

### Security and confinement

No change. Fail-closed on concurrent misuse; never fall back to empty data.

### Documentation and static guards

`docs/python-api.md`, `docs/api-stability.md`, `lowlevel.py` docstrings,
and the affected test docstrings state the concurrent rules. No new script is
required.

## 7. Ordered work packages

### Work package A — BodySource state machine

Intent: remove the silent lost read.

Required changes:

- Introduce the three-state interior and the drop-safe restore.
- Keep sequential re-read behavior; concurrent entry is an error.
- Map the error through the existing `BodySourceError` type with a stable
  code.

Acceptance evidence: a two-thread test where exactly one call returns the
bytes and the loser raises the typed error; a sequential re-read test still
passes; a mutation test that removes the guard fails.

### Work package B — Request-body bridge consistency

Intent: one take/restore helper and no pre-take mode mutation on a losing
call.

Required changes:

- Move `read_chunk`/`trailers` onto the shared helper.
- Set `incremental` only after the take succeeds.
- Keep `RequestBodyConsumedError` for consumed bodies.

Acceptance evidence: concurrent `read_chunk` tests (one winner, typed loser);
`trailers()` after terminal state still works.

### Work package C — Single-consumer regression tests

Intent: make the existing documented contracts executable.

Required changes:

- Concurrent `BodyChunkIterator.__next__` yields a typed
  `Already borrowed`/`RuntimeError`, never UB or a hang.
- Concurrent `Tunnel.recv` yields the Milestone 330 error (do not
  re-implement it; assert it).

Acceptance evidence: tests on both the GIL and free-threaded lanes.

### Work package D — Facade lifecycle determinism

Intent: remove the close/serve race seen in the audit.

Required changes:

- Lock or atomically transition `_closed`/`_native`.
- Define the outcome for close-during-start: one deterministic winner, the
  other raises the existing "server is closed" error; no native server is
  leaked.

Acceptance evidence: a repeated race test (close while `serve_forever`
starts) never leaves a running native server and never raises an unhandled
thread exception.

### Work package E — Async bridge affinity

Intent: make `track()` safe or explicitly loop-only.

Required changes:

- If the intended contract is loop-thread-only, detect the wrong thread and
  raise a clear error; otherwise schedule via `call_soon_threadsafe`.
- Document the choice in `lowlevel.py` and `docs/python-api.md`.

Acceptance evidence: a foreign-thread `track()` test matches the documented
contract.

### Work package F — GIL-free test honesty

Intent: stop the suite from depending on GIL serialization.

Required changes:

- Replace unlocked handler counters in
  `tests/test_server_integration.py:574` and
  `tests/test_boundary_hardening.py:386` with locked counters.
- Rewrite the `test_server_integration.py:640` docstring to describe the
  actual contract.
- Add one assertion that the free-threaded lane really has the GIL disabled
  (already partly covered by Milestone 330's guard; do not duplicate it).

Acceptance evidence: the suite passes on both lanes; the free-threaded lane
asserts `sys._is_gil_enabled() is False`.

### Work package G — Runtime envelope evidence

Intent: know the contention limit before documenting it.

Required changes:

- Measure the 2-worker runtime under `max_python_callbacks=8` with body
  uploads and streamed responses; record the observed behavior.
- Document the envelope in `docs/python-api.md` (no public knob).
- If a deadlock or starvation is proven, stop and report; a worker-shape
  change needs its own decision.

Acceptance evidence: a short measurement record in the closure (not a CI
timing gate).

## 8. Failure, cancellation, restart, and contention semantics

- Concurrent entry to a one-shot slot fails fast with a typed error; it never
  waits on another thread's work.
- A panic during a detached read must restore the slot to `Ready` (drop
  guard) or leave it `Consumed`; never `Busy` forever.
- Close-during-start is deterministic; the loser observes a closed server.
- Shutdown still cannot kill Python code; the existing limitation is
  unchanged.

## 9. Compatibility and migration

- No public API removal. The new concurrent-misuse error is additive.
- Sequential code sees no change.
- Both wheel families keep identical semantics.
- If a fix requires a public API change, stop (Section 14).

## 10. Required tests

### Focused unit tests

- `BodySource` concurrent entry (one winner, typed loser), sequential
  re-read, panic/disconnect restore.
- `read_chunk`/`trailers` concurrent entry and mode exclusivity.
- `BodyChunkIterator` concurrent `__next__`.
- `Tunnel.recv` contention (asserts Milestone 330's guard).
- `track()` foreign-thread behavior.

### Integration tests

- Full Python suite on the GIL lane and the free-threaded lane.
- Close/serve race under repeated starts.

### Restart and recovery tests

- Restart after a failed start still works; no leaked native server.

### Contention and cancellation tests

- The two-thread races above; the disconnect-during-read path restores the
  slot.

### Security and negative tests

- No silent empty body on any concurrent path; error codes are stable and do
  not leak request data.

### Migration and compatibility tests

- Both wheel families pass the same tests (the `-ft` tests skip only where
  they assert GIL-disabled execution).

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
PYTHON=python3.14 bash scripts/test-python-wheel.sh
PYTHON=python3.14t bash scripts/test-python-wheel.sh
./scripts/verify.sh fast
```

Run the focused test files first with the interpreter that owns the failure.

## 12. Documentation updates

- `docs/python-api.md`: `BodySource` concurrent rule; facade lifecycle;
  async `track()` affinity; runtime envelope.
- `docs/api-stability.md`: clarify the one-shot/single-consumer statement so
  it describes the actual error, not just "GIL constraints".
- `lowlevel.py`/`server.py` docstrings for the changed contracts.
- No change to tier statements (Milestone 332 owns them).

## 13. Acceptance criteria

1. A concurrent `BodySource.read_all`/`read_range` either returns the
   correct bytes once or raises a typed error; no silent empty/short result.
2. Sequential re-reads behave as before.
3. `read_chunk`/`trailers` share one restore helper and never mutate mode on
   a losing take.
4. Concurrent iterator/tunnel misuse is tested and typed.
5. Close-during-start is deterministic and leaks no native server.
6. The suite passes on both wheel families; the free-threaded lane asserts
   the GIL is disabled.
7. No public API, default, or dependency change; no `unsafe`.

## 14. Stop conditions

Stop and report when:

- the only correct fix would change a public API shape or add a public
  worker-count knob;
- the runtime measurement proves a deadlock requiring a runtime redesign;
- a fix would change `max_python_callbacks` semantics or the 16-chunk bridge;
- the GIL family would behave differently from the free-threaded family;
- Milestone 330 has not landed (hard dependency absent).

## 15. Closure evidence required

- The failing-before/passing-after tests for each work package.
- Exact command outcomes on both interpreters.
- The runtime envelope measurement.
- A list of any contract left documented-but-unchanged, with rationale.
- Residual findings classified by severity.

## 16. Handoff notes

- Re-verify line numbers after the Milestone 330 merge; the branch touches
  `body_bridge.rs`, `response_bridge.rs`, and `tunnel_bridge.rs`.
- Keep the take/restore helper in one place; do not add a second copy in
  `body_bridge.rs`.
- PyO3's non-frozen runtime borrow check is a feature here: do not "fix" it
  by making `BodyChunkIterator` `frozen` with a hand-rolled lock.
- The 2-worker runtime note (B86) is intentional; a change needs its own
  plan, not a drive-by edit.
