# Static Confinement Milestone 317 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/static-confinement/317-windows-runtime-qualification-and-corpus-seed-hygiene.md`

Source subsystem roadmap:

- `plans/subsystems/static-confinement-roadmap.md`

Repository baseline reviewed: `ce457ae` (Plans 313, 315, and 316 closed)

Implementation commits or pull requests:

- Commit `5823514` — the corpus and CI work: the `.gitattributes` rule, the
  rewritten corpus seeds, the cross-platform replay's non-vacuity
  assertions, the Windows test lane, and the parser's checked arithmetic.
- Commit `4a8452b` — the hosted-runner qualification: the NT information
  class pinned against a live kernel, the enumeration tests un-ignored, and
  the Windows race test's accepted-content set corrected.
- Commit `4a38141` — the corrective follow-ups: the erroneous
  `SAFETY:` comment, the two sentinel error sites, the two information-class
  comments, and the registry rows.

## 1. Executive finding

The plan's premise was **wrong in a way that only a real Windows host could
reveal**. The baseline recorded two `#[ignore]`d unit tests whose reason was
*"NtQueryDirectoryFile STATUS_INFO_LENGTH_MISMATCH on CI runners; needs
Windows VM qualification"*, and the roadmap deferred the Windows runtime
qualification as a manual lane on the assumption that qualification needed a
developer Windows VM.

It does not. GitHub Actions provides a `windows-latest` runner that is a real
Windows host, and the qualification ran there.

Three defects were found and fixed, and one hypothesis was retired:

1. **The `path_components` fuzz corpus was inert.** Every seed carried a
   trailing newline, so every seed was a whitespace-containing request
   target. The canonical request-target parser rejects those before any
   path-confinement logic runs, so the fuzz target's assertions were
   unreachable and the corpus exercised nothing. The Windows qualification
   failed on this: the replay reported `replayed 0 entries` and its
   non-vacuity assertion fired.
2. **The corpus was not byte-stable across platforms.** Git's end-of-line
   conversion rewrote a text seed's trailing LF as CRLF on a Windows
   checkout, so the same seed was a different input on a different platform.
   A fuzz corpus whose seeds differ per platform does not satisfy the
   reproducibility the harness assumes.
3. **The Windows race test's accepted-content set was incomplete.**
   `windows_race_index_file_replacement_during_resolution` asserted that a
   reader observes one of the two versions the writer alternates between,
   omitting the fixture's pre-replacement content. A reader that opens the
   handle before the writer's first replacement legitimately observes that
   version; reading a stale but *complete* file is the correct behaviour for
   a pinned handle.
4. **The `STATUS_INFO_LENGTH_MISMATCH` hypothesis was retired.** A probe
   against a live kernel established that information class 37 is accepted
   and returns records, that the user-mode numbering (16) is rejected with
   `STATUS_INVALID_PARAMETER`, and that the length mismatch appears only when
   the buffer cannot hold a whole fixed header. With the production 64 KiB
   buffer the call succeeds, so the recorded ignore reason never held against
   the current code.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Execute the Windows qualification on a hosted runner | Platform-qualification run `39915458146` (Windows leg) and `39922248877` (Windows leg) | pass |
| Make the corpus byte-stable across platforms | `.gitattributes` marks `fuzz/corpus/**` `-text`; `git check-attr text` reports `unset` for the corpus | pass |
| Make the corpus live | 35 seeds in `path_components`/`platform_component` rewritten to the exact bytes their targets consume; 3 accepted request targets added; the Windows replay reports `replayed 4 entries` | pass |
| Assert non-vacuity on every platform | `crates/eggserve-core/tests/corpus_replay.rs` asserts the no-trailing-terminator rule, at least one accepted request target, and at least one reserved device name | pass |
| Guard the rule mechanically | Gate rule `check_plan317_corpus_seed_hygiene` with six self-test mutations; self-test 83 → 89 checks with Plan 318's rule | pass |
| Run the Windows tests in routine CI | `ci.yml`'s `platform-compile` job gains the `Windows confinement test lane` and the `Windows fuzz corpus byte-identity lane` | pass |
| Pin the NT information class against a live kernel | `nt_query_directory_file_uses_the_kernel_information_class` asserts class 37 is accepted, class 16 is `STATUS_INVALID_PARAMETER`, and an undersized buffer is a length mismatch | pass |
| Correct the parser's file-name arithmetic | `crates/eggserve-static/src/fs/dirbuf.rs` uses `checked_add` for the name range | pass |
| Remove the fabricated sentinel errors | `WindowsFsError::ParseError` carries the parse error; the file-id read states its bound as a compile-time assertion | pass |
| Correct the erroneous `SAFETY:` comment | The `NtQueryDirectoryFile` comment now states what the code actually does | pass |

