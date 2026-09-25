# Direct H1 Runtime Milestone 297 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/297-direct-application-server-qualification-closure.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-7--downstream-like-qualification-and-closure`

Repository baseline reviewed: `f2d581e0ce1fb18540f63175c319242162c48b6d`
plus the 294/295/296 campaign work qualified herein

Implementation commits or pull requests:

- (this campaign commit set) — 294 fixtures/evidence, 295 P1/P2 + tests,
  296 WP-A + NO-GO evidence, 297 fixture/matrix/evidence, doc notes

## 1. Executive finding

The direct-application-server optimization campaign is complete and
closed. Every retained change (295 P1 known-length Tower responses, 295 P2
provably-empty request completion, 296 WP-A `tower-layer` deactivation) is
qualified against native H1, Tower/Axum, and an EggPool-shaped streaming
consumer with no correctness, security, compatibility, or resource
regression. All 295/296 NO-GO/DEFER dispositions stand. No publication in
this campaign; the release decision (minor-level, deferred) is recorded
with rationale. No new production optimization was discovered or added
during closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Same-machine A/B, buffered + SSE-like | 294 interleaved matrix + 297 extended matrix (c16, SSE-128, POST/JSON) | pass | Raw trials retained; relative deltas only where client-bound |
| Tail latency, CPU, allocations, RSS, errors, recovery | p50/p95/p99 per case; RSS HWM markers (3904→5596→5596 stable); slow-10 + cancellation recovery | pass | No allocator counters on host (accepted); errors/timeouts zero across trials |
| Direct-profile dependency + stripped-binary deltas | `297/raw/footprint.txt` (936784/951672/1038264 B; 182 lock pkgs; 45-pkg registry consumer) | pass | All size deltas linker noise |
| Every 295/296 candidate KEEP/REVERT/DEFER/NO-GO | P1 KEEP, P2 KEEP, WP-A KEEP, validate-split NO-GO, rendezvous DEFER, file/tunnel NO-GO | pass | §11 of 294/295/296 closures |
| Correctness/security/topology for retained changes | `verify.sh full` 31/31; conformance 51+55+17; supply-chain clean; topology + self-tests | pass | One environmental incident, remediated (see §4) |
| Native H1 / Tower/Axum convergence | parity suites + app-geometry fixture (5 tests) | pass | Includes Content-Length + keep-alive + admission + drain |
| Exact semver/publication decision | Minor-level (0.4.0) when cut, driven solely by WP-A; publication DEFERRED | pass | No registry proof (nothing published) |
| No new optimization during closure | Production diff frozen at 295/296 content | pass | Only tests/evidence/docs added in 297 |

## 3. Production implementation evidence

297 adds no production changes by design. Campaign production delta
(qualified here, implemented under 295/296):

- `src/interop.rs` P1 (+12/−1), `src/connection/pipeline.rs` P2 (+21/−1),
  `Cargo.toml` WP-A (feature edge + dev-dep), `src/tower.rs` doc reword.
