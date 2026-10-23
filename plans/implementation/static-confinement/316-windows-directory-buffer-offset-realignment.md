# Static Confinement Milestone 316 — Windows directory-buffer offset realignment

Status: active

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/static-confinement-roadmap.md#milestone-4--windows-directory-buffer-offset-realignment`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (the static authority owns every
  static/path/filesystem decision; no serving outside the configured root)
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `docs/threat-model.md` (the confinement boundary)
- `docs/unsafe-code-policy.md` (the reviewed Windows FFI boundary)
- `docs/fuzzing.md`
- `architecture/eggserve-static.md`
- `architecture/testing-and-conformance.md`

Primary class: invariant

## 1. Objective

Correct the NT `FILE_ID_BOTH_DIR_INFO` layout that the Windows directory-buffer
parser in `eggserve-static` uses, and regenerate the directory-buffer seed
corpus so it exercises the parser's happy path rather than only its rejection
path.

Plan 310 repaired the fuzz harness and made the parser reachable
cross-platform (`eggserve_static::parse_directory_buffer`, now public). Doing so
made the parser's inputs reachable by the corpus replay, and the replay
immediately exposed that the parser's byte offsets contradict the struct
definition and the doc comment in the same file. Two named findings came out
of Plan 310 and are **not** fixed by it:

- **F1 (Low)** — `parse_directory_buffer(buf, 0)` returns one entry. The entry
  budget is checked *after* the first push, so a zero budget is not honored.
- **F2 (High)** — the parser's offsets contradict the `FILE_ID_BOTH_DIR_INFO`
  struct and its doc comment in `crates/eggserve-static/src/fs/windows.rs`:
  the struct places `FileName` at offset 80 and the doc comment states a fixed
  header of 80 bytes, while the parser uses a 104-byte header and reads
  `FileId` at 96 instead of 72. Under the current offsets, 6 of the 9
  committed `fuzz_directory_buffer` seeds are rejected as malformed and the
  remainder decode to empty names. The corpus therefore gives the happy path
  **no** coverage, and the Windows directory-listing path is untrustworthy.

This milestone realigns the parser and regenerates the corpus. It does not
change the confinement boundary, the public API shape, or the listing
semantics.

## 2. Why this milestone is ready

- Plan 310 is closed, the parser is public and platform neutral, and the
  directory-buffer corpus replay runs in routine CI on every platform.
- The defect is confined to one parser module and one corpus directory.
- The correct layout is defined by the NT `FILE_ID_BOTH_DIR_INFO` structure
  and is already written down in the same file — the parser simply does not
  agree with it.
- No new dependency, no new unsafe, no change to the confinement boundary.

## 3. Current implementation evidence

- `crates/eggserve-static/src/fs/dirbuf.rs` — the platform-neutral parser
  moved out of `fs/windows.rs` by Plan 310. It carries the five NT constants
  (`FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET = 60`,
  `FILE_ID_BOTH_DIR_INFO_FILE_ATTRIBUTES_OFFSET = 56`,
  `FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET = 96`,
  `FILE_ID_BOTH_DIR_INFO_HEADER_SIZE`, and the two `FILE_ATTRIBUTE_*` values)
  and the pure `parse_directory_buffer` function.
- `crates/eggserve-static/src/fs/windows.rs` — still carries the
  `FILE_ID_BOTH_DIR_INFO` struct definition and the doc comment describing an
  80-byte fixed header and `FileName` at offset 80.
- `fuzz/corpus/fuzz_directory_buffer/` — 9 seeds, of which 6 are rejected as
  malformed under the current offsets and the rest decode to empty names.
- `crates/eggserve-core/tests/corpus_replay.rs::corpus_replay_directory_buffer`
  — the cross-platform replay that surfaced this; it must stay green.

## 4. Invariants that must not regress

- The parser never panics on arbitrary bytes.
- The parser's rejection semantics are unchanged: an over-long file name, a
  truncated header, an odd file-name length, an offset loop, and invalid
  UTF-16 remain rejections with the same `DirBufParseError` variants.
- The confinement boundary is unchanged: the parser reports what the buffer
  says; it does not decide what may be served. Reparse-point and
  dotfile decisions stay in the caller.
