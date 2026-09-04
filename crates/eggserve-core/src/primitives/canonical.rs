//! Canonical response types for transport-independent response construction.
//!
//! [`Response`] is the unified response value that all response producers
//! converge on before transport conversion. The [`normalize_response`] function
//! applies the final normalization rules (HEAD suppression, body-forbidden
//! enforcement, hop-by-hop stripping, content-length computation) immediately
//! before the response is sent on the wire.
//!
//! # Conversion model
//!
//! Existing response producers ([`super::response::StaticResponsePlan`],
//! Python callback handlers) are adapted to [`Response`] via `From`/`Into`
//! impls. The normalization function consumes the response body for HEAD and
//! body-forbidden statuses, enforcing the invariant that no body bytes are
//! transmitted for these responses.
//!
//! # Streaming bodies (Plan 162)
//!
//! A Rust [`crate::server::Service`] may return [`ResponseBody::Stream`] for
//! incrementally produced bodies. The runtime remains the only authority for
//! `Content-Length`, `Transfer-Encoding`, and connection reuse:
//!
//! - known-length streams send runtime-generated `Content-Length`; underrun or
//!   overrun closes the connection after commitment;
//! - unknown-length streams omit `Content-Length` and let HTTP/1 select
//!   chunked framing; successful completion may keep the connection reusable;
//! - `HEAD` and 1xx/204/205/304 never poll the stream; dropping releases the
//!   producer promptly.
//!
//! `handler_timeout` bounds only the service future (time to produce the
//! `Response`), not the subsequent body stream. Streaming is bounded by
//! `connection_total_timeout` and shutdown. Plan 164 will add
//! write/no-progress controls.

use std::fmt;

use super::body::BodySource;
use super::header_block::{HeaderBlock, HeaderError, HeaderName, HeaderValue};
pub use super::response_stream::{ResponseStream, ResponseStreamError};

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

/// Errors from response construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseConstructionError {
    /// The status code is outside the valid 100–599 range.
    InvalidStatus(u16),
    /// A header name or value failed validation.
    InvalidHeader(HeaderError),
    /// A framing header (Transfer-Encoding, Content-Length) was provided by
    /// the handler and must be removed or rejected.
    ForbiddenFramingHeader(String),
    /// The response body was already consumed.
    BodyAlreadyConsumed,
    /// The content-length header does not match the actual body length.
    ContentLengthMismatch { declared: u64, actual: u64 },
    /// No file-stream admission permit was available.
    FileStreamLimit,
}

impl fmt::Display for ResponseConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStatus(code) => write!(f, "invalid status code: {}", code),
            Self::InvalidHeader(e) => write!(f, "invalid header: {}", e),
            Self::ForbiddenFramingHeader(name) => {
                write!(f, "forbidden framing header: {}", name)
            }
            Self::BodyAlreadyConsumed => write!(f, "response body already consumed"),
            Self::ContentLengthMismatch { declared, actual } => {
                write!(
                    f,
                    "content-length mismatch: declared {}, actual {}",
                    declared, actual
                )
            }
            Self::FileStreamLimit => write!(f, "file stream admission limit reached"),
        }
    }
}

impl std::error::Error for ResponseConstructionError {}

impl From<HeaderError> for ResponseConstructionError {
    fn from(e: HeaderError) -> Self {
        Self::InvalidHeader(e)
    }
}

/// A validated HTTP status code (100–599).
///
/// Wraps a `u16` with range enforcement at construction time. Reason phrases
/// are not stored — they are not authoritative application data per HTTP/1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatusCode(u16);

