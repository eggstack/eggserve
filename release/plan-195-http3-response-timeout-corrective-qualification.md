# Plan 195 HTTP/3 Response-Timeout Corrective Qualification

Date: 2026-09-11
Candidate base: `a3c3c91 Close Plan 194 H3 response-producer timeout and promotion-trace correction`
(no runtime source change in this plan; Plan 195 adds two qualification tests
plus pointer/status documentation on top of the Plan 194 candidate)
Environment: Linux x86_64 (Ubuntu 24.04.5 LTS, kernel 6.8.0-139-generic), loopback qualification
Decision: **corrective qualification passes — H3 remains experimental (opt-in); no promotion granted.**

## Scope

This is a corrective evidence pass, not a promotion campaign and not a
capability change. It proves the Plan 194 H3 streaming-response timeout
correction behaves as documented, that the Plan 193 historical trace is
internally consistent, and that H1/H2/H3 support claims remain conservative.

Runtime source is byte-identical to the Plan 194 candidate (`git diff`
touches only `crates/eggserve-core/tests/http3_runtime.rs`, plan/status
docs, and pointer lists). Changes in this pass:

- `crates/eggserve-core/tests/http3_runtime.rs`: two new Track I/J
  regression tests (H3 suite 14 → 16 tests);
- `plans/195-*.md` status closure;
- pointer/status documentation: `README.md`, `AGENTS.md`,
  `.opencode/skills/eggserve-dev/SKILL.md`, `architecture/http3.md`,
  `plans/ROADMAP.md`, `docs/release-contract.md`,
  `docs/library-capability-matrix.md`, plus this record.

## Candidate freeze

| Item | Observed value |
|---|---|
| Candidate commit | `a3c3c91` (current `main`, directly after the Plan 194 closure) |
| `h3` / `h3-quinn` / `quinn` | 0.0.8 / 0.0.10 / 0.11.11 — identical to the Plan 192/193/194 frozen candidate; no dependency change in this plan |
| `rustls` / `tokio` | 0.23.41 / 1.52.3 — unchanged |
| Rust stable / MSRV | 1.98.1 / 1.88 (`cargo +1.88 check --features http3,tls` green) |
| OS/architecture | Linux 6.8.0-139-generic x86_64 (Ubuntu 24.04.5 LTS) |
| `response_write_timeout` values under test | 100 ms (stalled/sibling, observability), 150 ms (stall-after-progress, empty-no-refresh), 200 ms (slow-progress), 300 ms (empty-then-data), 10 s producer budget vs 100 ms graceful deadline (shutdown race) |
| Time mode | real Tokio time (no paused time); short real budgets with generous client bounds (2 s), no exact-millisecond acceptance |
| Feature sets | `http3,tls` (targeted + full), `http2,tls` (non-regression), default workspace (non-regression) |
| Plan 192 upstream snapshot (factual only) | `BLOCKED` stands; `hyperium/h3#338` still open with PR #339 unmerged and `h3` 0.0.8 still the latest published release (re-checked 2026-09-11); the `#262` remainder carried over from the Plan 194 inventory with the lockfile unchanged |

No H3 dependency was updated to execute this plan.

## Track A — Static implementation inspection

The `ResponseBody::Stream` branch of `send_canonical_response()` in
`crates/eggserve-core/src/server/http3.rs` (Plan 194 shape, unchanged here)
confirms the full contract:

- `stream.send_response(...)` stays bounded by `response_write_timeout`;
- the producer wait uses `tokio::time::timeout_at(producer_deadline,
  response_stream.next())` — no unbounded raw `next().await` path remains;
- the budget is one monotonic absolute deadline armed after HEADERS, not a
  fresh relative timeout per item (a wrapper that resets per `next()` call,
  including after empty chunks, would not pass — this implementation does not
  do that);
- empty chunks `continue` without re-arming, so they cannot refresh the budget;
- a non-empty chunk goes through the existing bounded `send_bytes()` path
  (each `send_data` keeps its own `response_write_timeout` under QUIC flow
  control); only successful send re-arms the producer deadline;
- `stream.finish()` stays bounded;
- producer timeout returns the private `Err("response producer timeout")`,
  handled by `send_response_or_cancel()` as a post-commit failure
  (lifecycle cancelled with `TransportFailure`, send direction reset with
  `H3_INTERNAL_ERROR`, stream-scoped — no `cancel_all()`);
- no second HTTP status/body is attempted after commitment;
- no new public H3/timeout type leaked into the stable canonical API (the
  error string is private; `ServiceError`/`ServerError` untouched).

## Tracks B–F — Producer-timeout behavior

All use the existing in-process H3 harness (real time, no external client):

- **B (stalled-before-first-item)** — `h3_stalled_response_producer_times_out_and_sibling_survives`
  (100 ms budget): handler response-start succeeds promptly, 200 headers
  commit, no DATA is produced, the task terminates within the 2 s client
  bound via the post-commit reset path, no second response is emitted.
