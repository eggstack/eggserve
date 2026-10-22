# Static Confinement Milestone 310 — Fuzz-harness repair and build gate

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/static-confinement-roadmap.md#milestone-3--fuzz-harness-repair-and-build-gate`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (no serving outside the configured root; confinement is structurally denied)
- `plans/001-terminology-and-domain-model.md` (`ConfinedPath`, `PathRejection`, `SecureRoot`)
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/path-confinement.md`
- `architecture/filesystem-confinement.md`
- `docs/fuzzing.md`
- `docs/threat-model.md`
- `release/plan-034-*` lineage (the original fuzz/corpus milestone)

Primary class: invariant

## 1. Objective

Restore the fuzz harness so it can actually run, and make its build part of
routine verification.

At the baseline, **four of the eleven fuzz targets do not compile**, and a
fifth is inert on every platform where it could run:

| Target | Build state | Cause |
|---|---|---|
| `request_target` | compiles | — |
| `percent_decode` | compiles | — |
| `path_components` | compiles | — |
| `if_none_match` | compiles | — |
| `platform_component` | compiles | — |
| `validate_method` | compiles | — |
| `range_header` | **8 errors** | `FileRange` fields are private; construction must go through `try_new`/`new` and access through accessors |
| `fuzz_header_block` | **1 error** | `HeaderValue::as_str()` → `to_str()` |
| `fuzz_normalize_response` | **1 error** | `HeaderValue::as_str()` → `to_str()` |
| `fuzz_request_body` | **1 error** | `Runtime::new` no longer exists |
| `fuzz_directory_buffer` | **inert / broken** | `#![cfg(windows)]` makes it a no-op on Linux, and on Windows it imports `eggserve_core::fs::windows`, a module deleted when filesystem ownership moved to `eggserve-static` |

Nothing detects this. The `fuzz/` directory is its own workspace with its
own `Cargo.lock`, so it is excluded from `cargo test --workspace`, and
routine CI never builds it. The `static-confinement` roadmap's verification
strategy claims "11 fuzz targets (corpus replay in `cargo test`)"
(`plans/subsystems/static-confinement-roadmap.md:109`) and Milestone 1's
exit conditions claim "fuzzer invariants hold" — a claim that is currently
false.

This milestone repairs the harness, adds a build gate so it cannot rot
again, and corrects the documentation that cites deleted source paths.

## 2. Why this milestone is ready

- The failures are ordinary API drift, fully enumerated above; each fix is
  a small call-site update.
- The corpus (`fuzz/corpus/`, 11 subdirectories) is intact and is already
  replayed deterministically by
  `crates/eggserve-core/tests/corpus_replay.rs`, so the invariants are
  covered by routine CI even though the libFuzzer targets are not.
- The `eggserve-static` crate owns the Windows directory-buffer parser
  (`crates/eggserve-static/src/fs/windows.rs:1523` `parse_directory_buffer`,
  `:1457` `DirBufParseError`, `:1448` `DirectoryEntryKind`), so the
  Windows target has a real owner to bind to.
- No hard dependency on other open milestones.

## 3. Current implementation evidence

### 3.1 The build failures

`cargo check --bins` in `fuzz/` at the baseline reports:

- `range_header`: `error[E0616]: field 'start' of struct
  eggserve_core::primitives::FileRange` is private` (and the same for
  `end_inclusive`) — 8 occurrences. This is the documented `FileRange`
  contract: private fields, constructed via `try_new`/`new`, read through
  accessors.
- `fuzz_header_block`: `error[E0599]: no method named 'as_str' found for
  struct 'eggserve_core::primitives::HeaderValue'` — the method is
  `to_str`.
- `fuzz_normalize_response`: the same `as_str` → `to_str` rename.
- `fuzz_request_body`: `error[E0599]: no associated function or constant
  named 'new' found for struct 'Runtime'`.

### 3.2 The Windows-only target

`fuzz/fuzz_targets/fuzz_directory_buffer.rs` opens with:

```rust
#![cfg(windows)]
```

so on Linux the target compiles to an empty harness. On Windows the file's
stale import is live:

```rust
use eggserve_core::fs::windows::{parse_directory_buffer, DirBufParseError};
use eggserve_core::fs::windows::DirectoryEntryKind;
```

`eggserve_core::fs` does not exist — `crates/eggserve-core/src/` contains
`config.rs`, `lib.rs`, `limits.rs`, `ops/`, `policy.rs`, `primitives/`,
`response.rs`, `runtime_limits.rs`, `server/`, and `tls.rs`. The `fs` module
was deleted by Plan 219 when filesystem ownership moved to `eggserve-static`.

The target's own comment says the stale import is "intentionally
untouched" because there is no public import to name yet. That leaves the
target broken on the only platform where it could run.

The target also contains a tautological assertion at line 34:

```rust
assert!(!entry.name.is_empty() || entry.name.is_empty());
```

which is always true and therefore asserts nothing.

### 3.3 The crate's dependency shape

`fuzz/Cargo.toml` declares both `eggserve-core` and `eggserve-static`.
The path-confinement targets (`path_components`, `percent_decode`,
`platform_component`) already drive `eggserve_static` directly, as the
crate comment intends ("Plan 219: path-confinement fuzz targets drive the
single static authority"), but the remaining targets go through
`eggserve_core::primitives`.

The crate is a separate workspace (`[workspace]` at the top of
`fuzz/Cargo.toml`, own `Cargo.lock`), which is why `cargo test --workspace`
does not cover it.

Its committed `fuzz/Cargo.lock` is **two release lines stale**: it pins
`eggserve-core 0.1.2` (`fuzz/Cargo.lock:48-49`) and
`eggserve-primitives 0.1.2` (`:80-81`), while the workspace is at
`eggserve-core 0.4.0` and `eggserve-primitives 0.2.2`. The stale lockfile
still records a `phf` edge on `eggserve-core` that the current crate no
longer has. Any `cargo` invocation inside `fuzz/` silently refreshes the
lockfile, which is how this drift stays invisible: the file is not
reviewed because nothing builds the crate.

### 3.4 What is still covered today

`crates/eggserve-core/tests/corpus_replay.rs` replays every committed
corpus input through the equivalent target logic, deterministically, in
routine CI. That test is the reason the *invariants* are still protected
even though the *targets* do not build. It is also, in effect, a second
implementation of the target logic that must be kept in step with the
targets.

### 3.5 Stale documentation

`docs/fuzzing.md` cites `crates/eggserve-core/src/path/decode.rs` and
`crates/eggserve-core/src/path/platform.rs`. Both were deleted with the
Plan 219 filesystem move; the code now lives in
`crates/eggserve-static/src/path/`.

## 4. Invariants that must not regress

- The fuzz harness must never weaken a confinement invariant. Every
  assertion must remain an assertion about the *production* behavior of
  the owning crate, not about the harness's own model.
- `eggserve-static` remains the sole static/path/filesystem authority. A
  fuzz target must not reimplement path validation; it must drive the
  authority and assert its outcomes.
- No target may assert a tautology. Every assertion must be able to fail.
- The committed corpus stays committed and stays deterministic. Corpus
  files are inputs, not expectations.
- The corpus replay in `crates/eggserve-core/tests/corpus_replay.rs` keeps
  running in routine CI and keeps covering the same invariants.
- The `fuzz/` crate stays excluded from the main workspace lockfile and
  dependency graph; it must not pull the Python crate or any new
  dependency into the supply chain.
- The unsafe-code policy is unchanged: no `unsafe` is added to the harness.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing.
- The `fuzz/` crate must not become a second implementation of the
  invariants it checks.

## 5. Scope

### In scope

- Repair the four non-compiling targets so that `cargo check --bins` and
  `cargo fuzz build` succeed for every target.
- Refresh the stale `fuzz/Cargo.lock` so it records the current crate
  versions and dependency edges.
- Rebind `fuzz_directory_buffer` to the Windows directory-buffer parser's
  real owner and make it meaningful on Windows; and either make it
  compile-and-run meaningfully on Unix too, or record clearly why the
  parser is Windows-only and move its coverage to a platform-appropriate
  place.
- Remove the tautological assertion.
- Add a routine-CI step that **builds** the fuzz crate (a `cargo check
  --bins` in `fuzz/` is sufficient and cheap; a full `cargo fuzz build`
  requires nightly and is not required for the gate).
- Add the fuzz build to `scripts/verify.sh fast` so a local run catches it
  too.
- Correct `docs/fuzzing.md`: the deleted source paths, the true target
  count and state, and the relationship between the libFuzzer targets and
  the CI corpus replay.
- Correct the `static-confinement` roadmap's verification-strategy claim
  so it matches reality after this milestone.

### Explicitly out of scope

- Changing the corpus contents to make a target pass. A corpus input that
  trips an assertion is a finding, not a nuisance.
- Changing production code to satisfy a fuzz target. If a repaired target
  reveals a production defect, that defect becomes a separate corrective
  plan; this milestone records it and does not fix it here.
- Adding new fuzz targets beyond restoring the existing eleven to a
  working state.
- Promoting the fuzz crate into the main workspace or into the supply
  chain.
- The Windows compile-coverage milestone (the `cli-ops-observability`
  subsystem owns that).
- The lint-policy milestone.
- Publication.

## 6. Required production changes

### Crates and ownership

Expected owners:

- `fuzz/`: all target repairs. This is the only crate whose source
  changes.
- `eggserve-static`: no production change expected. If the Windows
  directory-buffer parser must become reachable from the harness, expose
  it through an existing or new **public** API in `eggserve-static`
  (additive), rather than making the harness reimplement it.
- `eggserve-core`: no production change expected. The harness should
  prefer the leaf crates over the compatibility umbrella where the leaf
  crate owns the behavior.
- `eggserve-primitives`: no production change expected.
- `scripts/`, `.github/workflows/ci.yml`: the build gate.
- `docs/fuzzing.md`, `plans/subsystems/static-confinement-roadmap.md`:
  the corrections.

No new crate.

### Config and policy

None.

### Protocol and compatibility

None. The harness is a development tool.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and confinement

- The fuzz targets are the primary evidence for the confinement invariants.
  A repaired target must assert against the production authority, not a
  harness-local model.
- The Windows directory-buffer parser is a security-relevant parser
  (it parses untrusted directory data from the OS). Binding the harness to
  the real parser — rather than a copy — is the point of this milestone.

### Documentation and static guards

- `docs/fuzzing.md` — corrected paths, target inventory, and the
  target-versus-replay relationship.
- `plans/subsystems/static-confinement-roadmap.md` — the corrected
  verification-strategy claim and Milestone 3's status.
- `.github/workflows/ci.yml` and `scripts/verify.sh` — the build gate.
- `scripts/check-crate-topology.py` — a rule that the `fuzz/` crate's
  target set stays in the classified inventory (optional but
  recommended, so a target cannot be deleted without noticing).

## 7. Ordered work packages

### Work package A — Repair the four non-compiling targets

Fix each call site to the current API:

- `range_header`: construct `FileRange` through `try_new`/`new` and read
  through its accessors instead of the private fields.
- `fuzz_header_block` and `fuzz_normalize_response`: use `to_str()`.
- `fuzz_request_body`: use the current `Runtime` construction API.

Do not weaken an assertion to make it compile. If the current API makes an
assertion inexpressible, record the gap and raise it as a finding.

Acceptance evidence: `cargo check --bins` in `fuzz/` is clean.

### Work package A2 — Refresh the stale fuzz lockfile

Commit the refreshed `fuzz/Cargo.lock` so it records the current
`eggserve-core` / `eggserve-primitives` versions and the current
dependency edges, instead of the `0.1.2` entries it holds today.

Review the lockfile delta: it should be a version-and-edge refresh with no
new external dependency introduced into the fuzz crate's graph. If the
refresh pulls a new package into `fuzz/Cargo.lock`, that is a finding —
the fuzz crate's dependency surface must stay minimal.

Acceptance evidence: `fuzz/Cargo.lock` is committed with the current
versions, and the delta contains no new external package.

### Work package B — Rebind the Windows directory-buffer target

Replace the `eggserve_core::fs::windows` import with the real owner in
`eggserve-static`. Because that module is `#[cfg(windows)]`, decide and
record one of:

- **Make the parser reachable cross-platform.** Move the pure parsing
  logic (the `NtQueryDirectoryFile` buffer format is parseable off
  Windows) into a platform-independent module in `eggserve-static` so the
  target runs on every platform. This is the better option if it can be
  done without exposing Windows-only types.
- **Keep it Windows-only and make it honest.** Keep the
  `#[cfg(windows)]` gate, bind it to the `eggserve-static` Windows module
  through a public API, ensure it compiles on Windows, and record that its
  runtime coverage comes from the Windows qualification lane.

Option 1 is preferred. If it is not achievable without moving Windows
types, choose option 2 and state the reason.

Acceptance evidence: the target builds on Windows; under option 1 it also
builds and runs on Linux and is exercised by routine CI.

### Work package C — Remove the tautological assertion

Replace
`assert!(!entry.name.is_empty() || entry.name.is_empty());`
with a real property of the parsed entry, or delete it if the parser's
contract does not imply one. Every remaining assertion in the target must
be able to fail.

Acceptance evidence: no assertion in `fuzz/` is satisfiable by
construction; a reviewer can name the input that breaks each one.

### Work package D — Add the build gate

- Add a CI step that runs `cargo check --bins` (or `cargo build --bins`)
  in `fuzz/` and fails the job on error. Use the pinned toolchain the
  fuzz crate expects; if the fuzz crate needs nightly for `libfuzzer-sys`,
  use a stable `cargo check` where possible, or a dedicated nightly job
  with a narrow scope.