impl StatusCode {
    pub const CONTINUE: Self = Self(100);
    pub const SWITCHING_PROTOCOLS: Self = Self(101);
    pub const OK: Self = Self(200);
    pub const CREATED: Self = Self(201);
    pub const NO_CONTENT: Self = Self(204);
    pub const RESET_CONTENT: Self = Self(205);
    pub const NOT_MODIFIED: Self = Self(304);
    pub const MOVED_PERMANENTLY: Self = Self(301);
    pub const BAD_REQUEST: Self = Self(400);
    pub const FORBIDDEN: Self = Self(403);
    pub const NOT_FOUND: Self = Self(404);
    pub const METHOD_NOT_ALLOWED: Self = Self(405);
    pub const REQUEST_TIMEOUT: Self = Self(408);
    pub const PAYLOAD_TOO_LARGE: Self = Self(413);
    pub const RANGE_NOT_SATISFIABLE: Self = Self(416);
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);
    pub const SERVICE_UNAVAILABLE: Self = Self(503);

    /// Create a validated status code.
    ///
    /// # Errors
    ///
    /// Returns [`ResponseConstructionError::InvalidStatus`] if the code is
    /// outside 100–599. Only standard three-digit HTTP status codes are accepted.
    pub fn new(code: u16) -> Result<Self, ResponseConstructionError> {
        if !(100..=599).contains(&code) {
            return Err(ResponseConstructionError::InvalidStatus(code));
        }
        Ok(Self(code))
    }

    /// Returns the status code as a `u16`.
    pub fn as_u16(&self) -> u16 {
        self.0
    }

    /// Returns `true` if this is an informational (1xx) status.
    pub fn is_informational(&self) -> bool {
        (100..200).contains(&self.0)
    }

    /// Returns `true` if this is a success (2xx) status.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.0)
    }

    /// Returns `true` if this is a redirection (3xx) status.
    pub fn is_redirection(&self) -> bool {
        (300..400).contains(&self.0)
    }

    /// Returns `true` if this is a client-error (4xx) status.
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.0)
    }

    /// Returns `true` if this is a server-error (5xx) status.
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.0)
    }

    /// Returns `true` if this status permits a payload body per RFC 9110.
    ///
    /// Informational (1xx), 204 No Content, 205 Reset Content, and 304 Not
    /// Modified must not carry a payload body.
    pub fn permits_payload_body(&self) -> bool {
        !self.is_informational() && self.0 != 204 && self.0 != 205 && self.0 != 304
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<StatusCode> for u16 {
    fn from(s: StatusCode) -> u16 {
        s.0
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

/// A canonical HTTP response.
///
/// Combines a [`ResponseHead`] (status + headers) with a [`ResponseBody`].
/// The body is one-shot: consuming the response via [`normalize_response`] or
/// transport conversion consumes the body.
///
/// Normalization is idempotent: [`normalize_response`] sets an internal flag
/// and a second call is a no-op. This lets the static service normalize
/// eagerly while the connection pipeline normalizes every service response
/// without double-suppression losing a HEAD length or inventing
/// `Content-Length` for unknown-length streams. Mutating via
/// [`Response::head_mut`] or [`Response::take_body`] clears the flag.
///
/// # Construction
///
/// Use [`Response::builder()`] for validated construction, or convert from
/// existing types via `From`/`Into`.
pub struct Response {
    head: ResponseHead,
    body: Option<ResponseBody>,
    normalized: bool,
}

impl Response {
    /// Create a new response builder.
    pub fn builder() -> ResponseBuilder {
        ResponseBuilder {
            status: None,
            headers: HeaderBlock::new(),
        }
    }

    /// Returns a reference to the response head.
    pub fn head(&self) -> &ResponseHead {
        &self.head
    }

    /// Returns a mutable reference to the response head.
    ///
    /// This invalidates prior normalization: any header mutation requires a
    /// fresh normalize before transport.
    pub fn head_mut(&mut self) -> &mut ResponseHead {
        self.normalized = false;
        &mut self.head
    }

    /// Returns the status code.
    pub fn status(&self) -> StatusCode {
        self.head.status()
    }

    /// Returns a reference to the headers.
    pub fn headers(&self) -> &HeaderBlock {
        self.head.headers()
    }

    /// Returns true if normalize has been applied since last mutation.
    pub fn is_normalized(&self) -> bool {
        self.normalized
    }

    /// Take the body out of the response, leaving an empty body.
    ///
    /// Returns `None` if the body was already consumed. Invalidates prior
    /// normalization.
    pub fn take_body(&mut self) -> Option<ResponseBody> {
        self.normalized = false;
        self.body.take()
    }

    /// Returns a reference to the body, if present.
    pub fn body(&self) -> Option<&ResponseBody> {
        self.body.as_ref()
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("head", &self.head)
            .field("body", &self.body)
            .field("normalized", &self.normalized)
            .finish()
    }
}

/// Builder for constructing a [`Response`] with validated headers.
///
/// # Example
///
/// ```no_run
/// use eggserve_core::primitives::{Response, ResponseBody, StatusCode};
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let response = Response::builder()
///     .status(StatusCode::OK)
///     .header("content-type", "text/plain")?
///     .body(ResponseBody::Bytes(b"ok".to_vec()))?;
/// # let _ = response;
/// # Ok(())
/// # }
/// ```
pub struct ResponseBuilder {
    status: Option<StatusCode>,
    headers: HeaderBlock,
}

impl ResponseBuilder {
    /// Set the response status code.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    /// Add a validated header field.
    ///
    /// # Errors
    ///
    /// Returns an error if the header name or value is invalid.
    pub fn push_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push(name, value);
        self
    }

    /// Add a header from string slices, validating name and value.
    ///
    /// # Errors
    ///
    /// Returns an error if the header name or value is invalid (empty name,
    /// CR/LF/NUL in value, name exceeding 256 bytes).
    pub fn header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ResponseConstructionError> {
        let name = HeaderName::new(name)?;
        let value = HeaderValue::new(value)?;
        self.headers.push(name, value);
        Ok(self)
    }

    /// Build the response with the given body.
    ///
    /// # Errors
    ///
    /// Returns an error if no status code was set.
    pub fn body(self, body: ResponseBody) -> Result<Response, ResponseConstructionError> {
        let status = self
            .status
            .ok_or(ResponseConstructionError::InvalidStatus(0))?;
        Ok(Response {
            head: ResponseHead::new(status, self.headers),
            body: Some(body),
            normalized: false,
        })
    }

    /// Build the response with an empty body.
    pub fn empty(self) -> Result<Response, ResponseConstructionError> {
        self.body(ResponseBody::Empty)
    }
}

/// A normalization request describing the context for response normalization.
pub struct NormalizeRequest {
    /// Whether the original request was a HEAD request.
    pub is_head: bool,
}

impl NormalizeRequest {
    /// Create a new normalization request.
    pub fn new(is_head: bool) -> Self {
        Self { is_head }
    }
}

/// Normalize a response immediately before transport conversion.
///
/// This function applies the following rules:
///
/// 1. **HEAD suppression**: HEAD responses transmit no body bytes while
///    preserving representation headers appropriate to the equivalent GET.
///    The application stream is dropped without polling, releasing producer
///    resources promptly. A known equivalent-GET length is preserved as
///    `Content-Length`; an unknown length omits `Content-Length` (never
///    invents `0`).
/// 2. **Body-forbidden statuses**: 1xx, 204, 205, and 304 responses transmit
///    no payload body. Any provided body (including streams) is dropped
///    without polling.
/// 3. **Hop-by-hop header removal**: `Transfer-Encoding` (and all hop-by-hop
///    headers) is stripped — it is runtime-owned.
/// 4. **Content-Length computation**: `Content-Length` is set from the
///    representation length when known (`Known`), omitted when unknown
///    (`Unknown`). Unknown never becomes `Content-Length: 0`.
/// 5. **Conflicting framing rejection**: service-provided framing is removed
///    centrally; the runtime is the only framing authority.
///
/// Normalization is idempotent: a second call is a no-op. Mutating via
/// `head_mut`/`take_body` clears the flag.
///
/// Tests must prove a stream with side effects is not polled for
/// HEAD/body-forbidden responses.
///
/// # Errors
///
/// Returns an error if the response body was already consumed.
pub fn normalize_response(
    mut response: Response,
    request: &NormalizeRequest,
) -> Result<Response, ResponseConstructionError> {
    if response.normalized {
        return Ok(response);
    }
    let status = response.status();

    // Representation length before suppression (equivalent-GET length for
    // HEAD). For streams this is Known or Unknown; dropping below never polls.
    let pre_length: BodyLength = response
        .body
        .as_ref()
        .map(|b| b.body_length())
        .unwrap_or(BodyLength::Known(0));

    // Rule 1: HEAD suppression — drop without polling, preserve length.
    // A known representation length is retained via `EmptyWithLength` so
    // downstream consumers (metrics, adapter boundaries) still observe the
    // equivalent-GET length; unknown lengths stay `Empty` (no invented 0).
    if request.is_head {
        response.body = Some(match &pre_length {
            BodyLength::Known(len) if status.permits_payload_body() => {
                ResponseBody::EmptyWithLength(*len)
            }
            _ => ResponseBody::Empty,
        });
    }

    // Rule 2: Body-forbidden statuses — drop without polling.
    // All body-forbidden statuses except 304 have length zeroed so the
    // invariant `!permits_payload_body && status != 304 => Known(0)` holds.
    // For 304 the pre-suppression length (Known or Unknown) is retained and
    // validated in `normalize_metadata`; for 1xx/204/205 it is forced to
    // Known(0) so future changes to `permits_payload_body` cannot emit stale
    // framing. Dropping a suppressed stream releases producer resources.
    let mut body_len = pre_length;
    if !status.permits_payload_body() {
        response.body = Some(ResponseBody::Empty);
        if status != StatusCode::NOT_MODIFIED {
            body_len = BodyLength::Known(0);
        }
    }

    // Apply shared metadata normalization.
    // `head` is private to this module so direct field access is used here;
    // external callers must go through `head_mut` (which clears `normalized`).
    normalize_metadata(status, response.head.headers_mut(), body_len)?;

    response.normalized = true;
    Ok(response)
}

/// Normalize response metadata without consuming a response body.
///
/// This is the shared normalization entry point for both in-memory,
/// file-backed, and streaming response producers. `normalize_metadata` itself
/// is HEAD-agnostic: callers MUST pass the would-have-been-sent representation
/// length (the pre-suppression length for HEAD, i.e. the equivalent GET
/// length) as `body_length`. The function then applies:
///
/// 1. Strip runtime-owned framing (all hop-by-hop headers, including
///    `Transfer-Encoding`). Service-provided `Transfer-Encoding` remains
///    forbidden/stripped as runtime-owned.
/// 2. Payload-permitting statuses (including HEAD): set `Content-Length` to
///    the known length, retaining the representation length even for
///    zero-length bodies. When the length is `Unknown` (streaming), omit
///    `Content-Length` and let HTTP/1 select chunked framing — never invent
///    `Content-Length: 0`. HEAD callers pass the pre-suppression length so
///    the header is correct; unknown HEAD lengths omit the header.
/// 3. Body-forbidden statuses (1xx, 204, 205, 304): suppress `Content-Length`,
///    except that 304 may retain a matching representation length. A
///    caller-supplied `Content-Length` on 205 is rejected because RFC 9110
///    forbids it entirely.
/// 4. Preserve all other headers (including duplicates).
///
/// # Response architecture
///
/// All response producers must converge on `normalize_metadata()` for
/// response metadata and framing. The allowed sequences are:
///
/// ```text
/// // For in-memory bodies:
/// producer -> Response -> normalize_response() -> to_hyper_response()
///
/// // For file-backed bodies:
/// producer -> normalize_metadata(headers, body_len) -> streaming transport
///
/// // For streaming bodies:
/// producer -> Response(Stream) -> normalize_response() -> to_hyper_response()
/// ```
///
/// `normalize_metadata()` enforces:
/// - Transfer-Encoding is always stripped (runtime-owned)
/// - Content-Length is set from actual body length for known payload-permitting
///   responses (including HEAD with known length, including zero)
/// - Content-Length is omitted for unknown-length payload-permitting responses
/// - Content-Length is suppressed for body-forbidden (1xx/204/205/304)
///   responses, except for a matching 304 representation length
///
/// Callers MUST supply the would-have-been-sent representation length, which
/// is computed before suppressing a HEAD body. Passing a suppressed body's
/// length emits the wrong `Content-Length` for HEAD. Unknown lengths must be
/// passed as `BodyLength::Unknown`, never as `0`.
pub fn normalize_metadata(
    status: StatusCode,
    headers: &mut HeaderBlock,
    body_length: impl Into<BodyLength>,
) -> Result<(), ResponseConstructionError> {
    let body_length: BodyLength = body_length.into();
    // Rule 1: Strip all hop-by-hop headers.
    strip_hop_by_hop(headers);

    if status == StatusCode::RESET_CONTENT && headers.contains("content-length") {
        return Err(ResponseConstructionError::ForbiddenFramingHeader(
            "content-length".to_owned(),
        ));
    }

    // A 304 may retain the selected representation's length, but only when the
    // supplied value is unique, valid, and matches the planned representation.
    // Unknown lengths never retain: no Content-Length is invented. Non-UTF-8
    // values cannot be valid decimal lengths, so they yield no retention.
    let not_modified_length = if status == StatusCode::NOT_MODIFIED {
        match body_length {
            BodyLength::Known(known) => headers
                .get_unique("content-length")
                .map_err(|_| {
                    ResponseConstructionError::ForbiddenFramingHeader("content-length".to_owned())
                })?
                .and_then(|value| value.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|length| *length == known),
            BodyLength::Unknown => None,
        }
    } else {
        None
    };

    // Rule 2-4: Content-Length handling.
    remove_header(headers, "content-length");

    if status.permits_payload_body() {
        match body_length {
            BodyLength::Known(length) => {
                headers
                    .push_str("content-length", length.to_string())
                    .map_err(ResponseConstructionError::from)?;
            }
            BodyLength::Unknown => {
                // Omit: HTTP/1 transport selects chunked framing.
            }
        }
    } else if let Some(length) = not_modified_length {
        headers
            .push_str("content-length", length.to_string())
            .map_err(ResponseConstructionError::from)?;
    }

    Ok(())
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
fn remove_header(headers: &mut HeaderBlock, name: &str) {
    headers.retain(|f| !f.name.as_str().eq_ignore_ascii_case(name));
}

/// Remove all hop-by-hop headers from the block.
fn strip_hop_by_hop(headers: &mut HeaderBlock) {
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

/// Convert a canonical [`Response`] into the explicit Hyper transport boundary.
///
/// This is the final step after normalization. The response body is consumed.
/// The returned body type is intentionally opaque: downstream code should
/// depend on its `http_body::Body` behavior, not on a concrete erasure type.
/// The one-owner response-stream model remains `Send` without requiring
/// producer `Sync`; concurrent body polling is unsupported.
pub fn to_hyper_response(
    response: Response,
) -> Result<
    hyper::Response<impl http_body::Body<Data = bytes::Bytes, Error = std::io::Error>>,
    ResponseConstructionError,
> {
    to_hyper_response_with_optional_file_stream_semaphore(
        response,
        None,
        crate::limits::DEFAULT_STREAM_CHUNK_SIZE,
    )
}

/// Convert a canonical response while enforcing the runtime file-stream
/// admission limit for every file-backed body.
#[allow(dead_code)]
pub(crate) fn to_hyper_response_with_file_stream_semaphore(
    response: Response,
    semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
) -> Result<
    hyper::Response<http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>>,
    ResponseConstructionError,
> {
    to_hyper_response_with_optional_file_stream_semaphore(
        response,
        Some(semaphore),
        crate::limits::DEFAULT_STREAM_CHUNK_SIZE,
    )
}

/// Convert a canonical response using a configured file-stream chunk size.
pub(crate) fn to_hyper_response_with_file_stream_semaphore_and_chunk_size(
    response: Response,
    semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
) -> Result<
    hyper::Response<http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>>,
    ResponseConstructionError,
> {
    to_hyper_response_with_optional_file_stream_semaphore(
        response,
        Some(semaphore),
        stream_chunk_size,
    )
}

fn to_hyper_response_with_optional_file_stream_semaphore(
    response: Response,
    semaphore: Option<&std::sync::Arc<tokio::sync::Semaphore>>,
    stream_chunk_size: usize,
) -> Result<
    hyper::Response<http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>>,
    ResponseConstructionError,
> {
    use bytes::Bytes;
    use http_body_util::BodyExt;
    use http_body_util::Full;

    let status = response.status();
    let code = status.as_u16();
    let hyper_status = hyper::StatusCode::from_u16(code)
        .map_err(|_| ResponseConstructionError::InvalidStatus(code))?;

    let mut builder = hyper::Response::builder().status(hyper_status);
    for field in response.head.headers().iter() {
        // Byte-preserving outbound conversion: canonical bytes are already in
        // the transport-accepted domain, so this preserves exact octets
        // without UTF-8 coercion. Framing/privacy stripping already ran in
        // normalization; this step does not bypass response policy.
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidName))?;
        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
            .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))?;
        builder = builder.header(name, value);
    }

    let body = match response.body {
        Some(ResponseBody::Empty) => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
        Some(ResponseBody::Bytes(b)) => Full::new(Bytes::from(b))
            .map_err(|never| match never {})
            .boxed_unsync(),
        Some(ResponseBody::File(source)) => {
            let permit = semaphore
                .map(|s| s.clone().try_acquire_owned())
                .transpose()
                .map_err(|_| ResponseConstructionError::FileStreamLimit)?;
            let permit = permit.map(CountingFileStreamPermit::new);
            file_body(source, permit, stream_chunk_size)
        }
        Some(ResponseBody::Stream(stream)) => {
            // Defense-in-depth: body-forbidden statuses must never emit
            // application bytes even if a caller forgot `normalize_response`.
            // HEAD suppression requires request context and must happen in
            // `normalize_response`/pipeline; here we only guard statuses.
            if !status.permits_payload_body() {
                drop(stream);
                Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed_unsync()
            } else {
                stream_body(stream, stream_chunk_size)
            }
        }
        Some(ResponseBody::EmptyWithLength(_)) => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
        None => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
    };

    let mut response = builder
        .body(body)
        .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))?;
    crate::response::finalize_origin_headers(&mut response, std::time::SystemTime::now());
    Ok(response)
}

