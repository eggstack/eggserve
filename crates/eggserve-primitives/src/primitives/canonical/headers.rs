//! Canonical response headers (Plan 206 Track C).
//!
//! Owns [`ResponseHead`], the single hop-by-hop authority
//! ([`is_hop_by_hop_header`]), and the runtime-owned `Alt-Svc`
//! advertisement authority ([`alt_svc_advertisement_value`] /
//! [`apply_alt_svc_advertisement`]). `remove_header`/`strip_hop_by_hop` are
//! `pub(super)` for the sibling `response` normalizer only; the runtime
//! remains the sole framing authority.

use super::super::header_block::HeaderBlock;
use super::status::StatusCode;

/// The `Alt-Svc` response header field name.
///
/// The runtime owns this field: an application can never advertise a
/// different alternate endpoint.
pub const ALT_SVC_HEADER_NAME: &str = "alt-svc";

/// The runtime-owned `Alt-Svc` advertisement window.
///
/// The `ma` parameter of the advertised H3 endpoint, byte-for-byte as it
/// appears on the wire. This constant is the single owner of the
/// advertisement window: the compatibility H2/H3 path and the H3 adapter
/// both build their advertised value from it, so the two paths can never
/// advertise different windows.
pub const ALT_SVC_MAX_AGE: &str = "ma=86400";

/// Decide the runtime-owned `Alt-Svc` advertisement value.
///
/// Returns the value the runtime must advertise for the H3 endpoint bound
/// to `port`, or `None` when the runtime must not advertise at all
/// (advertisement is off, or `alt-svc` is denylisted).
///
/// The decision is runtime-owned: the caller must remove any
/// application-supplied `alt-svc` before applying the result, so an
/// application can never override the advertisement. A denylisted `alt-svc`
/// stays denylisted even when the H3 endpoint is active.
///
/// `denylisted_headers` is the operator's response-header denylist; the
/// match is case-insensitive.
pub fn alt_svc_advertisement_value<S: AsRef<str>>(
    port: u16,
    advertise: bool,
    denylisted_headers: &[S],
) -> Option<String> {
    if !advertise {
        return None;
    }
    if denylisted_headers
        .iter()
        .any(|name| name.as_ref().eq_ignore_ascii_case(ALT_SVC_HEADER_NAME))
    {
        return None;
    }
    Some(format!("h3=\":{port}\"; {ALT_SVC_MAX_AGE}"))
}

/// Apply the runtime-owned `Alt-Svc` advertisement to a canonical header
/// block.
///
/// Removes every application-supplied `alt-svc` first — an application can
/// never advertise a different H3 endpoint — then inserts the runtime-owned
/// value when the policy allows it. Callers that own a different header
/// container (for example a Hyper `HeaderMap`) use
/// [`alt_svc_advertisement_value`] for the decision and keep their own
/// insertion mechanics.
pub fn apply_alt_svc_advertisement<S: AsRef<str>>(
    headers: &mut HeaderBlock,
    port: u16,
    advertise: bool,
    denylisted_headers: &[S],
) {
    headers.retain(|field| {
        !field
            .name
            .as_str()
            .eq_ignore_ascii_case(ALT_SVC_HEADER_NAME)
    });
    if let Some(value) = alt_svc_advertisement_value(port, advertise, denylisted_headers) {
        // The runtime-generated value is always a valid header value; a
        // rejected value would mean the advertisement format changed.
        let _ = headers.push_str(ALT_SVC_HEADER_NAME, value);
    }
}

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
