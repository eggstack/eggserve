# Plan 317 — Windows runtime qualification and fuzz-corpus seed hygiene

## 1. Status

- **State**: active — implementation in progress
- **Plan number**: 317
- **Subsystem**: static confinement (`plans/subsystems/static-confinement-roadmap.md`, Milestone 5)
- **Source roadmap**: `plans/subsystems/static-confinement-roadmap.md`
- **Primary class**: corrective (Windows runtime qualification) + infrastructure (CI lanes, corpus hygiene)

## 2. Repository baseline

At the start of this plan, the repository is at `ce457ae` ("Plans 313, 315, and
316: closure records, registry, and roadmap updates"), with Plans 313, 315, and
316 closed and their closure records merged. Hosted CI run `39907003032` was
green on that commit across all five jobs.

The Windows filesystem implementation (`crates/eggserve-static/src/fs/windows.rs`)
is `#[cfg(windows)]`, so **no test in the repository had ever run on
Windows**. The Windows-only unit tests were compiled at most. The Windows
adversarial filesystem suites (`crates/eggserve-core/tests/windows_plan084.rs`,
`windows_plan086.rs`) are gated behind `#![cfg(all(windows, feature =
"windows-adversarial-qualification"))]` and were reachable only through the
manual `platform-qualification.yml` lane.

Two `#[ignore]`d unit tests in `crates/eggserve-static/src/fs/windows.rs`
(`enumerate_directory_entries`, `enumerate_directory_subdir`) carried the
recorded reason `"NtQueryDirectoryFile STATUS_INFO_LENGTH_MISMATCH on CI
runners; needs Windows VM qualification"`. They cover
`enumerate_directory`, which is the **production** Windows directory-listing
path (`windows.rs:1134`, reached from `list_directory_handle`).

## 3. Problem statement

The corrective program had closed Plan 316 with the Windows runtime
qualification explicitly deferred: *"The Windows runtime qualification suite
is a manual lane and is not re-run by this milestone."* That deferral left
three defects unexamined:

1. **The Windows runtime qualification had never been executed on a hosted
   runner.** The premise that Windows qualification required "a Windows VM"
   was never tested. GitHub Actions provides a `windows-latest` runner that
   is a real Windows host.

2. **The `path_components` fuzz corpus was inert.** Every seed carried a
   trailing newline, so every seed was a whitespace-containing request
   target. The canonical request-target parser rejects those before any
   path-confinement logic runs, so the fuzz target's assertions were
   unreachable and the corpus exercised nothing.

3. **The fuzz corpus was not byte-stable across platforms.** Git's
   end-of-line conversion rewrote a text seed's trailing LF as CRLF on a
   Windows checkout, so the same seed was a different input on different
   platforms. A fuzz corpus whose seeds differ per platform does not satisfy
   the reproducibility the harness assumes.

## 4. Goal

Make the Windows confinement path a routine, automatically qualified subject
rather than a manual one, and make the fuzz corpus a live, byte-stable
artifact on every platform.

## 5. Non-goals

- Not a replacement for the Windows adversarial filesystem suites. Those
  remain the manual `platform-qualification.yml` lane and remain gated on
  the `windows-adversarial-qualification` feature, because their fixtures
  require Developer Mode and elevated privileges that a hosted runner does
  not reliably provide.
- Not a change to any confinement decision, path policy, or request-target
  grammar. The defects found are in the *evidence* about that code and in
  the *corpus*, not in the confinement logic itself.
- Not a new fuzz target, and not an expansion of the fuzz input space.
- Not a change to the `fuzz/` crate's dependency set or lockfile.

## 6. Scope

### Work package A — Execute the Windows qualification on a hosted runner

Dispatch `platform-qualification.yml` against the branch and read the
Windows job's log. Record the actual failure, not the recorded ignore
reason, which is a hypothesis.

### Work package B — Make the fuzz corpus live and byte-stable

1. Mark `fuzz/corpus/**` as `-text` in `.gitattributes` so a seed is checked
   out byte-identically on every platform.
2. Rewrite the seeds whose target consumes the seed as an *exact value*
   (`path_components` → a request target, `platform_component` → a component
   name) so the seed contains exactly that value and nothing else.
3. Add accepted request targets to `path_components` so the corpus covers the
   happy path, not only the rejection path.
4. Extend `crates/eggserve-core/tests/corpus_replay.rs` to assert the
   no-trailing-terminator rule and to require non-vacuity: at least one
   accepted request target, and at least one reserved device name.

### Work package C — Make the Windows lane routine

Add a Windows test lane to the `platform-compile` job of `.github/workflows/ci.yml`
that runs the static crate's tests, so the Windows-only unit tests and the
corpus byte-identity check *run* rather than merely compile. Mirror the lane
in `scripts/verify.sh`.

### Work package D — Correctness fixes surfaced by the review of Plan 316

- Make the NT directory-buffer parser's file-name range arithmetic checked
  rather than an unchecked add.
- Remove the two fabricated `0xBAADF00D` sentinel error codes; carry the real
  parse error instead.
- Correct the `SAFETY:` comment on the `NtQueryDirectoryFile` call, which
  claimed an alignment guarantee and a re-zeroing guarantee the code does not
  provide.
- Name the two NT information classes separately and cite their source.

### Work package E — Governance

Registry entry, roadmap milestone, closure record, and documentation updates
(`docs/fuzzing.md`, `fuzz/README.md`, `docs/migration-guide.md`).

## 7. Invariants that must not regress

1. **The confinement decision surface is unchanged.** No path policy, request
   target grammar, or filesystem decision changes in this plan.
2. **`parse_directory_buffer` keeps its documented contract**: it rejects
   malformed kernel output rather than indexing unchecked, and it honors a
   zero entry budget by returning no entries.
3. **The corpus stays reproducible.** A committed seed must be byte-identical
   to what the generator produces and to what every other platform checks out.
4. **The corpus stays live.** At least one `path_components` seed must be an
   accepted request target and at least one `platform_component` seed must be
   a reserved device name, checked in routine CI on every platform.
5. **The Windows path is qualified automatically.** A regression in the
   Windows-only code must fail a hosted CI run, not wait for a manual lane.

## 8. Recorded material deviation

The plan's §1 baseline records the `#[ignore]` reason as
`"NtQueryDirectoryFile STATUS_INFO_LENGTH_MISMATCH on CI runners; needs
Windows VM qualification"`. Work package A treats that string as a
**hypothesis, not a finding**, and the recorded outcome is whatever the
hosted Windows runner actually reports. If the enumeration path turns out to
work on a hosted runner, the two `#[ignore]`d tests are un-ignored and the
recorded reason is retired; if it does not, the recorded reason is replaced
with the real cause and the defect is routed to its own corrective plan
rather than absorbed here, per `plans/subsystems/static-confinement-roadmap.md`
§10 ("a defect must be routed to a separate corrective plan, not absorbed
into the harness repair").

## 9. Verification strategy

- `python3 scripts/check-crate-topology.py` and `--self-test` (the new gate
  rule `check_plan317_corpus_seed_hygiene` and its mutations).
- `cargo test -p eggserve-core --test corpus_replay` (the non-vacuity
  assertions).
- `cargo test -p eggserve-static --lib --tests` and
  `cargo test -p eggserve-static --test directory_buffer_corpus`.
- `cargo clippy -p eggserve-static --lib --tests --target x86_64-pc-windows-msvc -- -D warnings`.
- `cargo test --workspace`, `cargo fmt --all -- --check`,
  `cargo clippy --workspace --lib --bins --tests -- -D warnings`.
- **Hosted Windows evidence**: the `platform-compile` job's Windows leg must
  run the new test lane, and the `platform-qualification.yml` Windows job
  must be dispatched and its log read.

## 10. Stop conditions

Stop and escalate rather than continue if any of the following holds.

- The Windows enumeration failure turns out to be a *confinement* defect
  (an entry escaping the root, a traversal accepted) rather than an
  enumeration-robustness defect. That is a security finding and gets its own
  plan under `docs/threat-model.md`.
- Making the corpus live requires changing a path policy, the request-target
  grammar, or the reserved-name list.
- The new Windows CI lane is flaky for reasons unrelated to this plan.

## 11. Risks and decision points

- **Risk**: the hosted Windows runner is not equivalent to a developer VM
  (no Developer Mode, no elevated privileges). The adversarial suites remain
  manual for exactly this reason; the new lane is scoped to the static
  crate's unit and integration tests, which need no special privilege.
- **Decision point**: rewriting corpus seeds changes fuzz inputs. Seeds are
  not crash artifacts, so this carries no regression history. The
  `fuzz_directory_buffer` corpus is untouched — it is binary and already
  byte-stable.
- **Decision point**: the trailing-terminator rule is applied to the two
  corpora whose target consumes the seed as an exact value. The
  `percent_decode`, `range_header`, `if_none_match`, and `validate_method`
  corpora keep their trailing newline deliberately: those targets parse a
  header value or a method, where a line terminator is a plausible input and
  the target's assertions are reached either way.

## 12. Dependencies

- Plan 310 (fuzz-harness repair) established the eleven-target harness and
  the corpus replay this plan extends.
- Plan 316 established the single-authority NT directory-buffer layout whose
  parser this plan hardens.

## 13. Closure requirements

A closure record under `plans/closure/static-confinement/` recording: the
hosted Windows evidence (run identifiers and the outcome of every new lane),
the corpus rewrite with the before/after byte counts, the gate rule and its
self-test mutations, the disposition of the two `#[ignore]`d enumeration
tests, and the confirmed statement that no confinement decision changed.

## 14. Implementation notes

**Deviation from §6 work package A.** The plan treated the recorded
`STATUS_INFO_LENGTH_MISMATCH` ignore reason as a finding to be confirmed. It
was a hypothesis, and it was wrong: a hosted Windows runner showed the
enumeration succeeding with the production 64 KiB buffer and the length
mismatch appearing only when the buffer cannot hold a whole fixed header.
The two enumeration tests were therefore un-ignored, and the probe that
settled it was kept as a permanent regression test rather than discarded.

**Deviation from §6 work package D.** The plan's scope did not anticipate
that the Windows qualification would surface a *second*, unrelated failure
(the race test's accepted-content set) that the first failure had masked by
aborting the step. Both were fixed in this milestone; the second was a
genuine test defect, not a product defect.

**Deviation from §6 work package B.** The plan expected to hold the corpus
to a no-trailing-terminator rule generally. It is held to it only for the two
corpora whose targets consume the seed as an exact value
(`path_components`, `platform_component`); the other four corpora parse a
header value or a method, where a line terminator is a plausible input and
the target's assertions are reached either way. The reason is recorded in
`docs/fuzzing.md`.

**One contradiction was resolved against the change.** The parser's
zero-length-name behavior is asserted by two tests that disagreed: an
active test (`parse_zero_length_filename`) that a zero-length name decodes
to an empty-named record, and a never-passing ignored test
(`parse_offset_before_current_record_end`) that expected one entry where the
parser produced two. The parser's behavior was **not** changed to satisfy the
never-passing test. The active, deliberately-written contract won; the
never-passing test became an active test that states the real contract. The
plan did not anticipate this and the resolution is recorded here.

## 15. Verification evidence

- Hosted Windows qualification: platform-qualification runs `39915458146`
  (the failure that exposed the inert corpus) and `39922248877` (green).
- Routine hosted CI: the `platform-compile` job's Windows leg runs the
  static crate's tests — CI run `39927249552`, `Windows confinement test
  lane` 363 passed / 0 failed / 7 ignored, `Windows static integration test
  lane` 4 passed / 0 failed / 1 ignored.
- Topology gate: `check_plan317_corpus_seed_hygiene` with six self-test
  mutations; the self-test grew 76 → 83 with this milestone.
- Local: `cargo test --workspace` exit 0; `cargo fmt --all -- --check` clean;
  `cargo clippy --workspace --lib --bins --tests -- -D warnings` clean;
  `cargo clippy -p eggserve-static --lib --tests --target
  x86_64-pc-windows-msvc -- -D warnings` clean.

## 16. Rollback

Every change in this plan is independently revertible: the `.gitattributes`
rule, the corpus rewrite, the CI lane, the parser arithmetic, the error
variant, the comment corrections, and the gate rule are separable. Reverting
the corpus rewrite without reverting the replay assertions would fail CI;
reverting both is safe.