fn file_body(
    source: BodySource,
    permit: Option<CountingFileStreamPermit>,
    stream_chunk_size: usize,
) -> http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error> {
    // B-03 note: each chunk iteration allocates a fresh `vec![0; chunk_len]`
    // (moved into `Bytes`). For the default 8 KiB chunk size the cost is modest;
    // for large `stream_chunk_size` (up to 1 MiB) the allocation rate scales with
    // `file_len / chunk_size`. `BytesMut` reuse would reduce this but is a
    // pure optimization with no correctness impact (see `benchmarks/088-baseline`).
    use bytes::Bytes;
    use futures_util::stream;
    use http_body_util::{BodyExt, StreamBody};
    use hyper::body::Frame;
    use tokio::io::AsyncSeekExt;

    let (file, start, remaining) = match source {
        BodySource::FileFull { file, len, .. } => (tokio::fs::File::from_std(file), 0, len),
        BodySource::FileRange { file, range, .. } => {
            (tokio::fs::File::from_std(file), range.start(), range.len())
        }
        BodySource::Empty => {
            return http_body_util::Full::new(Bytes::new())
                .map_err(|never| match never {})
                .boxed_unsync();
        }
        BodySource::Bytes(bytes) => {
            return http_body_util::Full::new(Bytes::from(bytes))
                .map_err(|never| match never {})
                .boxed_unsync();
        }
    };

    let stream = stream::unfold(
        (file, start, remaining, start > 0, permit),
        move |(mut file, offset, remaining, needs_seek, permit)| async move {
            if remaining == 0 {
                return None;
            }
            if needs_seek {
                if let Err(error) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    return Some((Err(error), (file, offset, 0, false, permit)));
                }
            }
            let chunk_len = remaining.min(stream_chunk_size as u64) as usize;
            let mut buffer = vec![0; chunk_len];
            match read_file_chunk(&mut file, &mut buffer).await {
                Ok(0) => Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "file ended before the advertised response length",
                    )),
                    (file, offset, 0, false, permit),
                )),
                Ok(bytes_read) => {
                    if bytes_read < chunk_len {
                        return Some((
                            Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "file ended before the advertised response length",
                            )),
                            (file, offset, 0, false, permit),
                        ));
                    }
                    let next_remaining = remaining - bytes_read as u64;
                    buffer.truncate(bytes_read);
                    Some((
                        Ok(Frame::data(Bytes::from(buffer))),
                        (
                            file,
                            offset + bytes_read as u64,
                            next_remaining,
                            false,
                            permit,
                        ),
                    ))
                }
                Err(error) => Some((Err(error), (file, offset, 0, false, permit))),
            }
        },
    );
    StreamBody::new(stream).boxed_unsync()
}

