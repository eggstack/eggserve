//! Canonical response headers (Plan 206 Track C).
//!
//! Owns [`ResponseHead`] and the single hop-by-hop authority
//! ([`is_hop_by_hop_header`]). `remove_header`/`strip_hop_by_hop` are
//! `pub(super)` for the sibling `response` normalizer only; the runtime
//! remains the sole framing authority.

use super::super::header_block::HeaderBlock;
use super::status::StatusCode;

/// The canonical response head: status code and validated headers.
///
/// This is the transport-independent representation of the response metadata.
/// It uses [`HeaderBlock`] for duplicate-preserving, validated header storage.
#[derive(Debug, Clone)]
pub struct ResponseHead {
    status: StatusCode,
    headers: HeaderBlock,
}

impl ResponseHead {
    /// Create a new response head.
    pub fn new(status: StatusCode, headers: HeaderBlock) -> Self {
        Self { status, headers }
    }

    /// Returns the status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns a reference to the headers.
    pub fn headers(&self) -> &HeaderBlock {
        &self.headers
    }

    /// Returns a mutable reference to the headers.
    ///
    /// This is only available during construction; normalization consumes
    /// the head immutably.
    pub fn headers_mut(&mut self) -> &mut HeaderBlock {
        &mut self.headers
    }
}

/// Returns `true` if the header is a hop-by-hop header that must not be
/// forwarded by intermediaries per RFC 7230 § 4.1.2.
pub fn is_hop_by_hop_header(name: &str) -> bool {
    [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ]
    .iter()
    .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Remove all headers with the given name (case-insensitive).
pub(super) fn remove_header(headers: &mut HeaderBlock, name: &str) {
    headers.retain(|f| !f.name.as_str().eq_ignore_ascii_case(name));
}

/// Remove all hop-by-hop headers from the block.
pub(super) fn strip_hop_by_hop(headers: &mut HeaderBlock) {
    // `Connection` tokens require text interpretation: opaque (non-UTF-8)
    // values cannot name headers, so they contribute no tokens. This keeps
    // generic forwarding byte-preserving while protocol semantics stay strict.
    let connection_tokens: Vec<String> = headers
        .iter()
        .filter(|field| field.name.as_str().eq_ignore_ascii_case("connection"))
        .filter_map(|field| field.value.to_str().ok())
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();

    headers.retain(|field| {
        !is_hop_by_hop_header(field.name.as_str())
            && !connection_tokens
                .iter()
                .any(|name| field.name.as_str().eq_ignore_ascii_case(name))
    });
}