## 3. Production implementation evidence

### The Windows qualification ran, and the premise was wrong

The baseline's premise was that the Windows runtime qualification "needs
Windows VM qualification". A hosted `windows-latest` runner is a Windows
host. The qualification was dispatched against the branch and its log read:

- Run `39915458146`: the Windows job failed, and the log showed
  `path_components corpus: replayed 0 entries` followed by
  `must replay at least one corpus entry`. The recorded reason was not a
  Windows limitation at all — the corpus was inert on every platform.
- Run `39922248877`: the Windows job succeeded on both legs after the fixes.

### The information class, established against a live kernel

A probe on the hosted runner printed, for each information class and buffer
size, the NTSTATUS and the first record's layout:

| Open | Class | Buffer | NTSTATUS | Bytes returned |
|---|---|---|---|---|
| `FILE_FLAG_OPEN_REPARSE_POINT` | 37 | 65536 | `0x00000000` | 468 |
| `FILE_FLAG_OPEN_REPARSE_POINT` | 37 | 616 | `0x00000000` | 468 |
| `FILE_FLAG_OPEN_REPARSE_POINT` | 37 | 104 | `0xc0000004` | 0 |
| `FILE_FLAG_OPEN_REPARSE_POINT` | 16 | any | `0xc0000003` | 0 |
| plain | 37 | 65536 | `0x00000000` | 468 |
| plain | 37 | 104 | `0xc0000004` | 0 |
| plain | 16 | any | `0xc0000003` | 0 |

Three conclusions, each of which the code now states:

1. **Class 37 is correct.** `NtQueryDirectoryFile` numbers its
   `FILE_INFORMATION_CLASS` with the kernel (`ntifs.h`) enumeration, where
   `FileIdBothDirectoryInformation` is 37. The user-mode `winnt.h` list
   numbers the same member 16, and the kernel rejects 16 with
   `STATUS_INVALID_PARAMETER`. Two independent sources agree with the
   kernel: `windows-sys` 0.61.2 and `ntapi` 0.4.3 both declare 37.
2. **`STATUS_INFO_LENGTH_MISMATCH` is a buffer-size condition**, appearing
   only when the buffer cannot hold a whole fixed header. The production
   buffer is 64 KiB, so the call succeeds. The recorded ignore reason never
   held against the current code.
3. **`FILE_FLAG_OPEN_REPARSE_POINT` is not implicated**: a plain directory
   handle behaves identically.

The probe was then converted into the permanent regression test
`nt_query_directory_file_uses_the_kernel_information_class`, so a future
edit that "corrects" 37 to 16 fails CI instead of silently breaking
enumeration.

### The corpus rewrite

A seed is the exact byte string the fuzz target consumes. 35 seeds in
`path_components` and `platform_component` carried a trailing newline that
was an authoring artifact rather than part of the input:

- In `path_components`, every seed became a whitespace-containing request
  target. The canonical request-target parser rejects those before any
  path-confinement logic runs, so the target's assertions were unreachable.
  The replay reported **0 accepted entries**.
- In `platform_component`, the five bare reserved device names (`CON`,
  `con`, `NUL`, `COM1`, `LPT1`) were hidden from the reserved-name check,
  because the trailing newline became part of the base name.

Three accepted request targets (`/a/b/c`, `/index.html`, `/a%20b/c`) were
added so the corpus covers the happy path, not only the rejection path. The
Windows qualification then reported `replayed 4 entries`.

### The race test's accepted-content set

`windows_race_index_file_replacement_during_resolution` asserted that a
reader observes one of the two versions the writer alternates between. The
fixture's pre-replacement content (`<html>index</html>`) was not in the set,
and the hosted runner's timing let the reader open the handle before the
writer's first replacement. The observed content was exactly that
pre-replacement version — a legitimate observation for a pinned handle. The
accepted set now includes it, and the test states the invariant it was
reaching for: the reader never observes a torn or mixed read.

### The parser's arithmetic and the error sites

- `name_start` and `name_end` are computed with `checked_add`. A hostile
  `FileNameLength` could previously wrap `usize` on a 32-bit target and turn
  an out-of-range name into an in-range one, contradicting the module's
  documented contract that it "rejects malformed kernel output rather than
  indexing unchecked".
