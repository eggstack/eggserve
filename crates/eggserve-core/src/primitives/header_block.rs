//! Duplicate-preserving HTTP header block.
//!
//! [`HeaderBlock`] stores HTTP headers as an ordered list of name/value pairs,
//! preserving duplicates and original field-name casing. Case-insensitive
//! lookup is provided by field name.

use std::fmt;

/// Errors from header validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    /// The header name is empty or contains invalid characters.
    InvalidName,
    /// The header value contains a carriage return, line feed, or NUL byte.
    InvalidValue,
    /// The header name is too long.
    NameTooLong,
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(f, "invalid header name"),
            Self::InvalidValue => write!(f, "invalid header value (contains CR/LF/NUL)"),
            Self::NameTooLong => write!(f, "header name too long"),
        }
    }
}

impl std::error::Error for HeaderError {}

/// Error returned by [`HeaderBlock::get_unique`] when duplicates exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateHeaderError {
    /// The header name that had duplicates.
    name: String,
    /// The number of values found.
    count: usize,
}

impl DuplicateHeaderError {
    /// Returns the header name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the number of duplicate values.
    pub fn count(&self) -> usize {
        self.count
    }
}

impl fmt::Display for DuplicateHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "header '{}' has {} values; use get_all() to access all",
            self.name, self.count
        )
    }
}

impl std::error::Error for DuplicateHeaderError {}

/// A validated HTTP header name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderName(String);

impl HeaderName {
    /// Create a validated header name.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderError::InvalidName`] if the name is empty or contains
    /// characters outside the visible ASCII range (0x21–0x7E excluding
    /// separators).
    pub fn new(name: impl Into<String>) -> Result<Self, HeaderError> {
        let s = name.into();
        if s.is_empty() {
            return Err(HeaderError::InvalidName);
        }
        if s.len() > 256 {
            return Err(HeaderError::NameTooLong);
        }
        if !is_valid_header_name(&s) {
            return Err(HeaderError::InvalidName);
        }
        Ok(Self(s))
    }

    /// Returns the header name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the lowercased header name for case-insensitive comparison.
    fn as_lower(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

impl fmt::Display for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated HTTP header value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderValue(String);

impl HeaderValue {
    /// Create a validated header value.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderError::InvalidValue`] if the value contains a
    /// carriage return (CR), line feed (LF), or NUL byte.
    pub fn new(value: impl Into<String>) -> Result<Self, HeaderError> {
        let s = value.into();
        if s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
            return Err(HeaderError::InvalidValue);
        }
        Ok(Self(s))
    }

    /// Returns the header value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HeaderValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A single header field as a name/value pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderField {
    pub name: HeaderName,
    pub value: HeaderValue,
}

/// An ordered, duplicate-preserving collection of HTTP headers.
///
/// Headers are stored as an ordered list of name/value pairs. Duplicate
/// field names are preserved. Case-insensitive lookup by field name is
/// provided.
///
/// # Canonical representation
///
/// This type is normatively a list, not a map. Dictionary conversion
/// (via `Into<HashMap>`) is lossy and should only be used as an explicit
/// convenience.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderBlock {
    fields: Vec<HeaderField>,
}

