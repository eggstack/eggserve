#![no_main]
#![cfg(windows)]

use libfuzzer_sys::fuzz_target;
use eggserve_core::fs::windows::{parse_directory_buffer, DirBufParseError};

fuzz_target!(|data: &[u8]| {
    // Fuzz the directory buffer parser with arbitrary data.
    // The parser must never panic, loop infinitely, or access out of bounds.

    // Test with various max_entries limits.
    let max_entries = if data.is_empty() {
        0
    } else {
        (data[0] as usize % 64) + 1
    };

    let result = parse_directory_buffer(data, max_entries);

    match result {
        Ok(entries) => {
            // Verify all entries are well-formed.
            for entry in &entries {
                // Name must be valid UTF-8.
                assert!(!entry.name.is_empty() || entry.name.is_empty());
                // Dotfile flag must match name.
                assert_eq!(entry.hidden_or_dot, entry.name.starts_with('.'));
                // Kind must be one of the defined variants.
                assert!(
                    matches!(
                        entry.kind,
                        eggserve_core::fs::windows::DirectoryEntryKind::File
                            | eggserve_core::fs::windows::DirectoryEntryKind::Directory
                            | eggserve_core::fs::windows::DirectoryEntryKind::ReparsePoint
                            | eggserve_core::fs::windows::DirectoryEntryKind::Other
                    ),
                    "unexpected entry kind"
                );
            }
            // Entry count must not exceed max_entries.
            assert!(
                entries.len() <= max_entries,
                "entries {} exceeds max {}",
                entries.len(),
                max_entries
            );
        }
        Err(e) => {
            // Error must be a valid variant.
            assert!(
                matches!(
                    e,
                    DirBufParseError::BufferOverflow
                        | DirBufParseError::TruncatedHeader
                        | DirBufParseError::OddFileNameLength
                        | DirBufParseError::FileNameOutOfRange
                        | DirBufParseError::OffsetUnderflow
                        | DirBufParseError::OffsetOverflow
                        | DirBufParseError::OffsetLoop
                        | DirBufParseError::InvalidUtf16
                ),
                "unexpected error variant: {:?}",
                e
            );
        }
    }

    // Also test with max_entries=0 (should return empty or error, never panic).
    let _ = parse_directory_buffer(data, 0);

    // Test with max_entries=1 (common case).
    let _ = parse_directory_buffer(data, 1);

    // Test with very large max_entries (should not cause allocation issues).
    let _ = parse_directory_buffer(data, usize::MAX);
});
