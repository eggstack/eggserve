# Direct H1 Runtime Milestone 309 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/309-accept-error-classification-parity.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md`

Repository baseline reviewed: `a7e2f22` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Commit `9a24d48` — the implementation: the new `accept_error` authority, the
  two accept loops, the new test files, and the topology-gate rule.
- Commit `88d78be` — this closure record and the registry/roadmap updates.

## 1. Executive finding

There is now **one** accept-error classification authority,
`eggserve_server::accept_error`, and both accept loops use it.

The compatibility loop in `eggserve-core` already classified accept errors
correctly; the direct runtime's accept loop treated every error identically —
count it, emit one `Debug` / `ListenerTransientError` event, sleep a flat
10 ms, retry forever. A fatal accept error was therefore retried indefinitely
in the direct loop. Milestone 309 brings the direct loop **to** the
compatibility behavior. The reference direction was fixed by the plan and was
not "averaged".

The compatibility loop's observable behavior is unchanged: same counter, same
rate limiting, same `error_kind` field, same ramp, same shutdown-aware sleep,
same fatal return. Only the classification, the descriptor-exhaustion
predicate, and the backoff ramp moved into the shared authority.

The dependency graph is unchanged. `eggserve-server` gained **no** dependency:
the descriptor-exhaustion predicate is expressed with raw OS codes
(`EMFILE`/`ENFILE`/`WSAEMFILE`/`WSAENFILE`) rather than by adding `rustix`, so
the Plan 276/291 direct-graph proofs stay valid.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| One classification authority | met | `crates/eggserve-server/src/accept_error.rs` (new, 205 lines); `crates/eggserve-core/src/server/accept.rs` delegates; `crates/eggserve-server/src/lib.rs` uses it |
| Placement decision recorded with a dependency argument | met | §3.1 below; `cargo tree` evidence in §4 |
| Compatibility loop delegates; no local re-implementation | met | `crates/eggserve-core/src/server/accept.rs` — thin delegation only; the local severity mapping, the `Errno::MFILE`/`NFILE` checks, the message fallback, and the `static BACKOFF_MS` table are gone |
| Direct loop uses the shared classification | met | `crates/eggserve-server/src/lib.rs:370` (`accept_error::report_accept_error`), `:380` (`accept_error::accept_backoff`) |
| Direct loop backoff matches the compatibility ramp | met | `ACCEPT_BACKOFF_MS = [1, 2, 4, 8, 50, 100, 250, 500]`, saturating at 500 ms, reset after a successful accept |
| Fatal accept error terminates the direct accept loop | met | `crates/eggserve-server/src/lib.rs:369-376` — `if classification.is_fatal { task_shutdown.shutdown(); break; }` via the server's existing `ServerShutdown` path, the same one `ServerHandle::shutdown()` uses |
| Backoff sleep is shutdown-interruptible | met | `tokio::select!` against the shutdown signal; the ramp's maximum 500 ms never delays shutdown |
| Exhaustion predicate matches the compatibility behavior | met | §3.2 table |
| Severity/event-kind mapping matches the compatibility behavior | met | §3.3 table |
| No new dependency in the direct crate | met | `cargo tree -p eggserve-server --no-default-features --features tower -e no-dev` — no `rustix`, no `eggserve-core`, no `eggserve-static`, no `phf` |
| Machine-checked authority | met | `scripts/check-crate-topology.py::check_plan309_accept_error_authority` + 10 `--self-test` mutations |
| No compatibility-loop behavior change | met | §5 invariant review; compat unit tests added |
| Required tests | met | 6 direct-crate tests + 2 compat-crate tests (§4) |

## 3. Production implementation evidence

### 3.1 Placement decision

**Option A — `eggserve-server` owns the authority; `eggserve-core` delegates.**

- `eggserve-server` already owns the `ops` module as the single implementation
  authority (`crates/eggserve-server/src/ops/events.rs` defines `Severity` and
  `EventKind`; `crates/eggserve-core/src/ops/mod.rs` is a plain
  `pub use eggserve_server::ops::*;` re-export). The mapping onto
  `Severity`/`EventKind` therefore belongs there.
