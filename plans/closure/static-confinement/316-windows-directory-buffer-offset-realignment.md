# Static Confinement Milestone 316 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/static-confinement/316-windows-directory-buffer-offset-realignment.md`

Source subsystem roadmap:

- `plans/subsystems/static-confinement-roadmap.md`

Repository baseline reviewed: `dfc9d2d` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Commit `09b805c` — the implementation: the parser's authoritative-layout
  documentation and zero-budget fix, the removal of the second layout
  definition in `fs/windows.rs`, the regenerated corpus and its recorded
  generator, the extended cross-platform replay, the fuzz target's
  zero-budget correction, the documentation of the authoritative record
  layout, and the topology-gate rule.
- Commit `c53dcf4` — Plan 313's lint-policy milestone, which also carries the
  doc comments added to `fs/dirbuf.rs` and `fs/windows.rs`. Those two files
  are the only ones both milestones touch, so the split is recorded here
  rather than hidden.

## 1. Executive finding

The plan's premise was **inverted**, and the milestone closed by correcting
it.

Plan 316 was authored on the reading that the platform-neutral parser and the
`FILE_ID_BOTH_DIR_INFO` struct in `fs/windows.rs` disagreed, and that the
parser's offsets were the side that had to move. Work package A established
the opposite: the **authoritative** layout is the one the parser already
implemented, and the hand-rolled FFI struct was wrong on every offset.

The authoritative layout is the NT `FILE_ID_BOTH_DIR_INFO` record as written by
the kernel and declared by `windows-sys` 0.61.2
(`Win32::Storage::FileSystem::FILE_ID_BOTH_DIR_INFO`, mirrored from
`winnt.h`):

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

The fixed header is **104 bytes**. The parser's constants were already
correct. The struct was the defective side.

Deleting the second definition — rather than realigning it — is what makes
the drift impossible to recur, and it matches the repository's established
one-authority pattern (Plans 306, 309, 311).

Removing the struct also surfaced two further defects that the struct had
caused, both in `get_file_id`:

1. It read `(*header).file_index` — bytes 8..16 of the real record, i.e. the
   low half of `CreationTime` — instead of `FileId` at offset 96. Its
   documented contract ("stable across renames within the same volume") was
   violated.
2. It sized its buffer as `80 + 256 * 2` bytes on a stated 80-byte header.
   The real record needs `104 + 2 * len`, so a file name longer than 244
   UTF-16 code units (NTFS permits 255) made `GetFileInformationByHandleEx`
   fail with `ERROR_INSUFFICIENT_BUFFER`.

`get_file_id` had **no production caller** (only a `#[test]` in the same
module), so both defects were latent rather than live. The security-critical
Windows enumeration path (`NtQueryDirectoryFile` → `parse_directory_buffer`)
was correct before this milestone and is unchanged.

Finding F1 — a zero entry budget yielding one entry — was fixed in the
parser. The committed corpus was regenerated from the authoritative layout,
so the happy path is covered for the first time.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The parser's constants agree with the authoritative NT layout | `fs/dirbuf.rs` unit test `layout_constants_match_the_authoritative_nt_layout`; topology rule `check_plan316_directory_buffer_layout` | pass |
| One layout authority; no second definition | The `struct FILE_ID_BOTH_DIR_INFO` is deleted; the topology rule rejects any reintroduction | pass |
| The Windows FFI reads the shared offset | `get_file_id` reads `FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET`; the topology rule rejects a byte literal | pass |
| `parse_directory_buffer(buf, 0)` returns zero entries | `parse_zero_budget_returns_no_entries`; `parse_budget_bounds_the_listing_exactly` (budgets 0..=12) | pass |
| The rejection set is unchanged | `every_reachable_error_variant_has_a_case`; every `DirBufParseError` variant and its meaning is preserved | pass |
| The parser never panics on arbitrary bytes | The libFuzzer target plus the corpus replay | pass |
| The corpus covers the happy path | `the_corpus_decodes_at_least_one_non_empty_name`; the topology rule's own decoder | pass |
| The corpus covers the rejections | `every_seed_has_its_expected_outcome`; the topology rule requires at least one rejected seed | pass |
| The corpus is reproducible | `committed_corpus_matches_the_generator` runs in routine CI; the generator is `crates/eggserve-static/tests/directory_buffer_corpus.rs` | pass |
| The replay asserts per-seed outcomes | `corpus_replay_directory_buffer` asserts a recorded outcome per seed, the budget-prefix property, and non-vacuity | pass |
| No new `unsafe`; no new dependency | The `unsafe {` count in `fs/windows.rs` went 35 → 34; no manifest gained a package; all three lockfiles byte-identical | pass |
| The Windows qualification suite is re-run | **Not run** — no Windows host in this environment | outstanding, manual |

