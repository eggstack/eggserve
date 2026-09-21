# Plan 258 — Post-256 async suppressed-body lifetime qualification and closure

## Baseline and candidates

- Plan 257 planning baseline: `cd6061a97f6538d013f0fac2adc1653a96097dd0`
  (`docs: clarify metadata-SHA record in Plan 256 closure (metadata-only, final)`).
- Plan 256 implementation candidate (preserved): `4c145421c851fffa5e1f6762a7ef742c5db1e5d8`
  (remote CI run `35653232800`, rust / supply-chain / python all success).
- Plan 257 implementation / Plan 258 closure candidate (exact, on `main`):
  `c22a2d20dc5dec31da17f8cc1b97c0378b022c88`
  (`plans: fix async suppressed-body permit lifetime (257)`).
- No public Rust/Python API, capability, support-tier, queue-bound, or
  security-behavior change. Rust/PyO3 sources untouched; the fix is
  Python-only (`lowlevel.py`) plus a new wheel test module and two
  six-line doc-truthfulness notes (`AGENTS.md`, skill).

## Track A — original defect proof

Baseline reproducer (repo `lowlevel.py` at planning baseline, `PYTHONPATH`
run, `max_async_tasks=1`, `response_write_timeout_secs=60` so expiry cannot
mask the bug; handler returns an async stream for HEAD, then a buffered GET):

```text
HEAD => 200 body=b'' polls=[] tasks=1 sem_locked=True
GET  => 503 body=b'Service Unavailable' polls=[] tasks=1 sem_locked=True
```

Causal chain confirmed:

```text
stream marker returned
 -> async permit transferred to bridge
 -> producer task created, parks on first_pull
 -> canonical Rust drops HEAD/body-forbidden iterable without polling
    (sync_handler.rs: drop(iterable), no __next__ call)
 -> never-entered Python generator `finally` never runs (Track B premise
    proven: unentered generator close() leaves cleanup unrun)
 -> producer remains until first-pull timeout (60s here)
 -> permit retained until producer task completion
 -> next valid request fails fast with 503
```

After the fix, the same reproducer reports:

```text
HEAD => 200 body=b'' polls=[] tasks=0 sem_locked=False
GET  => 200 body=b'next-ok' polls=[] tasks=0 sem_locked=False
```

## Final lifetime-owner design (Plan 257 Track C/D)

- New private `_AsyncStreamBridgeIterator` (in `lowlevel.py`, next to
  `_AsyncStreamMarker`): `__iter__ -> self`; `__next__` signals `first_pull`
  exactly once (thread-safe flag), then performs the same bounded
  `run_coroutine_threadsafe(queue.get()).result(timeout)` + bytes/EOF/error
  validation the generator performed.
- Cleanup attached to the object lifetime, not a generator frame:
  idempotent `close()` cancels the producer via
  `loop.call_soon_threadsafe(prod.cancel)`; `__del__` best-effort calls
  `close()`. Runs even when Rust drops the iterable without a first pull.
- Exactly-once permit ownership preserved: `close()` never releases the
  semaphore directly; the existing producer-task done callback
  (`_release_permit`) still releases once. No second semaphore, no hidden
  queue, no reference cycle (iterator -> task one-way; producer closure and
  done callback never capture the iterator).
- First-pull bound remains only as a wedged-loop fallback; the drop path
  cancels immediately.
- `threading` import added; stale `producer = asyncio.current_task()`
  placeholder removed; comments corrected (no claim that an unentered
  generator `finally` cancels).

## Track B — suppressed-body immediate reuse (final candidate)

`test_async_suppressed_lifetime.py::SuppressedPermitReleaseTests`
(`max_async_tasks=1`, `response_write_timeout_secs=60`, explicit
`_wait_for_baseline` polling, never sleeping for the timeout):

- HEAD stream: 200, empty body, `polls == []`, tasks/sem baseline restored
  immediately, then buffered GET 200 (not 503).
- 204 stream: same (204, empty, unadvanced, immediate reuse).
- 304 stream: same (304, empty, unadvanced, immediate reuse).

## Track C — repeated resource-lifetime evidence

`RepeatedSuppressedClosureTests` (`max_async_tasks=1`, 60s bound):

- 5× HEAD streams: each 200/empty, baseline restored each iteration,
  `polls == []`, final `len(_tasks) == 0`, `sem._value == 1`.
- 5× 204 streams: same for 204.
- Interleaved `HEAD -> buffered GET -> 204 -> buffered GET -> HEAD ->
  buffered GET`: every step admitted, no 503 from history, no app-state
  advance.

## Track D — ordinary streaming parity

- Existing Plan 254 matrix green unchanged: `test_async_lifecycle` +
  `test_async_bridge` 39/39 pass (admission 503, buffered return, stream
  permit hold, error release, no-double-release, timeout races,
  shutdown-during-handler, chunk ordering/empties/non-bytes/length
  mismatch/sync-iterable, HEAD/204 suppression, disconnect, tunnel,
  registry closure).
