//! Canonical request metadata and an owned, one-shot body value.

use crate::{HeaderBlock, HttpVersion, Method};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTarget(String);
impl RequestTarget {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty()
            || !value.starts_with('/')
            || value.bytes().any(|b| b <= 0x20 || b == 0x7f)
        {
            return Err("request target must be an origin-form path");
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct RequestHead {
    pub method: Method,
    pub target: RequestTarget,
    pub version: HttpVersion,
    pub headers: HeaderBlock,
}

#[derive(Debug, Clone, Default)]
pub struct RequestBody {
    bytes: Option<Vec<u8>>,
}
impl RequestBody {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes: Some(bytes) }
    }
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn take(&mut self) -> Option<Vec<u8>> {
        self.bytes.take()
    }
    pub fn len(&self) -> usize {
        self.bytes.as_ref().map_or(0, Vec::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug)]
pub struct Request {
    pub head: RequestHead,
    pub body: RequestBody,
}