async fn read_file_chunk(file: &mut tokio::fs::File, buffer: &mut [u8]) -> std::io::Result<usize> {
    use tokio::io::AsyncReadExt;

    // `tokio::io::AsyncReadExt::read` retries `Interrupted` internally, so no
    // explicit handling is needed here. `Ok(0)` is treated as EOF per the
    // `AsyncRead` contract and bubbled as `UnexpectedEof` by the caller.
    let mut bytes_read = 0;
    while bytes_read < buffer.len() {
        let count = file.read(&mut buffer[bytes_read..]).await?;
        if count == 0 {
            break;
        }
        bytes_read += count;
    }
    Ok(bytes_read)
}

/// Convert a transport-independent [`ResponseStream`] into a Hyper body.
///
/// Contract:
/// - pull/backpressure driven: polls the producer only when downstream is
///   ready; no unbounded channel;
/// - empty chunks skipped (never emit empty DATA frames);
/// - chunks larger than `stream_chunk_size` split zero-copy via `Bytes`
///   (not rejected), keeping downstream framing bounded;
/// - known-length overrun/underrun and producer failure close the connection
///   after commitment (Hyper closes on body error); no second HTTP error is
///   attempted and no producer detail reaches the client;
/// - panics while polling are contained at this task boundary, counted
///   separately, and close deterministically with a sanitized event;
/// - cancellation (client disconnect/shutdown) drops the producer promptly
///   and is counted via `Drop` when the stream never completed.
fn stream_body(
    stream: ResponseStream,
    stream_chunk_size: usize,
) -> http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error> {
    use http_body_util::{BodyExt, StreamBody};
    crate::ops::global_counters()
        .streaming_started
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Debug,
        crate::ops::EventKind::ResponseStreamStarted,
        "streaming response started",
    ));
    let adapter = ResponseStreamAdapter::new(stream, stream_chunk_size);
    StreamBody::new(adapter).boxed_unsync()
}

