# Direct H1 Runtime Milestone 309 — Accept-error classification parity between the direct and compatibility accept loops

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-15--accept-error-classification-parity`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (single H1 authority, single `Service` contract)
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/runtime.md` (accept loop, listener errors, admission)
- `architecture/structured-logging.md` (`OpsContext` event taxonomy)
- `docs/ops-logging.md`
- `docs/timeout-reference.md`
- `architecture/crate-topology.md` (crate ownership; direct-graph rules)

Primary class: invariant

## 1. Objective

Make the two accept loops behave the same when `accept()` fails.

`crates/eggserve-server/src/lib.rs:353-370` (the direct runtime's accept
loop) treats **every** accept error identically: count it, emit a `Debug`
event, sleep a flat 10 ms, and continue.

`crates/eggserve-core/src/server/accept.rs:994-1128` (the compatibility
accept loop) classifies first: it distinguishes severities, detects
descriptor exhaustion (including `EMFILE`/`ENFILE` on Unix and
`WSAEMFILE`/`WSAENFILE` on Windows), treats out-of-memory as non-fatal,
applies a shutdown-aware exponential backoff ramp
`[1, 2, 4, 8, 50, 100, 250, 500]` ms, and distinguishes fatal from
recoverable errors.

The consequence is a real operational difference: a direct embedder that
hits descriptor exhaustion runs a 10 ms flat retry loop where a CLI user
runs a ramp up to 500 ms — and the direct loop classifies a fatal error as
a transient one.

This milestone establishes **one** accept-error classification authority
used by both loops, and removes the operational divergence.

## 2. Why this milestone is ready

- The compatibility implementation is already written, reviewed, and in
  production use; it is the reference behavior.
- The classification is a pure function of an `io::Error` plus a
  platform-supplied exhaustion predicate — no architectural change is
  needed.
- The topology rules already permit the direct crate to own runtime
  behavior; the question is only which crate holds the helper.
- Hard dependencies (Plan 249 single H1 authority, Plan 272/275 observer
  work, Milestones 306–308) are closed or satisfied.

## 3. Current implementation evidence

### 3.1 The direct loop

`crates/eggserve-server/src/lib.rs:353-370`:

```rust
Err(error) => {
    // Never silently drop accept errors: account
    // them and back off briefly so a persistent
    // failure cannot hot-spin the accept loop.
    ops.counters()
        .listener_errors
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.emit(
        ops::Event::new(
            ops::Severity::Debug,
            ops::EventKind::ListenerTransientError,
            ops::sanitize_text_field(&format!("accept error: {error}")),
        ),
    );
    tokio::time::sleep(Duration::from_millis(10)).await;
}
```

Every accept error is: counted, emitted at `Debug` severity as
`ListenerTransientError`, and retried after 10 ms. There is no fatal
classification, so an unrecoverable accept failure retries forever.

### 3.2 The compatibility loop

`crates/eggserve-core/src/server/accept.rs:994-1128`
`classify_accept_error` returns a fatal flag and, when it is not fatal:

- maps the `io::ErrorKind` to a severity and an
  `ops::EventKind` (`Interrupted` → `Debug`, and so on);
- detects descriptor exhaustion with `is_fd_exhaustion` (`:1102`), which
  checks `rustix::io::Errno::MFILE`/`NFILE` on Unix, raw Winsock codes
  `10024` (`WSAEMFILE`) and `10023` (`WSAENFILE`) on Windows, and falls back
  to a case-insensitive message match (`"too many open files"`, `"emfile"`,
  `"enfile"`) for non-OS errors;
- treats out-of-memory as non-fatal;
- applies the shutdown-aware backoff ramp
  `BACKOFF_MS: [u64; 8] = [1, 2, 4, 8, 50, 100, 250, 500]` (`:1089`), with
  the sleep selecting on `shutdown_rx.recv()` so a shutdown is not delayed
  by the backoff.

### 3.3 The observable divergence

