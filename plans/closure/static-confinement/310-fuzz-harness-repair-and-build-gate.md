# Static Confinement Milestone 310 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/static-confinement/310-fuzz-harness-repair-and-build-gate.md`

Source subsystem roadmap:

- `plans/subsystems/static-confinement-roadmap.md`

Repository baseline reviewed: `a7e2f22` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Uncommitted working-tree implementation at closure time (the maintainer owns
  the commit and the release line).

## 1. Executive finding

The fuzz crate was **not** covering anything it claimed to cover. At the
baseline, four of its eleven targets did not compile, one was inert, and the
"deterministic CI corpus regression" iterated over **zero inputs** while
reporting success.

All eleven targets now compile, run, and are linted and format-checked in
routine CI. The NT directory-buffer parser — previously Windows-only and
unreachable — is now a platform-neutral public API, so the directory-buffer
target runs on every platform and the corpus replay covers it.

Two production defects were found and **deliberately not fixed**: the parser's
byte offsets contradict the `FILE_ID_BOTH_DIR_INFO` struct and doc comment in
the same file, and a zero entry budget is not honored. They are registered as
a separate corrective plan, Milestone 316 (§10).

The harness itself had four wrong oracles and one vacuous corpus path; all
five were fixed **in the harness only**. No production defect was fixed by
this milestone, and no production behavior changed except the addition of a
public API.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| All 11 targets compile | met | §3.1 table; `cargo check --bins --locked` in `fuzz/` passes |
| `range_header` repaired | met | `FileRange` accessors (`start()`, `end_inclusive()`, `is_empty()`) replace private-field reads |
| `fuzz_header_block` repaired | met | `HeaderValue::to_str()` replaces the non-existent `as_str()`; the assertion now encodes the documented OWS contract |
| `fuzz_normalize_response` repaired | met | `to_str()`; the HEAD assertion now encodes the `EmptyWithLength` contract |
| `fuzz_request_body` repaired | met | tokio's `rt-multi-thread` feature added — a **feature flag on an existing dependency**, so no new package enters the graph |
| Windows directory-buffer target rebound | met | Option 1: the parser moved to `crates/eggserve-static/src/fs/dirbuf.rs` and is now public; the target runs on every platform |
| Tautological assertion removed | met | The `assert!(!entry.name.is_empty() \|\| entry.name.is_empty())` arm is deleted and replaced by six properties that can each fail (§3.4) |
| `fuzz/Cargo.lock` refreshed | met | Package-name set **identical** (58 before, 58 after; `ADDED: []`, `REMOVED: []`); version and edge refresh only |
| No new external package | met | The tokio `rt-multi-thread` feature pulls no new package (`rt-multi-thread = ["rt"]` in tokio 1.52.3) |
| Corpus replay covers every target | met | 11 targets ↔ 11 corpus directories ↔ 11 `corpus_replay_*` tests; 196 seeds total |
| Fuzz crate built in routine CI | met | New CI step *Build fuzz harness*; plus *Lint and format-check fuzz harness* |
| `docs/fuzzing.md` corrected | met | Deleted source paths repointed to `eggserve-static`; real target inventory and state documented |
| Findings triaged, production untouched | met | §3.5 table; two findings deferred to Milestone 316 |
| Machine-checked inventory | met | `scripts/check-crate-topology.py::check_plan310_fuzz_target_inventory` + 7 `--self-test` mutations |

## 3. Production implementation evidence

### 3.1 Build status — all eleven targets

| Target | Before | After | Repair |
|---|---|---|---|
| `request_target` | built | built | clippy only (`&format!` → `format!`, `is_multiple_of`) |
| `percent_decode` | built | built | rustfmt only |
| `path_components` | built | built | rustfmt only |
| `validate_method` | built | built | clippy (`match` → `if let`) |
| `range_header` | **8 errors** | built | `FileRange` accessors |
| `if_none_match` | built (oracle wrong) | built | oracle corrected (§3.5 F4a) |
| `platform_component` | built | built | rustfmt only |
| `fuzz_header_block` | **1 error** | built | `to_str()` + oracle corrected (F4c) |
| `fuzz_normalize_response` | **1 error** | built | `to_str()` + oracle corrected (F4b) |
| `fuzz_request_body` | **1 error** | built | tokio `rt-multi-thread` + 3 oracles corrected (F4d) |
| `fuzz_directory_buffer` | **inert** (`#![cfg(windows)]`) | built **and runs on Linux** | parser rebinding |

