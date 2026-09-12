//! Canonical response representation and framing normalization.

use crate::{HeaderBlock, HeaderError};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatusCode(u16);
impl StatusCode {
    pub const OK: Self = Self(200);
    pub const BAD_REQUEST: Self = Self(400);
    pub const REQUEST_TIMEOUT: Self = Self(408);
    pub const PAYLOAD_TOO_LARGE: Self = Self(413);
    pub const NOT_FOUND: Self = Self(404);
    pub const METHOD_NOT_ALLOWED: Self = Self(405);
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);
    pub const SERVICE_UNAVAILABLE: Self = Self(503);
    pub fn new(value: u16) -> Result<Self, ResponseError> {
        if (100..=599).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ResponseError::InvalidStatus(value))
        }
    }
    pub fn as_u16(self) -> u16 {
        self.0
    }
    pub fn permits_payload_body(self) -> bool {
        !(self.0 < 200 || matches!(self.0, 204 | 205 | 304))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyLength {
    Known(u64),
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseBody {
    Empty,
    Bytes(Vec<u8>),
}
impl ResponseBody {
    pub fn len(&self) -> u64 {
        match self {
            Self::Empty => 0,
            Self::Bytes(b) => b.len() as u64,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseError {
    InvalidStatus(u16),
    InvalidHeader(HeaderError),
}
impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStatus(c) => write!(f, "invalid status {c}"),
            Self::InvalidHeader(e) => write!(f, "invalid header: {e}"),
        }
    }
}
impl std::error::Error for ResponseError {}
impl From<HeaderError> for ResponseError {
    fn from(value: HeaderError) -> Self {
        Self::InvalidHeader(value)
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderBlock,
    pub body: ResponseBody,
}
impl Response {
    pub fn new(status: StatusCode, body: ResponseBody) -> Self {
        Self {
            status,
            headers: HeaderBlock::new(),
            body,
        }
    }
    pub fn text(status: StatusCode, text: impl Into<Vec<u8>>) -> Self {
        Self::new(status, ResponseBody::Bytes(text.into()))
    }
    pub fn normalize(&mut self) {
        self.headers.remove("transfer-encoding");
        self.headers.remove("content-length");
        if self.status.permits_payload_body() {
            let _ = self
                .headers
                .push_str("content-length", self.body.len().to_string());
        } else {
            self.body = ResponseBody::Empty;
        }
    }
}