- The two `0xBAADF00D` sentinel sites are gone. A directory-buffer parse
  failure now carries the parse error through the new
  `WindowsFsError::ParseError` variant instead of collapsing into a
  fabricated I/O code, and the file-id read states its bound as a
  compile-time assertion (`const _: () = assert!(FILE_ID_END <=
  BUFFER_SIZE)`) rather than a runtime check that could never fail.
- The `SAFETY:` comment on the `NtQueryDirectoryFile` call claimed the
  buffer was "properly aligned" (a `Vec<u8>` has alignment 1) and "zeroed
  before each call" (it is not). Both claims were false and are now true
  statements about what the code does.
- The two NT information classes are named separately and cite their
  source: `FileIdBothDirectoryInformation` is 37 in the NT
  `FILE_INFORMATION_CLASS` (`ntifs.h`), while `FileIdBothDirectoryInfo` is
  10 in the user-mode `FILE_INFO_BY_HANDLE_CLASS`. The previous comment
  conflated the two enums.

### The routine CI lane

`ci.yml`'s `platform-compile` job's Windows leg gained two steps:

- `Windows confinement test lane` — `cargo test -p eggserve-static --lib`,
  so the Windows-only unit tests actually *run* rather than being compiled
  at most.
- `Windows static integration test lane` — `cargo test -p eggserve-static
  --test directory_buffer_corpus --test listing_renderer`, which asserts
  the committed corpus is byte-identical to the generator's output on
  Windows too and that the listing renderer behaves identically there.

`scripts/verify.sh` records that a cross-compiled target still cannot *run*
those tests, so the local proxy remains the compile and clippy cross-check.

## 4. Verification executed

