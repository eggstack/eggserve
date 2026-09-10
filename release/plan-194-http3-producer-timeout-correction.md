# Plan 194 HTTP/3 Response Producer Timeout Correction

Date: 2026-09-10
Candidate base: `8128fe7 Add Plan 195 H3 response timeout qualification`
(implementation on top of the Plan 194/195 planning handoff; behavior baseline
`368fc2a Close Plan 193 HTTP/3 promotion attempt as experimental`)
Environment: Linux x86_64, loopback qualification
Decision: **correction landed — H3 remains experimental (opt-in); no promotion granted.**

## Scope

Narrow corrective pass, not a promotion campaign. No transport capability was
added, no dependency was upgraded, no Python/default-feature/`server`-API
surface changed. Changes are confined to:

- `crates/eggserve-core/src/server/http3.rs`: absolute producer
  no-progress deadline (`timeout_at`) + `WriteStallTimeout` observability;
- `crates/eggserve-core/tests/http3_runtime.rs`: five new Track G
  regression tests (H3 suite 9 → 14 tests);
- Plan 193 trace correction: `plans/193-*.md` status, `release/plan-193-*`
  scope, and live references now say preflight-blocked rather than executed;
- Plan 192 readiness record: appended corrective note (history preserved);
- promotion-trace docs: `README.md`, `AGENTS.md`,
  `.opencode/skills/eggserve-dev/SKILL.md`, `architecture/http3.md`,
  `docs/timeout-reference.md`, `docs/ops-logging.md`, plus stale H3
  plan-pointer lists in `plans/ROADMAP.md`, `docs/deployment.md`,
  `docs/release-contract.md`, `docs/library-capability-matrix.md`,
  `docs/dependency-policy.md`, `docs/threat-model.md`,
  `docs/api-stability.md`, `docs/http-primitives.md`;
- this record + `plans/194-*.md` status closure.

## Defect corrected (Plan 192 Track F disposition)

`send_canonical_response` wrapped `send_response`, each `send_data` chunk,
and `finish` in `response_write_timeout`, but awaited
`response_stream.next().await` without any deadline. A canonical
`ResponseStream` that never yields after response commitment parked its H3
request task indefinitely — bypassing `handler_timeout` (response-start
already returned), with no send outstanding for the send-path timeout to
fire, `connection_total_timeout` intentionally inapplicable to H3, QUIC idle
transport-wide rather than a per-response producer guarantee, and shutdown
not a normal no-progress bound. Plan 192 Track F recorded "no new producer
deadline was added" on the theory that existing bounds already covered the
stall; re-examination shows that disposition was inaccurate for the
`next().await` park.

## Fix contract

- After response HEADERS succeed, an absolute producer deadline is armed at
  `now + response_write_timeout` and awaited via
  `tokio::time::timeout_at(deadline, response_stream.next())`.
- Empty chunks are **not** meaningful progress: they preserve the existing
  deadline (`continue` without re-arming), so an empty-chunk loop cannot
  refresh the budget indefinitely.
- A non-empty chunk is sent through the existing bounded `send_bytes()`
  path (each `send_data` keeps its own `response_write_timeout` under QUIC
  flow control); only after successful send is the producer deadline
  re-armed to `now + response_write_timeout`.
- Clean EOS keeps the existing declared-length validation; producer `Err`
  keeps the existing immediate-failure behavior.
