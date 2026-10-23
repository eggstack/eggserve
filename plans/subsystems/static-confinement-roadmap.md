# Static Confinement Roadmap

Status: active (reopened for Milestone 3 — verification-harness integrity)

Long-term references:

- `plans/000-long-term-specification.md#2` (sole static/path/filesystem authority)
- `plans/001-terminology-and-domain-model.md` (`ConfinedPath`, `SecureRoot`, `StaticPolicy.symlinks`)
- `plans/002-long-term-roadmap.md` (phase 1 + authority convergence)

Related ADRs:

- `architecture/adr-002-windows-handle-relative-filesystem.md` (accepted; stays in place, linked not moved)

## 1. Purpose and ownership boundary

Owns all static/path/filesystem decisions: request-target parsing, `ConfinedPath` validation, `SecureRoot`/`PinnedRoot` confinement (Unix descriptor-relative `statat`+`openat`, Windows handle-relative), capability bridge, MIME selection, conditional/range/ETag planning, and `StaticService` request planning + rendering.

Consumes: canonical `primitives` types, `server` admission/connection context. Must not own: H1 wire runtime, TLS identity, QUIC transport.

Checked by `scripts/check-crate-topology.py` (`eggserve-static` sole authority; `src/fs`, `src/path`, `src/mime.rs` deleted from core; Plan 224 NO-GO: no capability-filesystem crate).

## 2. Work classification

### Invariants

- No serving outside the configured root under safe defaults.
- Dual `DotfilePolicy` types (parsing + serving) must agree for dotfiles to be served.
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`; directory listing stays opt-in.

### Capabilities

- Confined `GET`/`HEAD` file serving with correct `Content-Length`, MIME, `Last-Modified`/ETag, index vs listing behavior.
- Conditional/range planning with HEAD parity and body-forbidden normalization.

### Infrastructure

- Descriptor/handle-relative resolver, planner, MIME tables, `StaticService::canonical_response()` adapter.
- Authority conformance fixture (`crates/eggserve-core/tests/static_authority_conformance.rs`).

### Polish

- Listing-buffer behavior, HTML escaping/headers, allocation fixed-cost cleanup (Plans 229/236).

## 3. Non-goals

- No uploads/writes, routing, middleware, templating, auth framework, reverse proxying (`docs/non-goals.md`).
- No second resolver or pathname check-then-open fallback.
- No capability-filesystem crate (Plan 224 NO-GO, immutable).

## 4. Current state

Closed on capability. `eggserve-static` is the single implementation authority; `eggserve-core` keeps re-export facades only. Windows handle-relative confinement qualified functional-only (two NTFS open-descendant root-rename cases skipped); Unix descriptor-relative traversal is the safe-default path.

The roadmap is reopened solely because its **verification strategy claim is false at the baseline**. §9 states "11 fuzz targets (corpus replay in `cargo test`)", and Milestone 1's exit conditions state "fuzzer invariants hold". In fact, four of the eleven fuzz targets do not compile (`range_header`, `fuzz_header_block`, `fuzz_normalize_response`, `fuzz_request_body`), and `fuzz_directory_buffer` is inert on Linux and broken on Windows because it still imports the `eggserve_core::fs::windows` module that Plan 219 deleted. Nothing in routine CI builds the `fuzz/` crate — it is a separate workspace with its own lockfile — so the breakage is invisible. The *invariants* are still covered, because `crates/eggserve-core/tests/corpus_replay.rs` replays the committed corpus deterministically in routine CI; what is missing is the unbounded-exploration layer and the build gate.

Milestone 3 repairs the harness and adds the build gate. No confinement capability is reopened.

## 5. Target architecture

No change: preserve single authority, facade discipline, and the pure-planner → `canonical_response()` adapter shape.

## 6. Dependency graph

```text
path confinement + SecureRoot (hard, closed)
    |
    +--> planner/MIME/StaticService (hard, closed)
             |
             `--> authority convergence + facade closure (closed: 219/224/225/245)
```

## 7. Milestones

### Milestone 1 — Path confinement and SecureRoot

Class: invariant

Objective: deny traversal/symlink escape at library level under safe defaults.

Dependencies: none (foundation).

Deliverable boundary: 6-stage pipeline, 17 `PathRejection` variants, fuzz targets + corpora.

User or operator value: root escape is structurally denied, not policy-advised.

Exit conditions: traversal/double-encoding/absolute/NUL/dotfile/symlink regressions green; fuzzer invariants hold.

Deferred work: none.

### Milestone 2 — Static planning and service authority

Class: capability

