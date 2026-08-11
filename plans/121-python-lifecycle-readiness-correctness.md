# Plan 121 — Python Lifecycle Readiness Correctness

## Status

**READY FOR HANDOFF — 2026-08-11.**

Parent roadmap: Plan 120.

Reviewed baseline:

```text
main = bae3dce5f8be876a083434918cdfc974b9781c75
crates/eggserve-python/src/server.rs blob = 7c343d2e2e53ca138c848bda47c7d1857a5e2d0c
```

---

## Problem statement

`PyServer::wait_ready()` has an incorrect success path while the native lifecycle is `Starting`.

The current implementation waits with:

```rust
let _ = tokio::time::timeout(STARTUP_TIMEOUT, handle.ready()).await;
```

and deliberately discards whether `timeout()` returned `Err(Elapsed)`. It then rereads lifecycle state and returns `Ok(())` for any state other than `Running` or `Failed`. A server that remains `Starting` after the timeout can therefore be reported to Python as ready.

There is also duplicated startup-wait logic. `PyServer::start()` separately polls the handle until `Running`, `Failed`, or `STARTUP_TIMEOUT`, once in the custom-handler branch and again in the static branch. `wait_ready()` then contains a third readiness implementation.

The fix should make readiness semantics exact and, where possible, reduce this duplication rather than add more state machinery.

---

## Goal

Establish one simple invariant:

> A successful Python readiness operation means the native `ServerHandle` is in `LifecycleState::Running` at the point success is returned.

All other terminal/timeout conditions must return an error. No code path may infer readiness from absence of `Failed`.

---

## Scope

Primary implementation surface:

```text
crates/eggserve-python/src/server.rs
crates/eggserve-python/tests/* lifecycle/server tests
crates/eggserve-python/python/eggserve/server.py only if wrapper assumptions require correction
crates/eggserve-python/python/eggserve/_native.pyi if signatures/docs change
relevant Python API docs only when behavior wording is currently inaccurate
```

No core server lifecycle redesign is authorized unless the existing `ServerHandle::ready()` contract itself is proven defective.

---

## Track A — Establish exact lifecycle semantics

Before editing, inspect:

```text
eggserve_core::server::lifecycle::LifecycleState
ServerHandle::state()
ServerHandle::ready()
ServerHandle::wait()
Server::start()/start_with_service()
PyServer::start()
PyServer::wait_ready()
PyServer::stop()/shutdown()/force_shutdown()/wait()
```

Record the actual contract of `ServerHandle::ready()`:

- which lifecycle transitions complete it;
- whether it returns a typed error/result or only completion;
- what happens if the server fails before Running;
- what happens if shutdown begins while readiness is pending.

Do not duplicate lifecycle semantics in Python if `ServerHandle` already exposes the authoritative answer.

### Acceptance criteria

- implementation is based on current core lifecycle behavior, not assumptions from this plan;
- `Running` is the only success state for readiness;
- every other state has an explicitly classified result;
- no new public lifecycle state is introduced merely to fix this method.

---

## Track B — Consolidate the native Python readiness helper

Prefer a private Rust helper along the conceptual lines of:

```text
wait_until_running(handle, runtime, timeout)
    -> Running: Ok
    -> Failed: startup failure
    -> timeout while Starting: timeout error
    -> Created: not started
    -> Draining/Stopped: not running
```

The exact implementation should use the existing `ServerHandle::ready()` signal where possible rather than 5 ms polling loops.

`PyServer::start()` currently waits synchronously for Running before returning. Preserve that externally observable behavior unless repository tests/documentation demonstrate that `start()` is intended to be asynchronous. If synchronous startup remains the contract, make both static and callback branches use the same helper instead of maintaining duplicate loops.

`wait_ready()` should remain valid/idempotent when already `Running`.

### Error behavior

Use the existing Python lifecycle exception family (`LifecycleError`) for lifecycle/state failures unless the current API consistently maps startup runtime errors to another established exception. Do not create a new exception class solely for timeout if a clear message on the existing lifecycle exception is sufficient.

Timeout text must identify readiness/startup timeout rather than generic “not running”.

