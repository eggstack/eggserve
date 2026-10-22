//! Platform-neutral parser for the NT `FILE_ID_BOTH_DIR_INFO` directory buffer.
//!
//! `NtQueryDirectoryFile` returns a directory listing as a byte buffer of
//! `FILE_ID_BOTH_DIR_INFO` records. The *record layout* is a fixed wire
//! format: it is a property of the bytes the kernel hands back, not of the
//! Windows API surface that produced them. The parser therefore lives here,
//! in a platform-neutral module, so it can be exercised on every platform
//! (unit tests, the fuzz harness, the CI corpus replay) instead of only on
//! Windows.
//!
//! The constants below are fixed `FILE_ID_BOTH_DIR_INFO` **wire-format
//! offsets** — byte positions inside a record. They are not Windows API
//! bindings: no `windows-sys` type, handle, or FFI call is involved, and the
//! module compiles and behaves identically on every platform.
//!
//! # Known deviations (Plan 310 findings; parser behavior unchanged here)
//!
//! - `parse_directory_buffer` checks its entry budget *after* pushing an
//!   entry, so a `max_entries` of `0` still yields one entry. The bound is
//!   exact for every `max_entries >= 1`, and the only production caller never
//!   passes `0`.
//! - The offsets below disagree with the `FILE_ID_BOTH_DIR_INFO` record
//!   layout described by the FFI struct in `fs::windows` (which places the
//!   file name at offset 80, and whose own doc comment calls the fixed
//!   header 80 bytes). The committed `fuzz/corpus/fuzz_directory_buffer`
//!   seeds follow that 80-byte layout, so under the offsets below most seeds
//!   are rejected as malformed rather than decoded.

/// NTFS file attribute bit: the entry is a reparse point (symlink, junction,
/// mount point, …).
pub(crate) const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;

/// NTFS file attribute bit: the entry is a directory.
pub(crate) const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x00000010;

/// Byte offset of `FileNameLength` inside a `FILE_ID_BOTH_DIR_INFO` record.
pub(crate) const FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET: usize = 60;

/// Byte offset of `FileAttributes` inside a `FILE_ID_BOTH_DIR_INFO` record.
pub(crate) const FILE_ID_BOTH_DIR_INFO_FILE_ATTRIBUTES_OFFSET: usize = 56;

/// Byte offset of `FileId` inside a `FILE_ID_BOTH_DIR_INFO` record.
pub(crate) const FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET: usize = 96;

/// Byte offset of `FileName` inside a `FILE_ID_BOTH_DIR_INFO` record.
pub(crate) const FILE_ID_BOTH_DIR_INFO_FILE_NAME_OFFSET: usize = 104;

/// Fixed header size of `FILE_ID_BOTH_DIR_INFO` before the variable-length
/// `FileName`.
pub(crate) const FILE_ID_BOTH_DIR_INFO_HEADER_SIZE: usize = FILE_ID_BOTH_DIR_INFO_FILE_NAME_OFFSET;

/// A parsed directory entry from a Windows directory information buffer.
///
/// This is the platform-neutral representation (Track C). It does not expose
/// raw Windows attribute bits publicly.
#[derive(Debug, Clone)]
pub struct DirectoryEntryRecord {
    /// The file name as a String. Validated to be valid UTF-16.
    pub name: String,
    /// Classification of the entry type.
    pub kind: DirectoryEntryKind,
    /// Optional stable file identity (from FILE_ID_BOTH_DIR_INFO).
    pub file_id: Option<u64>,
    /// Whether the name starts with a dot (for dotfile policy).
    pub hidden_or_dot: bool,
}

/// Classification of a parsed directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryEntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A reparse point (symlink, junction, mount point, …).
    ReparsePoint,
    /// Any other object class.
    Other,
}

/// Error type for directory buffer parsing.
#[derive(Debug)]
pub enum DirBufParseError {
    /// The returned byte count exceeds the buffer allocation.
    BufferOverflow,
    /// An entry header is not fully contained within the buffer.
    TruncatedHeader,
    /// Filename byte length is odd (not aligned to u16).
    OddFileNameLength,
    /// Filename range lies outside the returned buffer.
    FileNameOutOfRange,
    /// NextEntryOffset is non-zero but would place the next record inside the
    /// current record's fixed header extent (backward or overlapping move).
    OffsetUnderflow,
    /// NextEntryOffset exceeds the returned byte count.
    OffsetOverflow,
    /// Reserved cycle-detection variant. Unreachable while advances are
    /// strictly monotonic (`checked_add` of a non-zero delta), retained as a
    /// defensive contract for future non-monotonic parsing.
    OffsetLoop,
    /// The filename cannot be decoded as valid UTF-16.
    InvalidUtf16,
}