Objective: single `StaticService` plans/renders all static responses; core keeps a wrapper.

Dependencies: Milestone 1 (hard).

Deliverable boundary: conditional/range/ETag planner, HEAD parity, `normalize_response` convergence.

User or operator value: correct, predictable static semantics over any transport.

Exit conditions: authority conformance fixture + H1 parity suite green; topology gate owns the boundary.

Deferred work: none.

### Milestone 3 — Fuzz-harness repair and build gate

Class: invariant

Objective: restore the fuzz harness so it builds and runs, and make its
build part of routine verification, so the confinement invariants have the
unbounded-exploration layer their exit conditions claim.

Dependencies: Milestones 1–2 (hard, closed); the `eggserve-static`
ownership of the Windows directory-buffer parser (hard, closed).

Deliverable boundary: all eleven targets compiling; the Windows
directory-buffer target bound to its real owner with no tautological
assertion; a routine-CI build gate for the `fuzz/` crate; corrected
`docs/fuzzing.md`; a target-by-target corpus-replay coverage mapping.

User or operator value: a change that breaks a confinement invariant is
caught by the fuzzer, not only by the deterministic corpus replay — and a
fuzz target that cannot compile can no longer hide behind the fact that
nobody builds it.

Exit conditions: `cargo check --bins` in `fuzz/` is clean; routine CI
builds the crate and demonstrably fails on a deliberately broken target;
the roadmap's verification-strategy claim matches reality.

Deferred work: none.

### Milestone 4 — Windows directory-buffer offset realignment

Class: invariant

Objective: make the NT `FILE_ID_BOTH_DIR_INFO` layout a single, mechanically
enforced authority, so the Windows directory-listing path decodes real kernel
buffers instead of rejecting them.

Dependencies: Milestone 3 (hard, closed) — repairing the harness made the
parser reachable cross-platform, and the corpus replay it enabled exposed the
defect.

Deliverable boundary: one authoritative layout with the fixed header at 104
bytes and the field offsets matching the NT record; the second, divergent
layout definition removed rather than reconciled; the zero entry budget
honored; the directory-buffer corpus regenerated from a recorded generator so
the happy path is covered; a per-seed expectation in the cross-platform
replay.

User or operator value: a Windows directory listing decodes real kernel
output. Before this milestone the committed corpus rejected six of its nine
seeds as malformed and decoded the rest to empty names, so the listing path
was untrustworthy and its failure mode was silent.

Exit conditions: the layout is stated once and derived from one place; the
committed corpus is byte-reproducible and contains valid seeds that decode to
their expected entries; the replay asserts per-seed expectations rather than
merely "no panic"; the zero budget returns no entries.

Deferred work: the Windows adversarial qualification suite is a manual lane
and is not re-run by this milestone.


### Milestone 5 — Windows runtime qualification and corpus seed hygiene

Class: infrastructure

Objective: make the Windows confinement path a subject of routine, automatic
qualification rather than a manual one, and make the fuzz corpus a live,
byte-stable artifact on every platform.

Dependencies: Milestone 4 (hard, closed) — the corrected NT layout and the
recorded corpus generator are what the new Windows lanes exercise.

Deliverable boundary: a hosted Windows runner executes the Windows-only unit
tests and the corpus byte-identity check in routine CI; the NT information
class the enumeration path depends on is pinned against a live kernel; the
fuzz corpus is checked out byte-identically everywhere; the corpora whose
targets consume a seed as an exact value hold to that rule, enforced by a
gate rule and by the cross-platform replay.

User or operator value: a regression in the Windows-only filesystem code now
fails a pull request instead of waiting for someone with a Windows VM. The
fuzz corpus stops being inert: before this milestone every `path_components`
seed carried a trailing newline, so every one of them was a
whitespace-containing request target that the parser rejected before any
confinement logic ran, and the corpus exercised nothing.

Exit conditions: the `platform-compile` job's Windows leg runs the static
crate's tests; the qualification lane's Windows job is green; the NT
information class is asserted against a live kernel; the corpus is
byte-identical across platforms and non-vacuous on every platform.

Deferred work: the Windows adversarial filesystem suites remain a manual lane
because their fixtures need Developer Mode and elevated privileges that a
hosted runner does not reliably provide.



Storage: read-only serving; no migration. Protocol: origin-form only; absolute-form rejected by static (Plan 278 seam stays H1-embedding-only). Security: `docs/threat-model.md` layers 1–3. Concurrency: file-stream semaphore + bounded chunk reads (128 KiB default, Plans 232–233). Observability: `OpsContext` events, sanitized paths. Perf: per-trial retained evidence, never single best numbers.