#[allow(clippy::type_complexity)]
struct ResponseStreamAdapter {
    inner: Option<
        StdPin<
            Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, ResponseStreamError>> + Send>,
        >,
    >,
    declared: Option<u64>,
    emitted: u64,
    chunk_size: usize,
    pending_split: Option<bytes::Bytes>,
    finished: bool,
}

use std::pin::Pin as StdPin;
use std::task::{Context as TaskContext, Poll as TaskPoll};

impl ResponseStreamAdapter {
    fn new(stream: ResponseStream, chunk_size: usize) -> Self {
        let declared = stream.known_length();
        let chunk_size = chunk_size.max(1);
        Self {
            inner: Some(stream.into_inner()),
            declared,
            emitted: 0,
            chunk_size,
            pending_split: None,
            finished: false,
        }
    }

    fn fail_length_mismatch(&mut self, emitted: u64) -> std::io::Error {
        self.finished = true;
        crate::ops::global_counters()
            .stream_length_mismatches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::ops::Logger::global().emit(
            crate::ops::Event::new(
                crate::ops::Severity::Warn,
                crate::ops::EventKind::ResponseStreamLengthMismatch,
                "streaming response length mismatch; closing connection",
            )
            .field(crate::ops::Field::U64(
                "declared_bytes".into(),
                self.declared.unwrap_or(0),
            ))
            .field(crate::ops::Field::U64("emitted_bytes".into(), emitted)),
        );
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "response stream length mismatch",
        )
    }

    fn fail_producer(&mut self) -> std::io::Error {
        self.finished = true;
        crate::ops::global_counters()
            .stream_producer_errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Warn,
            crate::ops::EventKind::ResponseStreamProducerError,
            "streaming response producer failed; closing connection",
        ));
        std::io::Error::other("response stream failed")
    }

    fn fail_panic(&mut self) -> std::io::Error {
        self.finished = true;
        crate::ops::global_counters()
            .stream_producer_panics
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Error,
            crate::ops::EventKind::ResponseStreamProducerPanic,
            "streaming response producer panicked; closing connection",
        ));
        std::io::Error::other("response stream failed")
    }

    fn complete_ok(&mut self) {
        self.finished = true;
        crate::ops::global_counters()
            .streaming_completed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::ResponseStreamCompleted,
            "streaming response completed",
        ));
    }

    fn next_split_piece(&mut self) -> Option<bytes::Bytes> {
        let mut pending = self.pending_split.take()?;
        if pending.len() <= self.chunk_size {
            return Some(pending);
        }
        // `Bytes::split_off(at)`: self keeps [..at], returned is [at..].
        let remainder = pending.split_off(self.chunk_size);
        self.pending_split = Some(remainder);
        Some(pending)
    }
}

impl Drop for ResponseStreamAdapter {
    fn drop(&mut self) {
        if !self.finished {
            crate::ops::global_counters()
                .stream_cancelled
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            crate::ops::Logger::global().emit(crate::ops::Event::new(
                crate::ops::Severity::Debug,
                crate::ops::EventKind::ResponseStreamCancelled,
                "streaming response cancelled",
            ));
        }
    }
}

impl futures_util::Stream for ResponseStreamAdapter {
    type Item = Result<hyper::body::Frame<bytes::Bytes>, std::io::Error>;