- On producer timeout the function returns the private
  `Err("response producer timeout")`, reusing the existing
  `send_response_or_cancel` post-commit path: request lifecycle cancelled
  with `TransportFailure`, send direction reset with `H3_INTERNAL_ERROR` —
  stream-scoped, siblings survive. No H3 connection fallback is introduced
  (unlike H2's conservative connection-shutdown fallback). No second HTTP
  response is attempted after commitment.
- `Bytes`/`File`/`Empty` paths unchanged (already per-call bounded).
- Observability: `send_response_or_cancel` increments
  `write_stall_timeouts` and emits `WriteStallTimeout` at `Warn` with the
  connection ID for producer or send no-progress timeouts. Explicit
  producer `Err` keeps the generic failure path with no stall counter, so
  timeout stays distinguishable from producer error. Nothing response-,
  QUIC-, or TLS-sensitive is logged.

Resulting per-protocol contract (also updated in
`docs/timeout-reference.md`):

- H1: forward socket-write progress via `ProgressIo`; stall closes the
  connection.
- H2: per-response producer/poll progress; stall uses bounded connection
  shutdown (no public Hyper stream-reset hook).
- H3: absolute producer no-progress deadline (empty chunks excluded) plus
  per-send bounds; stall resets only the affected stream, siblings survive,
  `WriteStallTimeout` observed.

## Tests (Track G)

New deterministic regressions in `http3_runtime.rs` (short real timeouts,
generous client bounds, no external client):

- `h3_stalled_response_producer_times_out_and_sibling_survives` (never
  yields + sibling isolation + post-timeout usability): pending-forever
  `ResponseStream` on `/stalled` with `response_write_timeout = 100 ms`;
  200 headers observed, then termination without bytes (reset or EOS)
  within 2 s; sibling `/sibling` completes 200 on the same connection
  before and after semantics; clean shutdown.
- `h3_producer_stall_after_progress_times_out_from_last_progress`:
  one real chunk then pending (`150 ms` budget); client receives the
  first chunk, then the stream terminates — proving the deadline runs
  from last meaningful progress, not response creation.
- `h3_slow_progressing_producer_completes_beyond_one_timeout_interval`:
  four 60 ms-spaced chunks (`200 ms` budget, ~240 ms total); full
  `xxxx` body received — proving no-progress rather than
  total-duration semantics.
- `h3_empty_chunks_then_data_within_deadline_succeed` (E1): two empty
  chunks then `data` within a `300 ms` budget; succeeds with body
  `data` and exact accounting.
- `h3_empty_chunks_do_not_refresh_producer_deadline` (E2): two empty
  chunks then pending (`150 ms` budget); terminates without bytes —
  proving empty chunks cannot refresh the deadline.

Full H3 suite: 14/14 pass. H1/H2/Python gates unaffected (H3 edits
confined to the `http3` feature adapter and its tests).

## Docs corrected (Tracks E/F)

- Plan 193 status now reads preflight-blocked ("supported-tier
  qualification was not entered") with a Plan 194 corrective note;
  `release/plan-193-*` scope reframed as preflight/evidence inventory
  with evidence preserved; live references use the canonical
  preflight-blocked wording.
- Plan 192 readiness record keeps its history with an appended Plan 194
  corrective note on the overstated producer bound; BLOCKED decision
  unchanged.
- `docs/timeout-reference.md` row 9 + §9 + Known limitations: HEADERS
  send, absolute producer deadline (empty chunks excluded), per-send
  flow-control bound, finish, QUIC idle, and `connection_total_timeout`
  inapplicability split explicitly per the recommended wording.
- `docs/ops-logging.md`: `write_stall_timeout` row gains the H3
  per-stream clause.
- `architecture/http3.md`: response paragraph (absolute deadline,
  empty-chunk rule, reset), observability paragraph (write-stall
  counter), promotion pointer
  (`Plans 188, 190, 192, 193, and 194`), and Plan 193/194 notes.
- `README.md`: H2 (producer/poll + connection fallback) vs H3
  (absolute producer deadline + per-send bound + stream reset) split;
  Plan 193 preflight wording; pointer gains the Plan 194 record.
- `AGENTS.md` + skill: lumped "H2 ... so H2/H3 remain experimental"
  split into H2-accounting and H3-boundary bullets; H3 bullet gains the
  Plan 194 fix with retained experimental status.
- Stale pointer lists in `plans/ROADMAP.md`, `docs/deployment.md`,
  `docs/release-contract.md`, `docs/library-capability-matrix.md`,
  `docs/dependency-policy.md`, `docs/threat-model.md`,
  `docs/api-stability.md`, `docs/http-primitives.md` updated to name
  Plans 192/193/194 consistently with preflight-accurate wording; tier
  language unchanged.

## What did not change

- Dependency set identical to the Plan 192/193 frozen candidate (`h3`
  0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11 / rustls 0.23.41);
  `cargo audit` / `cargo deny check` green.
- Upstream blockers stand: `hyperium/h3#338` (open, no released fix)
  and the `#262` remainder (503-admission, Buffer-error, post-service
  unconsumed-body paths) are untouched by this plan.
- Plan 193 evidence inventory (two-family, browser, adversarial,
  impairment, platform) is preserved as inventoried-unavailable, not
  re-characterized as executed qualification.
- H3 remains **experimental (opt-in)**, disabled by default,
  Rust-only, Python HTTP/1.1-shaped. Plan 194 claims no `READY FOR H3
  PROMOTION`. Handoff to Plan 195 for corrective qualification.

## Verification

Full deterministic matrix green on the execution tree (see commit CI,
including the 14-test H3 suite);
`bash scripts/qualify-http3.sh` baseline passes with strict gates still
failing closed (exit 2) where external evidence is absent.
