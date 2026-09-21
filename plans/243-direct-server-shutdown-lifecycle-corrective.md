# Plan 243 — Direct-server shutdown and lifecycle correctness corrective

## Purpose

Fix the direct `eggserve-server::Server` shutdown/drain semantics before any
further runtime convergence.

Current `main` constructs an `Arc<tokio::sync::Notify>` in
`Server::start_with_service`, waits on `notified()` in the spawned accept
task and in per-connection relay tasks, and implements
`ServerHandle::shutdown()` with `notify_waiters()`. A waiter created after
`notify_waiters()` does not inherit a durable shutdown state. This creates a
race where an immediate shutdown or a shutdown concurrent with connection-task
startup can be missed.

The same path also detaches accepted connection tasks with `tokio::spawn`;
`ServerHandle::wait()` only joins the accept task. Thus a successful
`wait()` does not prove that all already-accepted direct-runtime connection
tasks have drained.

The compatibility runtime already has stronger lifecycle/drain semantics. This
plan corrects the direct runtime without changing its public API.

## Required behavior

Preserve these public paths and signatures:

- `eggserve_server::Server::builder()`;
- `ServerBuilder::{runtime,bind,ops_context,from_listener,from_std_listener,build}`;
- `Server::start_with_service`;
- `ServerHandle::{local_addr,shutdown,ops_context,ops_snapshot,wait}`;
- direct connection-driver and `Service` contracts.

No caller should need source changes.

After the corrective:

- a shutdown request is durable once issued;
- shutdown cannot be lost because a waiter has not yet been registered;
- accepted connection tasks observe shutdown even if task scheduling races with
  `shutdown()`;
- `wait()` does not return while runtime-owned accepted connection tasks are
  still live;
- ordinary connection completion and server start behavior remain unchanged;
- admission permits, request lifecycle cancellation, tunnel handling, and
  observability counters remain balanced.

## Design

### 1. Replace lossy notification with durable state

Use an internal primitive whose state is observable after the transition.

Preferred shape:

- a `tokio::sync::watch` shutdown channel carrying a boolean/state, or
- an atomic/lifecycle state plus a `Notify` used only as a wakeup.

Do not use `Notify::notify_waiters()` as the sole source of truth.

The accept loop and every accepted connection must first observe durable state
and then wait for changes. If shutdown was requested before a task starts, the
task must immediately take the shutdown path.

Do not expose the chosen primitive publicly.

### 2. Track accepted connection tasks

Replace detached connection spawning with runtime-owned task tracking, e.g.
`JoinSet` or an equivalent internal registry.

The accept loop owns the registry and, once shutdown is requested:

1. stops accepting new connections;
2. signals the canonical per-connection `ConnectionShutdown` tokens;
3. waits for accepted connection tasks to terminate;
4. only then allows the handle join/wait path to complete.

Preserve existing direct-runtime semantics where no public graceful-drain
deadline is presently promised. Do not silently invent a new public timeout or
result type in this plan.

If an internal bounded fallback is needed to avoid impossible hangs, it must
reuse an already-existing runtime limit or remain strictly private and be
documented; do not add an externally observable configuration field.

### 3. Make shutdown idempotent

Repeated `shutdown()` calls must be harmless. Shutdown before the accept task
first polls, shutdown after the accept loop has stopped, and shutdown during
connection drain must all converge on one terminal state.

### 4. Preserve connection-driver authority

Do not fork or rewrite `serve_http1_connection_with_id`. The lifecycle fix
belongs in listener/task ownership around the canonical driver.

Per-connection `ConnectionShutdown` remains the mechanism handed to the
driver. The server-level durable state merely guarantees that each token is
eventually triggered.

### 5. Observability

Keep existing counter/event semantics. Add only the minimum event accounting
needed to distinguish:

- server shutdown requested;
- accept loop stopped;
- connection drain completed;

if these concepts already have canonical event kinds. Do not expand the public
event taxonomy solely for this corrective.

## Tests

Add deterministic tests in the owning direct crate.

Required cases:

1. **Immediate shutdown before accept task scheduling**
   - start on port 0;
   - call `shutdown()` immediately without yielding;
   - bound the test with `tokio::time::timeout`;
   - `wait()` must complete.

2. **Repeated shutdown**
   - call `shutdown()` multiple times before and during wait;
   - no panic/hang.

3. **Accepted connection races with shutdown**
   - accept a client and arrange for the spawned connection task to be delayed
     before it enters its shutdown wait where feasible;
   - request shutdown;
   - prove the child still receives cancellation and exits.

4. **Wait accounts for in-flight connection**
   - service blocks on a test synchronization point;
   - issue shutdown;
   - prove `wait()` does not complete while the runtime-owned connection task
     remains live;
   - release/cancel it and prove completion.

5. **No admission leak**
   - exercise connection saturation + shutdown;
   - all permits/task records are reclaimed.

6. Existing `tcp_server_reports_real_socket_metadata` and prebound-listener
   tests remain green.

Add a regression test that would hang or fail reliably under the old
`notify_waiters()` behavior; avoid timing-only sleeps as the assertion.

## Qualification

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggserve-server --all-targets -- -D warnings
cargo test -p eggserve-server
cargo test -p eggserve-core --test direct_h1_parity
cargo test -p eggserve-core --test direct_service_convergence
python3 scripts/check-crate-topology.py
```

Then run routine workspace CI before closure.

## Acceptance criteria

- [ ] direct shutdown is backed by durable state;
- [ ] immediate shutdown cannot be missed;
- [ ] connection tasks cannot miss server shutdown because they start late;
- [ ] direct `wait()` accounts for accepted runtime-owned tasks;
- [ ] shutdown is idempotent;
- [ ] connection/admission permits are reclaimed;
- [ ] no public Rust signature/path/default changes;
- [ ] direct H1/core parity remains green;
- [ ] topology remains downward-only;
- [ ] exact corrective SHA passes remote CI.

## Non-goals

Do not move H2/TLS/proxy/listener behavior from core here yet. Do not redesign
`ServerHandle`, add a new public lifecycle enum, change the `Service` trait,
or alter timeout defaults. Those broader convergence steps belong to Plan 244.