Baseline errors reproduced from the pre-change `cargo check --bins`: `E0616`
(private field `start` / `end_inclusive`) at four sites, `E0599` (`as_str` on
`HeaderValue`) at two sites, `E0599` (`Runtime::new` not found).

**No assertion was weakened.** Where an oracle disagreed with the production
contract, the oracle was corrected to the *documented* contract.

### 3.2 Windows parser rebinding — Option 1 (cross-platform)

The parser is pure safe Rust — ordinary byte indexing, no `unsafe`, no
Windows types — and the NT offsets are wire-format constants rather than
Windows API bindings. `eggnet-tls`-style external bindings were therefore not
needed.

- New module `crates/eggserve-static/src/fs/dirbuf.rs` (587 lines) holds the
  parser, `DirectoryEntryRecord`, `DirectoryEntryKind`, `DirBufParseError`,
  `advance_offset`, and the five NT constants.
- `crates/eggserve-static/src/fs/windows.rs` now does
  `use super::dirbuf::{...}` and is otherwise unchanged: the `unsafe` block
  count is **35 before and 35 after**, the new module contains **zero**
  `unsafe`, and `docs/unsafe-code-policy.md` is untouched.
- `eggserve-static` gains **no** dependency (`windows-sys` is still absent).
- The 20 parser unit tests and the `build_dir_info_entry` fixture moved with
  the module, so they now run on Linux (19 pass, 1 pre-existing `#[ignore]`).

Public API added (additive → **patch**, Plan 291 precedent):

```rust
// crates/eggserve-static/src/lib.rs
pub use fs::dirbuf::{
    parse_directory_buffer, DirBufParseError, DirectoryEntryKind, DirectoryEntryRecord,
};

// crates/eggserve-static/src/fs/dirbuf.rs
pub struct DirectoryEntryRecord { pub name: String, pub kind: DirectoryEntryKind,
                                  pub file_id: Option<u64>, pub hidden_or_dot: bool }
pub enum DirectoryEntryKind { File, Directory, ReparsePoint, Other }
pub enum DirBufParseError { BufferOverflow, TruncatedHeader, OddFileNameLength,
    FileNameOutOfRange, OffsetUnderflow, OffsetOverflow, OffsetLoop, InvalidUtf16 }
pub fn parse_directory_buffer(buffer: &[u8], max_entries: usize)
    -> Result<Vec<DirectoryEntryRecord>, DirBufParseError>;
```

### 3.3 `fuzz/Cargo.lock` delta

Package-name set **identical**: 58 before, 58 after, `ADDED: []`,
`REMOVED: []`. The delta is a version-and-edge refresh:
`eggserve-core 0.1.2 → 0.5.0`, `eggserve-primitives 0.1.2 → 0.2.3`,
`eggserve-server 0.1.2 → 0.5.0`, `eggserve-static 0.1.2 → 0.5.0`, plus the
removal of the stale `phf` edge from `eggserve-core`'s dependency list (`phf`
itself remains a package, via `eggserve-static`). The main workspace
`Cargo.lock` is untouched.

### 3.4 Tautology removed

`assert!(!entry.name.is_empty() || entry.name.is_empty());` asserted nothing —
an empty name is legal per the parser contract — so it was **deleted**, not
replaced with a weaker equivalent. It is replaced by six properties that can
each fail:

- `entry.hidden_or_dot == entry.name.starts_with('.')` — breaks if the dotfile
  flag is derived wrongly.
- `entry.name != "." && entry.name != ".."` — breaks if the pseudo-entry
  filter is dropped.
