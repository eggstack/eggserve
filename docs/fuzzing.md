# Fuzzing and Property Testing

eggserve uses three layers of automated testing beyond hand-written unit tests:

1. **Property tests** (proptest) — run in normal CI via `cargo test`, bounded inputs, deterministic
2. **Corpus replay** (deterministic CI regression) — replays every committed fuzz seed through the
   same invariants the fuzz targets assert, on every commit
3. **Fuzz targets** (libFuzzer) — scheduled/manual, unbounded exploration, crash artifact upload

The fuzz crate is its own cargo workspace (`fuzz/Cargo.toml` + `fuzz/Cargo.lock`), excluded from the
main workspace. It depends on the crates by path, so it always drives the current implementation.

## Property tests

Located in `#[cfg(test)]` modules across the crates that own the behaviour:

| Module | Property tested |
|--------|----------------|
| `crates/eggserve-static/src/path/decode.rs` | No NUL in output, bounded length, valid UTF-8, no panic on arbitrary input |
| `crates/eggserve-static/src/path/platform.rs` | `check_component`/`is_windows_reserved_name`/`has_windows_drive_prefix` never panic; case-insensitivity; drive-prefix structure; clean components pass |
| `crates/eggserve-static/src/planner.rs` | Range within file size, ETag format, HEAD no body, 304 empty body, weak/strong ETag equivalence, wildcard always matches, no panic on arbitrary range/ETag strings |
| `crates/eggserve-static/src/fs/dirbuf.rs` | Directory-buffer parser: single/multi entry decoding, `.`/`..` filtering, dotfile flag, offset overflow/underflow, odd filename length, invalid UTF-16, entry-budget bound |
| `crates/eggserve-static/src/lib.rs` | `html_escape` no raw angle brackets, no panic; `percent_encode_path_segment` no raw `?`/`#`, no panic |

The path, planner, and filesystem decisions live in `eggserve-static` (the sole static/path/filesystem
authority); the code no longer lives under `crates/eggserve-core/src/path/`.

Run with:
```sh
cargo test -p eggserve-static
cargo test -p eggserve-core
```

## Fuzz targets

11 fuzz targets in `fuzz/fuzz_targets/`, one `[[bin]]` per target in `fuzz/Cargo.toml`:

| Target | Corpus dir | What it exercises | Key invariants | Corpus replay |
|--------|------------|-------------------|----------------|---------------|
| `request_target` | `request_target` | `RequestTarget::parse`, `ConfinedPath::from_path_component`, `validate_request_target`, `RequestHead` construction | No `..`/`.` components, no NUL, starts with `/`, valid request target, valid method/target/version | `corpus_replay_request_target` |
| `percent_decode` | `percent_decode` | `percent_decode` | No NUL in output, bounded decoded length, valid UTF-8 | `corpus_replay_percent_decode` |
| `path_components` | `path_components` | `split_components`/`validate_components` | No `..`/`.` accepted, no slash/backslash in component, starts with `/` | `corpus_replay_path_components` |
| `validate_method` | `validate_method` | `Method::new`, `validate_method`/`validate_request_body` | Valid method names, GET/HEAD only for read-only, bodies rejected for read-only methods | `corpus_replay_validate_method` |
| `range_header` | `range_header` | `evaluate_range_header` | Satisfiable range within file size, start ≤ end, end < file_size | `corpus_replay_range_header` |
| `if_none_match` | `if_none_match` | `evaluate_if_none_match` | Wildcard always matches, matching ETag returns true | `corpus_replay_if_none_match` |
| `platform_component` | `platform_component` | `check_component`/`has_windows_drive_prefix`/`is_windows_reserved_name` | Drive prefix requires `X:` pattern, clean components pass | `corpus_replay_platform_component` |
| `fuzz_header_block` | `fuzz_header_block` | `HeaderName`, `HeaderValue`, `HeaderBlock` operations | Token-only names, no CTL/DEL in values, valid header operations | `corpus_replay_header_block` |
| `fuzz_normalize_response` | `fuzz_normalize_response` | `StatusCode`, `Response` builder, `normalize_response`, Content-Length reconciliation | Range 100–599, body-forbidden empty, HEAD carries no payload, TE stripped, CL correct | `corpus_replay_normalize_response` |
| `fuzz_request_body` | `fuzz_request_body` | `RequestBody` state machine | One-shot enforcement, no double-consume | `corpus_replay_request_body` |
| `fuzz_directory_buffer` | `fuzz_directory_buffer` | `eggserve_static::parse_directory_buffer` (NT `FILE_ID_BOTH_DIR_INFO` records) | Entry budget bounds the listing, a zero budget is an empty listing, dotfile flag matches the name, `.`/`..` never leak, parse is deterministic, a rejected buffer stays rejected under a wider budget, each seed produces its recorded outcome | `corpus_replay_directory_buffer` |

