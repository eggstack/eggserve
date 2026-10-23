//! Plan 316: the `fuzz_directory_buffer` corpus, its generator, and its
//! per-seed expectations.
//!
//! The corpus holds **inputs only** — never expectations. This file is the
//! generator: it carries the authoritative NT `FILE_ID_BOTH_DIR_INFO` layout,
//! builds every seed from it, and pins the outcome each seed must produce.
//!
//! Three tests keep the corpus honest:
//!
//! - `committed_corpus_matches_the_generator` asserts the committed files are
//!   byte-identical to the generator's output and that no unrecorded file is
//!   committed. It runs in routine CI, so the corpus cannot drift.
//! - `every_seed_has_its_expected_outcome` asserts each seed decodes to the
//!   entries it was built to contain, or is rejected with the documented
//!   error variant.
//! - `regenerate_committed_corpus` rewrites the committed corpus. It is
//!   `#[ignore]`d so it never runs by accident:
//!
//!   ```sh
//!   cargo test -p eggserve-static --test directory_buffer_corpus -- --ignored
//!   ```
//!
//! The layout constants below are transcribed from the NT headers rather than
//! imported from the crate under test. The corpus must therefore prove the
//! parser agrees with the *documented NT layout*, not merely with itself.

use std::fs;
use std::path::Path;

use eggserve_static::{parse_directory_buffer, DirBufParseError, DirectoryEntryKind};

// The corpus lives at the repository root (`fuzz/corpus`), two levels above
// this crate's manifest directory.
const CORPUS_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fuzz/corpus/fuzz_directory_buffer"
);

// ── The authoritative NT layout ─────────────────────────────────────────────

/// The NT `FILE_ID_BOTH_DIR_INFO` record layout: `(offset, size, field)`.
///
/// Transcribed from `winnt.h`, mirrored by `windows-sys` 0.61.2
/// (`Win32::Storage::FileSystem::FILE_ID_BOTH_DIR_INFO`). The record is a
/// fixed 104-byte header followed by the variable-length UTF-16 file name.
const AUTHORITATIVE_LAYOUT: &[(usize, usize, &str)] = &[
    (0, 4, "NextEntryOffset"),
    (4, 4, "FileIndex"),
    (8, 8, "CreationTime"),
    (16, 8, "LastAccessTime"),
    (24, 8, "LastWriteTime"),
    (32, 8, "ChangeTime"),
    (40, 8, "EndOfFile"),
    (48, 8, "AllocationSize"),
    (56, 4, "FileAttributes"),
    (60, 4, "FileNameLength"),
    (64, 4, "EaSize"),
    (68, 1, "ShortNameLength"),
    (70, 24, "ShortName"),
    (96, 8, "FileId"),
    (104, 0, "FileName (variable)"),
];

/// Byte offset of `NextEntryOffset`.
const NEXT_ENTRY_OFFSET: usize = 0;
/// Byte offset of `FileAttributes`.
const FILE_ATTRIBUTES: usize = 56;
/// Byte offset of `FileNameLength`.
const FILE_NAME_LENGTH: usize = 60;
/// Byte offset of `FileId`.
const FILE_ID: usize = 96;
/// Byte offset of `FileName`.
const FILE_NAME: usize = 104;
/// Fixed header size, before the variable-length `FileName`.
const HEADER_SIZE: usize = FILE_NAME;

/// NTFS file attribute bit: a regular file (`FILE_ATTRIBUTE_NORMAL`).
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
/// NTFS file attribute bit: the entry is a directory.
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
/// NTFS file attribute bit: the entry is a reparse point.
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// The entry budget the per-seed expectations are checked against.
///
/// It is fixed rather than derived from the seed's first byte (which is the
/// low byte of `NextEntryOffset`) so that an expectation describes the seed
/// itself and not the budget the fuzz target happens to pick for it.
const EXPECTATION_BUDGET: usize = 64;

// ── The seed model ──────────────────────────────────────────────────────────

