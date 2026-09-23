# Plan 270 — Direct-server supervisory lifecycle split and terminal-result propagation

## Purpose

Make the public `eggserve-server` direct runtime usable by embedders that must
supervise the HTTP runtime as a critical task while independently retaining
shutdown authority.

This is an additive direct-server lifecycle/API corrective. It is motivated by
the first external downstream discovered after the 0.2.0 release, but the
result must remain generic EggServe infrastructure rather than a Gregg-specific
adapter.

Planning baseline:

```text
100b33c fix: exclude evidence MANIFEST from twine upload set
```

Depends on the completed direct-runtime lifecycle correction in Plan 243 and
the H1 authority/lifetime corrections through Plan 250.

## Problem statement

Plan 243 fixed two important internal lifecycle defects:

- shutdown state is durable rather than a lossy `Notify::notify_waiters()`
  edge;
- runtime-owned accepted connection tasks are tracked and drained before the
  direct accept task completes.

The public direct `ServerHandle` still exposes an insufficient supervisory
surface:

```rust
handle.shutdown();   // borrows &self
handle.wait().await; // consumes self, returns ()
```

Internally the handle owns a private `JoinHandle<()>`. `wait(self)` ignores
the join result:

```rust
let _ = join.await;
```

A downstream supervisor therefore cannot simultaneously:

1. wait for unexpected server-task termination;
2. retain an independent shutdown capability for a signal/service-manager
   branch; and
3. distinguish normal completion from a Tokio task panic/cancellation.

This matters for long-running daemons that treat the HTTP runtime as critical.
The caller must not have to reimplement EggServe's generic accept loop merely to
observe its lifecycle.

## Required public contract

Add a small, explicit control/completion split while preserving every existing
0.2.0 direct-server path.

Preferred shape:

```rust
let handle = server.start_with_service(service).await?;
let (control, completion) = handle.into_parts();

tokio::select! {
    result = completion.wait() => {
        // Result<ShutdownResult, ServerError>
    }
    _ = external_shutdown => {
        control.shutdown();
        let result = completion.wait().await;
    }
}
```

Exact names may vary if the implementation finds a clearer additive shape, but
the semantics are mandatory.

### Control half

Expose a cheap cloneable control value, for example `ServerControl`, which:

- can request graceful shutdown idempotently;
- does not own or consume the runtime completion task;
- may expose `local_addr`, `ops_context`, or `ops_snapshot` only if doing
  so avoids duplicated state cleanly;
- does not trigger shutdown merely because one clone is dropped;
- contains no listener, service, connection-task registry, or Hyper type.

A caller must be able to retain this value while the completion half is being
awaited in another select branch/task.

### Completion half

Expose one single-owner completion value, for example `ServerCompletion`,
which:

- owns the runtime join responsibility;
- provides an async `wait(self)` returning
  `Result<ShutdownResult, ServerError>` or an equivalently typed terminal
  result;
- reports Tokio task panic/cancellation as a terminal EggServe error instead of
  discarding the `JoinError`;
- returns `ShutdownResult::Clean` for the current ordinary graceful direct
  shutdown path;
- does not implicitly request shutdown merely because the caller starts waiting;
- remains cancellation-safe when pinned and selected, so the same completion
  future can be awaited after an external shutdown branch wins.

Do not expose `tokio::task::JoinError` as the stable error contract. Map it
into EggServe's existing non-exhaustive `ServerError` taxonomy. The existing
`ServerError::Terminal(String)` is available if it remains the clearest
fit; do not leak hostile request bytes or service payloads into the message.

## Existing API compatibility

Keep these existing direct APIs source-compatible:

- `Server::builder()`;
- `ServerBuilder::{runtime,bind,ops_context,from_listener,from_std_listener,build}`;
- `Server::start_with_service`;
- `ServerHandle::{local_addr,shutdown,ops_context,ops_snapshot,wait}`.

In particular, do not change the existing `ServerHandle::wait(self) -> ()`
signature in the 0.2.x patch line.

Implement the legacy `wait()` in terms of the new completion authority where
practical, intentionally preserving its legacy result-discarding behavior.
New embedders use the typed completion API.

Do not make `ServerHandle` itself `Clone`. Split the control capability from
the single-owner join capability instead.

## Internal runtime result ownership

Change the direct runtime task from an opaque `JoinHandle<()>` to a terminal
result shape owned by the completion path.

Requirements:

- graceful server shutdown completes with a typed clean result;
- a panic/cancellation of the top-level direct server task becomes
  `Err(ServerError::Terminal(...))` or equivalent;
- internal connection-task bookkeeping still drains before clean completion;
- infrastructure panics from runtime-owned connection tasks must not be silently
  discarded if they escape the existing service/pipeline panic containment;