    fn poll_next(
        mut self: StdPin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> TaskPoll<Option<Self::Item>> {
        // Emit any pending split remainder first (no producer poll → no busy loop).
        if self.pending_split.is_some() {
            if let Some(piece) = self.next_split_piece() {
                let len = piece.len() as u64;
                let emitted = self.emitted.saturating_add(len);
                if let Some(declared) = self.declared {
                    if emitted > declared {
                        let err = self.fail_length_mismatch(emitted);
                        return TaskPoll::Ready(Some(Err(err)));
                    }
                }
                self.emitted = emitted;
                return TaskPoll::Ready(Some(Ok(hyper::body::Frame::data(piece))));
            }
        }
        if self.finished {
            return TaskPoll::Ready(None);
        }
        let inner = match self.inner.as_mut() {
            Some(i) => i,
            None => return TaskPoll::Ready(None),
        };
        // Contain producer panics at this task boundary.
        let polled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inner.as_mut().poll_next(cx)
        }));
        let next = match polled {
            Ok(n) => n,
            Err(_) => {
                let err = self.fail_panic();
                return TaskPoll::Ready(Some(Err(err)));
            }
        };
        match next {
            TaskPoll::Pending => TaskPoll::Pending,
            TaskPoll::Ready(None) => {
                // End of producer stream.
                if let Some(declared) = self.declared {
                    if self.emitted != declared {
                        let emitted = self.emitted;
                        let err = self.fail_length_mismatch(emitted);
                        return TaskPoll::Ready(Some(Err(err)));
                    }
                }
                self.complete_ok();
                TaskPoll::Ready(None)
            }
            TaskPoll::Ready(Some(Ok(chunk))) => {
                if chunk.is_empty() {
                    // Skip empty chunks: wake immediately for next poll
                    // without emitting a frame. Producers that spam empty
                    // chunks synchronously will spin here — that is a
                    // producer bug; the contract says empty chunks are
                    // skipped and producers should avoid them.
                    cx.waker().wake_by_ref();
                    return TaskPoll::Pending;
                }
                // Split large chunks zero-copy instead of rejecting.
                let mut chunk = chunk;
                if chunk.len() > self.chunk_size {
                    let remainder = chunk.split_off(self.chunk_size);
                    self.pending_split = Some(remainder);
                    // `split_off` keeps [..chunk_size] in `chunk`? For
                    // `Bytes`, `split_off(at)` returns [at..] and keeps
                    // [..at] in self. So `chunk` is now the first piece.
                }
                let len = chunk.len() as u64;
                let emitted = self.emitted.saturating_add(len);
                if let Some(declared) = self.declared {
                    if emitted > declared {
                        // Buffer the overrun remainder? No — overrun is
                        // fatal; drop pending split to avoid reuse ambiguity.
                        self.pending_split = None;
                        let err = self.fail_length_mismatch(emitted);
                        return TaskPoll::Ready(Some(Err(err)));
                    }
                }
                self.emitted = emitted;
                TaskPoll::Ready(Some(Ok(hyper::body::Frame::data(chunk))))
            }
            TaskPoll::Ready(Some(Err(_detail))) => {
                // Never serialize producer detail to the client; wire sees
                // only a generic failure that closes the connection.
                let err = self.fail_producer();
                TaskPoll::Ready(Some(Err(err)))
            }
        }
    }
}

struct CountingFileStreamPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl CountingFileStreamPermit {
    fn new(permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        crate::ops::global_counters()
            .active_file_streams
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { _permit: permit }
    }
}

impl Drop for CountingFileStreamPermit {
    fn drop(&mut self) {
        crate::ops::global_counters()
            .active_file_streams
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::FileRange;
    use http_body_util::BodyExt;
    use std::fs::File;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn file_response(path: &std::path::Path, range: Option<FileRange>) -> Response {
        let file = File::open(path).unwrap();
        let metadata = file.metadata().unwrap();
        let source = match range {
            Some(range) => BodySource::FileRange {
                file,
                range,
                total_len: metadata.len(),
                mime: "application/octet-stream",
            },
            None => BodySource::FileFull {
                file,
                len: metadata.len(),
                mime: "application/octet-stream",
            },
        };
        Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::File(source))
            .unwrap()
    }

    #[tokio::test]
    async fn full_file_transport_body_owns_permit_until_drop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("full.bin");
        std::fs::write(&path, b"full body").unwrap();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let first =
            to_hyper_response_with_file_stream_semaphore(file_response(&path, None), &semaphore)
                .unwrap();
        assert!(matches!(
            to_hyper_response_with_file_stream_semaphore(file_response(&path, None), &semaphore),
            Err(ResponseConstructionError::FileStreamLimit)
        ));

