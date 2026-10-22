#![no_main]
// Cross-platform fuzzer for the directory-information buffer parser.
//
// The `FILE_ID_BOTH_DIR_INFO` record layout is a wire format, not a Windows
// API binding, so the parser that owns it lives in a platform-neutral module
// (`eggserve_static::fs::dirbuf`, re-exported from `eggserve-static`). This
// target therefore drives the real authority on every platform and no longer
// needs a `cfg(windows)` gate.
//
// The parser is the *only* implementation under test here: the target never
// re-derives a name, an attribute bit, or a record boundary of its own.

use eggserve_static::DirectoryEntryRecord;
use eggserve_static::{parse_directory_buffer, DirBufParseError, DirectoryEntryKind};
use libfuzzer_sys::fuzz_target;

/// Narrowest entry budget: stops after the first record.
const NARROW_BUDGET: usize = 1;
/// Wide entry budget: lets the parser walk the whole buffer.
const WIDE_BUDGET: usize = 64;

/// Comparable projection of a parsed record.
///
/// `DirectoryEntryRecord` intentionally does not derive `PartialEq`; this
/// projection compares every field the parser publishes.
fn signature(
    entries: &[DirectoryEntryRecord],
) -> Vec<(String, DirectoryEntryKind, Option<u64>, bool)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                entry.kind,
                entry.file_id,
                entry.hidden_or_dot,
            )
        })
        .collect()
}

fuzz_target!(|data: &[u8]| {
    // Entry budget derived from the input so different corpus entries drive
    // different bounds. It is at least 1 for a non-empty buffer: the parser
    // checks its budget *after* pushing an entry, so a budget of 0 would
    // still yield one entry (Plan 310 finding F1).
    let max_entries = if data.is_empty() {
        0
    } else {
        (data[0] as usize % 64) + 1
    };

    // === Happy path: the parsed listing must be well-formed ===

    let parsed = parse_directory_buffer(data, max_entries);
    match &parsed {
        Ok(entries) => {
            // The entry budget bounds the number of parsed entries.
            assert!(
                entries.len() <= max_entries,
                "entries {} exceeds max {}",
                entries.len(),
                max_entries
            );
            for entry in entries {
                // The dotfile flag is derived from the parsed name, so the
                // dotfile policy sees exactly the names the parser produced.
                assert_eq!(
                    entry.hidden_or_dot,
                    entry.name.starts_with('.'),
                    "hidden_or_dot mismatch for {:?}",
                    entry.name
                );
                // The parser filters the `.` and `..` pseudo-entries: a
                // listing must never carry them.
                assert!(
                    entry.name != "." && entry.name != "..",
                    "pseudo-entry {:?} leaked into the listing",
                    entry.name
                );
                // Every published entry carries the stable identity read
                // from the record's FileId field.
                assert!(
                    entry.file_id.is_some(),
                    "entry {:?} lost its file identity",
                    entry.name
                );
            }
        }
        Err(error) => {
            // A rejection is one of the parser's documented error kinds.
            // Matching every variant would be trivially true (the enum has
            // no other variants), so the variant-to-cause mapping is
            // asserted by the parser's unit tests in `eggserve-static`
            // instead. Here the *rejection* itself is what matters: see the
            // budget-independence check below.
            let _: &DirBufParseError = error;
        }
    }

    // === The parse is a pure function of its input ===

    // The same bytes must always produce the same listing: a listing that
    // changed between two reads of the same buffer could flip the dotfile
    // or reparse policy for the same name.
    let repeat = parse_directory_buffer(data, max_entries);
    match (&parsed, &repeat) {
        (Ok(first), Ok(second)) => assert_eq!(
            signature(first),
            signature(second),
            "directory buffer parse is not deterministic"
        ),
        (Err(first), Err(second)) => assert_eq!(
            std::mem::discriminant(first),
            std::mem::discriminant(second),
            "directory buffer rejection is not deterministic"
        ),
        _ => panic!(
            "directory buffer parse is not deterministic: {:?} vs {:?}",
            parsed, repeat
        ),
    };

    // === The entry budget bounds parsing; it never decides validity ===

    // A buffer rejected under the narrowest budget must be rejected under
    // every wider budget as well: the budget limits how much of the buffer
    // is parsed, it must not be usable to smuggle malformed bytes through.
    let narrow = parse_directory_buffer(data, NARROW_BUDGET);
    if narrow.is_err() {
        for budget in [2usize, 8, WIDE_BUDGET] {
            assert!(
                parse_directory_buffer(data, budget).is_err(),
                "buffer rejected with a budget of {} was accepted with a budget of {}",
                NARROW_BUDGET,
                budget
            );
        }
    }

    // === An empty buffer is an empty listing, not an error ===

    if data.is_empty() {
        let empty = parse_directory_buffer(data, NARROW_BUDGET)
            .expect("an empty buffer is an empty listing, not an error");
        assert!(
            empty.is_empty(),
            "an empty buffer produced {} entries",
            empty.len()
        );
    }

    // === Boundary budgets: panic-freedom only ===

    // A budget of 0 is not asserted on the entry count: the parser checks
    // its budget after pushing an entry, so 0 still yields one entry
    // (Plan 310 finding F1). The unbounded budget must stay bounded by the
    // buffer size.
    let _ = parse_directory_buffer(data, 0);
    let unbounded = parse_directory_buffer(data, usize::MAX);
    if let Ok(entries) = unbounded {
        assert!(
            entries.len() <= data.len(),
            "unbounded parse produced {} entries from {} bytes",
            entries.len(),
            data.len()
        );
    }
});
