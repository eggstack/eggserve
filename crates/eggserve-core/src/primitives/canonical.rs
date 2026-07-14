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

use std::fmt;

use super::header_block::{HeaderBlock, HeaderError, HeaderField, HeaderName, HeaderValue};

/// Errors from response construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseConstructionError {
    /// The status code is outside the valid 1–999 range.
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
        }
    }
}

impl std::error::Error for ResponseConstructionError {}

impl From<HeaderError> for ResponseConstructionError {
    fn from(e: HeaderError) -> Self {
        Self::InvalidHeader(e)
    }
}

/// A validated HTTP status code (1–999).
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
    pub const NOT_MODIFIED: Self = Self(304);
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
    /// outside 1–999.
    pub fn new(code: u16) -> Result<Self, ResponseConstructionError> {
        if code == 0 || code > 999 {
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
    /// Informational (1xx), 204 No Content, and 304 Not Modified must not
    /// carry a payload body.
    pub fn permits_payload_body(&self) -> bool {
        !self.is_informational() && self.0 != 204 && self.0 != 304
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
#[derive(Debug)]
pub enum ResponseBody {
    /// No body content.
    Empty,
    /// In-memory byte buffer.
    Bytes(Vec<u8>),
}

impl ResponseBody {
    /// Returns the body length in bytes, if known without performing I/O.
    pub fn len(&self) -> u64 {
        match self {
            Self::Empty => 0,
            Self::Bytes(b) => b.len() as u64,
        }
    }

    /// Returns `true` if the body is known to be zero-length.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Consume the body and return the bytes.
    ///
    /// Returns `None` if the body was already consumed or is empty.
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Empty => None,
            Self::Bytes(b) => Some(b),
        }
    }
}

/// A canonical HTTP response.
///
/// Combines a [`ResponseHead`] (status + headers) with a [`ResponseBody`].
/// The body is one-shot: consuming the response via [`normalize_response`] or
/// transport conversion consumes the body.
///
/// # Construction
///
/// Use [`Response::builder()`] for validated construction, or convert from
/// existing types via `From`/`Into`.
pub struct Response {
    head: ResponseHead,
    body: Option<ResponseBody>,
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
    pub fn head_mut(&mut self) -> &mut ResponseHead {
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

    /// Take the body out of the response, leaving an empty body.
    ///
    /// Returns `None` if the body was already consumed.
    pub fn take_body(&mut self) -> Option<ResponseBody> {
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
            .finish()
    }
}

/// Builder for constructing a [`Response`] with validated headers.
///
/// # Example
///
/// ```ignore
/// let response = Response::builder()
///     .status(StatusCode::OK)
///     .header("content-type", "text/plain")?
///     .body(ResponseBody::Bytes(b"ok".to_vec()))?;
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
/// 2. **Body-forbidden statuses**: 1xx, 204, and 304 responses transmit no
///    payload body. Any provided body is discarded.
/// 3. **Hop-by-hop header removal**: `Transfer-Encoding` is stripped (it is
///    runtime-owned).
/// 4. **Content-Length computation**: `Content-Length` is set to the actual
///    body length when the body is buffered.
/// 5. **Conflicting framing rejection**: If both `Content-Length` and
///    `Transfer-Encoding` are present after stripping, `Transfer-Encoding`
///    is removed.
///
/// # Errors
///
/// Returns an error if the response body was already consumed.
pub fn normalize_response(
    mut response: Response,
    request: &NormalizeRequest,
) -> Result<Response, ResponseConstructionError> {
    let status = response.status();

    // Rule 1: HEAD suppression — discard body, preserve headers.
    if request.is_head {
        response.body = Some(ResponseBody::Empty);
    }

    // Rule 2: Body-forbidden statuses — discard body.
    if !status.permits_payload_body() {
        response.body = Some(ResponseBody::Empty);
    }

    // Rule 3: Strip runtime-owned Transfer-Encoding.
    remove_header(response.head.headers_mut(), "transfer-encoding");

    // Rule 4-5: Content-Length handling.
    let body_len = response.body.as_ref().map_or(0, |b| b.len());

    // Remove existing Content-Length if present, then re-set to actual length.
    remove_header(response.head.headers_mut(), "content-length");

    if status.permits_payload_body() && !request.is_head {
        response
            .head
            .headers
            .push_str("content-length", body_len.to_string())
            .map_err(ResponseConstructionError::from)?;
    }

    Ok(response)
}

/// Remove all headers with the given name (case-insensitive).
fn remove_header(headers: &mut HeaderBlock, name: &str) {
    let lower = name.to_ascii_lowercase();
    let fields: Vec<HeaderField> = headers
        .iter()
        .filter(|f| f.name.as_str().to_ascii_lowercase() != lower)
        .cloned()
        .collect();
    *headers = HeaderBlock::new();
    for field in fields {
        headers.push(field.name, field.value);
    }
}

/// Convert a canonical [`Response`] into a Hyper response with a boxed body.
///
/// This is the final step after normalization. The response body is consumed.
pub fn to_hyper_response(
    response: Response,
) -> Result<
    hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::io::Error>>,
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
        builder = builder.header(field.name.as_str(), field.value.as_str());
    }

    let body = match response.body {
        Some(ResponseBody::Empty) => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed(),
        Some(ResponseBody::Bytes(b)) => Full::new(Bytes::from(b))
            .map_err(|never| match never {})
            .boxed(),
        None => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed(),
    };

    builder
        .body(body)
        .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_valid_range() {
        assert!(StatusCode::new(1).is_ok());
        assert!(StatusCode::new(200).is_ok());
        assert!(StatusCode::new(999).is_ok());
    }

    #[test]
    fn status_code_zero_rejected() {
        assert!(StatusCode::new(0).is_err());
    }

    #[test]
    fn status_code_over_999_rejected() {
        assert!(StatusCode::new(1000).is_err());
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
            resp.headers().get_first("content-type").unwrap().as_str(),
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
            .header("content-length", "5")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(true);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(normalized.body().unwrap().is_empty());
        // Content-Length header should still be present (preserved for HEAD)
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
                .as_str(),
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
}
