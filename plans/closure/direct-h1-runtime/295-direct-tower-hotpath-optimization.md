# Direct H1 Runtime Milestone 295 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/295-direct-tower-hotpath-optimization.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-5--toweraxum-hot-path-optimization`

Repository baseline reviewed: `f2d581e0ce1fb18540f63175c319242162c48b6d`
plus 294 evidence work and this milestone's production changes

Implementation commits or pull requests:

- (this closure commit) — P1 + P2, focused tests, benchmark evidence

## 1. Executive finding

Only the two 294 PROCEED candidates were implemented, both retained with
measured wins and no semantic regression. P1 gives Tower/Axum buffered
responses `Content-Length` via declare-then-verify known-length streams.
P2 restores keep-alive reuse for bodyless Buffer/Stream requests (native
and Tower alike) without weakening framing safety. All broader hot-path
rework is explicit NO-GO/DEFER with rationale. No public API, feature,
config, default, or dependency change.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Every retained optimization maps to a 294 finding | P1→F1, P2→F2 | pass | Nothing else touched |
| No duplicate parser/framing/security authority | P1 declares length, transport verifies; P2 uses Hyper end-stream guarantee | pass | Single validation/framing authorities unchanged |
| Allocation/CPU cost improves or redundant layer removed with simpler code | A/B: tower p50 0.083→0.074ms (~11%), +~8% rps; P2 removes per-request TCP churn for default shape | pass | `benchmarks/295-direct-tower-optimization/` |
| Streaming/trailers/cancellation/timeout/shutdown equivalent | 295 focused tests (7) + full suites | pass | Hintless bodies stay chunked; lying length fails closed; unread bodies still close |
| Native Service + core forwarding green | `verify.sh fast` 24/24, conformance matrix, core tower lanes | pass | — |
| No new production dependency | manifest diff empty | pass | — |
| Focused tests per plan §10 | `direct_tower_hotpath_295.rs`: metadata/body/trailer/HEAD/commitment-failure/readiness covered via existing + new suites | pass | Trailer wire gap is F3 (separate plan) |
| Same-machine A/B vs 294 baseline | interleaved 294 matrix, release-candidate profile, 2 retained trials | pass | Native control stable |

## 3. Production implementation evidence

- `crates/eggserve-server/src/interop.rs`: `response_from_http_body`
  computes `known_len` from the exact size hint and selects
  `with_known_length_and_trailers` vs `with_trailers`. Doc comment updated.
  (+12/−1)
- `crates/eggserve-server/src/connection/pipeline.rs`: Buffer/Stream branch
  completes provably end-streamed bodies as `RequestBody::empty()` with an
  invariant comment. (+21/−1)
- Ownership respected: `eggserve-server` only; `eggserve-primitives`
  untouched (no new representation; existing constructors reused).
- No user knob added; equivalent semantics selected internally (fast paths
  are transparent, not opt-in).

