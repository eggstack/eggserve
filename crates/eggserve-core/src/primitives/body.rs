//! Safe body source abstraction for response streaming.
//!
//! A [`BodySource`] owns the data needed to produce a response body without
//! reopening filesystem paths. For file-backed variants, the resolver-opened
//! file handle is carried forward — the service layer converts it to a Hyper
//! streaming body at response time.
//!
//! # Conversion model
//!
//! Converting a [`super::secure_root::ResolvedFile`] into a [`BodySource`]
//! **consumes** the file capability. This prevents accidental double-use:
//! each resolved file can produce exactly one body source.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use crate::primitives::response::FileRange;

/// Errors that can arise when converting a resolved file into a body source.
#[derive(Debug)]
pub enum BodySourceError {
    /// The requested byte range is invalid for the file size.
    InvalidRange,
    /// The resolved file has already been consumed into a body source.
    AlreadyConsumed,
}

impl std::fmt::Display for BodySourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRange => write!(f, "invalid byte range"),
            Self::AlreadyConsumed => write!(f, "resolved file already consumed"),
        }
    }
}

impl std::error::Error for BodySourceError {}

/// The kind of body a response will carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyKind {
    Empty,
    Bytes,
    FileFull,
    FileRange,
}

/// A resolved response body that owns its data without reopening paths.
///
/// For file-backed variants, the [`File`] was opened during path resolution
/// (e.g. via `openat(O_NOFOLLOW)` on Unix) and is carried forward here.
/// The service layer converts it to a Hyper streaming body at response time.
#[derive(Debug)]
pub enum BodySource {
    /// No body content (e.g. HEAD response, 304, 416).
    Empty,
    /// An in-memory byte buffer.
    Bytes(Vec<u8>),
    /// A full static file. The file was opened during resolution.
    FileFull {
        file: File,
        len: u64,
        mime: &'static str,
    },
    /// A byte range of a static file.
    FileRange {
        file: File,
        range: FileRange,
        total_len: u64,
        mime: &'static str,
    },
}

impl BodySource {
    /// Returns the [`BodyKind`] discriminant.
    pub fn kind(&self) -> BodyKind {
        match self {
            Self::Empty => BodyKind::Empty,
            Self::Bytes(_) => BodyKind::Bytes,
            Self::FileFull { .. } => BodyKind::FileFull,
            Self::FileRange { .. } => BodyKind::FileRange,
        }
    }

    /// Returns the content length in bytes, if known without performing I/O.
    pub fn len(&self) -> u64 {
        match self {
            Self::Empty => 0,
            Self::Bytes(b) => b.len() as u64,
            Self::FileFull { len, .. } => *len,
            Self::FileRange { range, .. } => range.len(),
        }
    }

    /// Returns `true` if the body is known to be zero-length.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the byte range, if this is a range body.
    pub fn range(&self) -> Option<FileRange> {
        match self {
            Self::FileRange { range, .. } => Some(*range),
            _ => None,
        }
    }