- Add the same check to `scripts/verify.sh fast` so a local run catches
  it.
- Keep the fuzz crate out of the main workspace and out of the main
  supply-chain lockfile.

Acceptance evidence: the CI step is present and fails when a target is
deliberately broken (demonstrate the failure, then revert).

### Work package E — Correct the documentation

- `docs/fuzzing.md`: replace the deleted source paths with the
  `eggserve-static` paths; state the real target inventory and that the
  targets are now built by CI; describe the relationship between the
  libFuzzer targets (unbounded exploration) and the corpus replay
  (deterministic CI regression).
- `plans/subsystems/static-confinement-roadmap.md`: correct the "11 fuzz
  targets" verification claim and Milestone 1's "fuzzer invariants hold"
  exit condition so they describe the restored state, and add Milestone 3.
- `plans/registry.md`: register the milestone.

Acceptance evidence: every source path named in `docs/fuzzing.md` exists.

### Work package F — Corpus replay alignment

Verify that `crates/eggserve-core/tests/corpus_replay.rs` still covers
every target's invariant after the repairs, and that the two have not
drifted. Where the repaired target asserts something the replay does not,
add the missing assertion to the replay (or record why the replay cannot
cover it).

Acceptance evidence: a target-by-target mapping from libFuzzer target to
corpus-replay coverage, recorded in the closure record.

### Work package G — Findings triage

Any production defect surfaced by a repaired target becomes a named finding
with a severity, and — if medium or higher — a separate corrective plan
reference. This milestone does not fix production defects.

Acceptance evidence: the findings table in the closure record, with
severity and disposition for each.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable to the harness itself. The harness must not gain
concurrency, async, or file-system side effects beyond what the targets
already do.

The corpus replay must remain deterministic: the same corpus input must
always produce the same result, with no time, randomness, or
environment-dependence.

## 9. Compatibility and migration

- No production behavior change, no public API change, and no semver
  event — unless the Windows parser must be exposed publicly from
  `eggserve-static` to be reachable by the harness. If so, that is an
  additive public API change on `eggserve-static` (patch per the 291
  precedent), and it must be recorded.
- The `fuzz/` crate is a development artifact and is not published.
- `docs/fuzzing.md` changes are documentation-only.

## 10. Required tests

### Focused unit tests

- The repaired targets' assertions are exercised by the corpus replay.
- The tautology check: a static check (or a reviewer-verified table)
  confirms no assertion in `fuzz/` is trivially true.