### GIL behavior

Any blocking wait on the Tokio runtime must execute with the GIL released, as the current code intends with `py.allow_threads()`.

Do not hold the Python handler mutex or another Python-visible lock across readiness waiting.

### Acceptance criteria

- `wait_ready()` returns `Ok(())` only after observing `LifecycleState::Running`;
- timeout result from `tokio::time::timeout` is checked, never discarded;
- `Starting` after a completed/elapsed wait is an error, never success;
- `Created` reports not-started;
- `Failed` reports startup failure;
- `Draining` and `Stopped` report not-running;
- already-`Running` remains an immediate success;
- static and callback startup paths do not retain two materially identical readiness loops if a shared helper is feasible;
- blocking readiness waits release the GIL;
- no new background thread, watcher, channel, or lifecycle framework is introduced.

---

## Track C — Add deterministic regression coverage

Do **not** add a test that sleeps for the production `STARTUP_TIMEOUT` of 30 seconds.

Make the internal readiness implementation testable with a short injected/private timeout or a lower-level state helper. The public production timeout remains unchanged unless there is an independent reason to change it.

Required cases:

1. already Running → success;
2. Created/not started → `LifecycleError`;
3. Starting → Running before timeout → success;
4. Starting → Failed before timeout → error;
5. Starting remains Starting until a short test timeout → timeout error;
6. shutdown/draining while readiness is pending → error, not success;
7. repeated `wait_ready()` on a running server remains safe;
8. compatibility façade `server_activate()` / `_start()` still succeeds with port `0` and publishes the actual native address only after readiness.

Where constructing an intentionally stuck production `ServerHandle` is impractical, factor only enough private logic to test timeout/state mapping directly. Do not add fake server abstractions or a public test hook.

### Acceptance criteria

- regression suite fails against the old ignored-timeout behavior;
- no readiness test takes tens of seconds;
- tests cover both direct native `Server` use and at least one `HTTPServer`/`ThreadingHTTPServer` compatibility path;
- port `0` publication cannot occur before the native server is Running;
- tests do not depend on nondeterministic scheduler timing beyond a small bounded timeout.

---

## Track D — Check neighboring lifecycle semantics without scope expansion

The same review should inspect, but not automatically refactor:

```text
stop()
shutdown()
force_shutdown()
wait()
state getter
```

Only correct a neighboring lifecycle bug in this plan if it is directly caused by the same state/timeout mistake and can be covered with a focused regression test.

In particular, do not turn this into a full lifecycle API redesign.

### Acceptance criteria

- any neighboring edit has a named failing behavior and regression test;
- otherwise neighboring methods remain untouched.

---

## Verification

Minimum local verification after implementation:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Run the existing TLS checks if shared startup code is touched in a way that can affect HTTPS startup:

```sh
cargo clippy -p eggserve-bin --features tls -- -D warnings
cargo test -p eggserve-bin --features tls
```

Routine CI must not gain a dedicated lifecycle job.

---

## Explicit acceptance criteria

Plan 121 is complete only when:

- [ ] the old `let _ = timeout(...).await` readiness bug is gone;
- [ ] timeout is surfaced as an error;
- [ ] no non-Running lifecycle state returns readiness success;
- [ ] `start()` and `wait_ready()` share readiness logic where practical;
- [ ] the Python GIL is not held while waiting;
- [ ] tests exercise timeout without a 30-second delay;
- [ ] tests demonstrate successful readiness for static and compatibility usage;
- [ ] port `0` address publication occurs only after Running;
- [ ] no new public API or dependency is introduced;
- [ ] full Rust and installed-wheel Python verification passes.

---

## Rejection conditions

Reject the implementation if it:

- merely changes the final `else { Ok(()) }` while still discarding timeout outcome;
- sleeps/polls from Python code to paper over native state handling;
- adds a 30-second regression test;
- changes `start()` to return before readiness without an explicit compatibility requirement;
- holds the GIL during the readiness wait;
- adds a new lifecycle manager/thread/channel when the existing `ServerHandle` signal is sufficient;
- broadens this pass into unrelated shutdown/API refactoring.