Build every target without running it:

```sh
cd fuzz
cargo check --bins        # all 11 targets must compile
cargo clippy --bins -- -D warnings
```

Run a single target:
```sh
cd fuzz
cargo fuzz run range_header -- -max_total_time=300  # 5 minutes
```

### The directory-buffer target

The `FILE_ID_BOTH_DIR_INFO` record layout is a wire format, not a Windows API binding, so the parser
that owns it lives in a platform-neutral module (`crates/eggserve-static/src/fs/dirbuf.rs`) and is
re-exported publicly by `eggserve-static`:

```rust
pub use fs::dirbuf::{
    parse_directory_buffer, DirBufParseError, DirectoryEntryKind, DirectoryEntryRecord,
};
```

The target therefore compiles and runs on every platform, and the parser's unit tests run on every
platform too (they used to be `cfg(windows)`-only). The Windows FFI module
(`crates/eggserve-static/src/fs/windows.rs`) imports the parser and the record types from that
module; it does not reimplement them.

#### The authoritative record layout

The record layout is written down once, in the parser's module documentation (`fs/dirbuf.rs`), as the
NT `FILE_ID_BOTH_DIR_INFO` layout (`winnt.h`, mirrored by `windows-sys` 0.61.2
`Win32::Storage::FileSystem::FILE_ID_BOTH_DIR_INFO`):

| Offset | Size | Field | Type |
|--------|------|-------|------|
| 0 | 4 | `NextEntryOffset` | `u32` |
| 4 | 4 | `FileIndex` | `u32` |
| 8 | 8 | `CreationTime` | `i64` |
| 16 | 8 | `LastAccessTime` | `i64` |
| 24 | 8 | `LastWriteTime` | `i64` |
| 32 | 8 | `ChangeTime` | `i64` |
| 40 | 8 | `EndOfFile` | `i64` |
| 48 | 8 | `AllocationSize` | `i64` |
| 56 | 4 | `FileAttributes` | `u32` |
| 60 | 4 | `FileNameLength` | `u32` |
| 64 | 4 | `EaSize` | `u32` |
| 68 | 1 | `ShortNameLength` | `i8` |
| 70 | 24 | `ShortName` | `[u16; 12]` |
| 96 | 8 | `FileId` | `i64` |
| 104 | 2·N | `FileName` | `[u16; N]` |

The fixed header is 104 bytes and `FileName` starts at offset 104. The parser's constants are the
single source of truth for those offsets; the Windows FFI module reads the same record at the same
offsets instead of declaring a second copy of the structure. The parser's entry budget bounds the
number of entries *returned*, so a budget of zero yields an empty listing.

#### The directory-buffer corpus

`fuzz/corpus/fuzz_directory_buffer/` is generated from that layout by
`crates/eggserve-static/tests/directory_buffer_corpus.rs`, which also pins each seed's expected
outcome. The corpus holds inputs only — never expectations. Regenerate it with:

```sh
cargo test -p eggserve-static --test directory_buffer_corpus -- --ignored
```

The same test file asserts, in routine CI, that the committed corpus is byte-identical to the
generator's output and that no unrecorded file is committed, so the corpus cannot drift from its
generator. The seeds cover the happy path (a valid single entry, a multi-entry buffer, a directory
entry, a reparse-point entry, a dotfile entry) and every rejection (a truncated header, an odd
file-name length, an over-long file name, an offset overflow, an offset underflow, invalid UTF-16),
plus an empty buffer, the `.`/`..` pseudo-entries and a pseudo-random blob.

