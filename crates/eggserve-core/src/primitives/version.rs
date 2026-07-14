//! Canonical HTTP version type.
//!
//! [`HttpVersion`] represents the HTTP version used in a request or response.
//! It covers the versions the runtime actually supports (HTTP/1.0 and HTTP/1.1).

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
/// Supports HTTP/1.0 and HTTP/1.1, which are the versions the runtime
/// actually handles. Keep-alive semantics are a runtime concern, not a
/// property of this value type.
///
/// # Serialization
///
/// `Display` produces the wire format: `HTTP/1.0` or `HTTP/1.1`.
///
/// # Comparison
///
/// Two versions are equal if and only if they represent the same HTTP
/// version. `HTTP/1.0 != HTTP/1.1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpVersion {
    /// HTTP/1.0.
    Http10,
    /// HTTP/1.1.
    Http11,
}

impl HttpVersion {
    /// Parse an HTTP version from the version string in a request line
    /// (e.g., `HTTP/1.1`).
    ///
    /// # Errors
    ///
    /// Returns [`HttpVersionError::Unsupported`] if the version is not
    /// `HTTP/1.0` or `HTTP/1.1`.
    pub fn parse(version_str: &str) -> Result<Self, HttpVersionError> {
        match version_str {
            "HTTP/1.0" => Ok(Self::Http10),
            "HTTP/1.1" => Ok(Self::Http11),
            _ => Err(HttpVersionError::Unsupported),
        }
    }

    /// Returns the wire-format string for this version.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http10 => "HTTP/1.0",
            Self::Http11 => "HTTP/1.1",
        }
    }

    /// Returns the major version number.
    pub fn major(&self) -> u8 {
        match self {
            Self::Http10 => 1,
            Self::Http11 => 1,
        }
    }

    /// Returns the minor version number.
    pub fn minor(&self) -> u8 {
        match self {
            Self::Http10 => 0,
            Self::Http11 => 1,
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

impl From<&hyper::http::Version> for HttpVersion {
    fn from(v: &hyper::http::Version) -> Self {
        match *v {
            hyper::http::Version::HTTP_10 => Self::Http10,
            hyper::http::Version::HTTP_11 => Self::Http11,
            _ => Self::Http11, // best-effort fallback; unsupported versions are rejected at transport
        }
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
