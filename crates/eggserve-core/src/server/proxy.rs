//! HAProxy PROXY protocol preamble reading (Plan 202 Track C).
//!
//! Pure parsing lives in [`crate::primitives::proxy`]; this module owns the
//! bounded, timeout-protected async read that runs before TLS/HTTP:
//!
//! ```text
//! TCP accept -> PROXY preamble (optional) -> TLS handshake (optional) -> HTTP
//! ```
//!
//! Disabled listeners never call here and interpret bytes normally (no
//! magic auto-detection). Enabled listeners call here only for explicitly
//! trusted immediate peers; untrusted peers are rejected without reading.
//! Malformed, oversized, or slow preambles close before TLS/HTTP and never
//! reach a service. `LOCAL`/`UNKNOWN`/`UNSPEC`/UNIX preserve truthful
//! absence (no identity invented). TLVs are ignored but bounded.

use std::fmt;
use std::time::Duration;

use crate::primitives::proxy::{
    parse_proxy_v1_line, parse_proxy_v2_header, ProxyEndpoints, ProxyParseError,
    PROXY_V1_MAX_BYTES, PROXY_V2_HEADER_LEN, PROXY_V2_MAX_LEN, PROXY_V2_SIGNATURE,
};

/// Maximum bytes buffered while waiting for a complete preamble.
///
/// Covers the largest bounded preamble (`16 + PROXY_V2_MAX_LEN`); v1 lines
/// are far smaller. Anything beyond this without a complete parse is
/// [`ProxyReadError::TooLong`].
const MAX_PREAMBLE_BUFFER: usize = PROXY_V2_HEADER_LEN + PROXY_V2_MAX_LEN;

/// Why a PROXY preamble was rejected (sanitized, no preamble bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyReadError {
    /// Preamble did not complete within the configured timeout.
    Timeout,
    /// Preamble exceeds the strict size ceiling.
    TooLong,
    /// Preamble is malformed or uses an unsupported family/protocol.
    Invalid,
    /// Transport error while reading.
    Io,
}

impl std::fmt::Display for ProxyReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => write!(f, "PROXY preamble timeout"),
            Self::TooLong => write!(f, "PROXY preamble exceeds size limit"),
            Self::Invalid => write!(f, "invalid PROXY preamble"),
            Self::Io => write!(f, "PROXY preamble read error"),
        }
    }
}

impl std::error::Error for ProxyReadError {}

impl From<ProxyParseError> for ProxyReadError {
    fn from(error: ProxyParseError) -> Self {
        match error {
            ProxyParseError::Incomplete => Self::Invalid,
            ProxyParseError::TooLong => Self::TooLong,
            ProxyParseError::Invalid => Self::Invalid,
        }
    }
}

