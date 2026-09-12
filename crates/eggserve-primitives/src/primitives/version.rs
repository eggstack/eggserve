//! Canonical HTTP version type.
//!
//! [`HttpVersion`] represents the HTTP version used in a request or response.
//! It represents protocol metadata independently of which wire protocols are
//! enabled by a particular runtime build.

use std::fmt;

/// Errors from HTTP version validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpVersionError {
    /// The version string is not recognized.
    Unsupported,
}

impl fmt::Display for HttpVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "unsupported HTTP version"),
        }
    }
}

impl std::error::Error for HttpVersionError {}

/// An HTTP version.
///
/// Keep-alive semantics are a runtime concern, not a property of this value
/// type. The HTTP/1 driver remains HTTP/1-only until the later protocol plans
/// enable additional wire transports.
///
/// # Serialization
///
/// `Display` produces descriptive canonical text (`HTTP/1.0`, `HTTP/1.1`,
/// `HTTP/2`, or `HTTP/3`). HTTP/2 and HTTP/3 do not have request-line forms;
/// these strings are metadata, not proof of wire acceptance.
///
/// # Comparison
///
/// Two versions are equal if and only if they represent the same HTTP
/// version. `HTTP/1.0 != HTTP/1.1`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpVersion {
    /// HTTP/1.0.
    Http10,
    /// HTTP/1.1.
    Http11,
    /// HTTP/2.
    Http2,
    /// HTTP/3.
    Http3,
}

impl HttpVersion {
    /// Parse an HTTP version from the version string in a request line
    /// (e.g., `HTTP/1.1`).
    ///
    /// # Errors
    ///
    /// Returns [`HttpVersionError::Unsupported`] if the version is not
    /// `HTTP/1.0`, `HTTP/1.1`, `HTTP/2`, or `HTTP/3`.
    pub fn parse(version_str: &str) -> Result<Self, HttpVersionError> {
        match version_str {
            "HTTP/1.0" => Ok(Self::Http10),
            "HTTP/1.1" => Ok(Self::Http11),
            "HTTP/2" => Ok(Self::Http2),
            "HTTP/3" => Ok(Self::Http3),
            _ => Err(HttpVersionError::Unsupported),
        }
    }

    /// Returns the wire-format string for this version.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http10 => "HTTP/1.0",
            Self::Http11 => "HTTP/1.1",
            Self::Http2 => "HTTP/2",
            Self::Http3 => "HTTP/3",
        }
    }

    /// Returns the major version number.
    pub fn major(&self) -> u8 {
        match self {
            Self::Http10 => 1,
            Self::Http11 => 1,
            Self::Http2 => 2,
            Self::Http3 => 3,
        }
    }

    /// Returns the minor version number.
    pub fn minor(&self) -> u8 {
        match self {
            Self::Http10 => 0,
            Self::Http11 => 1,
            Self::Http2 | Self::Http3 => 0,
        }
    }
}

impl fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for HttpVersion {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_1_0() {
        assert_eq!(HttpVersion::parse("HTTP/1.0").unwrap(), HttpVersion::Http10);
    }

    #[test]
    fn parse_http_1_1() {
        assert_eq!(HttpVersion::parse("HTTP/1.1").unwrap(), HttpVersion::Http11);
    }

    #[test]
    fn parse_unsupported() {
        assert_eq!(
            HttpVersion::parse("HTTP/2.0").unwrap_err(),
            HttpVersionError::Unsupported
        );
        assert_eq!(
            HttpVersion::parse("HTTP/0.9").unwrap_err(),
            HttpVersionError::Unsupported
        );
        assert_eq!(
            HttpVersion::parse("").unwrap_err(),
            HttpVersionError::Unsupported
        );
    }

    #[test]
    fn parse_future_protocol_metadata() {
        assert_eq!(HttpVersion::parse("HTTP/2").unwrap(), HttpVersion::Http2);
        assert_eq!(HttpVersion::parse("HTTP/3").unwrap(), HttpVersion::Http3);
        assert_eq!(HttpVersion::Http2.major(), 2);
        assert_eq!(HttpVersion::Http3.minor(), 0);
    }

    #[test]
    fn as_str() {
        assert_eq!(HttpVersion::Http10.as_str(), "HTTP/1.0");
        assert_eq!(HttpVersion::Http11.as_str(), "HTTP/1.1");
    }

    #[test]
    fn major_minor() {
        assert_eq!(HttpVersion::Http10.major(), 1);
        assert_eq!(HttpVersion::Http10.minor(), 0);
        assert_eq!(HttpVersion::Http11.major(), 1);
        assert_eq!(HttpVersion::Http11.minor(), 1);
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", HttpVersion::Http10), "HTTP/1.0");
        assert_eq!(format!("{}", HttpVersion::Http11), "HTTP/1.1");
    }

    #[test]
    fn as_ref_str() {
        let s: &str = HttpVersion::Http11.as_ref();
        assert_eq!(s, "HTTP/1.1");
    }

    #[test]
    fn error_display() {
        assert!(!HttpVersionError::Unsupported.to_string().is_empty());
    }

    #[test]
    fn error_is_error() {
        let err: &dyn std::error::Error = &HttpVersionError::Unsupported;
        assert!(!err.to_string().is_empty());
    }
}