    /// Read the entire body into memory.
    ///
    /// This is suitable for small files and test verification. For production
    /// streaming, the service layer should convert the body source to a Hyper
    /// streaming body instead of reading into memory.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file cannot be read or the range cannot be
    /// seeked to.
    pub fn read_all(&mut self) -> io::Result<Vec<u8>> {
        match self {
            Self::Empty => Ok(Vec::new()),
            Self::Bytes(b) => Ok(b.clone()),
            Self::FileFull { file, .. } => {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                Ok(buf)
            }
            Self::FileRange { file, range, .. } => {
                file.seek(SeekFrom::Start(range.start))?;
                let len = usize::try_from(range.len()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "body range too large")
                })?;
                let mut buf = vec![0u8; len];
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
        }
    }

    /// Read the entire body into memory, capped at `max_bytes`.
    ///
    /// Returns at most `max_bytes` bytes. If the body is larger, the excess
    /// is silently truncated. This prevents unbounded allocation when reading
    /// file-backed bodies whose size may be large.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file cannot be read or the range cannot be
    /// seeked to.
    pub fn read_all_bounded(&mut self, max_bytes: usize) -> io::Result<Vec<u8>> {
        match self {
            Self::Empty => Ok(Vec::new()),
            Self::Bytes(b) => {
                let len = b.len().min(max_bytes);
                Ok(b[..len].to_vec())
            }
            Self::FileFull { file, .. } => {
                let mut buf = vec![0u8; max_bytes];
                let n = file.read(&mut buf)?;
                buf.truncate(n);
                Ok(buf)
            }
            Self::FileRange { file, range, .. } => {
                file.seek(SeekFrom::Start(range.start))?;
                let len = (range.len() as usize).min(max_bytes);
                let mut buf = vec![0u8; len];
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
        }
    }

    /// Read a specific byte range from the body.
    ///
    /// For file-full bodies, `start` and `end_inclusive` are absolute offsets
    /// into the file. For file-range bodies, they are offsets within the range.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the seek or read fails.
    pub fn read_range(&mut self, start: u64, end_inclusive: u64) -> io::Result<Vec<u8>> {
        if end_inclusive < start {
            return Ok(Vec::new());
        }
        match self {
            Self::Empty => Ok(Vec::new()),
            Self::Bytes(b) => {
                let s: usize = start.try_into().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "start offset too large")
                })?;
                let e_plus_1: usize = end_inclusive
                    .checked_add(1)
                    .and_then(|v| v.try_into().ok())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "end offset too large")
                    })?;
                let e = e_plus_1.min(b.len());
                if s >= b.len() {
                    return Ok(Vec::new());
                }
                Ok(b[s..e].to_vec())
            }
            Self::FileFull { file, len, .. } => {
                if start >= *len {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "read start exceeds file length",
                    ));
                }
                if end_inclusive >= *len {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "read end exceeds file length",
                    ));
                }
                let request_len = end_inclusive
                    .checked_sub(start)
                    .and_then(|v| v.checked_add(1))
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid range"))?;
                let effective_len = request_len.min(*len - start);
                file.seek(SeekFrom::Start(start))?;
                let effective_len = usize::try_from(effective_len).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "read range too large")
                })?;
                let mut buf = vec![0u8; effective_len];
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
            Self::FileRange { file, range, .. } => {
                let sub_len = end_inclusive
                    .checked_sub(start)
                    .and_then(|v| v.checked_add(1))
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid range"))?;
                if sub_len > range.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "sub-range exceeds body range",
                    ));
                }
                let absolute_start = range.start.checked_add(start).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "absolute offset overflow")
                })?;
                let absolute_end = absolute_start + sub_len - 1;
                if absolute_end > range.end_inclusive {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "sub-range exceeds body range",
                    ));
                }
                file.seek(SeekFrom::Start(absolute_start))?;
                let sub_len = usize::try_from(sub_len).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "sub-range too large")
                })?;
                let mut buf = vec![0u8; sub_len];
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_file(content: &[u8]) -> (TempDir, File) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.bin");
        fs::write(&path, content).unwrap();
        let file = File::open(&path).unwrap();
        (tmp, file)
    }

    #[test]
    fn empty_body_source() {
        let mut bs = BodySource::Empty;
        assert_eq!(bs.kind(), BodyKind::Empty);
        assert_eq!(bs.len(), 0);
        assert!(bs.is_empty());
        assert!(bs.range().is_none());
        assert_eq!(bs.read_all().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn bytes_body_source() {
        let mut bs = BodySource::Bytes(b"hello".to_vec());
        assert_eq!(bs.kind(), BodyKind::Bytes);
        assert_eq!(bs.len(), 5);
        assert!(!bs.is_empty());
        assert_eq!(bs.read_all().unwrap(), b"hello");
    }

    #[test]
    fn file_full_body_source() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileFull {
            file,
            len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.kind(), BodyKind::FileFull);
        assert_eq!(bs.len(), 11);
        assert!(!bs.is_empty());
        assert!(bs.range().is_none());
        assert_eq!(bs.read_all().unwrap(), b"hello world");
    }

    #[test]
    fn file_range_body_source() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(0, 4),
            total_len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.kind(), BodyKind::FileRange);
        assert_eq!(bs.len(), 5);
        assert!(!bs.is_empty());
        assert_eq!(bs.range(), Some(FileRange::new(0, 4)));
        assert_eq!(bs.read_all().unwrap(), b"hello");
    }

    #[test]
    fn file_range_body_source_middle() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(6, 10),
            total_len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_all().unwrap(), b"world");
    }

    #[test]
    fn read_range_on_bytes() {
        let mut bs = BodySource::Bytes(b"abcdef".to_vec());
        assert_eq!(bs.read_range(1, 3).unwrap(), b"bcd");
    }

    #[test]
    fn read_range_on_file_full() {
        let (_tmp, file) = make_file(b"abcdef");
        let mut bs = BodySource::FileFull {
            file,
            len: 6,
            mime: "text/plain",
        };
        assert_eq!(bs.read_range(2, 4).unwrap(), b"cde");
    }

    #[test]
    fn read_range_on_file_range() {
        let (_tmp, file) = make_file(b"abcdef");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(1, 4),
            total_len: 6,
            mime: "text/plain",
        };
        // Absolute range 1-4, read sub-range 1-2 (relative to range start)
        assert_eq!(bs.read_range(1, 2).unwrap(), b"cd");
    }

    #[test]
    fn read_range_empty_on_out_of_bounds() {
        let mut bs = BodySource::Bytes(b"ab".to_vec());
        assert_eq!(bs.read_range(5, 10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn read_range_inverted_returns_empty() {
        let mut bs = BodySource::Bytes(b"ab".to_vec());
        assert_eq!(bs.read_range(3, 1).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn read_range_file_full_within_bounds() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileFull {
            file,
            len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_range(0, 4).unwrap(), b"hello");
        assert_eq!(bs.read_range(6, 10).unwrap(), b"world");
    }

    #[test]
    fn read_range_file_full_beyond_eof_rejected() {
        let (_tmp, file) = make_file(b"hello");
        let mut bs = BodySource::FileFull {
            file,
            len: 5,
            mime: "text/plain",
        };
        let result = bs.read_range(0, 99);
        assert!(result.is_err());
    }

    #[test]
    fn read_range_file_full_start_past_len_rejected() {
        let (_tmp, file) = make_file(b"hello");
        let mut bs = BodySource::FileFull {
            file,
            len: 5,
            mime: "text/plain",
        };
        let result = bs.read_range(10, 20);
        assert!(result.is_err());
    }

    #[test]
    fn read_range_file_range_within_bounds() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(0, 4),
            total_len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_range(0, 2).unwrap(), b"hel");
    }

    #[test]
    fn read_range_file_range_beyond_end_rejected() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(0, 4),
            total_len: 11,
            mime: "text/plain",
        };
        let result = bs.read_range(0, 10);
        assert!(result.is_err());
    }

    #[test]
    fn read_range_bytes_large_offset_rejected() {
        let mut bs = BodySource::Bytes(b"test".to_vec());
        let result = bs.read_range(u64::MAX, u64::MAX);
        assert!(result.is_err());
    }

    #[test]
    fn read_range_file_range_overflow_rejected() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(u64::MAX - 2, u64::MAX),
            total_len: 11,
            mime: "text/plain",
        };
        let result = bs.read_range(5, 10);
        assert!(result.is_err());
    }

    #[test]
    fn body_source_error_display() {
        assert_eq!(
            BodySourceError::InvalidRange.to_string(),
            "invalid byte range"
        );
        assert_eq!(
            BodySourceError::AlreadyConsumed.to_string(),
            "resolved file already consumed"
        );
    }

    #[test]
    fn read_all_bounded_bytes_within_limit() {
        let mut bs = BodySource::Bytes(b"hello".to_vec());
        assert_eq!(bs.read_all_bounded(10).unwrap(), b"hello");
    }

    #[test]
    fn read_all_bounded_bytes_truncated() {
        let mut bs = BodySource::Bytes(b"hello".to_vec());
        assert_eq!(bs.read_all_bounded(3).unwrap(), b"hel");
    }

    #[test]
    fn read_all_bounded_bytes_zero_limit() {
        let mut bs = BodySource::Bytes(b"hello".to_vec());
        assert_eq!(bs.read_all_bounded(0).unwrap(), b"");
    }

    #[test]
    fn read_all_bounded_empty() {
        let mut bs = BodySource::Empty;
        assert_eq!(bs.read_all_bounded(100).unwrap(), b"");
    }

    #[test]
    fn read_all_bounded_file_full_within_limit() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileFull {
            file,
            len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_all_bounded(100).unwrap(), b"hello world");
    }

    #[test]
    fn read_all_bounded_file_full_truncated() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileFull {
            file,
            len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_all_bounded(5).unwrap(), b"hello");
    }

    #[test]
    fn read_all_bounded_file_range_within_limit() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(0, 4),
            total_len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_all_bounded(100).unwrap(), b"hello");
    }

    #[test]
    fn read_all_bounded_file_range_truncated() {
        let (_tmp, file) = make_file(b"hello world");
        let mut bs = BodySource::FileRange {
            file,
            range: FileRange::new(0, 10),
            total_len: 11,
            mime: "text/plain",
        };
        assert_eq!(bs.read_all_bounded(3).unwrap(), b"hel");
    }
}