- `entry.file_id.is_some()` — breaks if the FileId stops being published.
- `entries.len() <= max_entries` — breaks if the budget is not enforced.
- determinism — two parses of the same bytes must yield identical records.
- budget-independence — a buffer rejected under the narrowest budget must be
  rejected under every wider budget.

The `matches!(entry.kind, File|Directory|ReparsePoint|Other)` arm was also
deleted: an enum value is trivially one of its variants.

### 3.5 Findings triage

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| **F1** | Low | `parse_directory_buffer(buf, 0)` returns **one** entry: the budget is checked *after* the push, so a zero budget is not honored. The only production caller passes ≥ 1, so no current path is affected. | **Not fixed.** Deferred to Milestone 316. |
| **F2** | **High** | The parser's offsets contradict the `FILE_ID_BOTH_DIR_INFO` struct and doc comment in the same file (`fs/windows.rs`: the struct places `FileName` at offset 80 and the doc comment states an 80-byte fixed header; the parser uses 104, and reads `FileId` at 96 instead of 72). The committed seeds follow the 80-byte layout, so **6 of 9 seeds are rejected as malformed** and the rest decode to empty names — the corpus gives the happy path no coverage, and the Windows listing path is untrustworthy. | **Not fixed.** Deferred to Milestone 316. |
| **F3** | **High** | `corpus_replay.rs`'s `CORPUS_DIR` pointed at `crates/fuzz/corpus` — one level too shallow — so the "deterministic CI regression" iterated over **zero inputs** and passed vacuously. | **FIXED.** Path corrected to `../../fuzz/corpus`; a `require_corpus(...)` guard added to all 11 replay tests so a missing corpus fails loudly. |
| **F4** | Medium (harness) | Four oracles asserted a contract the production does not promise; each crashed the repaired target on its own committed seed: (a) `if_none_match` compared against the unquoted tag `100-1234` (the entity-tag inner value is `"100-1234"`, quotes included) — seeds `list_match`, `matching`, `weak_match_strong`, `strong_match_weak`; (b) `fuzz_normalize_response` used `ResponseBody::is_empty()`, which reports *representation* length, while a HEAD response legitimately carries `EmptyWithLength(equivalent_get_len)` — seed `02_head_200`; (c) `fuzz_header_block` looked only for CR/LF/NUL, but the accepted domain is HTAB + SP–`~` + obs-text, so every other CTL and DEL is refused — seed `04 01 01 01`; (d) `fuzz_request_body`: `was_fully_consumed()` is true for an empty body at construction (seed `empty`); the post-chunk state is `Complete` when the chunk exhausted the body (seed `61`); and the "one-over-limit" test used a limit *equal* to the body length, which is legal (seed `hello world`). | **FIXED in the harness only.** Production behavior is unchanged and correct in all four cases. |
| **F5** | Low | `crates/eggserve-core/tests/windows_plan086.rs::windows_fuzz_corpus_replay_directory_buffer` never called the parser ("we can't call the parser directly from a test (it's internal)") — it only asserted the seed files were non-empty. | **FIXED by the parent.** The test is removed: the parser is now public, and the cross-platform `corpus_replay_directory_buffer` covers it on every CI run rather than only under the Windows qualification feature. |

No production defect was fixed in this milestone.

### 3.6 Target ↔ corpus-replay coverage

| libFuzzer target | corpus seeds | replay test | status |
|---|---|---|---|
| `request_target` | 34 | `corpus_replay_request_target` | pre-existing |
| `percent_decode` | 9 | `corpus_replay_percent_decode` | pre-existing |
| `path_components` | 11 | `corpus_replay_path_components` | pre-existing |
| `validate_method` | 26 | `corpus_replay_validate_method` | pre-existing |
| `range_header` | 17 | `corpus_replay_range_header` | pre-existing |
| `if_none_match` | 11 | `corpus_replay_if_none_match` | pre-existing; oracle corrected |
| `platform_component` | 24 | `corpus_replay_platform_component` | pre-existing |
| `fuzz_header_block` | 19 | `corpus_replay_header_block` | **extended** |
| `fuzz_normalize_response` | 21 | `corpus_replay_normalize_response` | **extended** |
| `fuzz_request_body` | 15 | `corpus_replay_request_body` | **extended** |
| `fuzz_directory_buffer` | 9 | `corpus_replay_directory_buffer` | **ADDED** (new, cross-platform) |