| Condition | Direct (`eggserve-server`) | Compatibility (`eggserve-core`) |
|---|---|---|
| `EMFILE` / `ENFILE` / `WSAEMFILE` | 10 ms flat retry, `Debug` | exhaustion-aware, ramp to 500 ms |
| fatal accept error | retried forever as transient | classified, fatal path taken |
| out of memory | retried as transient | non-fatal, classified |
| shutdown during backoff | sleeps the full 10 ms | `select!` on the shutdown receiver |
| backoff ramp | none | `[1, 2, 4, 8, 50, 100, 250, 500]` ms |

### 3.4 Ownership constraints

`scripts/check-crate-topology.py:151-156` forbids `eggserve-core` and
`eggserve-static` as dependencies of `eggserve-server`. `rustix` is *not*
forbidden for `eggserve-server` (the transport-neutral restriction at
`:105` applies to `eggserve-primitives`), but adding it would change the
direct dependency graph that the Plan 276/291 registry-only consumer proof
measures, and Plan 296 deliberately tightened that graph.

`eggserve-server` currently declares: `bytes`, `eggserve-primitives`,
`futures-util`, `http-body`, `http-body-util`, `httpdate`, `hyper`,
`hyper-util`, `tokio` (plus optional `http`/`tower-layer`/`tower-service`).

### 3.5 What the docs say

`architecture/runtime.md` and `docs/ops-logging.md` describe listener error
accounting and the `OpsContext` event taxonomy. The severity and event-kind
mapping used by the compatibility loop is the one that those documents
describe; the direct loop's uniform `Debug`/`ListenerTransientError`
emission is the deviation.

## 4. Invariants that must not regress

- Accept errors are never silently dropped. Both loops must count and
  emit every accept error.
- The `OpsContext` event taxonomy and the `listener_errors` counter keep
  their current meaning and are not renamed.
- Library code must not print; all diagnostics go through `OpsContext` with
  sanitized text fields.
- The compatibility accept loop's observable classification behavior does
  not change. This milestone brings the direct loop *to* it, not the other
  way around.
- The direct crate keeps no dependency on `eggserve-core` or
  `eggserve-static`.
- The direct dependency graph must not gain a dependency without an
  explicit, recorded purpose and a re-run of the Plan 276/291 ancestry
  proof.
- Shutdown responsiveness must not regress: a backoff sleep must remain
  interruptible by shutdown in both loops.
- No accepted connection, admission permit, or response is affected.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing.

## 5. Scope

### In scope

- Establish one accept-error classification authority and call it from
  both accept loops.
- Bring the direct accept loop to the compatibility classification:
  severity mapping, fatal/non-fatal decision, exhaustion awareness, and the
  backoff ramp, with a shutdown-interruptible sleep.
- Decide and record the placement of the shared helper and of the
  platform-specific descriptor-exhaustion predicate, including the
  dependency question in §6.
- Tests that inject synthetic accept errors (`EMFILE`, `ENFILE`,
  `WSAEMFILE`, `WSAENFILE`, out-of-memory, interrupted, and a plain
  transient error) and assert the classification, the emitted severity and
  event kind, the backoff schedule, and the fatal/non-fatal decision in
  both loops.
- Documentation updates for `architecture/runtime.md` and
  `docs/ops-logging.md`.

### Explicitly out of scope

- Changing the backoff ramp values. They are taken as the compatibility
  implementation's reference values. A change to them is a separate
  operator-facing decision.
- Changing the `OpsContext` event taxonomy or the `listener_errors` counter
  semantics.
- Changing accept-loop concurrency, admission, or connection accounting.
- The `runtime total lifetime` / `connection_total_timeout` semantics.
- The `RuntimeConfigBuilder` convergence and the ownership-knob reachability
  work — Milestone 308.
- The runtime error-representation consolidation — Milestone 306.
- The connection-overlap classification work — Milestone 307.
- Publication.