- New-owner smoke (`NewOwnerParitySmokeTests`): 40-chunk ordered stream
  exact, sync-iterable `[b"x", b"y"]`, trailer stream with `data` — all
  pass with baseline restoration.

## Track E — exactly-once cleanup / races

- `LifetimeOwnerUnitTests::test_explicit_close_idempotent_no_overrelease`:
  double `close()` + `del` never over-releases (`sem._value <= capacity`).
- `test_shutdown_race_with_unpulled_body`: suppressed HEAD then immediate
  `shutdown()` leaves `len(_tasks) == 0`, no over-release.
- Repeated/interleaved tests assert `sem._value` returns to capacity and
  task count to zero without shutdown or timeout waits.
- Producer EOF/error paths call `close()` (no-op when done); stall
  `TimeoutError` path cancels before raising, matching the old generator
  `finally` for entered streams.

## Track F — canonical suppression authority

- Diff review: `_bridge_streaming` and `_AsyncStreamBridgeIterator`
  contain no `method == "HEAD"` / status-table branch. Suppression
  decision stays in `sync_handler.rs` (`is_head || !permits_payload_body()`
  → `drop(iterable)`); Python reacts correctly to pull-or-drop.
- Structural test `test_bridge_has_no_python_suppression_table` pins this
  (code body, excluding comments/docstring, contains no `== "HEAD"`,
  `permits_payload_body`, or `marker.status` branch).

## Track G — sync-Python non-regression

- Plan 257 is Python-only; `crates/eggserve-python/src/` untouched
  (`git diff` shows no Rust changes).
- Full installed-wheel run retains the existing sync suites (callback,
  body, boundary, compat, TLS) — all green inside the 845-test wheel
  result below.

## Track H — public surface freeze

- `git diff 74fb872..c22a2d2 --stat`: only `lowlevel.py` (private class +
  comments, `threading` import), new private test module,
  `AGENTS.md`/skill six-line truthfulness notes. No `lowlevel.pyi`,
  no native signature, no `RuntimeConfig` field/default, no import-path,
  no Rust public-item, no H1-tier change.

## Track I — local qualification (final candidate)

- `check-python-release-metadata.py`: pass (0.2.0).
- `check-crate-topology.py`: green.
- `verify-conformance-matrix.py`: 51 matrix + 55 app-server (47 routine)
  + 17 H3 entries valid.
- `cargo fmt --all -- --check`: clean.
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`: clean.
- Focused: new `test_async_suppressed_lifetime` 13/13 pass (direct
  `PYTHONPATH` run); existing `test_async_lifecycle` + `test_async_bridge`
  39/39 pass.
- `bash scripts/test-python-wheel.sh`: full installed-wheel green —
  **845 tests OK** (832 pre-existing + 13 new), strict mypy fixture green,
  smoke green.
- No Rust/PyO3 change, so no Rust matrix re-run beyond remote CI; supply
  chain / packages unchanged (no manifest edits).

## Track J — remote CI provenance

- Candidate SHA: `c22a2d20dc5dec31da17f8cc1b97c0378b022c88`
- Run ID: `35658950539`
- URL: `https://github.com/eggstack/eggserve/actions/runs/35658950539`
- Created/completed: `2026-09-21T21:44:54Z` / `2026-09-21T22:00:42Z`
- Conclusions: `rust` success, `supply-chain` success, `python` success.

## Track K — historical reconciliation (this commit)

- `plans/ROADMAP.md`: Plans 257–258 marked complete with this evidence link.
- `release/plan-256-post-convergence-maintenance-interop-closure.md`:
  narrow supersession note only — Plan 256 remains valid for typing,
  topology, overlap, import cleanup, and the rest of the async parity
  matrix; only its suppressed-body permit/task-lifetime claim is
  corrected by Plans 257–258. Plan 256 is not rewritten as though the
  defect were known at its closure.
- `AGENTS.md` + skill: async invariant now states the corrected drop
  owner (`_AsyncStreamBridgeIterator` releases immediately, never until
  `response_write_timeout_secs`). `README.md`, `docs/python-api.md`, and
  `architecture/eggserve-python.md` needed no change: their existing
  promises (suppressed bodies never advance app state; producers hold
  bounded admission only while owned/active; shutdown/disconnect cancels)
  are now literally true.

## Acceptance

All Plan 258 acceptance criteria hold on the candidate above. If a
suppressed body can still retain a permit until
`response_write_timeout_secs`, this corrective is not closed — the
reproducer, repeated, and interleaved tests above prove it cannot (with
a 60s bound, reuse is immediate).

A later metadata-only commit records these run IDs in this file and flips
the roadmap line to complete; that commit is documentation only and is
recorded separately below.

- Metadata-only documentation SHA: recorded in git history (see
  `git log --oneline`). Per the closure rule it is not claimed as an
  independently qualified candidate.