### Integration tests

- `cargo check --bins` in `fuzz/` succeeds for all eleven targets.
- The corpus replay test suite remains green.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- Each repaired target still asserts the confinement property it was
  written for: traversal, double-encoding, absolute-form, NUL, dotfile,
  reserved-name, and drive-prefix rejection.
- The Windows directory-buffer target asserts the parser's rejection
  behavior on malformed buffers, not only its happy path.

### Migration and compatibility tests

- The corpus files are unchanged (a `git diff` on `fuzz/corpus/` is empty),
  unless a corpus entry was added to capture a newly found defect, in which
  case the finding it captures is recorded.
- The refreshed `fuzz/Cargo.lock` introduces no new external package into
  the fuzz crate's dependency graph.

## 11. Required verification commands

```bash
# The gate this milestone adds
cd fuzz && cargo check --bins && cd ..

# Existing coverage that must stay green
cargo test -p eggserve-core --test corpus_replay
cargo test -p eggserve-core --test stateful_fuzz_replay
cargo test -p eggserve-static --lib
cargo clippy --workspace --lib --bins --tests -- -D warnings
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
./scripts/verify.sh fast
```

If the fuzz crate requires nightly for a full `cargo fuzz build`, record
that requirement and the nightly build result in the closure record; do
not claim a nightly build that was not run.

## 12. Documentation updates

- `docs/fuzzing.md` — corrected paths, target inventory, CI-build
  statement, and the target-versus-replay relationship.
- `plans/subsystems/static-confinement-roadmap.md` — corrected
  verification strategy, Milestone 3, and the milestone status table.
- `plans/registry.md` — the milestone row and the next-number line.
- `plans/closure/static-confinement/310-fuzz-harness-repair-and-build-gate.md`
  (new).

## 13. Acceptance criteria

- All eleven fuzz targets compile; `cargo check --bins` in `fuzz/` is
  clean.
- `fuzz/Cargo.lock` is committed with the current crate versions and
  introduces no new external package.
- `fuzz_directory_buffer` is bound to the real parser owner in
  `eggserve-static`, contains no tautological assertion, and builds on
  every platform it claims to support.
- Routine CI builds the fuzz crate, and the check demonstrably fails when
  a target is broken.
- `./scripts/verify.sh fast` includes the fuzz build check.
- Every source path named in `docs/fuzzing.md` exists.
- The `static-confinement` roadmap's verification claim matches reality.
- The corpus replay still covers every target's invariant, with a
  recorded target-by-target mapping.
- No production behavior change, no confinement invariant weakened, and no
  new dependency in the main workspace or lockfile.

## 14. Stop conditions

Stop and report rather than improvise if:

- repairing a target requires changing production code to make an
  assertion hold — that is a production finding, handled by a separate
  corrective plan;
- a target's assertion is inexpressible against the current public API, and
  exposing it would require a non-additive change to a published crate —
  report the gap instead;
- making the Windows parser cross-platform would require moving
  Windows-specific types into a shared module in a way that risks the
  confinement boundary — prefer option 2 in Work package B;
- a corpus entry must be deleted to make a target pass — that is a
  regression signal, not a cleanup;
- adding the fuzz build to routine CI would make the job materially slow —
  narrow the scope (build only, one target set) and record the decision
  rather than dropping the gate.

## 15. Closure evidence required

Create `plans/closure/static-confinement/310-fuzz-harness-repair-and-build-gate.md`
containing:

- the before-and-after build status for all eleven targets;
- the diff for each repaired call site and the reason for each;
- the Windows parser rebinding decision (option 1 or 2) with reasoning;
- the removed tautology and the assertion that replaced it;
- the CI and `verify.sh` diffs, plus the demonstration that the gate fails
  on a deliberately broken target;
- the target-by-target corpus-replay coverage mapping;
- the findings triage table with severities and dispositions;
- the documentation diffs;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The temptation is to "fix" a target by relaxing its assertion. That
inverts the purpose of a fuzz harness and can hide a real defect in the
confinement path. If a repaired target trips, triage it as a finding and
route it to a corrective plan; do not weaken the invariant here.