196 seeds in total. Every target has a matching replay, and every replay
asserts its corpus directory exists.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cd fuzz && cargo check --bins` | PASS — all 11 compile |
| `cd fuzz && cargo check --bins --locked` | PASS — the committed lockfile is in sync |
| `cd fuzz && cargo clippy --bins -- -D warnings` | PASS (7 lint errors before) |
| `cd fuzz && cargo build --bins` + replay every corpus through its target | PASS — 11/11 clean, 196 seeds, 0 crashes |
| 20 s libFuzzer run per target (seed 42) | PASS — 0 crashes; 10 M–25 M execs per target (directory-buffer 20.4 M, header-block 6.4 M, normalize 8.0 M, request-body 35 k) |
| `cargo test -p eggserve-core --test corpus_replay` | PASS — 11 passed |
| `cargo test -p eggserve-static --lib` | PASS — 366 passed, 3 ignored |
| `cargo clippy -p eggserve-static --lib --tests -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS — 0 diffs |
| `cargo fmt --manifest-path fuzz/Cargo.toml --all -- --check` | PASS — 0 diffs |
| `python3 scripts/check-crate-topology.py` | PASS (exit 0) |
| `git diff --stat -- fuzz/corpus/` | PASS — empty (corpus untouched) |
| `git diff -- Cargo.toml Cargo.lock` | PASS — empty (main workspace lockfile untouched) |
| `git diff -- docs/unsafe-code-policy.md` | PASS — empty |

## 5. Invariant review

| Invariant | Result |
|---|---|
| The harness never weakens a confinement invariant; every assertion is about production behavior of the owning crate | Held. Four oracles were corrected to the documented production contract; no assertion was weakened to make a target compile. |
| `eggserve-static` remains the sole static/path/filesystem authority | Held. The harness drives the authority; the parser was made public, not reimplemented. |
| No target asserts a tautology | Held. One tautology removed; six fail-able properties added. |
| The committed corpus stays committed and unchanged | Held. `git diff -- fuzz/corpus/` is empty. |
| The corpus replay keeps running in routine CI and covers the same invariants | Held — and materially improved: the replay was vacuous before (F3) and now covers 196 seeds, including the directory-buffer target. |
| The `fuzz/` crate stays excluded from the main workspace and its lockfile | Held. |
| No new dependency enters the main workspace or the main supply-chain lockfile | Held. The main `Cargo.lock` is untouched. |
| No new `unsafe`; the unsafe-code policy is unchanged | Held. `unsafe` count in `fs/windows.rs` is 35 before and after; the new module has none; `docs/unsafe-code-policy.md` is untouched. |
| No production behavior changed other than the additive public re-export | Held. No production defect was fixed; the only production change is the `pub use`. |

## 6. Failure and recovery review

- The fuzz build gate now fails the routine CI `rust` job when any target
  stops compiling. Before this milestone that failure was invisible: four
  targets had been broken while the repository was green.
- The corpus replay now fails loudly when its corpus directory is missing or
  empty (`require_corpus`), so a lost or relocated corpus cannot produce a
  vacuous green.
- A fuzz crash artifact (`fuzz/crash-<hash>`) found during this milestone was
  triaged (it was an oracle bug, F4d) and removed. The `fuzz/` directory has
  no committed crash artifact.
- The 20-second libFuzzer sessions found no production crash.

## 7. Migration and compatibility review

- `eggserve-static` gains an additive public API (`parse_directory_buffer`
  and its three types) — a **patch** release event.