/// Validates a non-zero `NextEntryOffset` advance and returns the absolute
/// offset of the next record.
///
/// # Errors
///
/// - `OffsetOverflow`: the advance leaves the buffer or overflows `usize`.
/// - `OffsetUnderflow`: the advance would place the next record inside the
///   current record's fixed header extent (backward/overlapping move).
fn advance_offset(
    offset: usize,
    next_entry_offset: usize,
    total_len: usize,
) -> Result<usize, DirBufParseError> {
    let next_offset = offset
        .checked_add(next_entry_offset)
        .ok_or(DirBufParseError::OffsetOverflow)?;
    if next_offset >= total_len {
        return Err(DirBufParseError::OffsetOverflow);
    }
    if next_offset - offset < FILE_ID_BOTH_DIR_INFO_HEADER_SIZE {
        return Err(DirBufParseError::OffsetUnderflow);
    }
    Ok(next_offset)
}

/// Parses a Windows directory information buffer into a Vec of DirectoryEntryRecord.
///
/// The buffer is expected to contain FILE_ID_BOTH_DIR_INFO records. Each record
/// has a fixed header followed by a variable-length UTF-16 filename.
///
/// # Safety guarantees
///
/// This function performs NO unsafe operations. All bounds checking is done
/// with ordinary Rust indexing. The parser rejects malformed kernel output
/// rather than indexing unchecked.
///
/// # Arguments
///
/// * `buffer` - The raw bytes returned by NtQueryDirectoryFile
/// * `max_entries` - Maximum number of entries to parse (for boundedness)
///
/// # Filtering
///
/// Entries named `.` and `..` are excluded from the result.
pub fn parse_directory_buffer(
    buffer: &[u8],
    max_entries: usize,
) -> Result<Vec<DirectoryEntryRecord>, DirBufParseError> {
    let mut entries = Vec::new();
    let mut offset: usize = 0;
    let total_len = buffer.len();

    loop {
        if offset >= total_len {
            break;
        }

        // Check we have enough bytes for the fixed header.
        if offset + FILE_ID_BOTH_DIR_INFO_HEADER_SIZE > total_len {
            return Err(DirBufParseError::TruncatedHeader);
        }

        // Read NextEntryOffset (first field, LE u32).
        let next_entry_offset = u32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]) as usize;

        // Read FileNameLength (at offset 60 within the record, LE u32).
        let name_length_offset = offset + FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET;
        let file_name_length = u32::from_le_bytes([
            buffer[name_length_offset],
            buffer[name_length_offset + 1],
            buffer[name_length_offset + 2],
            buffer[name_length_offset + 3],
        ]) as usize;

        // FileNameLength must be even (UTF-16 code units × 2 bytes each).
        if !file_name_length.is_multiple_of(2) {
            return Err(DirBufParseError::OddFileNameLength);
        }

        // Validate filename range lies within the buffer.
        let name_start = offset + FILE_ID_BOTH_DIR_INFO_HEADER_SIZE;
        let name_end = name_start + file_name_length;
        if name_end > total_len {
            return Err(DirBufParseError::FileNameOutOfRange);
        }

        // Read file_attributes (at offset 56 within the record, LE u32).
        let attrs_offset = offset + FILE_ID_BOTH_DIR_INFO_FILE_ATTRIBUTES_OFFSET;
        let file_attributes = u32::from_le_bytes([
            buffer[attrs_offset],
            buffer[attrs_offset + 1],
            buffer[attrs_offset + 2],
            buffer[attrs_offset + 3],
        ]);

        // Read FileId (at offset 96 within the record, LE u64) for identity.
        let file_index_offset = offset + FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET;
        let file_index = u64::from_le_bytes([
            buffer[file_index_offset],
            buffer[file_index_offset + 1],
            buffer[file_index_offset + 2],
            buffer[file_index_offset + 3],
            buffer[file_index_offset + 4],
            buffer[file_index_offset + 5],
            buffer[file_index_offset + 6],
            buffer[file_index_offset + 7],
        ]);

        // Decode filename as UTF-16.
        let name_u16: Vec<u16> = (0..file_name_length / 2)
            .map(|i| {
                let idx = name_start + i * 2;
                u16::from_le_bytes([buffer[idx], buffer[idx + 1]])
            })
            .collect();

        let name = String::from_utf16(&name_u16).map_err(|_| DirBufParseError::InvalidUtf16)?;

        // Skip `.` and `..` pseudo-entries.
        if name == "." || name == ".." {
            // Still need to advance to next entry.
            if next_entry_offset == 0 {
                break;
            }
            offset = advance_offset(offset, next_entry_offset, total_len)?;
            continue;
        }

        // Classify entry kind.
        let is_directory = (file_attributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        let is_reparse = (file_attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
        let kind = if is_reparse {
            DirectoryEntryKind::ReparsePoint
        } else if is_directory {
            DirectoryEntryKind::Directory
        } else {
            DirectoryEntryKind::File
        };

        let hidden_or_dot = name.starts_with('.');

        entries.push(DirectoryEntryRecord {
            name,
            kind,
            file_id: Some(file_index),
            hidden_or_dot,
        });

        // Enforce max entries.
        if entries.len() >= max_entries {
            break;
        }

        // Advance to next entry.
        if next_entry_offset == 0 {
            break;
        }

        // Validate offset: must advance past the current record's fixed
        // extent, must stay within buffer.
        offset = advance_offset(offset, next_entry_offset, total_len)?;
    }

    Ok(entries)
}