- `eggserve-core` already depends on `eggserve-server`, so the delegation is a
  direct call with no new dependency.
- `eggserve-primitives` was rejected: it is transport-neutral and forbids
  `rustix`/`tokio`/`hyper`/`windows-sys`, so the platform exhaustion predicate
  would have to be duplicated across two crates.

**Required dependency delta: none.** No `Cargo.toml` and no `Cargo.lock` was
changed by this milestone.

### 3.2 Descriptor-exhaustion predicate

| Input | Detected | Note |
|---|---|---|
| `EMFILE` (raw 24) | yes | Unix code |
| `ENFILE` (raw 23) | yes | Unix code |
| `WSAEMFILE` (raw 10024) | yes | Windows code |
| `WSAENFILE` (raw 10023) | yes | Windows code |
| `Error::other("Too many open files")` | yes | **message fallback (documented fallback)** |
| `Error::other("EMFILE: …")` / `Error::other("enfile")` | yes | message fallback, case-insensitive |
| `Error::other("connection refused")` | no | message fallback miss |

**Documented divergence, accepted deliberately:** the compatibility loop
previously matched only the platform's own raw code plus the message
fallback. The shared predicate matches the *union* of the Unix and Windows raw
codes on every platform, so it is a strict superset of the old Unix behavior
and a strict superset of the old Windows behavior. The alternative — making
the predicate platform-conditional — was rejected because it would let the two
loops classify the same error differently on different hosts, which is exactly
the drift this milestone removes. The message fallback remains for errors
whose raw code is absent (for example, errors constructed by hand).

### 3.3 Classification mapping

| `io::ErrorKind` | Severity | Event kind | Backoff | Fatal | `fd_exhausted` |
|---|---|---|---|---|---|
| `Interrupted` | `Debug` | `ListenerTransientError` | yes | no | false |
| `ConnectionRefused` / `ConnectionReset` / `ConnectionAborted` / `BrokenPipe` | `Debug` | `ListenerTransientError` | yes | no | false |
| `WouldBlock` / `TimedOut` | `Warn` | `ListenerTransientError` | yes | no | false |
| `OutOfMemory` | `Error` | `ResourceExhaustion` | yes | no | false |
| `Other` + exhaustion | `Error` | `ResourceExhaustion` | yes | no | true |
| `Other` | `Error` | `ListenerPersistentError` | no | **yes** | false |

### 3.4 Public API added to `eggserve-server`

```rust
pub const EMFILE_RAW: i32 = 24;
pub const ENFILE_RAW: i32 = 23;
pub const WSAEMFILE_RAW: i32 = 10024;
pub const WSAENFILE_RAW: i32 = 10023;
pub const ACCEPT_BACKOFF_MS: [u64; 8] = [1, 2, 4, 8, 50, 100, 250, 500];

pub struct AcceptErrorClassification { /* severity, event_kind, should_backoff,
                                          is_fatal, fd_exhausted */ }

pub fn classify_accept_error(error: &std::io::Error) -> AcceptErrorClassification;
pub fn is_fd_exhaustion(error: &std::io::Error) -> bool;
pub fn accept_backoff_ms(index: usize) -> u64;
pub fn accept_backoff(index: usize) -> std::time::Duration;
pub fn report_accept_error(error: &std::io::Error, ops: &ops::OpsContext)
    -> AcceptErrorClassification;
```

This is an additive public surface on the direct crate — a **minor** release
event when it is eventually published.

## 4. Verification executed

### Commands run