- **C (progress-then-stall)** — `h3_producer_stall_after_progress_times_out_from_last_progress`
  (150 ms budget): the first real chunk reaches the client, then the parked
  stream terminates — the deadline runs from last meaningful progress, not
  response creation. A declared known length would still fail closed on
  mismatch rather than wait forever (existing length validation untouched).
- **D (slow-but-progressing)** — `h3_slow_progressing_producer_completes_beyond_one_timeout_interval`
  (200 ms budget, four 60 ms-spaced chunks, ~240 ms total): full `xxxx` body
  received in order — reset-on-progress semantics, no total-duration limit.
- **E1 (empty then data)** — `h3_empty_chunks_then_data_within_deadline_succeed`
  (300 ms budget): two empty chunks then `data` succeeds with exact `data`
  accounting.
- **E2 (empty then pending)** — `h3_empty_chunks_do_not_refresh_producer_deadline`
  (150 ms budget): two empty chunks then parked terminates without bytes —
  timeout measured from the last meaningful progress point.
- **E boundary (infinite ready empty chunks)** — not exercised as a live
  test: an always-ready infinite empty-chunk producer never yields to the
  runtime, so no time-based future can preempt it. No such reproducer was
  observed in the deterministic suite; per the Plan 194 authorization no
  cooperative-yield guard or chunk-count limit was added. If profiling ever
  demonstrates a CPU-spin problem there, it belongs in a new narrow plan.
- **F (sibling isolation, mandatory gate)** — the stalled test completes a
  `/sibling` 200 on the same connection while A is stalled and again after
  A's termination; A's lifecycle cancellation does not cancel B; the
  connection stays usable with no connection-wide `cancel_all()` from the
  producer timeout. **Pass.**

## Track G — Existing response-semantics regression

Full H3 suite green: **16/16** (9 pre-194 + 5 Plan 194 + 2 Plan 195), plus
the whole `--features http3,tls` tree (**1650 passed, 3 ignored, 46
suites**): buffered `Bytes`, file responses, known-length success/mismatch,
producer `Err`, peer close, send failure, graceful shutdown, lifecycle
cleanup, max-request drain, body Reject/Buffer/Stream, DATA without
`Content-Length` (Plans 189–190), and early-error stream scoping (Plan 192)
all unchanged. Ordinary content, HEAD, range, conditional, error, and
canonical response semantics are untouched (no shared-path source change).

## Track H — Transport flow-control separation

By inspection plus existing tests: application-data wait is governed by the
absolute producer deadline; once non-empty bytes exist, `send_bytes()`
bounds each awaited `send_data()` under QUIC flow control with its own
`response_write_timeout`; a producer timeout claims no packet-transmission
measurement and a QUIC send timeout requires no stalled producer; both
failure strings funnel through the same bounded, stream-scoped
post-commit boundary (`send_response_or_cancel`). No cheap deterministic
receive-credit-withholding fixture exists, so no new wire-level
flow-control run was manufactured; no external H3 tooling was made a
prerequisite.

## Track I — Shutdown race (new test)

`h3_stalled_producer_shutdown_race_drains_without_surviving_tasks`: 200
headers commit (producer parked post-commit, 10 s budget), then
`shutdown()` with a 100 ms graceful deadline. `wait()` completes well
inside the 5 s bound with `ShutdownResult::Timeout` (forced drain, as
expected — the parked task outlives the grace period); the lifecycle
cancels with `ServerShutdown` (shutdown wins the race deterministically;
the plan accepts either ordering); `write_stall_timeouts` stays 0 (the
producer timeout never fires afterwards — no double-report); all active
gauges return to 0 (no double-release, no surviving task). The pre-existing
`h3_forced_shutdown_wakes_remaining_lifecycle_waiter` (pre-response park)
still passes, covering the other ordering. Stable across repeated runs.

## Track J — Observability (new test)

`h3_stalled_producer_timeout_observes_write_stall_and_releases_permits`
(100 ms budget, explicit `OpsContext`): after client-observed termination
the snapshot shows exactly one `write_stall_timeouts` (`WriteStallTimeout`
event at `Warn` with connection ID), `streaming_completed == 0` (timeout is
never counted as completion — H3 does not traverse the Hyper-facing
`ResponseStreamAdapter` that owns that counter), `stream_producer_errors
== 0` (explicit producer `Err` keeps the generic failure path with no
stall counter, so timeout stays distinguishable), `active_service_requests
== 0` and `active_file_streams == 0`; after shutdown
`active_connections == 0` with the stall count still exactly 1 (emitted at
most once per timed-out response). Normal logs expose no response bytes,
QUIC identifiers, TLS secrets, or dependency internals (message is the
fixed `"H3 response write stall timeout"`).

## Track K — H1/H2 non-regression

No shared-path source change in this plan (or in Plan 194 outside the
`http3` adapter): H1 still defines progress through socket
writes/`ProgressIo`; H2 still observes application-body producer/poll
progress with the conservative connection fallback (not stream-level wire
progress); `connection_total_timeout` still covers TCP/H1/H2 only, not H3;
no Python behavior change; the no-default-features tree still contains no
`h3`/`h3-quinn`/`quinn` (enforced by `scripts/qualify-http3.sh`). Suites:
workspace **1729 passed, 3 ignored (53 suites)**; `http2,tls` core **1630
passed** and bin **141 passed**; `tls` bin **141 passed**; `qualify-http2.sh`
passes (H2/TLS wire via curl + nghttp).