        drop(first);
        assert!(to_hyper_response_with_file_stream_semaphore(
            file_response(&path, None),
            &semaphore
        )
        .is_ok());
    }

    #[tokio::test]
    async fn range_file_transport_body_releases_permit_on_completion() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("range.bin");
        std::fs::write(&path, b"range body").unwrap();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let response = to_hyper_response_with_file_stream_semaphore(
            file_response(&path, Some(FileRange::new(0, 4))),
            &semaphore,
        )
        .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"range");
        assert!(to_hyper_response_with_file_stream_semaphore(
            file_response(&path, Some(FileRange::new(5, 9))),
            &semaphore
        )
        .is_ok());
    }

    #[tokio::test]
    async fn file_transport_uses_configured_chunk_size() {
        use futures_util::StreamExt;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("chunked.bin");
        std::fs::write(&path, vec![b'x'; 130]).unwrap();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let response = to_hyper_response_with_file_stream_semaphore_and_chunk_size(
            file_response(&path, None),
            &semaphore,
            64,
        )
        .unwrap();

        let mut body = response.into_body().into_data_stream();
        let mut chunk_lengths = Vec::new();
        while let Some(chunk) = body.next().await {
            chunk_lengths.push(chunk.unwrap().len());
        }
        assert_eq!(chunk_lengths, [64, 64, 2]);
    }

    #[tokio::test]
    async fn truncated_file_transport_reports_unexpected_eof() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("truncated.bin");
        std::fs::write(&path, b"short").unwrap();
        let file = File::open(&path).unwrap();
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::File(BodySource::FileFull {
                file,
                len: 10,
                mime: "application/octet-stream",
            }))
            .unwrap();

        let error = to_hyper_response(response)
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn non_file_and_normalized_head_bodies_bypass_file_admission() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let held = semaphore.clone().try_acquire_owned().unwrap();

        for body in [
            ResponseBody::Bytes(b"bytes".to_vec()),
            ResponseBody::Empty,
            ResponseBody::EmptyWithLength(5),
        ] {
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(body)
                .unwrap();
            assert!(to_hyper_response_with_file_stream_semaphore(response, &semaphore).is_ok());
        }

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("head.bin");
        std::fs::write(&path, b"head body").unwrap();
        let normalized =
            normalize_response(file_response(&path, None), &NormalizeRequest::new(true)).unwrap();
        assert!(to_hyper_response_with_file_stream_semaphore(normalized, &semaphore).is_ok());

        drop(held);
    }

    #[test]
    fn status_code_valid_range() {
        assert!(StatusCode::new(100).is_ok());
        assert!(StatusCode::new(200).is_ok());
        assert!(StatusCode::new(600).is_err());
    }

    #[test]
    fn status_code_zero_rejected() {
        assert!(StatusCode::new(0).is_err());
    }

    #[test]
    fn status_code_below_100_rejected() {
        assert!(StatusCode::new(1).is_err());
        assert!(StatusCode::new(42).is_err());
        assert!(StatusCode::new(99).is_err());
    }

    #[test]
    fn status_code_over_599_rejected() {
        assert!(StatusCode::new(600).is_err());
        assert!(StatusCode::new(1000).is_err());
    }

    #[test]
    fn status_code_boundary_values() {
        assert!(StatusCode::new(100).is_ok());
        assert!(StatusCode::new(199).is_ok());
        assert!(StatusCode::new(200).is_ok());
        assert!(StatusCode::new(599).is_ok());
    }

    #[test]
    fn status_code_classification() {
        assert!(StatusCode::CONTINUE.is_informational());
        assert!(!StatusCode::OK.is_informational());
        assert!(StatusCode::OK.is_success());
        assert!(StatusCode::NOT_MODIFIED.is_redirection());
        assert!(StatusCode::BAD_REQUEST.is_client_error());
        assert!(StatusCode::INTERNAL_SERVER_ERROR.is_server_error());
    }

    #[test]
    fn status_code_permits_payload() {
        assert!(!StatusCode::CONTINUE.permits_payload_body());
        assert!(!StatusCode::NO_CONTENT.permits_payload_body());
        assert!(!StatusCode::NOT_MODIFIED.permits_payload_body());
        assert!(!StatusCode::new(205).unwrap().permits_payload_body());
        assert!(StatusCode::OK.permits_payload_body());
        assert!(StatusCode::RANGE_NOT_SATISFIABLE.permits_payload_body());
    }

    #[test]
    fn response_body_len() {
        assert_eq!(ResponseBody::Empty.len(), 0);
        assert_eq!(ResponseBody::Bytes(b"hello".to_vec()).len(), 5);
    }

    #[test]
    fn response_body_into_bytes() {
        assert!(ResponseBody::Empty.into_bytes().is_none());
        assert_eq!(
            ResponseBody::Bytes(b"hi".to_vec()).into_bytes(),
            Some(b"hi".to_vec())
        );
    }

    #[test]
    fn response_builder_creates_response() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .unwrap()
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap();

        assert_eq!(resp.status().as_u16(), 200);
        assert_eq!(
            resp.headers()
                .get_first("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn response_builder_empty_body() {
        let resp = Response::builder()
            .status(StatusCode::NO_CONTENT)
            .empty()
            .unwrap();
        assert_eq!(resp.status().as_u16(), 204);
        assert!(resp.body().unwrap().is_empty());
    }

    #[test]
    fn response_builder_no_status_returns_error() {
        let result = Response::builder()
            .header("content-type", "text/plain")
            .unwrap()
            .empty();
        assert!(result.is_err());
    }

    #[test]
    fn response_builder_invalid_header_name_rejected() {
        let result = Response::builder()
            .status(StatusCode::OK)
            .header("", "value");
        assert!(result.is_err());
    }

    #[test]
    fn response_builder_invalid_header_value_rejected() {
        let result = Response::builder()
            .status(StatusCode::OK)
            .header("x-test", "val\r\ninjection");
        assert!(result.is_err());
    }

    #[test]
    fn normalize_head_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(true);
        let normalized = normalize_response(resp, &req).unwrap();
        // No bytes are sent for HEAD, but the equivalent-GET representation
        // length is retained so consumers still observe it.
        assert!(matches!(
            normalized.body().unwrap(),
            ResponseBody::EmptyWithLength(5)
        ));
        assert_eq!(normalized.body().unwrap().len(), 5);
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_head_unknown_length_sends_no_body_and_omits_length() {
        use futures_util::stream;
        let inner = stream::iter(vec![Ok::<_, ResponseStreamError>(
            bytes::Bytes::from_static(b"chunk"),
        )]);
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(inner)))
            .unwrap();

        let req = NormalizeRequest::new(true);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(matches!(normalized.body().unwrap(), ResponseBody::Empty));
        assert!(!normalized.headers().contains("content-length"));
    }

    #[test]
    fn normalize_304_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("etag", "W/\"123\"")
            .unwrap()
            .body(ResponseBody::Empty)
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert_eq!(normalized.status().as_u16(), 304);
        assert!(normalized.body().unwrap().is_empty());
    }

    #[test]
    fn normalize_304_discards_buffered_body_length() {
        let response = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("content-length", "5")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();
        let normalized = normalize_response(response, &NormalizeRequest::new(false)).unwrap();
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );

        let mismatched = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("content-length", "4")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();
        let normalized = normalize_response(mismatched, &NormalizeRequest::new(false)).unwrap();
        assert!(!normalized.headers().contains("content-length"));
    }

    #[test]
    fn normalize_head_304_preserves_representation_length() {
        let response = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("content-length", "10")
            .unwrap()
            .body(ResponseBody::Bytes(vec![b'x'; 10]))
            .unwrap();

        let normalized = normalize_response(response, &NormalizeRequest::new(true)).unwrap();
        assert!(normalized.body().unwrap().is_empty());
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "10"
        );
    }

    #[test]
    fn normalize_204_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(ResponseBody::Bytes(b"unexpected".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(normalized.body().unwrap().is_empty());
    }

    #[test]
    fn normalize_205_suppresses_body_and_content_length() {
        let resp = Response::builder()
            .status(StatusCode::RESET_CONTENT)
            .body(ResponseBody::Bytes(b"unexpected".to_vec()))
            .unwrap();
        let normalized = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
        assert!(normalized.body().unwrap().is_empty());
        assert!(!normalized.headers().contains("content-length"));
    }

    #[test]
    fn normalize_205_rejects_caller_content_length() {
        let response = Response::builder()
            .status(StatusCode::RESET_CONTENT)
            .header("content-length", "5")
            .unwrap()
            .body(ResponseBody::Empty)
            .unwrap();

        assert_eq!(
            normalize_response(response, &NormalizeRequest::new(false)).unwrap_err(),
            ResponseConstructionError::ForbiddenFramingHeader("content-length".to_owned())
        );
    }

    #[test]
    fn normalize_strips_transfer_encoding() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("transfer-encoding", "chunked")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(!normalized.headers().contains("transfer-encoding"));
    }

    #[test]
    fn normalize_sets_content_length() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_1xx_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::CONTINUE)
            .body(ResponseBody::Bytes(b"data".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(normalized.body().unwrap().is_empty());
    }

    #[test]
    fn normalize_duplicate_headers_preserved() {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap();
        resp.head.headers.push_str("set-cookie", "a=1").unwrap();
        resp.head.headers.push_str("set-cookie", "b=2").unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        let all = normalized.headers().get_all("set-cookie");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn response_construction_error_display() {
        let err = ResponseConstructionError::InvalidStatus(0);
        assert!(err.to_string().contains("0"));

        let err = ResponseConstructionError::ForbiddenFramingHeader("transfer-encoding".into());
        assert!(err.to_string().contains("transfer-encoding"));

        let err = ResponseConstructionError::BodyAlreadyConsumed;
        assert!(!err.to_string().is_empty());

        let err = ResponseConstructionError::ContentLengthMismatch {
            declared: 100,
            actual: 50,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("50"));
    }

    #[test]
    fn status_code_display() {
        assert_eq!(format!("{}", StatusCode::OK), "200");
        assert_eq!(format!("{}", StatusCode::NOT_FOUND), "404");
    }

    #[test]
    fn status_code_into_u16() {
        let code: u16 = StatusCode::OK.into();
        assert_eq!(code, 200);
    }

    #[test]
    fn is_hop_by_hop_header_recognizes_all_variants() {
        assert!(is_hop_by_hop_header("connection"));
        assert!(is_hop_by_hop_header("Connection"));
        assert!(is_hop_by_hop_header("CONNECTION"));
        assert!(is_hop_by_hop_header("keep-alive"));
        assert!(is_hop_by_hop_header("Keep-Alive"));
        assert!(is_hop_by_hop_header("proxy-authenticate"));
        assert!(is_hop_by_hop_header("proxy-authorization"));
        assert!(is_hop_by_hop_header("proxy-connection"));
        assert!(is_hop_by_hop_header("te"));
        assert!(is_hop_by_hop_header("TE"));
        assert!(is_hop_by_hop_header("trailer"));
        assert!(is_hop_by_hop_header("Trailer"));
        assert!(is_hop_by_hop_header("transfer-encoding"));
        assert!(is_hop_by_hop_header("Transfer-Encoding"));
        assert!(is_hop_by_hop_header("upgrade"));
        assert!(is_hop_by_hop_header("Upgrade"));
    }

    #[test]
    fn is_hop_by_hop_header_rejects_end_to_end() {
        assert!(!is_hop_by_hop_header("content-type"));
        assert!(!is_hop_by_hop_header("content-length"));
        assert!(!is_hop_by_hop_header("host"));
        assert!(!is_hop_by_hop_header("set-cookie"));
        assert!(!is_hop_by_hop_header("etag"));
        assert!(!is_hop_by_hop_header("authorization"));
        assert!(!is_hop_by_hop_header("cache-control"));
    }

    #[test]
    fn normalize_metadata_strips_all_hop_by_hop() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-type", "text/plain").unwrap();
        headers.push_str("transfer-encoding", "chunked").unwrap();
        headers.push_str("connection", "keep-alive").unwrap();
        headers.push_str("trailer", "x-checksum").unwrap();
        headers.push_str("upgrade", "h2c").unwrap();
        headers.push_str("te", "deflate").unwrap();

        normalize_metadata(code, &mut headers, 5).unwrap();

        assert!(!headers.contains("transfer-encoding"));
        assert!(!headers.contains("connection"));
        assert!(!headers.contains("trailer"));
        assert!(!headers.contains("upgrade"));
        assert!(!headers.contains("te"));
        assert!(headers.contains("content-type"));
        assert_eq!(
            headers
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_metadata_strips_connection_nominated_headers() {
        let mut headers = HeaderBlock::new();
        headers
            .push_str("Connection", "keep-alive, X-Secret")
            .unwrap();
        headers.push_str("X-Secret", "private").unwrap();
        headers.push_str("x-visible", "public").unwrap();

        normalize_metadata(StatusCode::OK, &mut headers, 0).unwrap();

        assert!(!headers.contains("connection"));
        assert!(!headers.contains("x-secret"));
        assert!(headers.contains("x-visible"));
    }

    #[test]
    fn duplicate_content_length_replaced_by_normalized_value() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "999").unwrap();
        headers.push_str("content-length", "888").unwrap();

        normalize_metadata(code, &mut headers, 42).unwrap();

        let all_cl = headers.get_all("content-length");
        assert_eq!(all_cl.len(), 1, "only one Content-Length must remain");
        assert_eq!(all_cl[0].to_str().unwrap(), "42");
    }

    #[test]
    fn duplicate_content_length_rejected_for_not_modified() {
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "42").unwrap();
        headers.push_str("Content-Length", "42").unwrap();

        let error = normalize_metadata(StatusCode::NOT_MODIFIED, &mut headers, 42).unwrap_err();
        assert_eq!(
            error,
            ResponseConstructionError::ForbiddenFramingHeader("content-length".to_owned())
        );
    }

    #[test]
    fn transfer_encoding_plus_content_length_strips_te() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("transfer-encoding", "chunked")
            .unwrap()
            .header("content-length", "100")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();

        assert!(!normalized.headers().contains("transfer-encoding"));
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_metadata_preserves_duplicate_set_cookie() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("set-cookie", "a=1").unwrap();
        headers.push_str("set-cookie", "b=2").unwrap();

        normalize_metadata(code, &mut headers, 0).unwrap();

        let all = headers.get_all("set-cookie");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].to_str().unwrap(), "a=1");
        assert_eq!(all[1].to_str().unwrap(), "b=2");
    }

    #[test]
    fn normalize_metadata_head_preserves_content_length_when_body_nonempty() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "100").unwrap();

        normalize_metadata(code, &mut headers, 100).unwrap();

        assert_eq!(
            headers
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "100",
            "HEAD with non-empty body must preserve Content-Length"
        );
    }

    #[test]
    fn normalize_metadata_head_preserves_zero_content_length_when_body_empty() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "100").unwrap();

        normalize_metadata(code, &mut headers, 0).unwrap();

        assert_eq!(
            headers
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "0",
            "HEAD with empty body must preserve zero Content-Length"
        );
    }
}