| Check | Command | Result |
|---|---|---|
| Topology gate | `python3 scripts/check-crate-topology.py` | exit 0 |
| Topology self-test | `python3 scripts/check-crate-topology.py --self-test` | 89 checks passed (76 → 83 with this milestone, 89 with Plan 318's) |
| Static crate tests | `cargo test -p eggserve-static --lib` | 373 passed, 0 failed, 2 ignored |
| Corpus generator | `cargo test -p eggserve-static --test directory_buffer_corpus` | 4 passed, 1 ignored |
| Cross-platform replay | `cargo test -p eggserve-core --test corpus_replay` | 11 passed |
| Windows-target clippy | `cargo clippy -p eggserve-static --lib --tests --target x86_64-pc-windows-msvc -- -D warnings` | clean |
| Workspace clippy | `cargo clippy --workspace --lib --bins --tests -- -D warnings` | clean |
| Workspace tests | `cargo test --workspace` | exit 0 |
| Format | `cargo fmt --all -- --check` | clean |
| Windows qualification (before the fixes) | Platform-qualification run `39915458146` | Windows job **failed** — the inert corpus |
| Windows qualification (after the fixes) | Platform-qualification run `39922248877` | Windows job **success**: static crate 373 passed / 0 failed / 2 ignored; adversarial suite 111 passed / 0 failed / 2 ignored (windows_plan084: 11 passed) |
| Hosted CI | CI run `39927249552` on `main` (`4a38141`) | all five jobs green; the Windows leg's `Windows confinement test lane` ran 363 passed / 0 failed / 7 ignored and `Windows static integration test lane` ran 4 passed / 0 failed / 1 ignored |

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| The confinement decision surface is unchanged | The diff touches no path policy, request-target grammar, or filesystem decision | holds |
| `parse_directory_buffer` keeps its documented contract | It still rejects malformed kernel output rather than indexing unchecked (now with checked arithmetic), and still honors a zero entry budget by returning no entries | holds |
| The corpus stays reproducible | `.gitattributes` marks the corpus `-text`; the generator's byte-identity assertion runs in routine CI, including on Windows | holds |
| The corpus stays live | The replay asserts at least one accepted request target and at least one reserved device name, on every platform | holds |
| The Windows path is qualified automatically | The `platform-compile` job's Windows leg runs the static crate's tests | holds |

## 6. Failure and recovery review

The qualification failed once, at run `39915458146`, and the failure was
read rather than worked around:

- The first read produced `path_components corpus: replayed 0 entries`. The
  recorded ignore reason (`needs Windows VM qualification`) was not
  consulted as a finding; the log was. The corpus was inert on every
  platform, and the CRLF conversion compounded it on Windows.
- The second read, at run `39922248877`, surfaced a *different* failure —
  the Windows race test — which had been masked by the corpus failure
  aborting the step. Both are fixed and both are now proven by the same
  run.
- No assertion was weakened to make a test pass. The race test's accepted
  set was widened to include a *legitimate* observation, not to stop
  failing; the corpus seeds were rewritten to be the inputs their targets
  consume; the parser's behavior was not changed to satisfy a never-passing
  test.

## 7. Migration and compatibility review

No public API change. `parse_directory_buffer`'s zero-budget behavior is
unchanged from the published `0.5.0` (the zero-budget fix landed in Plan
316, before this milestone, and is carried by the `eggserve-static` 0.5.1
patch). The new `WindowsFsError::ParseError` variant is `pub(crate)`, so it
is not part of the crate's public surface. The corpus is test data, not
shipped API.

## 8. Security review

- **No escape.** The parser's name range is now computed with `checked_add`,
  so a hostile length cannot wrap the index on a 32-bit target.
- **The enumeration path's information class is pinned**, so a plausible
  "correction" from 37 to 16 fails CI rather than breaking enumeration
  silently.
- **No confinement decision changed.** No path policy, no request-target
  grammar, no symlink or dotfile rule moved.
- **The misaligned-offset case was resolved against the existing,
  deliberately-written contract.** `parse_zero_length_filename` (an active
  test) documents that a zero-length `FileNameLength` decodes to an
  empty-named record, while the never-passing `parse_offset_before_current_record_end`
  expected the opposite. The contradiction was resolved in favour of the
  active, deliberate contract: the parser's behavior is unchanged, and the
  never-passing test is now an active test that states the real contract —
  a misaligned advance yields a bounded, terminating parse that can never
  publish a *named* entry and can never escape the buffer. An empty-named
  record renders as an empty link and is unreachable from a real kernel
  buffer, which never emits a zero-length `FileName`.

## 9. Documentation and operations

- `docs/fuzzing.md` — a new section states the two seed rules and why the
  trailing-terminator rule applies to two corpora and deliberately not to
  four others.
- `fuzz/README.md` — the same rules, in the corpus section.
- `scripts/verify.sh` — the platform-check comment records that a
  cross-compiled target cannot run the Windows tests, so the local proxy
  remains compile and clippy.
- `docs/migration-guide.md` — the Plan 317 section.

## 10. Unresolved findings

1. **Five `#[ignore]`d timing-sensitive race tests remain ignored** in
   `crates/eggserve-static/src/fs/windows.rs`
   (`race_file_to_reparse_point_denied`, `race_file_to_directory_type_change`,
   `race_delete_and_recreate`, `race_permission_change_after_enumeration`,
   `race_directory_entry_count_stability`). Their recorded reason is
   "Race condition timing differs on CI; needs Windows VM qualification". The
   hosted runner does not prove them either way, and the qualification lane
   now runs them with `--include-ignored` so their behavior is recorded
   rather than unknown. They stay ignored: their reason is a genuine
   timing-sensitivity claim, not a stale Windows claim, and un-ignoring a
   timing-sensitive test in a routine lane would make CI flaky.
2. **The Windows adversarial filesystem suites remain a manual lane.** Their
   fixtures need Developer Mode and elevated privileges that a hosted runner
   does not reliably provide. That is a property of the fixtures, not of
   the runner, and it is unchanged by this milestone.
3. **The `docs/dependency-policy.md` allowed-categories table still lists
   `windows-sys`**, which no manifest declares. Pre-existing drift,
   unrelated to this milestone, not actioned here.

## 11. Roadmap disposition

`plans/subsystems/static-confinement-roadmap.md` gains **Milestone 5 —
Windows runtime qualification and corpus seed hygiene**, and the milestone
status table records it as closed. The roadmap's §9 verification strategy
now states that the `#[cfg(windows)]` implementation is *run* on a hosted
Windows runner in routine CI, not merely compiled there, and §11 records
that the last gap in the roadmap's verification claim is closed.

## 12. Registry updates

`plans/registry.md`:

- The Plan 317 row is added to the implementation-plan table.
- The Plan 300 and Plan 304 rows and the 0.4.0-line publication row are
  corrected: they read `conditionally closed` with an open condition naming
  Milestone 305, which discharged that condition by republishing the
  corrected `0.5.0` line on 2026-10-22. All three now read `closed` with
  the discharge recorded.
- The direct-H1-runtime roadmap row's "was never closed" wording is
  corrected.
