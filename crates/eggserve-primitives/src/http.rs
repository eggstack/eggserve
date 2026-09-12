//! Validated HTTP metadata with no dependency on a transport implementation.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodError {
    Empty,
    InvalidToken,
}

impl fmt::Display for MethodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("method is empty"),
            Self::InvalidToken => f.write_str("method is not a valid token"),
        }
    }
}
impl std::error::Error for MethodError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Method(String);

impl Method {
    pub fn new(value: impl Into<String>) -> Result<Self, MethodError> {
        let value = value.into();
        if value.is_empty() {
            return Err(MethodError::Empty);
        }
        if value.bytes().any(|b| !is_token_byte(b)) {
            return Err(MethodError::InvalidToken);
        }
        Ok(Self(value))
    }
    pub fn get() -> Self {
        Self("GET".into())
    }
    pub fn head() -> Self {
        Self("HEAD".into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_head(&self) -> bool {
        self.0 == "HEAD"
    }
    pub fn permits_static_resolution(&self) -> bool {
        self.0 == "GET" || self.0 == "HEAD"
    }
}
impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpVersion {
    Http10,
    Http11,
    Http2,
    Http3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    InvalidName,
    InvalidValue,
}
impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidName => "invalid header name",
            Self::InvalidValue => "invalid header value",
        })
    }
}
impl std::error::Error for HeaderError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Header {
    name: String,
    value: Vec<u8>,
}
impl Header {
    pub fn new(name: impl Into<String>, value: impl AsRef<[u8]>) -> Result<Self, HeaderError> {
        let name = name.into();
        if name.is_empty() || name.bytes().any(|b| !is_token_byte(b)) {
            return Err(HeaderError::InvalidName);
        }
        let value = value.as_ref();
        if value.iter().any(|b| *b < 0x20 && *b != b'\t' || *b == 0x7f) {
            return Err(HeaderError::InvalidValue);
        }
        let start = value
            .iter()
            .position(|b| *b != b' ' && *b != b'\t')
            .unwrap_or(value.len());
        let end = value
            .iter()
            .rposition(|b| *b != b' ' && *b != b'\t')
            .map_or(start, |i| i + 1);
        Ok(Self {
            name,
            value: value[start..end].to_vec(),
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

fn is_token_byte(byte: u8) -> bool {
    matches!(byte, 0x21..=0x7e)
        && !matches!(
            byte,
            b'(' | b')'
                | b'<'
                | b'>'
                | b'@'
                | b','
                | b';'
                | b':'
                | b'\\'
                | b'"'
                | b'/'
                | b'['
                | b']'
                | b'?'
                | b'='
                | b'{'
                | b'}'
                | b' '
                | b'\t'
        )
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderBlock(Vec<Header>);
impl HeaderBlock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, header: Header) {
        self.0.push(header);
    }
    pub fn push_str(
        &mut self,
        name: impl Into<String>,
        value: impl AsRef<[u8]>,
    ) -> Result<(), HeaderError> {
        self.push(Header::new(name, value)?);
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = &Header> {
        self.0.iter()
    }
    pub fn get(&self, name: &str) -> Option<&Header> {
        self.0.iter().find(|h| h.name.eq_ignore_ascii_case(name))
    }
    pub fn remove(&mut self, name: &str) {
        self.0.retain(|h| !h.name.eq_ignore_ascii_case(name));
    }
    pub fn retain(&mut self, f: impl FnMut(&Header) -> bool) {
        self.0.retain(f);
    }
}