## Track L — Plan-history consistency audit

Searched all `Plan 193` / `plan-193` references. Correct state everywhere:

- Plan 192: `BLOCKED` readiness gate (unchanged);
- Plan 193: **closed at preflight 2026-09-10; supported-tier qualification
  was not entered** (`plans/193-*.md` status + Plan 194 corrective note,
  `release/plan-193-*` scope reframed as preflight/evidence inventory with
  test counts, environment details, blocker inventory, and gate results
  preserved);
- live references (`README.md`, `plans/ROADMAP.md`,
  `architecture/http3.md`, `docs/release-contract.md`,
  `docs/api-stability.md`, `docs/dependency-policy.md`,
  `docs/http-primitives.md`, `docs/library-capability-matrix.md`,
  `docs/deployment.md`, `docs/threat-model.md`, `AGENTS.md`,
  `.opencode/skills/eggserve-dev/SKILL.md`) all use the canonical
  preflight-blocked wording. Remaining "executed" strings are either the
  accurate Plan 191 record or descriptions of the pre-194 wording — no live
  document claims Plan 193 executed promotion qualification. Immutable
  commit subjects untouched.

## Track M — Timeout documentation audit

`response_write_timeout` claims are consistent: `docs/timeout-reference.md`
(row 9 + §9 + Known limitations) splits HEADERS send, absolute producer
deadline (empty chunks excluded), per-send flow-control bound, finish, QUIC
idle, and `connection_total_timeout` inapplicability with the Plan 194
recommended wording; `architecture/http3.md` response paragraph and Plan
194 note match; `README.md` keeps the H2 (producer/poll + connection
fallback) vs H3 (absolute producer deadline + per-send bound + stream
reset) split; the Plan 192 readiness record keeps history with the appended
Plan 194 corrective note (not a silent rewrite). This plan adds the Plan
195 pointer without changing the semantics text.

## Track N — Support-tier preservation

Verified no live document claims: H2 supported (all say experimental after
Plan 191), H3 supported (all say experimental; Plans 193/194/195 retain the
tier), Plan 192 READY (all say `BLOCKED`), a released `h3#338` fix (all say
open/unfixed — re-confirmed 2026-09-11), closed `#262` paths (all record
the three-path remainder), or browser/two-family/adversarial/network/platform
H3 evidence (all inventoried as unavailable). Final tiers:

- HTTP/1.1: supported default/baseline;
- HTTP/2: experimental, opt-in (Plan 191 blockers unchanged);
- HTTP/3: experimental, opt-in (Plan 192 dependency/readiness blockers unchanged).

A future promotion of either protocol needs a new scoped plan and fresh evidence.

## Track O — Verification matrix

Green on the execution tree (2026-09-11):

```text
python3 scripts/verify-conformance-matrix.py            # 51 entries validated
python3 scripts/check-python-release-metadata.py        # preflight passed (0.1.2)
cargo fmt --all -- --check                              # clean
cargo +1.88 check --workspace --all-targets --features http3,tls  # green (MSRV floor)
cargo clippy --workspace --lib --bins --tests -- -D warnings       # no issues
cargo test --workspace                                 # 1729 passed, 3 ignored (53 suites)
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls        # 1630 passed, 3 ignored (46 suites)
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls         # 141 passed (7 suites)
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls               # 141 passed (7 suites)
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls        # 1650 passed, 3 ignored (46 suites; incl. 16-test H3 suite)
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls         # 141 passed (7 suites)
cargo audit                                             # no vulnerabilities (locked graph)
cargo deny check                                        # advisories, bans, licenses, sources ok
bash scripts/qualify-http2.sh                           # wire qualification passed
bash scripts/qualify-http3.sh                           # baseline passes; strict gates fail closed (exit 2)
```

`qualify-http3.sh` still reports `direct-h3-clients: <none>` with every
evidence class `SKIP` (never `PASS`); all six strict gates
(`REQUIRE_H3_CLIENTS`, `REQUIRE_TWO_H3_CLIENTS`, `REQUIRE_ADVERSARIAL_H3`,
`REQUIRE_H3_BROWSER`, `REQUIRE_H3_IMPAIRMENT`, `REQUIRE_H3_PLATFORM`) exit 2
as designed. Per the plan this is not used to bypass support-tier gates.

## Remaining H2/H3 promotion blockers (unchanged)

H2: browser evidence, macOS/Windows runtime evidence, trailer-scope
determinism, public safe per-stream reset/wire-progress hook (Plan 191).
H3: maintained disposition for `h3#338`, closure/mitigation of the residual
`h3#262` stream-termination paths, plus an environment capable of the
independent-client/browser/adversarial/network/platform promotion matrix
(Plans 192/193). Plan 195 is **not** an H3 support qualification record —
it is a corrective timeout/history qualification record. The corrective
line stops here per the plan's stop condition.