/// What a seed must produce.
enum Expectation {
    /// The buffer decodes into exactly these entries.
    Entries(Vec<ExpectedEntry>),
    /// The buffer is rejected with this `DirBufParseError` variant.
    Rejected(&'static str),
}

/// One decoded directory entry.
struct ExpectedEntry {
    name: &'static str,
    kind: DirectoryEntryKind,
    file_id: u64,
    hidden_or_dot: bool,
}

const fn entry(
    name: &'static str,
    kind: DirectoryEntryKind,
    file_id: u64,
    hidden_or_dot: bool,
) -> ExpectedEntry {
    ExpectedEntry {
        name,
        kind,
        file_id,
        hidden_or_dot,
    }
}

/// One committed corpus seed.
struct Seed {
    name: &'static str,
    bytes: Vec<u8>,
    expectation: Expectation,
}

// ── The generator ───────────────────────────────────────────────────────────

/// Builds one `FILE_ID_BOTH_DIR_INFO` record in the authoritative layout.
///
/// The record is the fixed 104-byte header followed by the UTF-16LE file
/// name, padded to the 8-byte alignment the kernel uses. `NextEntryOffset` is
/// left at zero; [`chain`] fills it in.
fn build_record(name: &str, attributes: u32, file_id: u64) -> Vec<u8> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = name_utf16.len() * 2;
    let size = HEADER_SIZE + name_bytes;
    let aligned = (size + 7) & !7;
    let mut buf = vec![0u8; aligned];

    buf[FILE_ATTRIBUTES..FILE_ATTRIBUTES + 4].copy_from_slice(&attributes.to_le_bytes());
    buf[FILE_NAME_LENGTH..FILE_NAME_LENGTH + 4].copy_from_slice(&(name_bytes as u32).to_le_bytes());
    buf[FILE_ID..FILE_ID + 8].copy_from_slice(&file_id.to_le_bytes());
    for (index, unit) in name_utf16.iter().enumerate() {
        let at = FILE_NAME + index * 2;
        buf[at..at + 2].copy_from_slice(&unit.to_le_bytes());
    }

    buf
}

/// Chains records into one buffer.
///
/// `NextEntryOffset` is relative to the current record, so each record points
/// at the aligned size of itself; the last record's offset stays zero.
fn chain(records: Vec<Vec<u8>>) -> Vec<u8> {
    let count = records.len();
    let mut buf = Vec::new();
    for (index, mut record) in records.into_iter().enumerate() {
        if index + 1 < count {
            let next = record.len() as u32;
            record[NEXT_ENTRY_OFFSET..NEXT_ENTRY_OFFSET + 4].copy_from_slice(&next.to_le_bytes());
        }
        buf.extend_from_slice(&record);
    }
    buf
}

/// A tiny deterministic PRNG, so the `random` seed is reproducible without a
/// wall of hex in the source. The seed is fixed, so the bytes are fixed.
fn pseudo_random_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            // xorshift64*
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u8
        })
        .collect()
}

