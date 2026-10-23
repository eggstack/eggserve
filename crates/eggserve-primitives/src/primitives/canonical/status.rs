//! Canonical status vocabulary (Plan 206 Track C).
//!
//! Owns [`StatusCode`] (100-599, transport-neutral reason phrases) and
//! [`ResponseConstructionError`]. Byte/header validation stays singular
//! in `header_block`; framing authority stays in `response`/`adapters`.

use std::fmt;

use super::super::header_block::HeaderError;

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
    ContentLengthMismatch {
        /// The length the response declared in its `Content-Length` header.
        declared: u64,
        /// The length of the body actually produced.
        actual: u64,
    },
    /// No file-stream admission permit was available.
    FileStreamLimit,
}

impl fmt::Display for ResponseConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStatus(code) => write!(f, "invalid status code: {code}"),
            Self::InvalidHeader(e) => write!(f, "invalid header: {e}"),
            Self::ForbiddenFramingHeader(name) => {
                write!(f, "forbidden framing header: {name}")
            }
            Self::BodyAlreadyConsumed => write!(f, "response body already consumed"),
            Self::ContentLengthMismatch { declared, actual } => {
                write!(
                    f,
                    "content-length mismatch: declared {declared}, actual {actual}",
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
    /// `100 Continue`.
    pub const CONTINUE: Self = Self(100);
    /// `101 Switching Protocols`.
    pub const SWITCHING_PROTOCOLS: Self = Self(101);
    /// `200 OK`.
    pub const OK: Self = Self(200);
    /// `201 Created`.
    pub const CREATED: Self = Self(201);
    /// `204 No Content`.
    pub const NO_CONTENT: Self = Self(204);
    /// `205 Reset Content`.
    pub const RESET_CONTENT: Self = Self(205);
    /// `304 Not Modified`.
    pub const NOT_MODIFIED: Self = Self(304);
    /// `301 Moved Permanently`.
    pub const MOVED_PERMANENTLY: Self = Self(301);
    /// `400 Bad Request`.
    pub const BAD_REQUEST: Self = Self(400);
    /// `403 Forbidden`.
    pub const FORBIDDEN: Self = Self(403);
    /// `404 Not Found`.
    pub const NOT_FOUND: Self = Self(404);
    /// `405 Method Not Allowed`.
    pub const METHOD_NOT_ALLOWED: Self = Self(405);
    /// `408 Request Timeout`.
    pub const REQUEST_TIMEOUT: Self = Self(408);
    /// `413 Content Too Large`.
    pub const PAYLOAD_TOO_LARGE: Self = Self(413);
    /// `416 Range Not Satisfiable`.
    pub const RANGE_NOT_SATISFIABLE: Self = Self(416);
    /// `500 Internal Server Error`.
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);
    /// `503 Service Unavailable`.
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

    /// Returns the standard reason phrase, when this status has one.
    ///
    /// This deliberately lives on the transport-neutral status type so every
    /// protocol can derive the same generic runtime-error representation
    /// without importing a transport status table.
    #[allow(dead_code)]
    pub(crate) fn canonical_reason(&self) -> Option<&'static str> {
        Some(match self.0 {
            100 => "Continue",
            101 => "Switching Protocols",
            102 => "Processing",
            103 => "Early Hints",
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            203 => "Non-Authoritative Information",
            204 => "No Content",
            205 => "Reset Content",
            206 => "Partial Content",
            207 => "Multi-Status",
            208 => "Already Reported",
            226 => "IM Used",
            300 => "Multiple Choices",
            301 => "Moved Permanently",
            302 => "Found",
            303 => "See Other",
            304 => "Not Modified",
            305 => "Use Proxy",
            307 => "Temporary Redirect",
            308 => "Permanent Redirect",
            400 => "Bad Request",
            401 => "Unauthorized",
            402 => "Payment Required",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            406 => "Not Acceptable",
            407 => "Proxy Authentication Required",
            408 => "Request Timeout",
            409 => "Conflict",
            410 => "Gone",
            411 => "Length Required",
            412 => "Precondition Failed",
            413 => "Payload Too Large",
            414 => "URI Too Long",
            415 => "Unsupported Media Type",
            416 => "Range Not Satisfiable",
            417 => "Expectation Failed",
            418 => "I'm a teapot",
            421 => "Misdirected Request",
            422 => "Unprocessable Content",
            423 => "Locked",
            424 => "Failed Dependency",
            425 => "Too Early",
            426 => "Upgrade Required",
            428 => "Precondition Required",
            429 => "Too Many Requests",
            431 => "Request Header Fields Too Large",
            451 => "Unavailable For Legal Reasons",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            505 => "HTTP Version Not Supported",
            506 => "Variant Also Negotiates",
            507 => "Insufficient Storage",
            508 => "Loop Detected",
            510 => "Not Extended",
            511 => "Network Authentication Required",
            _ => return None,
        })
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
