//! Canonical response bodies (Plan 206 Track C).
//!
//! Owns [`BodyLength`] (framing-authoritative length) and [`ResponseBody`]
//! (one-shot ownership: `Empty`/`Bytes`/`File`/`Stream`/`EmptyWithLength`).
//! `Unknown` never becomes `Content-Length: 0`; HEAD/body-forbidden
//! suppression drops streams without polling (see `response`).

use super::super::body::BodySource;
use super::super::response_stream::ResponseStream;

/// Representation length for normalization.
///
/// `Known(n)` means the exact payload length is known before transport and
/// the runtime must emit `Content-Length: n` for payload-permitting
/// responses. `Unknown` means the length is not known (streaming) and the
/// runtime must omit `Content-Length` and let HTTP/1 select chunked framing.
/// Unknown must never become `Content-Length: 0` accidentally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyLength {
    Known(u64),
    Unknown,
}

impl From<u64> for BodyLength {
    fn from(len: u64) -> Self {
        Self::Known(len)
    }
}

impl BodyLength {
    /// Returns the known length, if any.
    pub fn known(&self) -> Option<u64> {
        match self {
            Self::Known(len) => Some(*len),
            Self::Unknown => None,
        }
    }
}

/// The canonical response body.
///
/// Body ownership is one-shot: once the body is consumed (e.g. by
/// [`normalize_response`] or transport conversion), it cannot be reused.
///
/// `Stream` is the transport-independent application stream from Plan 162.
/// It is pull/backpressure driven with no Hyper types. The runtime owns
/// framing: known-length streams emit `Content-Length`, unknown-length
/// streams use chunked framing selected by HTTP/1.
#[derive(Debug)]
pub enum ResponseBody {
    /// No body content.
    Empty,
    /// In-memory byte buffer.
    Bytes(Vec<u8>),
    /// An already-resolved file capability. The transport consumes the
    /// capability directly; it is never reopened by path.
    File(BodySource),
    /// Incrementally produced application bytes with optional known length.
    Stream(ResponseStream),
    /// No bytes are sent, but metadata must retain this representation length
    /// (used for HEAD responses crossing an adapter boundary).
    EmptyWithLength(u64),
}

impl ResponseBody {
    /// Returns the body length in bytes, if known without performing I/O.
    ///
    /// For unknown-length streams this returns 0, but callers must not use
    /// it for framing — use [`ResponseBody::body_length`] instead. Using
    /// `len()` for an unknown stream would invent a bogus `Content-Length: 0`.
    pub fn len(&self) -> u64 {
        match self {
            Self::Empty => 0,
            Self::Bytes(b) => b.len() as u64,
            Self::File(source) => source.len(),
            Self::Stream(s) => s.known_length().unwrap_or(0),
            Self::EmptyWithLength(len) => *len,
        }
    }

    /// Returns the representation length as [`BodyLength`].
    ///
    /// This is the framing-authoritative length: `Known` for buffered, file,
    /// and known-length streams; `Unknown` for unknown-length streams.
    pub fn body_length(&self) -> BodyLength {
        match self {
            Self::Empty => BodyLength::Known(0),
            Self::Bytes(b) => BodyLength::Known(b.len() as u64),
            Self::File(source) => BodyLength::Known(source.len()),
            Self::Stream(s) => match s.known_length() {
                Some(len) => BodyLength::Known(len),
                None => BodyLength::Unknown,
            },
            Self::EmptyWithLength(len) => BodyLength::Known(*len),
        }
    }

    /// Returns `true` if the body is known to be zero-length.
    ///
    /// Unknown-length streams are never considered empty: their length is
    /// not known without polling.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Stream(s) => matches!(s.known_length(), Some(0)),
            _ => self.len() == 0,
        }
    }

    /// Consume the body and return the bytes.
    ///
    /// Returns `None` if the body was already consumed, is empty, is a file,
    /// or is a stream (streams require transport polling, not buffering).
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Empty => None,
            Self::Bytes(b) => Some(b),
            Self::File(_) => None,
            Self::Stream(_) => None,
            Self::EmptyWithLength(_) => None,
        }
    }
}