/// Read one PROXY preamble from `stream`, returning endpoints plus any
/// bytes already read beyond the preamble.
///
/// The caller must replay `leftover` before TLS/HTTP (via
/// `PrefixedIo::new(leftover, stream)`). On error the connection must close
/// before TLS/HTTP; no bytes are trusted.
///
/// `timeout` bounds the entire read. `stream` is borrowed mutably so the
/// caller retains ownership for the wrapped handoff.
pub async fn read_proxy_preamble<S>(
    stream: &mut S,
    timeout: Duration,
) -> Result<(ProxyEndpoints, Vec<u8>), ProxyReadError>
where
    S: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let read_future = async {
        let mut buffer: Vec<u8> = Vec::with_capacity(128);
        let mut tmp = [0u8; 256];
        loop {
            // Try to parse what we have before reading more.
            if let Some(result) = try_parse_buffer(&buffer) {
                return result;
            }
            if buffer.len() > MAX_PREAMBLE_BUFFER {
                return Err(ProxyReadError::TooLong);
            }
            let count = stream
                .read(&mut tmp)
                .await
                .map_err(|_| ProxyReadError::Io)?;
            if count == 0 {
                return Err(ProxyReadError::Invalid);
            }
            buffer.extend_from_slice(&tmp[..count]);
            if buffer.len() > MAX_PREAMBLE_BUFFER + 256 {
                return Err(ProxyReadError::TooLong);
            }
            // Early v1 length guard: a v1 line without CRLF beyond the max
            // is oversized even if a v2 parse is still incomplete. Only
            // applies when the buffer cannot be a v2 preamble (does not
            // start with the v2 signature prefix).
            if !starts_with_v2_prefix(&buffer) && !buffer.starts_with(b"PROXY ") {
                // Not enough bytes to decide? Require at least 6 bytes
                // (`PROXY `) or 1 byte of the v2 signature before failing.
                // A single non-matching first byte is already invalid.
                if !buffer.is_empty() && !could_be_proxy_prefix(&buffer) {
                    return Err(ProxyReadError::Invalid);
                }
            }
            if !starts_with_v2_prefix(&buffer) {
                if let Some(crlf_pos) = find_crlf(&buffer) {
                    let line_len = crlf_pos + 2;
                    if line_len > PROXY_V1_MAX_BYTES {
                        return Err(ProxyReadError::TooLong);
                    }
                    // Full line available; parse strictly.
                    let line = buffer[..line_len].to_vec();
                    let leftover = buffer[line_len..].to_vec();
                    let endpoints = parse_proxy_v1_line(&line).map_err(ProxyReadError::from)?;
                    return Ok((endpoints, leftover));
                }
                if buffer.len() > PROXY_V1_MAX_BYTES && !starts_with_v2_prefix(&buffer) {
                    // No CRLF within the v1 ceiling and not a v2 preamble.
                    // Could still be a fragmented v2 signature? No: v2
                    // signature is 12 bytes starting with 0x0D; a buffer
                    // longer than v1 max that does not start with the v2
                    // prefix cannot become valid.
                    if buffer.len() >= 12 || !could_be_v2_prefix(&buffer) {
                        return Err(ProxyReadError::TooLong);
                    }
                }
            }
        }
    };

    match tokio::time::timeout(timeout, read_future).await {
        Ok(result) => result,
        Err(_) => Err(ProxyReadError::Timeout),
    }
}

/// Attempt a complete parse from the current buffer, if possible.
///
/// Returns `Some(Ok/Err)` when the buffer deterministically completes
/// (valid or invalid), `None` when more bytes are needed.
fn try_parse_buffer(buffer: &[u8]) -> Option<Result<(ProxyEndpoints, Vec<u8>), ProxyReadError>> {
    if buffer.is_empty() {
        return None;
    }
    // v2 path when the buffer starts with (a prefix of) the signature and
    // has enough bytes to decide. A full 12-byte mismatch falls through to
    // the v1 path (which will reject unless it is a valid v1 line).
    if starts_with_v2_prefix(buffer) && buffer.len() >= PROXY_V2_HEADER_LEN {
        // Need the framed length to know completeness.
        if buffer.len() >= PROXY_V2_HEADER_LEN {
            let len = u16::from_be_bytes([buffer[14], buffer[15]]) as usize;
            if len > PROXY_V2_MAX_LEN {
                return Some(Err(ProxyReadError::TooLong));
            }
            let total = PROXY_V2_HEADER_LEN + len;
            if buffer.len() >= total {
                let slice = buffer[..total].to_vec();
                let leftover = buffer[total..].to_vec();
                match parse_proxy_v2_header(&slice) {
                    Ok((endpoints, consumed)) => {
                        debug_assert_eq!(consumed, total);
                        return Some(Ok((endpoints, leftover)));
                    }
                    Err(ProxyParseError::Incomplete) => return None,
                    Err(error) => return Some(Err(ProxyReadError::from(error))),
                }
            }
            return None;
        }
    }
    // v1 path when the buffer looks like text.
    if buffer.starts_with(b"PROXY ") {
        if let Some(crlf_pos) = find_crlf(buffer) {
            let line_len = crlf_pos + 2;
            if line_len > PROXY_V1_MAX_BYTES {
                return Some(Err(ProxyReadError::TooLong));
            }
            let line = buffer[..line_len].to_vec();
            let leftover = buffer[line_len..].to_vec();
            match parse_proxy_v1_line(&line) {
                Ok(endpoints) => return Some(Ok((endpoints, leftover))),
                Err(error) => return Some(Err(ProxyReadError::from(error))),
            }
        }
        if buffer.len() > PROXY_V1_MAX_BYTES {
            return Some(Err(ProxyReadError::TooLong));
        }
        return None;
    }
    // Deterministic rejection once neither prefix can still match.
    if buffer.len() >= 6 && !could_be_proxy_prefix(buffer) {
        // Could still be a v2 preamble whose first bytes happen to look
        // non-textual? v2 starts with 0x0D; `could_be_proxy_prefix` already
        // accounts for both families. If neither can match, fail closed.
        if buffer.len() >= 12 || !could_be_v2_prefix(buffer) {
            return Some(Err(ProxyReadError::Invalid));
        }
    }
    None
}

