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
    form: RequestTargetForm,
    scheme: Option<String>,
    authority: Option<crate::primitives::authority::Authority>,
    path_start: usize,
    path_end: usize,
    query_start: Option<usize>,
}

/// The accepted request-target syntax at the HTTP boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTargetForm {
    Origin,
    Absolute,
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
        if raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(RequestTargetError::ContainsWhitespace);
        }
        if raw.starts_with('/') {
            if raw.starts_with("//") {
                return Err(RequestTargetError::AuthorityForm);
            }
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
        // Fragment (`#`) handling: origin-form has no fragment component
        // (RFC 9110), so a literal `#` is kept as part of the path — same
        // contract as `crate::path::parse_origin_form`. `/a?b#frag` strips
        // to path `/a`; `/foo#bar` keeps the literal `#` in the path.
        let path_end = raw.find('?').unwrap_or(raw.len());
        let query_start =
            (path_end < raw.len() && path_end + 1 < raw.len()).then_some(path_end + 1);

        Ok(Self {
            raw,
            form: RequestTargetForm::Origin,
            scheme: None,
            authority: None,
            path_start: 0,
            path_end,
            query_start,
        })
    }

    /// Construct an absolute-form target from URI components already parsed by the transport adapter.
    pub fn from_absolute_components(
        scheme: impl Into<String>,
        authority: crate::primitives::authority::Authority,
        path_and_query: impl AsRef<str>,
    ) -> Result<Self, RequestTargetError> {
        let scheme = scheme.into();
        let pq = path_and_query.as_ref();
        if scheme.is_empty()
            || !scheme.bytes().enumerate().all(|(i, b)| {
                b.is_ascii_alphabetic()
                    || (i > 0 && (b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.')))
            })
            || !scheme.as_bytes()[0].is_ascii_alphabetic()
            || pq.starts_with("//")
        {
            return Err(RequestTargetError::AbsoluteUri);
        }
        if pq
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        {
            return Err(RequestTargetError::ContainsWhitespace);
        }
        let path = if pq.is_empty() { "/" } else { pq };
        if !path.starts_with('/') {
            return Err(RequestTargetError::NotOriginForm);
        }
        let raw = format!("{scheme}://{}{path}", authority.as_str());
        let path_start = scheme.len() + 3 + authority.as_str().len();
        let path_end = path_start + path.find('?').unwrap_or(path.len());
        let query_start =
            (path_end < raw.len() && path_end + 1 < raw.len()).then_some(path_end + 1);
        Ok(Self {
            raw,
            form: RequestTargetForm::Absolute,
            scheme: Some(scheme),
            authority: Some(authority),
            path_start,
            path_end,
            query_start,
        })
    }

    /// Returns whether the target is origin-form or absolute-form.
    pub fn form(&self) -> RequestTargetForm {
        self.form
    }
    /// Returns the URI scheme for absolute-form targets.
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }
    /// Returns the URI authority for absolute-form targets.
    pub fn uri_authority(&self) -> Option<&crate::primitives::authority::Authority> {
        self.authority.as_ref()
    }

    /// Returns the raw request target string.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns the path component (before the `?`).
    pub fn path(&self) -> &str {
        &self.raw[self.path_start..self.path_end]
    }

    /// Returns the query component (after the `?`), if present.
    ///
    /// An empty query (`/path?`) canonicalizes to `None`: `/path` and
    /// `/path?` are deliberately equivalent. This matches the historical
    /// contract and avoids a bare-`?` distinction most application semantics
    /// do not require.
    pub fn query(&self) -> Option<&str> {
        self.query_start.map(|start| &self.raw[start..])
    }

    /// Returns the full target including query, if present.
    pub fn path_and_query(&self) -> &str {
        &self.raw[self.path_start..]
    }

    /// Returns the raw target octets.
    ///
    /// Accepted origin-form targets are visible ASCII/percent-encoded data, so
    /// the `String` storage round-trips losslessly and this is exactly the
    /// accepted wire representation seen after Hyper parsing. If Hyper
    /// normalizes an accepted target before EggServe sees it, this cannot
    /// truthfully provide original raw-path bytes for those cases — a
    /// downstream server should then omit optional `raw_path` rather than
    /// fabricate it. No second parser is introduced to recover such bytes.
    pub fn raw_bytes(&self) -> &[u8] {
        self.raw.as_bytes()
    }

    /// Returns the path-component octets.
    pub fn path_bytes(&self) -> &[u8] {
        &self.raw.as_bytes()[self.path_start..self.path_end]
    }

    /// Returns the query-component octets, if present.
    pub fn query_bytes(&self) -> Option<&[u8]> {
        self.query_start.map(|start| &self.raw.as_bytes()[start..])
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
    fn absolute_components_preserve_full_target_and_expose_path() {
        let authority =
            crate::primitives::authority::Authority::parse("example.test:8080").unwrap();
        let target = RequestTarget::from_absolute_components("http", authority, "/a?b=1").unwrap();
        assert_eq!(target.form(), RequestTargetForm::Absolute);
        assert_eq!(target.raw(), "http://example.test:8080/a?b=1");
        assert_eq!(target.scheme(), Some("http"));
        assert_eq!(
            target.uri_authority().unwrap().as_str(),
            "example.test:8080"
        );
        assert_eq!(target.path(), "/a");
        assert_eq!(target.query(), Some("b=1"));
        assert_eq!(target.path_and_query(), "/a?b=1");
        assert!(matches!(
            RequestTarget::parse(target.raw()),
            Err(RequestTargetError::AbsoluteUri)
        ));
    }

    #[test]
    fn network_path_reference_is_rejected() {
        assert_eq!(
            RequestTarget::parse("//example.com/file").unwrap_err(),
            RequestTargetError::AuthorityForm
        );
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
    fn reject_all_ascii_controls() {
        assert_eq!(
            RequestTarget::parse("/foo\x1fbar").unwrap_err(),
            RequestTargetError::ContainsWhitespace
        );
    }

    #[test]
    fn only_ascii_whitespace_is_rejected() {
        assert!(RequestTarget::parse("/foo\u{00a0}bar").is_ok());
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

    #[test]
    fn percent_encoded_path() {
        let t = RequestTarget::parse("/foo%20bar").unwrap();
        assert_eq!(t.raw(), "/foo%20bar");
        assert_eq!(t.path(), "/foo%20bar");
        assert!(t.query().is_none());
    }

    #[test]
    fn fragment_without_query_is_literal_path() {
        // Same contract as `crate::path::parse_origin_form`: with no `?`,
        // `#` is an ordinary path character, not a fragment delimiter.
        let t = RequestTarget::parse("/foo#bar").unwrap();
        assert_eq!(t.path(), "/foo#bar");
        assert!(t.query().is_none());
    }

    #[test]
    fn fragment_after_query_stays_in_query() {
        let t = RequestTarget::parse("/a?b#frag").unwrap();
        assert_eq!(t.path(), "/a");
        assert_eq!(t.query(), Some("b#frag"));
    }

    #[test]
    fn percent_encoded_slash() {
        let t = RequestTarget::parse("/foo%2Fbar").unwrap();
        assert_eq!(t.raw(), "/foo%2Fbar");
        assert_eq!(t.path(), "/foo%2Fbar");
        assert!(t.query().is_none());
    }

    #[test]
    fn dot_segment_paths() {
        let t = RequestTarget::parse("/./foo").unwrap();
        assert_eq!(t.path(), "/./foo");
        assert!(t.query().is_none());
    }

    #[test]
    fn dot_segment_parent() {
        let t = RequestTarget::parse("/foo/../bar").unwrap();
        assert_eq!(t.path(), "/foo/../bar");
        assert!(t.query().is_none());
    }

    #[test]
    fn backslash_in_path() {
        let t = RequestTarget::parse("/foo\\bar").unwrap();
        assert_eq!(t.path(), "/foo\\bar");
        assert!(t.query().is_none());
    }

    #[test]
    fn non_ascii_bytes() {
        let t = RequestTarget::parse("/foo\u{00E9}\u{00FF}").unwrap();
        assert_eq!(t.raw(), "/foo\u{00E9}\u{00FF}");
        assert_eq!(t.path(), "/foo\u{00E9}\u{00FF}");
        assert!(t.query().is_none());
    }

    #[test]
    fn origin_form_only_enforced() {
        assert_eq!(
            RequestTarget::parse("foo").unwrap_err(),
            RequestTargetError::NotOriginForm
        );
    }

    #[test]
    fn query_with_equals() {
        let t = RequestTarget::parse("/path?key=val=ue").unwrap();
        assert_eq!(t.path(), "/path");
        assert_eq!(t.query(), Some("key=val=ue"));
    }

    #[test]
    fn query_with_encoded_chars() {
        let t = RequestTarget::parse("/path?key=hello%20world").unwrap();
        assert_eq!(t.path(), "/path");
        assert_eq!(t.query(), Some("key=hello%20world"));
    }

    #[test]
    fn multiple_question_marks() {
        let t = RequestTarget::parse("/path?a=1?b=2").unwrap();
        assert_eq!(t.path(), "/path");
        assert_eq!(t.query(), Some("a=1?b=2"));
    }
}