/// Every seed the corpus holds.
///
/// The happy path is covered by the single-entry, multi-entry, directory,
/// reparse-point and dotfile seeds; every rejection is covered by the
/// truncated-header, odd-name-length, over-long-name, offset-overflow,
/// offset-underflow and invalid-UTF-16 seeds; the pseudo-random seed keeps a
/// malformed blob in the corpus.
fn corpus() -> Vec<Seed> {
    vec![
        Seed {
            name: "empty",
            bytes: Vec::new(),
            expectation: Expectation::Entries(Vec::new()),
        },
        Seed {
            name: "single_file",
            bytes: build_record("hello.txt", FILE_ATTRIBUTE_NORMAL, 42),
            expectation: Expectation::Entries(vec![entry(
                "hello.txt",
                DirectoryEntryKind::File,
                42,
                false,
            )]),
        },
        Seed {
            name: "single_dir",
            bytes: build_record("subdir", FILE_ATTRIBUTE_DIRECTORY, 10),
            expectation: Expectation::Entries(vec![entry(
                "subdir",
                DirectoryEntryKind::Directory,
                10,
                false,
            )]),
        },
        Seed {
            name: "reparse",
            bytes: build_record("link", FILE_ATTRIBUTE_REPARSE_POINT, 20),
            expectation: Expectation::Entries(vec![entry(
                "link",
                DirectoryEntryKind::ReparsePoint,
                20,
                false,
            )]),
        },
        Seed {
            name: "dotfile",
            bytes: build_record(".hidden", FILE_ATTRIBUTE_NORMAL, 30),
            expectation: Expectation::Entries(vec![entry(
                ".hidden",
                DirectoryEntryKind::File,
                30,
                true,
            )]),
        },
        Seed {
            name: "multi_entries",
            bytes: chain(vec![
                build_record("a.txt", FILE_ATTRIBUTE_NORMAL, 1),
                build_record("sub", FILE_ATTRIBUTE_DIRECTORY, 2),
                build_record("link", FILE_ATTRIBUTE_REPARSE_POINT, 3),
                build_record(".hidden", FILE_ATTRIBUTE_NORMAL, 4),
            ]),
            expectation: Expectation::Entries(vec![
                entry("a.txt", DirectoryEntryKind::File, 1, false),
                entry("sub", DirectoryEntryKind::Directory, 2, false),
                entry("link", DirectoryEntryKind::ReparsePoint, 3, false),
                entry(".hidden", DirectoryEntryKind::File, 4, true),
            ]),
        },
        Seed {
            name: "dot_dotdot",
            bytes: chain(vec![
                build_record(".", FILE_ATTRIBUTE_DIRECTORY, 1),
                build_record("..", FILE_ATTRIBUTE_DIRECTORY, 2),
            ]),
            // The `.` and `..` pseudo-entries never reach a listing.
            expectation: Expectation::Entries(Vec::new()),
        },
        Seed {
            name: "truncated_header",
            bytes: vec![0u8; 4],
            expectation: Expectation::Rejected("TruncatedHeader"),
        },
        Seed {
            name: "odd_name_length",
            bytes: {
                let mut record = build_record("ab", FILE_ATTRIBUTE_NORMAL, 1);
                record[FILE_NAME_LENGTH..FILE_NAME_LENGTH + 4].copy_from_slice(&5u32.to_le_bytes());
                record
            },
            expectation: Expectation::Rejected("OddFileNameLength"),
        },
        Seed {
            name: "overlong_name",
            bytes: {
                // The name claims far more bytes than the record holds.
                let mut record = build_record("ab", FILE_ATTRIBUTE_NORMAL, 1);
                record[FILE_NAME_LENGTH..FILE_NAME_LENGTH + 4]
                    .copy_from_slice(&0x0001_0000u32.to_le_bytes());
                record
            },
            expectation: Expectation::Rejected("FileNameOutOfRange"),
        },
        Seed {
            name: "offset_overflow",
            bytes: {
                let mut record = build_record("a.txt", FILE_ATTRIBUTE_NORMAL, 1);
                record[NEXT_ENTRY_OFFSET..NEXT_ENTRY_OFFSET + 4]
                    .copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
                record
            },
            expectation: Expectation::Rejected("OffsetOverflow"),
        },
        Seed {
            name: "offset_underflow",
            bytes: {
                // The advance lands inside the current record's fixed header:
                // a backward or overlapping move.
                let mut record = build_record("a.txt", FILE_ATTRIBUTE_NORMAL, 1);
                record[NEXT_ENTRY_OFFSET..NEXT_ENTRY_OFFSET + 4]
                    .copy_from_slice(&8u32.to_le_bytes());
                record
            },
            expectation: Expectation::Rejected("OffsetUnderflow"),
        },
        Seed {
            name: "invalid_utf16",
            bytes: {
                // The name carries an unpaired surrogate.
                let mut record = build_record("A", FILE_ATTRIBUTE_NORMAL, 1);
                record[FILE_NAME..FILE_NAME + 2].copy_from_slice(&0xD800u16.to_le_bytes());
                record
            },
            expectation: Expectation::Rejected("InvalidUtf16"),
        },
        Seed {
            name: "random",
            bytes: pseudo_random_bytes(0x9E37_79B9_7F4A_7C15, 256),
            expectation: Expectation::Rejected("FileNameOutOfRange"),
        },
    ]
}