## 6. Required production changes

### Crates and ownership

Two placements are admissible; the implementing agent must choose one and
record the choice with its reasoning.

**Option A — the direct runtime owns the classifier.**
`eggserve-server` gains a public accept-error classification API (the
severity/event-kind/fatal mapping plus the backoff schedule), and
`eggserve-core` calls it from its compatibility accept loop, replacing its
crate-private `classify_accept_error`.

This matches the crate-ownership rule ("`eggserve-server` owns the H1
runtime") and makes the direct crate the single runtime authority. It
requires the descriptor-exhaustion predicate to be available in
`eggserve-server`, which currently has no `rustix` dependency.

**Option B — a shared neutral policy module.**
The classification policy moves to a crate that both already depend on.

`eggserve-primitives` is the only shared leaf, and it explicitly forbids
`rustix`, `tokio`, `hyper`, `windows-sys`, and every transport dependency
(`scripts/check-crate-topology.py:105-107`). A pure classification policy
that takes an already-computed exhaustion flag would fit, but the platform
predicate would then have to exist in both crates.

Expected owners under either option:

- `eggserve-server`: the classification authority, the direct accept loop's
  use of it, and the direct graph's dependency decision.
- `eggserve-core`: the compatibility accept loop delegates to the shared
  authority; its crate-private `classify_accept_error` and
  `is_fd_exhaustion` are removed or reduced to a thin delegation.
- `eggserve-static`, `eggserve-h3`, `eggserve-python`, `eggserve-bin`,
  `eggserve-primitives`: unchanged, unless Option B is chosen and
  `eggserve-primitives` is the neutral owner.

No new crate.

### Config and policy

No configuration default changes. The backoff ramp is a fixed schedule, not
a configurable knob, and this milestone does not make it one.

If the implementation requires a new `RuntimeConfig` field (it should
not), that is a stop condition.

### Protocol and compatibility

No protocol change. Accept failures happen before any request exists, so
no wire behavior is affected.

The only observable change is that a direct embedder now sees the
compatibility loop's severity, event-kind, backoff, and fatal
classification. This is the intended correction.

### Runtime and concurrency

- The backoff sleep must select on the shutdown signal so shutdown is not
  delayed by up to 500 ms.
- The backoff index must saturate at the end of the ramp rather than
  overflow or wrap.
- A fatal accept error must reach the same shutdown path the
  compatibility loop reaches today; the direct loop's fatal path must be
  reviewed carefully because it currently has none.
- No permit, connection slot, or admission resource is affected.

### Frontend or operator surface

None. The CLI already uses the compatibility loop and keeps its current
behavior exactly.

### Security and confinement

No confinement change. Accept-loop behavior is not a filesystem boundary.

The relevant security property is denial-of-service resistance: a
descriptor-exhaustion condition must back off rather than hot-spin, and a
fatal condition must not retry forever. The direct loop currently fails
both.

### Documentation and static guards

- `architecture/runtime.md` — one accept-error classification behavior for
  both loops.
- `docs/ops-logging.md` — the severity and event-kind mapping for accept
  errors.
- `docs/timeout-reference.md` — the accept backoff schedule, if it is
  documented there today.
- `scripts/check-crate-topology.py` — a guard that the compatibility
  accept loop does not re-implement the classification locally.

## 7. Ordered work packages

### Work package A — Decide and record the placement

Choose Option A or Option B. Record:

- the chosen owner;
- the dependency consequence, if any (a new dependency in
  `eggserve-server` must be justified against
  `docs/dependency-policy.md` and must be recorded as a deliberate change
  to the direct graph, with the Plan 276/291 ancestry proof to be re-run);
- how the platform-specific descriptor-exhaustion predicate is expressed
  without weakening correctness on Linux, macOS, and Windows.

Acceptance evidence: a written decision in the closure record with the
dependency argument, plus the exact dependency delta (`git diff` of the
manifests).

### Work package B — Implement the shared classification authority

Implement the classification once:

- severity and `ops::EventKind` mapping per `io::ErrorKind`;
- fatal/non-fatal decision;
- descriptor-exhaustion detection;
- the backoff ramp and the shutdown-interruptible sleep;
- the saturated backoff index.

Acceptance evidence: the classification is implemented once and is callable
from both crates.

### Work package C — Wire the direct accept loop

Replace the direct loop's flat 10 ms sleep with the shared classification.

Keep the existing counter increment and the sanitized text field. The
emitted severity and event kind must now follow the shared mapping.

Acceptance evidence: the direct loop emits the same severity and event
kind as the compatibility loop for the same error.

### Work package D — Wire the compatibility accept loop

Replace the compatibility loop's crate-private `classify_accept_error` with
a delegation to the shared authority, and remove the now-duplicated
helpers.

The compatibility loop's observable behavior must not change.

Acceptance evidence: the compatibility loop's classification is
byte-identical to the baseline.

### Work package E — Guard against re-duplication

Add a topology-gate rule that fails if the compatibility accept loop
re-defines the classification or the exhaustion predicate locally, and
include a `--self-test` mutation.

The rule must tolerate the thin delegation wrapper.

Acceptance evidence: the gate passes; the `--self-test` mutation proves
the rule is wired.

### Work package F — Classification tests

For both loops, inject synthetic accept errors and assert:

| Injected error | Expected classification |
|---|---|
| `EMFILE` (raw OS code) | exhaustion-aware, non-fatal, backoff |
| `ENFILE` (raw OS code) | exhaustion-aware, non-fatal, backoff |
| `WSAEMFILE` (Windows raw code) | exhaustion-aware, non-fatal, backoff |
| `WSAENFILE` (Windows raw code) | exhaustion-aware, non-fatal, backoff |
| out-of-memory | non-fatal, classified |
| interrupted | non-fatal, lowest severity, no long backoff |
| plain transient error | non-fatal, transient severity |
| fatal error kind | fatal, shutdown path taken |

Also assert the backoff sequence `[1, 2, 4, 8, 50, 100, 250, 500]` and that
the index saturates at 500 ms rather than exceeding it.

Acceptance evidence: the matrix is green for both loops.

### Work package G — Documentation

Update `architecture/runtime.md`, `docs/ops-logging.md`, and
`docs/timeout-reference.md` so they describe one accept-error behavior.

Acceptance evidence: no document describes a flat 10 ms accept retry for
the direct runtime.

## 8. Failure, cancellation, restart, and contention semantics

- A fatal accept error must terminate the accept loop through the same
  shutdown path in both crates, and must not be retried indefinitely.
- A shutdown signal during the backoff sleep must interrupt it
  immediately.
- The backoff index is per-accept-loop state and saturates; it must not
  reset on a transient error, and it must reset when the loop restarts.
- No connection, permit, or admission state is affected by an accept
  error.
- Descriptor exhaustion must not cause the loop to spin at the 1 ms ramp
  step indefinitely — the ramp saturates at 500 ms.

## 9. Compatibility and migration

- No wire-visible change and no protocol change.
- The direct runtime's accept-error observability changes: a direct
  embedder now sees the compatibility loop's severity and event-kind
  mapping, and a fatal accept error now terminates the loop instead of
  retrying forever. This is the intended correction and should be called
  out in `docs/migration-guide.md` as an operational behavior change for
  direct embedders.
- If Option A adds a dependency to `eggserve-server`, the direct graph
  changes. That is a supply-chain-visible change and must be recorded in
  the closure record and in `docs/dependency-policy.md` if it lands.

## 10. Required tests

### Focused unit tests

- The classification table over every `io::ErrorKind` the runtime handles.
- The exhaustion predicate over the Unix and Windows raw codes and over
  the message fallback.
- The backoff ramp and its saturation.

### Integration tests

- The direct server's accept loop classifies an injected accept error the
  same way the compatibility loop does (same severity, same event kind,
  same fatal decision).
- The compatibility loop's classification is unchanged from the baseline.

### Restart and recovery tests

- The accept loop restarts cleanly after a fatal accept error and the
  backoff index resets.

### Contention and cancellation tests

- Descriptor exhaustion does not hot-spin: the observed retry interval
  reaches the 500 ms saturation step.
- Shutdown during the backoff sleep is not delayed.

### Security and negative tests

- A fatal accept error is not retried forever.
- The error text is sanitized before emission (no raw path or credential
  leakage in the event payload).

### Migration and compatibility tests

- The CLI's observable accept-error behavior is unchanged (it uses the
  compatibility loop).

