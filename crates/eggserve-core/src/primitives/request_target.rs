//! Canonical HTTP request target.
//!
//! [`RequestTarget`] represents the request target from an HTTP request
//! line, split into path and optional query components. It preserves the
//! raw target for logging while providing validated access to components.

use std::fmt;

/// Errors from request target validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestTargetError {
    /// The target is empty.
    Empty,
    /// The target is not valid origin-form (does not start with `/`).
    NotOriginForm,
    /// The target contains whitespace.
    ContainsWhitespace,
    /// The target is an absolute URI (contains `://`).
    AbsoluteUri,
    /// The target is an authority-form URI (contains `@` without `/`).
    AuthorityForm,
    /// The target is an asterisk-form (`*`).
    AsteriskForm,
}

impl fmt::Display for RequestTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "request target is empty"),
            Self::NotOriginForm => write!(f, "request target must start with '/'"),
            Self::ContainsWhitespace => write!(f, "request target contains whitespace"),
            Self::AbsoluteUri => write!(f, "absolute URI not supported"),
            Self::AuthorityForm => write!(f, "authority-form URI not supported"),
            Self::AsteriskForm => write!(f, "asterisk-form URI not supported"),
        }
    }
}

impl std::error::Error for RequestTargetError {}

/// A validated HTTP request target in origin form.
///
/// The raw target is preserved for logging or downstream parsing. The
/// validated path and optional query are available through accessor
/// methods.
///
/// # Security
///
/// This type does not perform percent decoding or path normalization.
/// Those operations belong to the [`ConfinedPath`] validation pipeline
/// and must not be duplicated here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTarget {
    raw: String,
    path: String,
    query: Option<String>,
}

impl RequestTarget {
    /// Parse and validate a request target.
    ///
    /// # Errors
    ///
    /// Returns [`RequestTargetError`] if the target is not valid origin-form.
    pub fn parse(raw: impl Into<String>) -> Result<Self, RequestTargetError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(RequestTargetError::Empty);
        }
        if raw == "*" {
            return Err(RequestTargetError::AsteriskForm);
        }
        if raw.contains(char::is_whitespace) {
            return Err(RequestTargetError::ContainsWhitespace);
        }
        if raw.starts_with('/') {
            return Self::parse_origin_form(raw);
        }
        if raw.contains("://") {
            return Err(RequestTargetError::AbsoluteUri);
        }
        if raw.contains('@') || raw.contains(':') {
            return Err(RequestTargetError::AuthorityForm);
        }
        // Non-`/`-prefixed, no `://`, no `@`, no `:` — not origin-form
        Err(RequestTargetError::NotOriginForm)
    }

    fn parse_origin_form(raw: String) -> Result<Self, RequestTargetError> {
        debug_assert!(raw.starts_with('/'));
        let (path, query) = match raw.find('?') {
            Some(pos) => {
                let path = raw[..pos].to_string();
                let q = &raw[pos + 1..];
                if q.is_empty() {
                    (path, None)
                } else {
                    (path, Some(q.to_string()))
                }
            }
            None => (raw.clone(), None),
        };

        Ok(Self { raw, path, query })
    }

    /// Returns the raw request target string.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns the path component (before the `?`).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the query component (after the `?`), if present.
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// Returns the full target including query, if present.
    pub fn path_and_query(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for RequestTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_path() {
        let t = RequestTarget::parse("/").unwrap();
        assert_eq!(t.raw(), "/");
        assert_eq!(t.path(), "/");
        assert!(t.query().is_none());
    }

    #[test]
    fn path_with_query() {
        let t = RequestTarget::parse("/foo?bar=baz").unwrap();
        assert_eq!(t.path(), "/foo");
        assert_eq!(t.query(), Some("bar=baz"));
    }

    #[test]
    fn path_with_empty_query() {
        let t = RequestTarget::parse("/foo?").unwrap();
        assert_eq!(t.path(), "/foo");
        assert!(t.query().is_none());
    }

    #[test]
    fn path_with_multiple_query_params() {
        let t = RequestTarget::parse("/a?b=1&c=2").unwrap();
        assert_eq!(t.path(), "/a");
        assert_eq!(t.query(), Some("b=1&c=2"));
    }

    #[test]
    fn complex_path() {
        let t = RequestTarget::parse("/foo/bar/file.txt?x=1&y=2").unwrap();
        assert_eq!(t.path(), "/foo/bar/file.txt");
        assert_eq!(t.query(), Some("x=1&y=2"));
    }

    #[test]
    fn reject_empty() {
        assert_eq!(
            RequestTarget::parse("").unwrap_err(),
            RequestTargetError::Empty
        );
    }

    #[test]
    fn reject_no_slash_prefix() {
        assert_eq!(
            RequestTarget::parse("foo").unwrap_err(),
            RequestTargetError::NotOriginForm
        );
    }

    #[test]
    fn reject_absolute_uri() {
        assert_eq!(
            RequestTarget::parse("http://example.com/").unwrap_err(),
            RequestTargetError::AbsoluteUri
        );
    }

    #[test]
    fn reject_authority_form() {
        assert_eq!(
            RequestTarget::parse("example.com:443").unwrap_err(),
            RequestTargetError::AuthorityForm
        );
    }

    #[test]
    fn reject_asterisk_form() {
        assert_eq!(
            RequestTarget::parse("*").unwrap_err(),
            RequestTargetError::AsteriskForm
        );
    }

    #[test]
    fn reject_whitespace() {
        assert_eq!(
            RequestTarget::parse("/foo bar").unwrap_err(),
            RequestTargetError::ContainsWhitespace
        );
        assert_eq!(
            RequestTarget::parse("/foo\tbar").unwrap_err(),
            RequestTargetError::ContainsWhitespace
        );
    }

    #[test]
    fn path_and_query_combined() {
        let t = RequestTarget::parse("/foo?bar").unwrap();
        assert_eq!(t.path_and_query(), "/foo?bar");
    }

    #[test]
    fn display() {
        let t = RequestTarget::parse("/foo?bar").unwrap();
        assert_eq!(format!("{t}"), "/foo?bar");
    }

    #[test]
    fn error_display() {
        assert!(!RequestTargetError::Empty.to_string().is_empty());
        assert!(!RequestTargetError::NotOriginForm.to_string().is_empty());
        assert!(!RequestTargetError::AbsoluteUri.to_string().is_empty());
        assert!(!RequestTargetError::AuthorityForm.to_string().is_empty());
        assert!(!RequestTargetError::AsteriskForm.to_string().is_empty());
        assert!(!RequestTargetError::ContainsWhitespace
            .to_string()
            .is_empty());
    }

    #[test]
    fn error_is_error() {
        let err: &dyn std::error::Error = &RequestTargetError::Empty;
        assert!(!err.to_string().is_empty());
    }
}