| Command | Outcome |
|---|---|
| `cargo fmt --all -- --check` | PASS (clean) |
| `cargo clippy -p eggserve-server --lib --bins --tests -- -D warnings` | PASS |
| `cargo clippy -p eggserve-core --lib --tests -- -D warnings` | PASS (first run failed on `crates/eggserve-core/tests/corpus_replay.rs` — a concurrent in-flight edit by another agent; re-ran after the edit settled → PASS) |
| `cargo test -p eggserve-server --lib` | PASS — 89 passed, 0 failed, 1 ignored |
| `cargo test -p eggserve-core --lib` | PASS — 160 passed, 0 failed |
| `cargo test -p eggserve-server --test accept_error_classification` | PASS — 6 passed, 0 failed |
| `cargo test -p eggserve-core --test accept_error_parity` | PASS — 2 passed, 0 failed |
| `cargo tree -p eggserve-server --no-default-features --features tower -e no-dev` | PASS — no `eggserve-core`, no `eggserve-static`, no `rustix`, no `phf` |
| `cargo +1.89 check -p eggserve-server --lib` / `-p eggserve-core --lib` | PASS (MSRV 1.89) |
| `python3 scripts/check-crate-topology.py` | PASS (exit 0) |
| `python3 scripts/check-crate-topology.py --self-test` | PASS — "self-test: 41 checks passed" at implementation time; the parent has since added the Plan 309 rule (51 checks), the Plan 310 rule (59 checks), and the Plan 311 rule (67 checks) |
| Extra: `cargo test -p eggserve-core --test ops_integration` / `--test direct_h1_parity` | PASS (63 / 16) — no regressions from the direct-loop change |

### Test inventory

Direct crate (`crates/eggserve-server/tests/accept_error_classification.rs`,
6 tests): baseline-value classification matrix asserted against the
**hard-coded compatibility baseline** (not merely crate-versus-crate
agreement, which would pass if both were wrong); exhaustion table including
the message fallback; ramp sequence and saturation; direct-path emission and
fatal decision; text sanitization.

Compatibility crate (`crates/eggserve-core/tests/accept_error_parity.rs`,
2 tests): compat-side baseline matrix through
`eggserve_core::layers::server::accept_error`; cross-crate agreement plus
compile-time vocabulary type-identity assertions.

## 5. Invariant review

| Invariant | Result |
|---|---|
| Accept errors are never silently dropped; both loops count and emit every accept error | Held. The direct loop emits every error; the compatibility loop's existing rate limiting is unchanged. |
| The `OpsContext` event taxonomy and the `listener_errors` counter keep their meaning | Held. No rename; the counter increment is unchanged in both loops. |
| Library code does not print | Held. Diagnostics go through `OpsContext` with `sanitize_text_field`. |
| The compatibility accept loop's observable classification behavior does not change | Held. Same counter, same rate limiting, same `error_kind` field, same ramp, same `tokio::select!` sleep, same fatal return. |
| `eggserve-server` keeps no dependency on `eggserve-core` or `eggserve-static` | Held. `cargo tree` evidence in §4. |
| No manifest changes at all | Held. |
| Shutdown responsiveness does not regress | Held. The ramp saturates at 500 ms and the sleep is shutdown-interruptible; a fatal error ends the loop through the existing shutdown path. |
| No accepted connection, admission permit, or response is affected | Held. The change is confined to the accept-error path. |
| No protocol or wire change | Held. |

## 6. Failure and recovery review

- A transient accept error is retried on the shared ramp, saturating at
  500 ms. The index resets only after a successful accept, so a persistent
  transient failure stays at the saturation step rather than oscillating.
- A fatal accept error (`ErrorKind::Other` that is not exhaustion) now ends
  the direct accept loop through `ServerShutdown` and breaks out of the loop,
  instead of being retried forever. The compatibility loop's fatal path
  already ended in `ShutdownResult::Clean`, so both loops now terminate the
  same way.
- Descriptor exhaustion is reported as `Error` / `ResourceExhaustion` with
  `fd_exhausted == true`, which is an observability change for the
  compatibility loop only in the case where the raw code was previously
  unmatched (the union-predicate divergence documented in §3.2). The severity
  and event kind for those inputs were already `Error` / `ResourceExhaustion`
  in the compatibility loop, so no emitted event changes.

## 7. Migration and compatibility review

- The compatibility crate's public surface is unchanged.
- The direct crate gains a public `accept_error` module — additive, a minor
  release event.