// ── Assertions ──────────────────────────────────────────────────────────────

fn corpus_dir() -> std::path::PathBuf {
    Path::new(CORPUS_DIR).to_path_buf()
}

/// The `DirBufParseError` variant's name, for expectation matching.
fn variant_name(error: &DirBufParseError) -> &'static str {
    match error {
        DirBufParseError::BufferOverflow => "BufferOverflow",
        DirBufParseError::TruncatedHeader => "TruncatedHeader",
        DirBufParseError::OddFileNameLength => "OddFileNameLength",
        DirBufParseError::FileNameOutOfRange => "FileNameOutOfRange",
        DirBufParseError::OffsetUnderflow => "OffsetUnderflow",
        DirBufParseError::OffsetOverflow => "OffsetOverflow",
        DirBufParseError::OffsetLoop => "OffsetLoop",
        DirBufParseError::InvalidUtf16 => "InvalidUtf16",
    }
}

/// Asserts one seed produces its expected outcome.
fn assert_expected_outcome(seed: &Seed) {
    match &seed.expectation {
        Expectation::Entries(expected) => {
            let parsed = parse_directory_buffer(&seed.bytes, EXPECTATION_BUDGET)
                .unwrap_or_else(|error| panic!("[{}] expected a parse, got {error:?}", seed.name));
            assert_eq!(
                parsed.len(),
                expected.len(),
                "[{}] decoded {} entries, expected {}",
                seed.name,
                parsed.len(),
                expected.len()
            );
            for (actual, expected) in parsed.iter().zip(expected.iter()) {
                assert_eq!(
                    actual.name, expected.name,
                    "[{}] entry name mismatch",
                    seed.name
                );
                assert_eq!(
                    actual.kind, expected.kind,
                    "[{}] entry kind mismatch for {:?}",
                    seed.name, actual.name
                );
                assert_eq!(
                    actual.file_id,
                    Some(expected.file_id),
                    "[{}] entry identity mismatch for {:?}",
                    seed.name,
                    actual.name
                );
                assert_eq!(
                    actual.hidden_or_dot, expected.hidden_or_dot,
                    "[{}] dotfile flag mismatch for {:?}",
                    seed.name, actual.name
                );
            }
        }
        Expectation::Rejected(expected_variant) => {
            let error = match parse_directory_buffer(&seed.bytes, EXPECTATION_BUDGET) {
                Ok(entries) => panic!(
                    "[{}] expected a rejection, decoded {} entries",
                    seed.name,
                    entries.len()
                ),
                Err(error) => error,
            };
            assert_eq!(
                variant_name(&error),
                *expected_variant,
                "[{}] rejected with the wrong variant",
                seed.name
            );
        }
    }
}