The CI replay (`crates/eggserve-core/tests/corpus_replay.rs::corpus_replay_directory_buffer`) asserts
each seed's recorded outcome and that the corpus is not vacuous (at least one seed decodes to a
non-empty name).

## Seed corpora

`fuzz/corpus/<target>/` contains hand-crafted seeds for each target. Seeds cover:
- Normal valid inputs
- Edge cases (empty, max-length, boundary values)
- Malformed inputs (truncated, special chars, traversal attempts)
- Regression inputs from existing test suites

Corpus filenames must be portable across supported checkout platforms; avoid
Windows device names such as `nul` even when the seed contents are valid.

Seeds are automatically loaded by libFuzzer at startup.

Corpus files are **inputs, not expectations**: a seed is a byte string handed to the target. Never
edit a seed to make an assertion pass — a seed that trips an assertion is a finding.

## CI integration

### Normal CI

Property tests run as part of `cargo test` in the standard CI workflow (`.github/workflows/ci.yml`).

### Fuzz build gate

The standard CI workflow also compiles the fuzz crate, so a target that no longer builds (or no
longer matches the current API) fails CI instead of silently rotting:

```sh
cd fuzz && cargo check --bins
```

The fuzz crate is excluded from the main workspace and has its own lockfile; the gate never touches
`Cargo.lock` in the repository root.

### Corpus regression

Corpus regression is part of the standard CI workflow. `cargo test -p eggserve-core --test
corpus_replay` replays every committed corpus input through its target logic deterministically,
failing on panic or invariant violation.

The replay is the **deterministic** half of the fuzzing story: it runs on every commit, over a
closed, committed input set, with no randomness and no time budget. The libFuzzer targets are the
**exploratory** half: they search an unbounded input space for inputs nobody has thought of, but
they are scheduled or run manually, and a crash only protects the project once the minimal input
has been minimized and committed as a seed.

The two halves are complementary, and the replay mirrors the target invariants:

- Every libFuzzer target has a matching `corpus_replay_*` test, and every replay test names the
  target it covers.
- A replay asserts the corpus directory exists. A replay that iterates over zero inputs is a false
  green, so a missing or relocated corpus fails the test instead of passing it vacuously.
- A repaired target and its replay must assert the same invariant against the same authority. If a
  target's oracle and the production contract disagree, the *oracle* is what gets fixed — never the
  production behaviour, and never by weakening the assertion.

### Fuzzing workflow

Fuzzing is run manually (not in CI). Each target can be run via `cargo fuzz run <target>` for 60 seconds or longer.

## Failure handling

When a fuzz target finds a crash:

1. **Minimize**: `cargo fuzz merge <target>` to reduce the input
2. **Reproduce**: Add the minimal input to `fuzz/corpus/<target>/` as a regression seed
3. **Classify**: Determine if the failure is a security issue (path escape, OOB, panic) or a correctness issue
4. **Fix**: Patch the root cause in the affected module
5. **Verify**: Re-run the fuzz target to confirm the fix; the corpus seed and the corpus replay prevent regression

A crash in a target that only reproduces on a corpus seed is usually an **oracle defect**, not a
production defect: check that the target asserts the production contract before changing
production code. Production defects found this way are recorded as findings and fixed in a separate
corrective change — never inside a harness repair.

## Adding a new fuzz target

1. Create `fuzz/fuzz_targets/<name>.rs` with a `fuzz_target!(|data: &[u8]| { ... })` entry point
2. Parse the fuzz input (typically `std::str::from_utf8` or manual splitting)
3. Call the target function and assert invariants on the output — every assertion must be able to
   fail, and no assertion may be a tautology (a variant match over every enum variant, or
   `!x.is_empty() || x.is_empty()`, asserts nothing)
4. Add a `[[bin]]` section to `fuzz/Cargo.toml`
5. Create `fuzz/corpus/<name>/` with seed files
6. Add a matching `corpus_replay_*` test in `crates/eggserve-core/tests/corpus_replay.rs` that
   asserts the same invariant over the same seeds
7. Verify the new target works with `cargo fuzz run <name>`