fn starts_with_v2_prefix(buffer: &[u8]) -> bool {
    let prefix_len = buffer.len().min(PROXY_V2_SIGNATURE.len());
    buffer[..prefix_len] == PROXY_V2_SIGNATURE[..prefix_len] && !buffer.is_empty()
}

fn could_be_v2_prefix(buffer: &[u8]) -> bool {
    if buffer.is_empty() {
        return true;
    }
    let prefix_len = buffer.len().min(PROXY_V2_SIGNATURE.len());
    buffer[..prefix_len] == PROXY_V2_SIGNATURE[..prefix_len]
}

fn could_be_proxy_prefix(buffer: &[u8]) -> bool {
    const V1_PREFIX: &[u8] = b"PROXY ";
    if buffer.is_empty() {
        return true;
    }
    let v1_len = buffer.len().min(V1_PREFIX.len());
    if buffer[..v1_len] == V1_PREFIX[..v1_len] {
        return true;
    }
    could_be_v2_prefix(buffer)
}

fn find_crlf(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|window| window == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_v1_preamble_with_leftover() {
        let line = b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\n";
        let mut payload = line.to_vec();
        payload.extend_from_slice(b"GET / HTTP/1.1\r\n");
        let mut cursor = tokio::io::BufReader::new(std::io::Cursor::new(payload));
        // BufReader over Cursor implements AsyncRead; read via the helper.
        let (endpoints, leftover) = read_proxy_preamble(&mut cursor, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(endpoints.source.unwrap().to_string(), "192.168.0.1:56324");
        assert!(leftover.starts_with(b"GET /"));
    }

    #[tokio::test]
    async fn reads_v2_preamble() {
        let mut preamble = Vec::new();
        preamble.extend_from_slice(&PROXY_V2_SIGNATURE);
        preamble.push(0x21);
        preamble.push(0x11);
        preamble.extend_from_slice(&12u16.to_be_bytes());
        preamble.extend_from_slice(&[10, 0, 0, 1, 10, 0, 0, 2]);
        preamble.extend_from_slice(&1234u16.to_be_bytes());
        preamble.extend_from_slice(&80u16.to_be_bytes());
        let mut cursor = std::io::Cursor::new(preamble);
        let (endpoints, leftover) = read_proxy_preamble(&mut cursor, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(endpoints.source.unwrap().to_string(), "10.0.0.1:1234");
        assert!(leftover.is_empty());
    }

    #[tokio::test]
    async fn rejects_non_proxy_bytes() {
        let mut cursor = std::io::Cursor::new(b"GET / HTTP/1.1\r\n".to_vec());
        let error = read_proxy_preamble(&mut cursor, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert_eq!(error, ProxyReadError::Invalid);
    }

    #[tokio::test]
    async fn rejects_oversized_v1() {
        let mut long = b"PROXY TCP4 1.1.1.1 2.2.2.2 1 80".to_vec();
        long.resize(PROXY_V1_MAX_BYTES + 10, b'X');
        let mut cursor = std::io::Cursor::new(long);
        let error = read_proxy_preamble(&mut cursor, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProxyReadError::TooLong | ProxyReadError::Invalid
        ));
    }
}