- No production behavior changes, so no migration is required.
- The fuzz crate's tokio feature set changes (`rt-multi-thread` added). This is
  internal to the excluded fuzz workspace.

## 8. Security review

- The change does not touch the confinement boundary. The parser reports what a
  buffer says; it does not decide what may be served.
- The parser is now reachable from any platform, which widens the *tested*
  surface of the Windows directory-enumeration code. That is a coverage
  improvement, not an exposure change: the parser was already reachable from
  the Windows path in production.
- Making the parser public means its error type and record type are now part
  of `eggserve-static`'s public contract. The types expose only names, kinds,
  a file id, and a dotfile flag — no paths and no content.
- F2 remains open: the parser's offsets do not match the struct in the same
  file, and 6 of 9 seeds are rejected as malformed. Until Milestone 316
  closes it, the Windows directory-listing path and the directory-buffer
  corpus are **not** trustworthy. This is stated plainly rather than
  papered over.

## 9. Documentation and operations

| Surface | Change |
|---|---|
| `docs/fuzzing.md` | Deleted source paths repointed to `eggserve-static`; the real target inventory and state; the three-layer model (property tests / corpus replay / fuzz targets); the build gate; the no-tautology rule. |
| `fuzz/README.md` | The same corrections: the stale "Windows-only — empty harness on Linux/macOS" line, the assertion list, and the build-gate section. |
| `architecture/testing-and-conformance.md` | The `fuzz_directory_buffer` row and the corpus-replay coverage description (parent-owned file, updated by the parent). |
| `.github/workflows/ci.yml` | New steps *Build fuzz harness* and *Lint and format-check fuzz harness* in the `rust` job. |
| `scripts/verify.sh` | `cmd_fast` now builds, lints, and format-checks the fuzz crate. |
| `scripts/check-crate-topology.py` | New rule `check_plan310_fuzz_target_inventory` plus 7 `--self-test` mutations and a live-tree check. |

## 10. Unresolved findings

1. **F2 (High) — the NT directory-buffer offsets contradict the struct in the
   same file.** Deferred to
   `plans/implementation/static-confinement/316-windows-directory-buffer-offset-realignment.md`.
   Until that plan closes, the Windows directory-listing path and the
   directory-buffer corpus are not trustworthy.
2. **F1 (Low) — a zero entry budget returns one entry.** Deferred to the same
   plan.
3. **F2's consequence for the corpus** — 6 of the 9 committed seeds are
   rejected as malformed and the rest decode to empty names. The corpus gives
   the happy path no coverage until the corpus is regenerated. Regeneration is
   part of Milestone 316.
4. The optional timing assertions ("descriptor exhaustion does not hot-spin")
   and a live-loop fatal-termination test are not expressible without an
   injected `accept(2)` failure; both are reported in Milestone 309's closure
   record.

## 11. Roadmap disposition

The static-confinement roadmap's Milestone 310 ("Fuzz-harness repair and build
gate") is **closed**. The roadmap's verification claim that "11 fuzz targets"
exist and are meaningful is now true for the first time.

Milestone 316 ("Windows directory-buffer offset realignment") is **ready** and
takes the two deferred parser findings. Publication is a separate milestone;
`eggserve-static`'s additive public surface is a patch release event when it
ships.

## 12. Registry updates

- `plans/registry.md` — Milestone 310 moved to `closed` with a pointer to this
  closure record; Milestone 316 added as `ready`; the static-confinement
  roadmap row's current milestone moved to 316.
- `plans/subsystems/static-confinement-roadmap.md` — Milestone 310 status and
  closure-record column updated; Milestone 316 added.
- A new topology-gate rule `check_plan310_fuzz_target_inventory` was added to
  `scripts/check-crate-topology.py`. It keeps the three sets equal — the
  `[[bin]]` entries in `fuzz/Cargo.toml`, the corpus directories under
  `fuzz/corpus/`, and the `corpus_replay_*` tests — so a target can only be
  removed deliberately, from all three at once, and it rejects a replay that
  could pass vacuously.