- The direct accept loop's **observable** behavior changes: accept errors now
  carry a meaningful severity and event kind, the backoff is the shared ramp
  rather than a flat 10 ms, and a fatal error terminates the loop. Direct
  embedders that read `listener_errors` or matched on
  `ListenerTransientError` should review `docs/ops-logging.md`.
- The CLI and the Python facade are untouched.

## 8. Security review

- The change does not alter the confinement boundary, the request path, or
  any response. It only classifies *accept* failures.
- Descriptor exhaustion is now distinguishable in the event stream, which is
  a denial-of-service observability improvement: an operator can tell
  "the process ran out of file descriptors" from "the listener failed for
  another reason". Previously the direct loop emitted an undifferentiated
  `Debug` event and retried forever.
- Terminating on a fatal accept error removes an unbounded retry loop, which
  is a resource-containment improvement.
- No new `unsafe`, no new dependency, no new attack surface.

## 9. Documentation and operations

| Surface | Change |
|---|---|
| `architecture/runtime.md` | One accept-error behavior for both loops; replaced the "fixed 10 ms" description; added the mapping table. |
| `architecture/structured-logging.md` | The "Listener Error Classification" section now names the single authority and the shared ramp, and states that a persistent error is **fatal**. |
| `docs/ops-logging.md` | Accept-error severity/event-kind mapping table and per-loop outcome; corrected the flooding note (rate limiting is compatibility-only). |
| `docs/timeout-reference.md` | Backoff catalog row and §1: the shared ramp, its saturation, the shutdown-interruptible sleep, and the shared enforcement owner. |
| `.opencode/skills/eggserve-dev/SKILL.md` | The listener-error-classification bullet now names the authority, the ramp, and the fatal behavior. |
| `crates/eggserve-core/Cargo.toml` | The `rustix` dependency comment no longer claims core needs `rustix::io::Errno`; it does not (Plan 309 moved the predicate). |
| `docs/migration-guide.md` | Plan 309 section (parent-owned file, updated by the parent). |
| `scripts/check-crate-topology.py` | New rule `check_plan309_accept_error_authority` plus 10 `--self-test` mutations and a live-tree check. |

## 10. Unresolved findings

1. **A live-loop fatal-termination test is not expressible in this
   milestone.** Forcing `accept(2)` to fail needs either `libc::shutdown(2)`
   on the listening descriptor (unsafe — the workspace sets
   `unsafe_code = "deny"`) or a new dev-dependency (no manifest changes were
   permitted). The **fatal decision** is asserted instead
   (`report_accept_error(...).is_fatal == true`,
   `event_kind == ListenerPersistentError`), together with the loop's branch
   on it. This is reported, not fixed.
2. **A timing test for "descriptor exhaustion does not hot-spin"** — the
   plan's optional timing assertion that the observed retry interval reaches
   the 500 ms saturation step — needs the same injected-failure facility. The
   saturation property is asserted directly on the ramp accessor instead.
   This is reported, not fixed.
3. **The union predicate is a superset of the old compatibility behavior**
   (§3.2). This is a deliberate, documented divergence, not an oversight.

## 11. Roadmap disposition

The direct-H1-runtime roadmap's Milestone 309 ("Accept-error classification
parity") is **closed**. The static-confinement, Python-facade, and
CLI/ops/observability roadmaps are unaffected by this milestone. Publication
is a separate milestone; the direct crate's additive public surface is a
minor release event when it ships.

## 12. Registry updates

- `plans/registry.md` — Milestone 309 moved to `closed` with a pointer to this
  closure record.
- `plans/subsystems/direct-h1-runtime-roadmap.md` — Milestone 309 status and
  closure-record column updated.
- A new topology-gate rule `check_plan309_accept_error_authority` was added
  to `scripts/check-crate-topology.py`, with ten `--self-test` mutations
  (local severity mapping, local exhaustion predicate, local ramp, dropped
  delegation, flat direct backoff, hard-coded direct event kind, missing
  authority file, `rustix` in the direct crate) and a live-tree positive check.