Per-candidate decisions: **P1 KEEP, P2 KEEP**. Validate/project split,
unsafe aliasing, second framing implementation, broad Service redesign:
**NO-GO**. `Arc<Mutex>` rendezvous replacement and dedicated no-trailer
machine: **DEFER** (SSE overhead ~5%, risk exceeds measured budget).

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-server --no-default-features --features tower --test direct_tower_hotpath_295
cargo test -p eggserve-server --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
./scripts/verify.sh fast
PROFILE_294=release-candidate cargo test --release -p eggserve-server --no-default-features --features tower --test direct_tower_baseline_294 baseline_timing_matrix -- --ignored --nocapture
```

### Results

- New focused suite: 7 passed.
- Server tower lane: all suites ok (lib 94, fixtures 7+7, integration rest).
- Core tower lane: all suites ok (154 + 34 + 12 + 9 + 8 + 18 + 27 + 12).
- Conformance matrix: 51 entries + 55 app-server scenarios + 17 H3 scenarios validated.
- Topology gate: pass.
- `verify.sh fast`: 24/24 lanes green.
- A/B: 2 retained release-candidate trials in
  `benchmarks/295-direct-tower-optimization/raw/`; native control stable
  across baseline/candidate runs.

## 5. Invariant review

- Hyper-facing request validation executes exactly once under EggServe
  authority: unchanged (P2 only selects which body wrapper represents a
  Hyper-guaranteed-complete stream).
- Host/target/TE+CL/ceilings/rejection semantics: unchanged; full
  conformance green.
- Tower middleware cannot bypass framing/denylist/Date-Server/body
  limits/no-progress/lifecycle/shutdown: unchanged; P1 strips application
  framing before declaring (same code path).
- Streaming stays incremental: P1 buffers nothing (verified by
  hintless-stream test + code: declaration only).
- Trailers terminal-only and bounded: unchanged (F3 wire gap untouched,
  convergence test still passes).
- HEAD/body-forbidden never poll producer state: covered by new HEAD test;
  normalization path untouched.
- Errors sanitized; no second response after commitment: lying-length case
  fails closed by construction (declared-verification path shared with
  native known-length streams).

## 6. Failure and recovery review

- Service readiness failure / body producer error / panic containment:
  existing suites green (adapter error mapping untouched).
- Client disconnect + cancellation before/after first chunk: existing
  cancellation suites green; P2 only affects already-complete bodies which
  have no producer to cancel.
- Terminal trailers: invalid-trailer and suppression suites green.
- Server quiesce/drain with active streams: `verify.sh fast` TLS/shutdown
  lanes green.
- Concurrent slow streams: covered by existing suites; SSE parity holds.
- P2 worst case: if `is_end_stream` ever lied, unread bytes could be parsed
  as a next request — mitigated by sourcing the guarantee from Hyper's own
  transport state (not from headers), and by the unread-present-body test
  proving the close path still triggers for real bodies.

## 7. Migration and compatibility review

Source-compatible private changes only. No public type/feature/API change;
no semver consequence. Core compatibility Tower paths forward to the same
direct authority (core tower lanes green). Observable wire changes are
strict improvements within documented contracts: `Content-Length` added
where length is verified (clients may rely on *presence* only where the
server guarantees it — now true for buffered Tower bodies); keep-alive
reuse extended to bodyless Stream/Buffer requests (previously closed).

## 8. Security review

- No validation bypass: P1 strips application framing first; declared
  lengths are verified, mismatches fail closed (truncated close, sanitized).
- P2 cannot smuggle: only Hyper-guaranteed end-stream steady state takes
  the empty path; chunked/CL/upgrade bodies keep the wrapped path with
  abandon→close intact (proven by unread-body test).
- Confinement/safe defaults unchanged.

## 9. Documentation and operations

- `benchmarks/295-direct-tower-optimization/README.md` + `results.json` +
  `raw/` (2 candidate trials).
- Interop doc (`docs/http-interop.md`) behavior compatible (framing now
  exact for buffered bodies); no doc change required (no contract text
  promises chunked).
- 294 tripwire test updated to the P2 contract with pointer comment.
- Static guards unchanged (topology gate still authoritative).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | P1 declares whatever exact hint a body reports; pathological hints (e.g. `u64::MAX`) create a declared stream that fails on first byte | Fails closed (truncated close), same as native lying producers | None; documented native-equivalent behavior |
| low | Trailer rendezvous still `Arc<Mutex>` per streaming response | 2 allocs on a path measured at ~5% overhead | DEFER (recorded; revisit only with new evidence) |
| medium | F3 H1 response-trailer wire gap unchanged | Silent trailer-section loss persists | Separate scoped correctness plan (reaffirmed NO-GO here) |

## 11. Roadmap disposition

Milestone closed; retained P1/P2 production changes proceed to Milestone
297 qualification. No corrective pass required.

## 12. Registry updates

- `plans/registry.md`: 295 → closed; 297 still blocked on 296.
- `plans/subsystems/direct-h1-runtime-roadmap.md`: milestone 5 → closed
  with closure link.
- `plans/implementation/direct-h1-runtime/295-*.md`: status → closed.