- No new `unsafe`; the parser stays pure safe Rust.
- `eggserve-static` keeps sole ownership of the static/path/filesystem
  decisions; no second parser appears.
- The public API shape (`parse_directory_buffer`, `DirectoryEntryRecord`,
  `DirectoryEntryKind`, `DirBufParseError`) is unchanged — this is a behavior
  fix, not an API change.
- The corpus files remain committed inputs. A regenerated corpus must contain
  only inputs, never expectations.

## 5. Scope

**In scope**

- The NT offset constants in `crates/eggserve-static/src/fs/dirbuf.rs`.
- The struct definition and doc comment in
  `crates/eggserve-static/src/fs/windows.rs`, so that the parser and the
  documented layout agree.
- The `max_entries` budget check in `parse_directory_buffer` (F1).
- Regeneration of `fuzz/corpus/fuzz_directory_buffer/` so that the happy path
  is covered.
- The directory-buffer unit tests and the cross-platform corpus replay.
- `docs/fuzzing.md` and `architecture/testing-and-conformance.md` if they
  describe the layout or the corpus.

**Out of scope**

- Any change to the Windows FFI, the 35 `unsafe` blocks, or
  `docs/unsafe-code-policy.md`.
- Any change to the listing semantics, the `ListingEntry` type, the link
  encoding, or the directory-listing renderer (that is Plan 311).
- Any change to the response-byte ceiling.
- The Windows runtime qualification suites beyond what the offset change
  requires.
- Publication. The `eggserve-static` public API does not change shape, so this
  is a **patch** release event when it is eventually published.

## 6. Required production changes