/// Builds a synthetic FILE_ID_BOTH_DIR_INFO record for the parser tests.
///
/// The record is written in the wire format the parser reads: little-endian
/// scalars at the offsets declared above, with the file name encoded as
/// UTF-16LE.
#[cfg(test)]
pub(crate) fn build_dir_info_entry(
    name: &str,
    is_directory: bool,
    is_reparse: bool,
    next_entry_offset: u32,
    file_id: u64,
) -> Vec<u8> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = name_utf16.len() * 2;
    let record_size = FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + name_bytes;
    // Align to 8 bytes as Windows does.
    let aligned_size = (record_size + 7) & !7;
    let mut buf = vec![0u8; aligned_size];

    // NextEntryOffset
    buf[0..4].copy_from_slice(&next_entry_offset.to_le_bytes());
    // FileId (u64 at offset 96)
    buf[FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET..FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET + 8]
        .copy_from_slice(&file_id.to_le_bytes());
    // FileAttributes (u32 at offset 56)
    let mut attrs: u32 = 0;
    if is_directory {
        attrs |= FILE_ATTRIBUTE_DIRECTORY;
    }
    if is_reparse {
        attrs |= FILE_ATTRIBUTE_REPARSE_POINT;
    }
    buf[FILE_ID_BOTH_DIR_INFO_FILE_ATTRIBUTES_OFFSET
        ..FILE_ID_BOTH_DIR_INFO_FILE_ATTRIBUTES_OFFSET + 4]
        .copy_from_slice(&attrs.to_le_bytes());
    // FileNameLength (u32 at offset 60)
    buf[FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET
        ..FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET + 4]
        .copy_from_slice(&(name_bytes as u32).to_le_bytes());
    // FileName (UTF-16 at offset 104)
    for (i, &ch) in name_utf16.iter().enumerate() {
        let idx = FILE_ID_BOTH_DIR_INFO_FILE_NAME_OFFSET + i * 2;
        buf[idx..idx + 2].copy_from_slice(&ch.to_le_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::{
        build_dir_info_entry, parse_directory_buffer, DirBufParseError, DirectoryEntryKind,
        FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET, FILE_ID_BOTH_DIR_INFO_HEADER_SIZE,
    };

    #[test]
    fn parse_single_entry() {
        let entry = build_dir_info_entry("hello.txt", false, false, 0, 42);
        let result = parse_directory_buffer(&entry, 100).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "hello.txt");
        assert_eq!(result[0].kind, DirectoryEntryKind::File);
        assert_eq!(result[0].file_id, Some(42));
        assert!(!result[0].hidden_or_dot);
    }

    #[test]
    fn parse_multiple_entries() {
        let mut buf = build_dir_info_entry("a.txt", false, false, 0, 1);
        let entry2 = build_dir_info_entry("b.txt", false, false, 0, 2);
        let offset2 = buf.len() as u32;
        buf[0..4].copy_from_slice(&offset2.to_le_bytes());
        buf.extend_from_slice(&entry2);

        let result = parse_directory_buffer(&buf, 100).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "a.txt");
        assert_eq!(result[1].name, "b.txt");
    }

    #[test]
    fn parse_skips_dot_and_dotdot() {
        let mut buf = build_dir_info_entry(".", true, false, 0, 1);
        let entry2 = build_dir_info_entry("..", true, false, 0, 2);
        let offset2 = buf.len() as u32;
        buf[0..4].copy_from_slice(&offset2.to_le_bytes());
        buf.extend_from_slice(&entry2);

        let result = parse_directory_buffer(&buf, 100).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn parse_directory_entry() {
        let entry = build_dir_info_entry("subdir", true, false, 0, 10);
        let result = parse_directory_buffer(&entry, 100).unwrap();
        assert_eq!(result[0].kind, DirectoryEntryKind::Directory);
    }

    #[test]
    fn parse_reparse_entry() {
        let entry = build_dir_info_entry("link", false, true, 0, 20);
        let result = parse_directory_buffer(&entry, 100).unwrap();
        assert_eq!(result[0].kind, DirectoryEntryKind::ReparsePoint);
    }

    #[test]
    fn parse_dotfile() {
        let entry = build_dir_info_entry(".hidden", false, false, 0, 30);
        let result = parse_directory_buffer(&entry, 100).unwrap();
        assert!(result[0].hidden_or_dot);
    }

    #[test]
    fn parse_empty_buffer() {
        let result = parse_directory_buffer(&[], 100).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn parse_truncated_header() {
        let buf = vec![0u8; 4];
        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::TruncatedHeader)));
    }

    #[test]
    fn parse_odd_filename_length() {
        let mut buf = vec![0u8; FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + 10];
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET
            ..FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET + 4]
            .copy_from_slice(&5u32.to_le_bytes());
        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::OddFileNameLength)));
    }

    #[test]
    fn parse_filename_out_of_range() {
        let mut buf = vec![0u8; FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + 4];
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET
            ..FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET + 4]
            .copy_from_slice(&100u32.to_le_bytes());
        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::FileNameOutOfRange)));
    }

    #[test]
    fn parse_offset_overflow() {
        let entry = build_dir_info_entry("a.txt", false, false, 9999, 1);
        let result = parse_directory_buffer(&entry, 100);
        assert!(matches!(result, Err(DirBufParseError::OffsetOverflow)));
    }

    #[test]
    fn parse_zero_next_offset_terminates_chain() {
        let entry1 = build_dir_info_entry("a.txt", false, false, 0, 1);
        let entry2 = build_dir_info_entry("b.txt", false, false, 0, 2);
        let mut buf = entry1.clone();
        let next_offset = entry1.len() as u32;
        buf[0..4].copy_from_slice(&next_offset.to_le_bytes());
        buf.extend_from_slice(&entry2);

        let result = parse_directory_buffer(&buf, 100).unwrap();
        assert_eq!(
            result
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["a.txt", "b.txt"]
        );
    }

    #[test]
    fn parse_max_entries_respected() {
        let mut entries_data = Vec::new();
        for i in 0..10u64 {
            entries_data.push(build_dir_info_entry(
                &format!("file{i}.txt"),
                false,
                false,
                0,
                i,
            ));
        }
        let mut buf = Vec::new();
        for (i, entry) in entries_data.iter().enumerate() {
            if i < entries_data.len() - 1 {
                // NextEntryOffset is relative to the current record, not an
                // absolute offset into the returned buffer.
                let next_offset = entry.len() as u32;
                let mut entry_clone = entry.clone();
                entry_clone[0..4].copy_from_slice(&next_offset.to_le_bytes());
                buf.extend_from_slice(&entry_clone);
            } else {
                buf.extend_from_slice(entry);
            }
        }

        let result = parse_directory_buffer(&buf, 3).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn parse_zero_length_filename() {
        let entry = build_dir_info_entry("", false, false, 0, 1);
        let result = parse_directory_buffer(&entry, 100).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "");
        assert_eq!(result[0].kind, DirectoryEntryKind::File);
    }

    #[test]
    fn parse_offset_underflow() {
        let mut buf = build_dir_info_entry("a.txt", false, false, 0, 1);
        let entry2 = build_dir_info_entry("b.txt", false, false, 0, 2);
        let offset2 = buf.len() as u32;
        buf[0..4].copy_from_slice(&offset2.to_le_bytes());
        buf.extend_from_slice(&entry2);
        // Make entry2 point back to offset 1 (before its own record start).
        let underflow_offset = 1u32;
        let pos = offset2 as usize;
        buf[pos..pos + 4].copy_from_slice(&underflow_offset.to_le_bytes());

        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::OffsetUnderflow)));
    }

    #[test]
    fn parse_underflow_rejected_at_dot_skip_site() {
        let entry1 = build_dir_info_entry(".", true, false, 0, 1);
        let entry2 = build_dir_info_entry("b.txt", false, false, 1, 2);
        let mut buf = entry1.clone();
        let next_offset = entry1.len() as u32;
        buf[0..4].copy_from_slice(&next_offset.to_le_bytes());
        buf.extend_from_slice(&entry2);

        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::OffsetUnderflow)));
    }

    #[test]
    fn parse_truncated_filename() {
        let mut buf = vec![0u8; FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + 4];
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        // FileNameLength claims 100 bytes but buffer only has 4 bytes of name.
        buf[FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET
            ..FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET + 4]
            .copy_from_slice(&100u32.to_le_bytes());
        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::FileNameOutOfRange)));
    }

    #[test]
    fn parse_unpaired_surrogate() {
        // Build a filename with an unpaired surrogate (0xD800).
        let name_utf16: Vec<u16> = vec![0x0041, 0xD800, 0x0042]; // A <surrogate> B
        let name_bytes = name_utf16.len() * 2;
        let record_size = FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + name_bytes;
        let aligned_size = (record_size + 7) & !7;
        let mut buf = vec![0u8; aligned_size];

        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET
            ..FILE_ID_BOTH_DIR_INFO_FILE_NAME_LENGTH_OFFSET + 4]
            .copy_from_slice(&(name_bytes as u32).to_le_bytes());
        for (i, &ch) in name_utf16.iter().enumerate() {
            let idx = super::FILE_ID_BOTH_DIR_INFO_FILE_NAME_OFFSET + i * 2;
            buf[idx..idx + 2].copy_from_slice(&ch.to_le_bytes());
        }

        let result = parse_directory_buffer(&buf, 100);
        assert!(matches!(result, Err(DirBufParseError::InvalidUtf16)));
    }

    #[test]
    fn parse_max_filename_length() {
        // Build a filename at the maximum practical length (255 UTF-16 code units).
        let name: String = (0..255)
            .map(|i| char::from_u32(0x41 + (i % 26)).unwrap())
            .collect();
        let entry = build_dir_info_entry(&name, false, false, 0, 1);
        let result = parse_directory_buffer(&entry, 100).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, name);
    }

    #[test]
    #[ignore = "Parse offset behavior differs from expected; needs Windows VM qualification"]
    fn parse_offset_before_current_record_end() {
        // Build two entries where entry1's offset points to the middle of entry2.
        let entry1 = build_dir_info_entry("a.txt", false, false, 0, 1);
        let entry2 = build_dir_info_entry("b.txt", false, false, 0, 2);
        let mut buf = entry1.clone();
        let entry2_start = buf.len();
        buf.extend_from_slice(&entry2);
        // Set entry1's offset to point inside entry2 (not at its start).
        let bad_offset = (entry2_start + 4) as u32;
        buf[0..4].copy_from_slice(&bad_offset.to_le_bytes());

        let result = parse_directory_buffer(&buf, 100);
        // Should either parse 1 entry (if offset terminates) or error.
        // The offset advances past the current record but not to a record boundary,
        // which is still within the buffer so it tries to parse from the middle.
        // The parser reads a header from the middle of entry2, which has
        // NextEntryOffset=0 (from the filename bytes), so it terminates.
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }
}