## 3. Production implementation evidence

### 3.1 The layout authority

`crates/eggserve-static/src/fs/dirbuf.rs` now carries the authoritative
layout table in its module documentation, naming its source, plus the
`ntifs.h` vs `winnt.h` `EndOfFile`/`AllocationSize` ordering note and the
`FileId: i64 → u64` reinterpretation note. The constants remain the single
source of truth, and `FILE_ID_BOTH_DIR_INFO_HEADER_SIZE` is derived from
`FILE_ID_BOTH_DIR_INFO_FILE_NAME_OFFSET` rather than restated.

The module's "Known deviations" block was removed: both of its bullets are
false now.

### 3.2 The second layout definition is gone

`crates/eggserve-static/src/fs/windows.rs` no longer declares
`struct FILE_ID_BOTH_DIR_INFO`. `get_file_id` reads the eight-byte `FileId`
at the shared offset directly from the raw byte buffer — safe Rust, no cast,
no struct. Its buffer size is now
`FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + 256 * 2` (616 bytes, previously 592).

The `FILE_ID_BOTH_DIRECTORY_INFO = 37` information class, the
`GetFileInformationByHandleEx` class `10`, and every FFI signature are
untouched. The `unsafe` count fell by one; no `unsafe` block was added and
the approved Windows FFI boundary in `docs/unsafe-code-policy.md` is
unchanged.

### 3.3 The zero budget

The entry budget is now checked **before** the next record is parsed, so
`parse_directory_buffer(buf, 0)` returns `Ok(vec![])`. For every
`max_entries >= 1` the behavior is bit-for-bit identical to before: the new
check can only fire when the budget is zero.

### 3.4 The regenerated corpus

Fourteen seeds, inputs only, regenerated from the authoritative layout:

| Seed | Bytes | Outcome |
|---|---|---|
| `dot_dotdot` | 224 | `Ok([])` — `.` and `..` filtered |
| `dotfile` | 120 | `Ok([(".hidden", File, 30, dot=true)])` |
| `empty` | 0 | `Ok([])` |
| `invalid_utf16` | 112 | `Err(InvalidUtf16)` |
| `multi_entries` | 464 | `Ok([("a.txt",File,1), ("sub",Directory,2), ("link",ReparsePoint,3), (".hidden",File,4,dot)])` |
| `odd_name_length` | 112 | `Err(OddFileNameLength)` |
| `offset_overflow` | 120 | `Err(OffsetOverflow)` |
| `offset_underflow` | 120 | `Err(OffsetUnderflow)` |
| `overlong_name` | 112 | `Err(FileNameOutOfRange)` |
| `random` | 256 | `Err(FileNameOutOfRange)` |
| `reparse` | 112 | `Ok([("link", ReparsePoint, 20)])` |
| `single_dir` | 120 | `Ok([("subdir", Directory, 10)])` |
| `single_file` | 128 | `Ok([("hello.txt", File, 42)])` |
| `truncated_header` | 4 | `Err(TruncatedHeader)` |

Regeneration command:

```sh
cargo test -p eggserve-static --test directory_buffer_corpus -- --ignored
```

The generator transcribes the NT offsets as **literals** rather than
importing the crate's constants. That duplication is deliberate: it makes the
corpus an independent check on the parser instead of a self-consistent one.

### 3.5 The machine check

`scripts/check-crate-topology.py` gained
`check_plan316_directory_buffer_layout`, which enforces three things:

1. the parser's constants equal the authoritative layout, and the header size
   is derived from the file-name offset;