0. **Recorded material deviation (resolved).** This plan was authored on the
   premise that the *parser* disagreed with the `FILE_ID_BOTH_DIR_INFO` struct
   in `fs/windows.rs` and that the parser's offsets were the side that had to
   move. Work package A established the opposite: the authoritative layout is
   the one the parser already implements, and the hand-rolled FFI struct was
   wrong on every offset.

   The authoritative layout is the NT `FILE_ID_BOTH_DIR_INFO` record as written
   by the kernel and declared by `windows-sys` (mirrored from `winnt.h`):

   | Field | Type | Offset |
   |---|---|---|
   | `NextEntryOffset` | `u32` | 0 |
   | `FileIndex` | `u32` | 4 |
   | `CreationTime` | `i64` | 8 |
   | `LastAccessTime` | `i64` | 16 |
   | `LastWriteTime` | `i64` | 24 |
   | `ChangeTime` | `i64` | 32 |
   | `EndOfFile` | `i64` | 40 |
   | `AllocationSize` | `i64` | 48 |
   | `FileAttributes` | `u32` | 56 |
   | `FileNameLength` | `u32` | 60 |
   | `EaSize` | `u32` | 64 |
   | `ShortNameLength` | `i8` | 68 |
   | `ShortName` | `[u16; 12]` | 70..94 |
   | `FileId` | `i64` | 96 |
   | `FileName` | `[u16; 1]` | 104 |

   The fixed header is **104 bytes**. (`ntifs.h` orders `EndOfFile` before
   `AllocationSize` and `winnt.h` orders it the other way; both occupy bytes
   40..56, so no offset the parser reads is affected.)

   The parser's constants — `FileNameLength` 60, `FileAttributes` 56, `FileId`
   96, `FileName` 104, header 104 — are **correct** and must not change. The
   struct in `fs/windows.rs` is the defective side: its `file_index: u64` field
   forces four bytes of padding after the real `u32` `FileIndex` and shifts
   every later field, so its computed layout puts `file_attributes` at 64,
   `file_name_length` at 68, `file_id` at 76, and `file_name` at 84.

   §6.1 below already anticipated this outcome ("If the authoritative layout
   turns out to be the one the parser currently implements, then the struct and
   the doc comment are what change"). The requirement of this milestone is
   unchanged — one layout, one authority, agreement enforced mechanically — but
   the direction of the realignment is the opposite of the one first assumed.

   The deviation surfaced two further defects in `fs/windows.rs` that are
   consequences of the same wrong struct and are therefore in scope here:

   - `get_file_id` read `(*header).file_index`, i.e. bytes 8..16 of the real
     record (the low half of `CreationTime`), instead of `FileId` at offset
     96. Its documented contract — "stable across renames within the same
     volume" — was violated. It had no production caller, so the defect was
     latent rather than live.
   - `get_file_id` sized its buffer as `80 + 256 * 2` bytes on the stated
     assumption of an 80-byte header. The real record needs `104 + 2 * len`,
     so file names longer than 244 UTF-16 code units (NTFS permits 255) made
     `GetFileInformationByHandleEx` fail with `ERROR_INSUFFICIENT_BUFFER`.

   The scope of this milestone is widened accordingly, and only accordingly: it
   removes the second layout definition so the drift cannot recur, and it fixes
   the two defects that the second definition caused. It does not add `unsafe`,
   does not change the Windows FFI signatures, and does not change the
   confinement boundary.

1. **Realign the offsets.** Determine the authoritative `FILE_ID_BOTH_DIR_INFO`
   layout from the NT structure (not from the parser) and make the constants
   agree with it. At minimum the fixed header size, the file-name length
   offset, the file-attributes offset, and the `FileId` offset must be
   mutually consistent and must agree with the struct definition and the doc
   comment already present in `fs/windows.rs`. If the authoritative layout
   turns out to be the one the parser currently implements, then the struct
   and the doc comment are what change — the requirement is agreement, and
   the resolution must be recorded.
2. **Honor a zero budget (F1).** `parse_directory_buffer(buf, 0)` must return
   an empty vector, not one entry. The budget is a bound on the number of
   entries returned.
3. **Keep the rejection set intact.** Changing the offsets must not turn a
   malformed-buffer rejection into a panic or a silently accepted garbage
   entry.
4. **Reconcile the two sources of truth.** The struct definition, the doc
   comment, and the constants must describe the same layout. A reader must be
   able to check the parser against the struct without a second lookup.

## 7. Ordered work packages

### A. Establish the authoritative layout

Write down the layout in one place — the doc comment above the parser — with
each field's offset, and derive the constants from it. Record the source of
the layout in the comment. Do not "fix" the constants by trial and error
against the corpus; the corpus is the thing being fixed.

### B. Realign the constants and the budget check

Apply the corrected offsets, add the zero-budget fix, and keep the parser's
public behavior otherwise unchanged.

### C. Reconcile the struct and doc comment in `fs/windows.rs`

Make the `FILE_ID_BOTH_DIR_INFO` struct definition and its doc comment agree
with the corrected layout. The Windows FFI and its `unsafe` blocks are not
touched.

### D. Regenerate the directory-buffer corpus

Regenerate `fuzz/corpus/fuzz_directory_buffer/` so that it contains:

- seeds that exercise the happy path — a valid single-entry buffer, a
  multi-entry buffer, a buffer whose entries include a directory, a
  reparse point, and a dotfile;
- seeds that exercise each rejection — truncated header, odd file-name
  length, over-long file name, offset loop, invalid UTF-16;
- the existing malformed seeds, retained.

The corpus is regenerated from the corrected layout. It must be reproducible:
record how the seeds are produced so a reviewer can rebuild them. The
existing `build_dir_info_entry` fixture helper in the static crate is the
natural generator; if it cannot express the layout, it is corrected as part of
this milestone.

### E. Extend the coverage

The cross-platform corpus replay must assert, for every seed, that the
parser's outcome is one of the expected outcomes — and that the valid seeds
actually produce their expected entries. A replay that only asserts "did not
panic" is not sufficient evidence; it is the vacuity that Plan 310 finding F3
removed.

### F. Re-verify the Windows path

Re-run the Windows adversarial qualification suite
(`crates/eggserve-core/tests/windows_plan086.rs`, feature
`windows-adversarial-qualification`) on a Windows host to confirm the
directory-listing path now decodes real buffers. This is manual qualification
and is not part of routine CI.

## 8. Failure, cancellation, restart, and contention semantics

- The parser stays a pure function with no I/O, so it has no cancellation or
  restart semantics of its own.
- A corpus regeneration is a developer action producing files in the
  repository; it must be atomic in the sense that the committed corpus is
  never left half-regenerated.
- The Windows qualification suite is not run in routine CI, so a Windows-only
  regression would not be caught there. The cross-platform corpus replay is
  the routine guard.

## 9. Compatibility and migration

- No public API shape change: `parse_directory_buffer` keeps its signature
  and its `Result` type.
- A behavior change **is** observable: buffers that were previously rejected
  as malformed may now parse, and buffers that previously parsed to empty
  names may now produce entries. Anyone who depended on the old behavior was
  depending on a defect.
- The Windows directory-listing path changes output for real directories.
  This is a **patch** release event for `eggserve-static`; it must not be
  bundled into a minor release without a note.

## 10. Required tests

- Unit tests in the static crate covering: a valid single-entry buffer; a
  valid multi-entry buffer; a buffer with a directory entry, a reparse-point
  entry, and a dotfile entry; a zero budget; and one case per
  `DirBufParseError` variant.
- A test that the offsets in the parser agree with the struct definition in
  `fs/windows.rs` — a machine check that reads the constants and the struct
  and compares them, so the two cannot drift apart again.
- The cross-platform corpus replay extended to assert per-seed expectations,
  not merely "no panic".
- The existing `cargo test -p eggserve-static --lib` suite must stay green.

## 11. Required verification commands

```sh
cd /home/sugarwookie/projects/eggserve
cargo fmt --all -- --check
cargo clippy -p eggserve-static --lib --tests -- -D warnings
cargo test -p eggserve-static --lib
cargo test -p eggserve-core --test corpus_replay
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
```

Manual, on a Windows host:

```sh
cargo test -p eggserve-core --features windows-adversarial-qualification --test windows_plan086
```

## 12. Documentation updates

- `docs/fuzzing.md` — the directory-buffer target's description of the layout
  and the corpus must match the corrected reality.
- `architecture/testing-and-conformance.md` — the `fuzz_directory_buffer` row
  and the corpus-replay coverage description.
- `architecture/eggserve-static.md` — if it describes the Windows directory
  enumeration or the parser.

## 13. Acceptance criteria

- The parser's constants, the `FILE_ID_BOTH_DIR_INFO` struct definition, and
  the documented layout agree, and a machine check enforces that agreement.
- `parse_directory_buffer(buf, 0)` returns zero entries.
- The regenerated corpus contains valid seeds that decode to their expected
  entries, and the replay asserts those expectations.
- Every `DirBufParseError` variant is still reachable from the corpus.
- The parser still never panics on arbitrary bytes.
- The static crate's test suite, the cross-platform corpus replay, the fuzz
  build gate, and the topology gate are green.

## 14. Stop conditions

- If the authoritative `FILE_ID_BOTH_DIR_INFO` layout cannot be established
  from a primary source, stop and record the ambiguity rather than choosing an
  offset that merely makes the corpus pass.
- If realigning the offsets requires changing the Windows FFI or adding
  `unsafe`, stop: that is a different milestone.
- If the corrected layout cannot express the committed corpus's intent, stop
  and report — do not regenerate the corpus to match the parser.
- If fixing F1 changes the `max_entries` contract in a way that breaks the
  static service, stop and report; the contract change needs its own plan.

## 15. Closure evidence required

- The diff showing the corrected constants and the zero-budget fix.
- The machine check that enforces struct/parser agreement, plus a
  `--self-test` mutation proving the check fails when the two drift.
- The regenerated corpus with the generator recorded.
- Per-seed expectations in the replay, and the replay's pass output.
- The verification command table from §11, with actual outcomes.
- The Windows qualification result, or an explicit statement that it was not
  run and why.
- A statement of the release-event classification (patch).

## 16. Handoff notes

- The parser lives in `crates/eggserve-static/src/fs/dirbuf.rs` after Plan
  310. Read it before planning any change; it is platform neutral and pure
  safe Rust.
- The Windows FFI in `crates/eggserve-static/src/fs/windows.rs` is the
  reviewed `unsafe` boundary described in `docs/unsafe-code-policy.md`. It is
  out of scope here.
- Plan 311 owns the directory-listing renderer; do not change the renderer or
  the response-byte ceiling here.
- The corpus replay path was corrected by Plan 310 (finding F3). The
  `require_corpus(...)` guard must remain.