impl HeaderBlock {
    /// Create an empty header block.
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Create a header block with a pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            fields: Vec::with_capacity(capacity),
        }
    }

    /// Push a validated header field.
    pub fn push(&mut self, name: HeaderName, value: HeaderValue) {
        self.fields.push(HeaderField { name, value });
    }

    /// Push a header field from string slices.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderError`] if the name or value is invalid.
    pub fn push_str(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), HeaderError> {
        let name = HeaderName::new(name)?;
        let value = HeaderValue::new(value)?;
        self.push(name, value);
        Ok(())
    }

    /// Returns the first value for the given header name (case-insensitive).
    pub fn get_first(&self, name: &str) -> Option<&HeaderValue> {
        let lower = name.to_ascii_lowercase();
        self.fields
            .iter()
            .find(|f| f.name.as_lower() == lower)
            .map(|f| &f.value)
    }

    /// Returns all values for the given header name (case-insensitive),
    /// in order.
    pub fn get_all(&self, name: &str) -> Vec<&HeaderValue> {
        let lower = name.to_ascii_lowercase();
        self.fields
            .iter()
            .filter(|f| f.name.as_lower() == lower)
            .map(|f| &f.value)
            .collect()
    }

    /// Returns the unique value for the given header name (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns [`DuplicateHeaderError`] if the header appears more than once.
    /// Returns `Ok(None)` if the header is absent.
    pub fn get_unique(&self, name: &str) -> Result<Option<&HeaderValue>, DuplicateHeaderError> {
        let values = self.get_all(name);
        match values.len() {
            0 => Ok(None),
            1 => Ok(Some(values[0])),
            _ => Err(DuplicateHeaderError {
                name: name.to_string(),
                count: values.len(),
            }),
        }
    }

    /// Returns `true` if a header with the given name exists
    /// (case-insensitive).
    pub fn contains(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.fields.iter().any(|f| f.name.as_lower() == lower)
    }

    /// Returns an iterator over all header fields.
    pub fn iter(&self) -> impl Iterator<Item = &HeaderField> {
        self.fields.iter()
    }

    /// Returns the number of header fields.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns `true` if there are no header fields.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Retains only the elements specified by the predicate.
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&HeaderField) -> bool,
    {
        self.fields.retain(f);
    }
}

impl fmt::Display for HeaderBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for field in &self.fields {
            writeln!(f, "{}: {}", field.name, field.value)?;
        }
        Ok(())
    }
}