/// The layout table is the authoritative NT layout, and the generator writes
/// the fields at the offsets the table declares.
#[test]
fn the_layout_table_is_the_authoritative_nt_layout() {
    let offset_of = |field: &str| {
        AUTHORITATIVE_LAYOUT
            .iter()
            .find(|(_, _, name)| *name == field)
            .map(|(offset, _, _)| *offset)
            .unwrap_or_else(|| panic!("{field} is missing from the layout table"))
    };

    assert_eq!(offset_of("NextEntryOffset"), NEXT_ENTRY_OFFSET);
    assert_eq!(offset_of("FileAttributes"), FILE_ATTRIBUTES);
    assert_eq!(offset_of("FileNameLength"), FILE_NAME_LENGTH);
    assert_eq!(offset_of("FileId"), FILE_ID);
    assert_eq!(offset_of("FileName (variable)"), FILE_NAME);

    // The fields never overlap.
    let mut previous_end = 0usize;
    for (offset, size, name) in AUTHORITATIVE_LAYOUT {
        if *name == "FileName (variable)" {
            break;
        }
        assert!(
            *offset >= previous_end,
            "{name} overlaps the previous field"
        );
        previous_end = offset + size;
    }
    assert_eq!(
        previous_end, 104,
        "the fixed fields end where the variable-length name starts"
    );

    // The only gaps are the alignment padding that precedes `ShortName` (a
    // 2-byte-aligned array) and `FileId` (8-byte aligned).
    assert_eq!(
        offset_of("ShortName") - (offset_of("ShortNameLength") + 1),
        1,
        "ShortName is 2-byte aligned"
    );
    assert_eq!(
        offset_of("FileId") - (offset_of("ShortName") + 24),
        2,
        "the FileId is 8-byte aligned"
    );
    assert_eq!(FILE_ID, 96, "the FileId is 8-byte aligned");
    assert_eq!(
        FILE_ID + 8,
        HEADER_SIZE,
        "the header ends where the name starts"
    );
    assert_eq!(HEADER_SIZE, 104);
}

/// The committed corpus is byte-identical to the generator's output.
#[test]
fn committed_corpus_matches_the_generator() {
    let dir = corpus_dir();
    assert!(dir.exists(), "corpus directory missing: {}", dir.display());

    let seeds = corpus();
    for seed in &seeds {
        let path = dir.join(seed.name);
        let committed = fs::read(&path)
            .unwrap_or_else(|error| panic!("[{}] committed seed unreadable: {error}", seed.name));
        assert_eq!(
            committed.len(),
            seed.bytes.len(),
            "[{}] committed seed is {} bytes, the generator produces {}",
            seed.name,
            committed.len(),
            seed.bytes.len()
        );
        assert_eq!(
            committed, seed.bytes,
            "[{}] committed seed differs from the generator",
            seed.name
        );
    }

    for entry in fs::read_dir(&dir).expect("read corpus directory") {
        let name = entry
            .expect("corpus directory entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        assert!(
            seeds.iter().any(|seed| seed.name == name),
            "[{name}] is committed but the generator does not produce it"
        );
    }
}

/// Every seed produces its expected outcome.
#[test]
fn every_seed_has_its_expected_outcome() {
    for seed in corpus() {
        assert_expected_outcome(&seed);
    }
}

/// The corpus is not vacuous: at least one seed decodes to a non-empty name.
#[test]
fn the_corpus_decodes_at_least_one_non_empty_name() {
    let decoded: Vec<String> = corpus()
        .iter()
        .filter_map(
            |seed| match parse_directory_buffer(&seed.bytes, EXPECTATION_BUDGET) {
                Ok(entries) => entries.first().map(|entry| entry.name.clone()),
                Err(_) => None,
            },
        )
        .filter(|name| !name.is_empty())
        .collect();
    assert!(
        !decoded.is_empty(),
        "no corpus seed decodes to a non-empty name"
    );
}

/// Regenerates the committed corpus from the generator.
///
/// ```sh
/// cargo test -p eggserve-static --test directory_buffer_corpus -- --ignored
/// ```
#[test]
#[ignore = "rewrites the committed corpus; run it explicitly"]
fn regenerate_committed_corpus() {
    let dir = corpus_dir();
    fs::create_dir_all(&dir).expect("create the corpus directory");

    let seeds = corpus();
    for seed in &seeds {
        fs::write(dir.join(seed.name), &seed.bytes)
            .unwrap_or_else(|error| panic!("[{}] seed unwritable: {error}", seed.name));
    }

    for entry in fs::read_dir(&dir).expect("read corpus directory") {
        let name = entry
            .expect("corpus directory entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if !seeds.iter().any(|seed| seed.name == name) {
            fs::remove_file(dir.join(name)).expect("remove the stale seed");
        }
    }
}
