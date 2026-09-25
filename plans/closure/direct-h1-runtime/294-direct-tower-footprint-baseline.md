# Direct H1 Runtime Milestone 294 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/294-direct-tower-footprint-baseline.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-4--direct-tower--footprint-baseline`

Repository baseline reviewed: `f2d581e0ce1fb18540f63175c319242162c48b6d`
(planning baseline `81605fc36b970440d6e3edbfd1e0d1cfa3ec4d91` + this
milestone's evidence work)

Implementation commits or pull requests:

- (this closure commit) — 294 fixtures, benchmark evidence, and closure record

## 1. Executive finding

The reproducible current-HEAD baseline for the direct
`eggserve-server --no-default-features --features tower` application-server
profile is established with no production changes. Native and direct
Tower/Axum fixtures are behaviorally comparable; allocation/CPU/latency
evidence identifies a bounded small-response adaptation cost (~13µs p50,
interleaved-round stable) and separately characterizes two structural costs
that dominate it (F1 always-chunked Tower responses; F2 Stream-policy
keep-alive close) plus one out-of-scope correctness gap (F3 H1 response
trailers). Direct-profile graph and stripped sizes are recorded. Milestones
295 and 296 are unblocked **only** for the evidence-gated candidates named in
§11; everything else is explicit NO-GO/DEFER.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Identical native/Tower/Axum fixtures | `crates/eggserve-server/tests/direct_tower_baseline_294.rs` (7 CI tests) | pass | 1 KiB, header sweep, SSE-32, POST echo, trailers, connection behavior, cancellation |
| Allocation/CPU/latency: GET→1 KiB | `benchmarks/294-direct-tower-baseline/raw/timing-release-trial{1,2}.txt` | pass | 3 interleaved rounds; native ~13.7k rps p50 0.069ms vs tower ~11.8k p50 0.083ms |
| POST small body → small response | parity test + Stream-policy record test | pass | Correctness parity; F2 close behavior pinned |
| 1/16/64 header fields | header sweep timing + parity | pass | Tower 11.7k/11.0k/9.2k rps; native/tower observe identical header sets |
| 1 MiB known-length stream | streaming timing cases | pass | No tower penalty (bytes dominate): native 2.86k, axum 2.82k, tower-full 2.9–3.0k |
| SSE-like unknown-length (32 chunks) | SSE timing + parity | pass | Native 8.8k vs tower 8.3k streams/s (~5%) |
| No-trailer vs terminal-trailer | convergence test | pass | Both deliver body; wire trailers dropped on both (F3) |
| Dependency/feature ancestry | `raw/tree-*-no-dev.txt`, `raw/tree-features-*.txt` | pass | Tower delta is exactly `tower-layer` + `tower-service` |
| Stripped binary sizes | `raw/sizes-dist.txt` (936608 / 951480 / 1037768 B) | pass | Identical 1 KiB fixtures, dist profile, all executed once |
| Tokio/futures/Tower activation | feature tree | pass | `macros net time io-util fs sync rt`; `fs` unconditional |
| File-body/tunnel reachability | source: no cfg gates; RuntimeState owns file semaphore | pass | Link impact unproven (LTO) → 296 experiment, not assumed |
| Cancellation/client-disconnect/shutdown/admission | cancellation test + existing suites via `verify.sh fast` | pass | Streaming cancel recovers; no harness saturation misread (interleaved rounds) |
| No production behavior/API change | `git status` production tree untouched | pass | Only tests/benchmarks/plans changed |
| No performance claim without profile + raw evidence | `raw/environment.txt`, per-sample `profile:release` | pass | Single-connection loopback latency; never quoted as capacity |

## 3. Production implementation evidence

None — evidence milestone by design. Production tree (`crates/*/src`) is
byte-identical to the planning baseline. New files are test-only fixtures
(`crates/eggserve-server/tests/direct_tower_baseline_294.rs`), benchmark
evidence (`benchmarks/294-direct-tower-baseline/`), and one index row in
`benchmarks/README.md`.

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-server --no-default-features --features tower --test direct_tower_baseline_294
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-server --no-default-features --features tower
cargo test -p eggserve-core --no-default-features --features tower
python3 scripts/check-crate-topology.py
./scripts/verify.sh fast
PROFILE_294=release cargo test --release -p eggserve-server --no-default-features --features tower --test direct_tower_baseline_294 baseline_timing_matrix -- --ignored --nocapture
```

### Results

- Fixture suite: 7 passed, 1 ignored (timing matrix, by design).
- Tower clippy/tests lanes: clean (2 clippy findings fixed: dead field, useless format).
- Topology gate: pass (direct Tower graph free of core/static/PHF confirmed).
- `./scripts/verify.sh fast`: 24/24 lanes green, no failures.
- Release timing matrix: 2 retained interleaved trials + 1 pre-interleave run
  + 1 `/usr/bin/time -v` run (peak harness RSS 8508 KiB), all in `raw/`.
  One trial shows a background-load dip (tower-r1 9823); reported, not discarded.

## 5. Invariant review

- One H1/parser/validation/framing/policy/timeout/admission/cancellation/
  shutdown authority: untouched (no production edits); parity tests prove
  native/Tower convergence at the transport-policy boundary.
- Direct Tower graph free of core/static/PHF: proven by no-dev tree diff
  (delta is exactly the two tower crates) + topology gate.
- One-shot bounded bodies, runtime-owned framing, HEAD/body-forbidden and
  trailer rules: preserved; trailer test pins convergence including the F3
  boundary behavior.
- No performance claim without workload/profile/evidence: every number in
  `results.json`/README names the release loopback harness and raw file.

## 6. Failure and recovery review

- In-flight stream cancellation, client disconnect, and server shutdown with
  active streams: covered by `baseline_294_streaming_cancellation_recovers`
  (drop mid-stream → producer released → fresh connection serves).
- Admission recovery: unchanged code paths, covered by existing suites in
  `verify.sh fast`.
- Harness failure vs server regression: interleaved rounds + warm-up separate
  order/cold-start bias from adapter cost; a background-load dip in trial2
  is recorded rather than interpreted.
- Malformed framing/security inputs: correctness controls only, per plan
  (existing conformance suites green).

## 7. Migration and compatibility review

None: no production, API, feature, or config change. Fixture
`RuntimeConfig` uses an explicit 1 MiB body ceiling because the runtime
default is 0 (reject); documented in the fixture, not a product change.

## 8. Security review

No security surface changed. Confinement/safe-default posture untouched
(`docs/threat-model.md` unaffected). F2/F3 are availability/correctness
observations with no confinement impact; F2's any future change keeps the
framing-safety stop condition (unread wire bytes must still force close).

## 9. Documentation and operations

- `benchmarks/294-direct-tower-baseline/README.md` (method, reproduction,
  tables, decision record), `results.json` (machine-readable evidence +
  candidate dispositions), `raw/` (environment, trees, sizes, timing trials).
- `benchmarks/README.md` evidence-index row added.
- No normative API docs changed (no documentation defect found).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | F1: Tower responses always chunked even when exact length known | Extra framing bytes; no Content-Length for Tower consumers | 295 P1 (evidence-gated) |
| medium | F2: Stream-policy unconsumed (empty) body forces `connection: close` | Default Tower shape cannot reuse keep-alive; connection churn dominates adapter cost | 295 P2 with framing-safety stops; else corrective plan |
| medium | F3: H1 response trailers never reach wire (missing `Trailer` head declaration; hyper requires it, EggServe strips + never synthesizes) | Silent trailer-section loss on all H1 paths | Separate scoped correctness plan; NOT 295/296 (NO-GO here) |
| low | Alloc/CPU profiling without allocator counters or flamegraphs | Mechanical accounting only | Accepted per Plan 240 precedent; 295/297 A/B uses same-machine latency deltas |

## 11. Roadmap disposition

Milestone closed and next dependencies may proceed **with bounds**:

- **295 UNBLOCKED** for exactly: P1 known-length Tower response fast path
  (honor exact `size_hint`; framing stays runtime-owned) and P2
  unconsumed-empty-body keep-alive close (complete-but-unpolled bodies only;
  any unread wire bytes keep current close; lifecycle redesign → stop and
  DEFER). Validate/project split, unsafe aliasing, second framing
  implementation, and broad Service redesign remain NO-GO.
- **296 UNBLOCKED** for: (a) `tower-layer` removal from the direct `tower`
  feature (mechanically unused by production code) with dev-dependency
  retention for tests; (b) file-body/Tokio-`fs` and tunnel gating as
  EXPERIMENTS with KEEP only on measured link/graph benefit, semver
  classified before landing, no new crate.
- **297 remains BLOCKED** until retained 295/296 changes (or explicit NO-GO
  dispositions) close.

## 12. Registry updates

- `plans/registry.md`: 294 → closed; 295 → ready (bounded to P1/P2);
  296 → ready (bounded to graph pruning + gated experiments);
  297 → blocked (unchanged, awaiting 295/296).
- `plans/subsystems/direct-h1-runtime-roadmap.md`: milestone 4 → closed
  with closure link; milestones 5/6 → ready with evidence bounds.
- `plans/implementation/direct-h1-runtime/294-*.md`: status → closed
  (implementation record retained; closure is this file).