2. no second layout definition exists, and the Windows FFI reads the shared
   offset constant;
3. the committed corpus contains a seed that decodes to a non-empty name and
   a seed that is rejected — checked by a Python decoder inside the gate that
   mirrors the Rust parser's acceptance rules.

The self-test grew from 67 to 76 checks, including eight for Plan 316: a
good tree, a reintroduced second layout, a wrong offset, an underived header
size, an FFI that drops the shared constant, a corpus with no happy path, a
corpus with no rejection, and a missing corpus.

## 4. Verification executed

Run by the implementing agent and re-run by the parent:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p eggserve-static --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --lib --tests -- -D warnings` | clean |
| `cargo test -p eggserve-static --lib` | 372 passed, 0 failed, 3 ignored |
| `cargo test -p eggserve-static --test directory_buffer_corpus` | 4 passed, 1 ignored |
| `cargo test -p eggserve-core --test corpus_replay` | 11 passed |
| `cargo check --manifest-path fuzz/Cargo.toml --bins --locked` | clean |
| `cargo check -p eggserve-static --lib --target x86_64-pc-windows-msvc` | clean |
| `cargo clippy -p eggserve-static --target x86_64-pc-windows-msvc --lib --tests -- -D warnings` | clean |
| `python3 scripts/check-crate-topology.py` | exit 0 |
| `python3 scripts/check-crate-topology.py --self-test` | 76 checks passed |
| `git diff --stat -- '*Cargo.lock'` | empty — all three lockfiles byte-identical |

**Not run:** the Windows runtime qualification suite
(`cargo test -p eggserve-core --features windows-adversarial-qualification
--test windows_plan086`) — no Windows host is available in this environment.
The Windows *compile* path was checked with the `x86_64-pc-windows-msvc`
target, which is compile coverage, not runtime qualification.

## 5. Invariant review

| Invariant | Result |
|---|---|
| The parser never panics on arbitrary bytes | holds — the libFuzzer target and the corpus replay exercise it |
| The rejection semantics are unchanged | holds — every `DirBufParseError` variant keeps its meaning; the two variants that are unreachable by construction (`BufferOverflow`, `OffsetLoop`) are covered by a discriminim-based unreachability test rather than an invented producer |
| The confinement boundary is unchanged | holds — the parser reports what the buffer says; reparse-point and dotfile decisions stay in the caller |
| No new `unsafe`; the parser stays pure safe Rust | holds — the `unsafe` count fell by one; the parser module remains platform-neutral and `unsafe`-free |
| `eggserve-static` keeps sole ownership of the static/path/filesystem decisions | holds — no second parser appeared |
| The public API shape is unchanged | holds — `parse_directory_buffer`, `DirectoryEntryRecord`, `DirectoryEntryKind`, and `DirBufParseError` are unchanged; this was a behavior fix, not an API change |
| The corpus holds inputs only, never expectations | holds — the expectation table lives in the generator test, not in the corpus files |

## 6. Failure and recovery review

- The parser stays a pure function with no I/O, so it has no cancellation or
  restart semantics of its own.
- A corpus regeneration is atomic in the sense that the committed corpus is
  never left half-regenerated: the generator writes every seed, and routine CI
  asserts the committed bytes match the generator exactly.
- The zero-budget change makes `parse_directory_buffer(malformed, 0)` return
  `Ok(vec![])` rather than `Err`. This is deliberate — the budget bounds the
  entries *returned* and bounds the work — it is pinned by a test, and it is
  unreachable in production: `enumerate_directory` breaks out before calling
  the parser when the entry budget is exhausted, so the parser is never
  invoked with a zero budget there.
- The Windows qualification suite is not in routine CI, so a Windows-only
  regression would not be caught there. The cross-platform corpus replay is
  the routine guard.

## 7. Migration and compatibility review

- No public API shape change: `parse_directory_buffer` keeps its signature
  and its `Result` type.
- A behavior change **is** observable. Buffers that were previously rejected
  as malformed may now parse, and buffers that previously parsed to empty
  names may now produce entries. Anyone who depended on the old behavior was
  depending on a defect.
- The Windows directory-listing path changes output for real directories.
  This is a **patch** release event for `eggserve-static` and must not be
  bundled into a minor release without a note.

## 8. Security review

- The confinement boundary is untouched. The parser reports what the buffer
  says; it does not decide what may be served.
- The change *improves* the Windows listing path: before this milestone the
  committed corpus showed that real kernel buffers were being rejected as
  malformed, and the listing path was therefore untrustworthy.
- `get_file_id` now returns the stable `FileId` instead of the low half of
  `CreationTime`, so its documented stability contract is actually true.
- No `unsafe` was added and the approved boundaries in
  `docs/unsafe-code-policy.md` are unchanged.
- No new dependency; all three lockfiles are byte-identical.

## 9. Documentation and operations

- `crates/eggserve-static/src/fs/dirbuf.rs` — the authoritative layout table
  and the one-authority statement.
- `crates/eggserve-static/src/fs/windows.rs` — the corrected `get_file_id`
  documentation, stating the real field and offset.
- `crates/eggserve-static/tests/directory_buffer_corpus.rs` — the recorded
  generator, the per-seed expectations, and the drift guard.
- `docs/fuzzing.md` — the layout, the corpus regeneration command, and the
  coverage.
- `fuzz/README.md` — the generated-corpus note and the assertions.
- `architecture/eggserve-static.md` — the Windows directory-enumeration
  section with the layout table.
- `architecture/testing-and-conformance.md` — confirmed to need no change: it
  describes the `fuzz_directory_buffer` target but states no offsets or
  corpus facts.
- `plans/implementation/static-confinement/316-windows-directory-buffer-offset-realignment.md` —
  the header block, the recorded material deviation, and the source-roadmap
  anchor.
- `plans/subsystems/static-confinement-roadmap.md` — the new Milestone 4
  section, which the plan's own source-roadmap reference resolves to.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| medium | The Windows runtime qualification suite was not re-run: no Windows host is available in this environment. The Windows compile path was checked with `x86_64-pc-windows-msvc`, which is compile coverage, not runtime qualification. | Manual lane; the cross-platform corpus replay is the routine guard. The advisory Windows security posture stays `supported-functional`. |
| low | `DirBufParseError::BufferOverflow` and `OffsetLoop` are unreachable by construction (the parser receives a slice, so a returned byte count can never exceed the allocation; every advance is a strictly positive `checked_add`, so the walk cannot cycle). | Retained as a defensive contract and covered by a discriminim-based unreachability test, not by an invented producer. |
| low | The plan's §7E suggestion that the `#[cfg(test)] pub(crate)` `build_dir_info_entry` helper is "the natural generator" is not directly usable: an integration test cannot call it. | Resolved by locating the generator in the test file itself, transcribing the NT offsets as literals so the corpus is an independent check. |
| low | The plan's §10 "machine check that reads the constants and the struct" is unsatisfiable once the struct is deleted. | Resolved as a check against the authoritative NT layout — an in-crate constants test plus the out-of-crate generator's independent transcription — which is strictly stronger than a struct/parser comparison. |

## 11. Roadmap disposition

**Milestone closed.**

The NT directory-buffer layout is now one authority, stated in one place and
derived from one place; the second definition is gone rather than
reconciled; the zero entry budget is honored; the corpus is regenerated from
the authoritative layout with a recorded generator and is byte-reproducible
in routine CI; the cross-platform replay asserts per-seed expectations instead
of "did not panic"; and the topology gate enforces all of it, with eight
self-test mutations proving the gate fails when the layout drifts.

The roadmap's Milestone 3 exit condition — that the harness restores the
fuzz layer and that its verification-strategy claim is true — remains met.
The roadmap gains a Milestone 4 section for this milestone.

## 12. Registry updates

`plans/registry.md`:

- The Static confinement row's current milestone becomes 316, closed, with
  this closure record linked and the deviation recorded.
- The dependency-ready implementation plans row for 316 moves `ready` →
  `closed`, with this closure record and the resolution recorded.

`plans/subsystems/static-confinement-roadmap.md`:

- Milestone 4 moves `ready` → `closed`, with this closure record linked and
  the deviation recorded.