- ordinary service errors/panics that the canonical H1 pipeline intentionally
  converts into safe HTTP responses must retain that behavior and must not
  become server-terminal merely because this plan adds join propagation;
- transient listener accept errors retain their current counter/event/backoff
  behavior and do not become terminal unless the existing runtime explicitly
  classifies them as such.

If propagating a runtime-owned connection task panic requires the accept task to
return `Result<ShutdownResult, ServerError>`, make that the single internal
authority rather than adding a second error channel.

## Shutdown/drop semantics

Preserve Plan 243's durable shutdown and drain guarantees.

Required cases:

- shutdown before the accept task first polls;
- repeated control-handle shutdown calls;
- shutdown while a connection task is starting;
- shutdown with active request/response work;
- control clone dropped while the server keeps running;
- completion half dropped/cancelled according to an explicitly documented
  behavior that cannot leave an unobservable detached task accidentally.

Do not change the direct runtime into implicit shutdown-on-control-drop unless
that behavior already exists. Explicit control is preferred for embedding.

## Compatibility-core relationship

`eggserve-core` already has a richer compatibility `ServerHandle` and
lifecycle state machine. Do not route direct callers through core.

Instead:

- keep `eggserve-server` as the direct H1 lifecycle authority;
- reuse compatible naming/result semantics where useful;
- do not add H2/TLS/static dependencies to the direct crate;
- do not redesign the compatibility handle unless a tiny adapter/re-export is
  necessary for consistency.

## Tests

Add deterministic direct-server tests covering at minimum:

1. **Independent supervision**
   - start a direct server;
   - split control and completion;
   - prove the completion future can be pending while the control value remains
     usable;
   - request shutdown through control;
   - completion returns `Ok(ShutdownResult::Clean)`.

2. **Immediate shutdown**
   - split immediately after start without yielding;
   - control shutdown;
   - completion finishes under a bounded test timeout.

3. **Repeated shutdown**
   - multiple control clones call shutdown;
   - completion remains single-owner and returns once.

4. **Control-drop neutrality**
   - dropping an extra control clone does not stop the server.

5. **Terminal task panic propagation**
   - use a test-only seam/helper rather than production panic injection where
     possible;
   - prove a runtime task panic/cancellation reaches the typed completion path
     as an error;
   - prove legacy `ServerHandle::wait()` remains source-compatible.

6. **Connection drain**
   - retain Plan 243's in-flight connection drain proof under the split API.

7. **No resource leak**
   - connection permits/tasks return to baseline after shutdown.

Add a compile/API fixture showing the intended `tokio::select!` supervision
pattern. This is important: the API exists to make that pattern possible
without borrow/lifetime tricks.

## Documentation

Update current-state documentation for the direct embedding API:

- crate-level `eggserve-server` docs;
- public rustdoc on the new control/completion values;
- `docs/public-api-boundary.md`;
- `docs/migration-guide.md` only to note the additive 0.2.x capability;
- architecture/runtime docs that currently describe direct `wait()`.

Explicitly document that the legacy `wait()` discards terminal detail for
compatibility and that critical supervisors should use the typed completion
path.

## Verification

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggserve-server --all-targets -- -D warnings
cargo test -p eggserve-server
cargo test -p eggserve-core --test direct_h1_parity
cargo test -p eggserve-core --test direct_service_convergence
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
```

Then run the routine repository CI matrix on the exact implementation SHA.

## Acceptance criteria

- [ ] Direct callers can retain a cloneable shutdown/control capability while
      independently awaiting one single-owner completion value.
- [ ] The new completion path returns a typed terminal result.
- [ ] Top-level runtime task panic/cancellation is observable and is no longer
      silently discarded by the new API.
- [ ] Runtime-owned connection-task panic is not silently discarded when it
      escapes existing request/service containment.
- [ ] Starting to await completion does not itself request shutdown.
- [ ] Existing `ServerHandle::wait(self) -> ()` remains source-compatible.
- [ ] Existing direct `shutdown()` remains idempotent.
- [ ] Plan 243 durable-shutdown and connection-drain behavior remains intact.
- [ ] No direct H2/TLS/static/Python capability is introduced.
- [ ] A compile fixture demonstrates a critical-task `tokio::select!` pattern
      with independent external shutdown.
- [ ] Focused tests, topology/conformance checks, and routine CI pass.

## Non-goals

- No general lifecycle framework.
- No process/signal/service-manager integration.
- No application routing or middleware API.
- No H2/H3/TLS migration into `eggserve-server`.
- No change to public request/response/service types.
- No change to timeout defaults.
- No new async executor.
- No Gregg-specific type or callback.

This plan supplies a generic embedding primitive that downstream supervisors
may use; downstream policy remains downstream-owned.