- No public API, config, default, dependency, or ownership change beyond
  the classified WP-A feature edge.

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-server --no-default-features --features tower --test application_server_qualification_297
cargo test -p eggserve-server --no-default-features --features tower --test direct_tower_baseline_294
cargo test -p eggserve-server --no-default-features --features tower --test direct_tower_hotpath_295
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-server --no-default-features --features tower
cargo test -p eggserve-core --no-default-features --features tower
./scripts/verify.sh fast
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
./scripts/verify.sh full
PROFILE_297=release-qual cargo test --release -p eggserve-server --no-default-features --features tower --test application_server_qualification_297 extended_timing_matrix -- --ignored --nocapture
```

### Results

- App-geometry fixture: 5 passed. 294 fixture: 7 passed + 1 ignored
  (timing by design). 295 focused: 7 passed.
- Extended matrix: 2 retained trials (`raw/timing-qual-trial{1,2}.txt`):
  c16 native 922 vs tower 857 rps; SSE-128 4793 vs 4284; POST/JSON 10382
  vs 9086; slow-10 all complete with clean drain; RSS HWM stable.
- Footprint: all deltas noise; counts recorded.
- `verify.sh fast`: 24/24. `verify.sh full`: 31/31 (see incident note).
- Supply-chain: advisories/bans/licenses/sources ok, both lockfiles.
- Package dry-run: pass with registry-consumer shape proof.

**Environmental incident (honestly recorded):** the first `verify.sh
full` run failed at `cargo test --workspace` with a linker bus error
(`ld terminated with signal 7`). Root cause: filesystem 100% full
(workspace `target/debug` 246 G incl. 119 G incremental; host disk
previously near capacity). No code defect. Remediation: removed
regenerable artifacts only (own throwaway fixture targets + `target/debug/incremental`),
freeing 118 G; re-linked the failing target green, then re-ran the full
suite green from scratch (EXIT=0, 31 ✓). Lesson: run `df` before
publication-grade verification on shared hosts.

## 5. Invariant review

- All direct-H1 roadmap invariants hold with 295/296 landed (single H1
  authority; structured shutdown; runtime-owned framing/denylist;
  one-shot bounded bodies; explicit `OpsContext`; no library printing):
  proven by the full conformance + topology + focused suites.
- Native `Service` and direct Tower converge at the transport-policy
  boundary (parity + app-geometry + downstream-consumer suites).
- No feature/profile weakens framing/body ceilings/timeout/shutdown
  (P1 verifies lengths; P2 closes on any unread bytes; WP-A removes only
  an unused activation edge).
- Core compatibility remains forwarding/composition (core tower lanes +
  facade topology assertions green).
- No win purchased with buffering or detached tasks (P1 buffers nothing;
  P2 completes only Hyper-guaranteed-empty bodies).

## 6. Failure and recovery review

- Admission saturation/recovery, slow/stalled consumers, producer error,
  client disconnect, shutdown-while-streaming, repeated start/stop cycles:
  covered by existing suites (all green) plus the new app-geometry tests
  (over-limit 413, mid-stream abandonment + recovery, controlled drain,
  10 slow streams + shutdown).
- No resource count grows across cycles (RSS HWM flat across phases).
- Fuzz/race/proxy-interop (`verify.sh deep`) intentionally not run: manual
  expensive suites, unchanged attack surface, no framing/parser edits in
  the campaign (P2 touches lifecycle selection, covered by conformance).

## 7. Migration and compatibility review

- Source/feature/runtime compatibility: P1/P2 transparent improvements;
  WP-A one-line manifest recipe for transitive-`tower-layer` consumers
  (`docs/http-interop.md`). No exhaustive-literal, enum, or config change.
- Semver: next `eggserve-server` release is MINOR (0.4.0) when cut —
  driven solely by the WP-A feature edge on the pre-1.0 line. No version
  bumped here.
- Publication: DEFERRED. Rationale: polish-only campaign with no
  downstream-blocking need; publishing stays manual maintainer action and
  no push/tag/merge publishes. Registry-only consumer proof is therefore
  N/A; the consumer SHAPE is proven via local-registry package dry-run.

## 8. Security review

- P1: application framing stripped before declaration; mismatches fail
  closed. P2: close preserved for any unread bytes (proven by test).
  WP-A: graph narrowed by one unused node.
- Confinement, safe defaults, path validation, privilege boundaries,
  denial-of-service bounds, redaction, audit behavior: unchanged and
  covered by the full qualification suites (threat-model docs untouched).

## 9. Documentation and operations

- `benchmarks/297-application-server-qualification/` (README,
  results.json, raw freeze/footprint/timing trials).
- `benchmarks/README.md` index row.
- `architecture/runtime.md`: provably-empty completion note.
- `docs/http-interop.md`: known-length declaration note (P1) + Layer
  composition recipe (WP-A).
- `docs/dependency-policy.md`: tower-layer activation status (WP-A).
- Source roadmap + registry updated (see §12).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | F3 H1 response-trailer wire gap (from 294) | Silent trailer-section loss on all H1 paths | Separate scoped correctness plan (reaffirmed out of campaign) |
| low | ~35 KB file-body / ~30 KB tunnel code in narrowest binaries | 3%-order link overhead | Accepted; cheaper seam only |
| low | No allocator/CPU-profile tooling on host | Mechanical accounting + latency A/B only | Accepted per Plan 240 precedent |
| low | Shared-host disk filled mid-campaign (linker bus error) | One full-verify rerun needed | Remediated; operational note for future publication runs |

## 11. Roadmap disposition

Campaign complete: milestones 4–7 closed. The reopened polish campaign
closes with 294 baseline + retained 295/296 changes qualified and no
unresolved correctness/security regression. Subsystem returns to
closed-capability posture; future H1-boundary widening needs a new scoped
plan.

## 12. Registry updates

- `plans/registry.md`: 297 → closed; subsystem current milestone →
  campaign complete (capability closed through 0.3.1 + polish 294–297).
- `plans/subsystems/direct-h1-runtime-roadmap.md`: milestone 7 → closed
  with closure link; campaign completion noted.
- `plans/implementation/direct-h1-runtime/297-*.md`: status → closed.