## 11. Required verification commands

```bash
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --lib
cargo clippy --workspace --lib --bins --tests -- -D warnings
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
cargo tree -p eggserve-server --no-default-features --features tower -e no-dev
cargo fmt --all -- --check
./scripts/verify.sh fast
```

If the direct graph gains a dependency, also re-run the Plan 276/291
registry-only consumer ancestry proof.

## 12. Documentation updates

- `architecture/runtime.md` — one accept-error behavior for both loops.
- `docs/ops-logging.md` — the accept-error severity/event-kind mapping.
- `docs/timeout-reference.md` — the accept backoff schedule.
- `docs/migration-guide.md` — the direct-embedder operational change.
- `docs/dependency-policy.md` — if a dependency is added to the direct
  crate.
- `plans/subsystems/direct-h1-runtime-roadmap.md` — Milestone 15 status.
- `plans/registry.md` — the Milestone 15 row.
- `plans/closure/direct-h1-runtime/309-accept-error-classification-parity.md`
  (new).

## 13. Acceptance criteria

- One accept-error classification authority exists and is used by both
  accept loops.
- The direct loop's severity, event kind, backoff ramp, and
  fatal/non-fatal decision match the compatibility loop's for every
  injected error in the test matrix.
- The compatibility loop's observable classification is unchanged from the
  baseline.