## 9. Verification strategy

Rust unit/integration suites + `conformance_matrix.toml` replay + 11 fuzz targets (corpus replay in `cargo test`) + topology gate + a hosted Windows test lane in routine CI (Milestone 5) + the Windows adversarial suites (manual qualification lane).

The 11-target claim is restored by Milestone 3. Until that milestone closes, the accurate statement is: **6 of 11 fuzz targets compile; the corpus replay in `crates/eggserve-core/tests/corpus_replay.rs` covers the invariants deterministically in routine CI; the `fuzz/` crate is not built by CI.** Milestone 3 restores the harness and adds the build gate.

Milestone 5 adds the last piece the confinement path was missing: the `#[cfg(windows)]`
implementation is now *run* on a hosted Windows runner in routine CI, not merely compiled there.
The adversarial filesystem suites stay a manual lane because their fixtures need Developer Mode
and elevated privileges.

## 10. Risks and decision points

None open on the confinement boundary. Any future confinement-boundary change requires an ADR.

Milestone 3 risk: repairing a fuzz target may reveal a production defect in the confinement path. Such a defect must be routed to a separate corrective plan, not absorbed into the harness repair, and must never be "fixed" by weakening an assertion.

Milestone 5 risk: a hosted Windows runner is not a developer Windows VM. It has no Developer Mode
and no elevated privileges, so it cannot stand in for the adversarial filesystem suites; it can
and does stand in for the deterministic Windows unit tests.

## 11. Completion definition

Authority collapse proven by fixture + gate, facades classified, no second implementation, evidence in `release/plan-225-compatibility-facade-closure.md` and `release/plan-248-maintainability-convergence-closure.md`. Met for capability. The roadmap returns to closed status when Milestone 3 restores the fuzz harness and its build gate, and its verification-strategy claim is true. Milestone 5 closes the last gap in that claim: the confinement path's Windows implementation is now verified by execution, not by compilation alone.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | legacy `plans/002-*`, `007-*`, `017-*`, `062-*`–`065-*` | `release/plan-034-*` lineage; `plan-225-*` facade proof | — |
| 2 | closed | legacy `plans/081-*`, `219-*`, `245-*` | `release/plan-225-compatibility-facade-closure.md` | — |
| 3 | closed | `plans/implementation/static-confinement/310-fuzz-harness-repair-and-build-gate.md` | `plans/closure/static-confinement/310-fuzz-harness-repair-and-build-gate.md` | All 11 fuzz targets compile, run, and are linted/format-checked in routine CI; the NT directory-buffer parser is public and platform neutral; the corpus replay's vacuous corpus path is fixed. Gate rule `check_plan310_fuzz_target_inventory`. Two parser defects (F1 zero budget, F2 offsets) are deferred to Milestone 4. |
| 4 | closed | `plans/implementation/static-confinement/316-windows-directory-buffer-offset-realignment.md` | `plans/closure/static-confinement/316-windows-directory-buffer-offset-realignment.md` | The NT `FILE_ID_BOTH_DIR_INFO` layout is one authority. Work package A established that the *parser* was correct and the FFI struct was wrong, so the recorded deviation is that the struct and its doc comment changed: the hand-rolled struct was deleted rather than realigned, `get_file_id` now reads the stable `FileId` at the shared offset (it previously read the low half of `CreationTime`) with a correctly sized buffer, the zero entry budget is honored, and the corpus was regenerated from the authoritative layout with a recorded generator so the happy path is covered. Gate rule `check_plan316_directory_buffer_layout`. The Windows runtime qualification suite remains a manual lane. |
| 5 | closed | `plans/implementation/static-confinement/317-windows-runtime-qualification-and-corpus-seed-hygiene.md` | `plans/closure/static-confinement/317-windows-runtime-qualification-and-corpus-seed-hygiene.md` | The Windows confinement path is qualified by hosted CI, not by a manual lane: the `platform-compile` job's Windows leg runs the static crate's tests, so the Windows-only unit tests and the corpus byte-identity check actually run. The NT information class the enumeration path depends on (37, the `ntifs.h` numbering, which the user-mode `winnt.h` numbering contradicts at 16) is pinned against a live kernel. The fuzz corpus is byte-stable across platforms (`-text` in `.gitattributes`) and live: the `path_components` and `platform_component` seeds are the exact bytes their targets consume, and the cross-platform replay asserts non-vacuity. Gate rule `check_plan317_corpus_seed_hygiene`. The Windows adversarial filesystem suites stay a manual lane. |