/// Check if a character is a valid HTTP header name character (RFC 9110
/// section 5.6.2, token).
fn is_valid_header_name(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, 0x21 | 0x23..=0x27 | 0x2A | 0x2B | 0x2D..=0x2E | 0x30..=0x39 | 0x41..=0x5A | 0x5E..=0x7A | 0x7C | 0x7E))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_name_valid() {
        assert!(HeaderName::new("Content-Type").is_ok());
        assert!(HeaderName::new("x-custom-header").is_ok());
        assert!(HeaderName::new("X").is_ok());
    }

    #[test]
    fn header_name_empty_rejected() {
        assert_eq!(HeaderName::new("").unwrap_err(), HeaderError::InvalidName);
    }

    #[test]
    fn header_name_invalid_chars_rejected() {
        assert_eq!(
            HeaderName::new("foo bar").unwrap_err(),
            HeaderError::InvalidName
        );
        assert_eq!(
            HeaderName::new("foo\tbar").unwrap_err(),
            HeaderError::InvalidName
        );
    }

    #[test]
    fn header_name_too_long_rejected() {
        let long_name = "x".repeat(257);
        assert_eq!(
            HeaderName::new(long_name).unwrap_err(),
            HeaderError::NameTooLong
        );
    }

    #[test]
    fn header_value_valid() {
        assert!(HeaderValue::new("text/html").is_ok());
        assert!(HeaderValue::new("").is_ok());
        assert!(HeaderValue::new("hello world").is_ok());
    }

    #[test]
    fn header_value_cr_rejected() {
        assert_eq!(
            HeaderValue::new("foo\rbar").unwrap_err(),
            HeaderError::InvalidValue
        );
    }

    #[test]
    fn header_value_lf_rejected() {
        assert_eq!(
            HeaderValue::new("foo\nbar").unwrap_err(),
            HeaderError::InvalidValue
        );
    }

    #[test]
    fn header_value_nul_rejected() {
        assert_eq!(
            HeaderValue::new("foo\0bar").unwrap_err(),
            HeaderError::InvalidValue
        );
    }

    #[test]
    fn empty_header_block() {
        let block = HeaderBlock::new();
        assert!(block.is_empty());
        assert_eq!(block.len(), 0);
        assert!(block.get_first("foo").is_none());
    }

    #[test]
    fn push_and_get_first() {
        let mut block = HeaderBlock::new();
        block.push_str("content-type", "text/html").unwrap();
        assert_eq!(
            block.get_first("content-type").unwrap().as_str(),
            "text/html"
        );
        assert_eq!(
            block.get_first("Content-Type").unwrap().as_str(),
            "text/html"
        );
        assert!(block.get_first("missing").is_none());
    }

    #[test]
    fn duplicates_preserved() {
        let mut block = HeaderBlock::new();
        block.push_str("set-cookie", "a=1").unwrap();
        block.push_str("set-cookie", "b=2").unwrap();
        assert_eq!(block.len(), 2);
        assert_eq!(block.get_first("Set-Cookie").unwrap().as_str(), "a=1");
    }

    #[test]
    fn get_all_returns_all_values() {
        let mut block = HeaderBlock::new();
        block.push_str("set-cookie", "a=1").unwrap();
        block.push_str("set-cookie", "b=2").unwrap();
        block.push_str("set-cookie", "c=3").unwrap();
        let all = block.get_all("set-cookie");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].as_str(), "a=1");
        assert_eq!(all[1].as_str(), "b=2");
        assert_eq!(all[2].as_str(), "c=3");
    }

    #[test]
    fn get_unique_single() {
        let mut block = HeaderBlock::new();
        block.push_str("content-type", "text/html").unwrap();
        let result = block.get_unique("content-type").unwrap();
        assert_eq!(result.unwrap().as_str(), "text/html");
    }

    #[test]
    fn get_unique_absent() {
        let block = HeaderBlock::new();
        assert!(block.get_unique("content-type").unwrap().is_none());
    }

    #[test]
    fn get_unique_duplicate_error() {
        let mut block = HeaderBlock::new();
        block.push_str("set-cookie", "a=1").unwrap();
        block.push_str("set-cookie", "b=2").unwrap();
        let err = block.get_unique("set-cookie").unwrap_err();
        assert_eq!(err.name(), "set-cookie");
        assert_eq!(err.count(), 2);
    }

    #[test]
    fn contains_case_insensitive() {
        let mut block = HeaderBlock::new();
        block.push_str("Content-Type", "text/html").unwrap();
        assert!(block.contains("content-type"));
        assert!(block.contains("CONTENT-TYPE"));
        assert!(block.contains("Content-Type"));
        assert!(!block.contains("missing"));
    }

    #[test]
    fn iteration_order() {
        let mut block = HeaderBlock::new();
        block.push_str("a", "1").unwrap();
        block.push_str("b", "2").unwrap();
        block.push_str("c", "3").unwrap();
        let names: Vec<&str> = block.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn display() {
        let mut block = HeaderBlock::new();
        block.push_str("a", "1").unwrap();
        let display = format!("{}", block);
        assert!(display.contains("a: 1"));
    }

    #[test]
    fn header_name_display() {
        assert_eq!(format!("{}", HeaderName::new("foo").unwrap()), "foo");
    }

    #[test]
    fn header_value_display() {
        assert_eq!(format!("{}", HeaderValue::new("bar").unwrap()), "bar");
    }

    #[test]
    fn duplicate_header_error_display() {
        let err = DuplicateHeaderError {
            name: "set-cookie".to_string(),
            count: 3,
        };
        let msg = err.to_string();
        assert!(msg.contains("set-cookie"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn duplicate_header_error_is_error() {
        let err: &dyn std::error::Error = &DuplicateHeaderError {
            name: "x".to_string(),
            count: 2,
        };
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn header_error_display() {
        assert!(!HeaderError::InvalidName.to_string().is_empty());
        assert!(!HeaderError::InvalidValue.to_string().is_empty());
        assert!(!HeaderError::NameTooLong.to_string().is_empty());
    }

    #[test]
    fn header_error_is_error() {
        let err: &dyn std::error::Error = &HeaderError::InvalidName;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn with_capacity() {
        let block = HeaderBlock::with_capacity(10);
        assert!(block.is_empty());
    }
}