- A fatal accept error terminates the direct accept loop rather than
  retrying forever.
- Descriptor exhaustion backs off to the 500 ms saturation step rather
  than spinning.
- The topology gate fails if the classification is re-implemented locally
  in the compatibility crate, and the `--self-test` mutation proves the
  rule is wired.
- Any dependency delta in `eggserve-server` is justified in the closure
  record and in `docs/dependency-policy.md`.
- No protocol change, no wire change, and no CLI behavior change.

## 14. Stop conditions

Stop and report rather than improvise if:

- bringing the direct loop to the compatibility behavior would change the
  compatibility loop's observable behavior — the reference direction is
  fixed;
- the shared classification would require `eggserve-primitives` to depend
  on `rustix`, `tokio`, `hyper`, or `windows-sys`;
- adding a dependency to `eggserve-server` cannot be justified under
  `docs/dependency-policy.md`, in which case the exhaustion predicate must
  be expressed without it;
- the platform descriptor-exhaustion codes cannot be established
  correctly for every supported platform — a wrong code is worse than no
  detection, and the message fallback must be recorded as the fallback;
- the direct loop's fatal-error path turns out to require a shutdown-path
  change that would affect the CLI or the wheel.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/309-accept-error-classification-parity.md`
containing:

- the placement decision and its dependency argument;
- the exact dependency delta (or an explicit "no new dependency" statement);
- the classification table and the exhaustion-predicate table;
- the test matrix results for both loops;
- the backoff-sequence and saturation evidence;
- the compatibility-loop non-regression comparison;
- the gate rule text and the `--self-test` mutation result;
- the documentation diff;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The reference behavior is the compatibility loop's; do not "average" the
two. The most common way to get this wrong is to reimplement the
classifier in the direct crate because moving it looked awkward. Use one
authority, and if the placement decision is genuinely contested, record
both options and pick one with reasons rather than shipping two copies.
