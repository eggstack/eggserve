//! Canonical HTTP method type.
//!
//! [`Method`] represents an HTTP method as a validated string, supporting
//! both standard methods and extension methods without information loss.
//! It is transport-independent and contains no Hyper types.

use std::fmt;

/// Errors from method validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodError {
    /// The method string is empty.
    Empty,
    /// The method contains invalid characters (not a valid HTTP token).
    InvalidToken,
}

impl fmt::Display for MethodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "method must not be empty"),
            Self::InvalidToken => write!(f, "method contains invalid characters"),
        }
    }
}

impl std::error::Error for MethodError {}

/// A validated HTTP method.
///
/// Standard methods (`GET`, `HEAD`, `POST`, `PUT`, `DELETE`, `PATCH`,
/// `OPTIONS`, `TRACE`, `CONNECT`) are recognized. Extension methods are
/// preserved without information loss.
///
/// # Case sensitivity
///
/// HTTP methods are case-sensitive per RFC 9110 section 9.1. `Method`
/// preserves the original casing.
///
/// # Validation
///
/// A valid method is a non-empty sequence of visible ASCII characters
/// (code points 0x21–0x7E) excluding separators. This matches the
/// `token` production in RFC 9110.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Method(String);

impl Method {
    /// Create a validated method from a string.
    ///
    /// # Errors
    ///
    /// Returns [`MethodError`] if the string is empty or contains invalid
    /// characters.
    pub fn new(value: impl Into<String>) -> Result<Self, MethodError> {
        let s = value.into();
        if s.is_empty() {
            return Err(MethodError::Empty);
        }
        if !is_http_token(&s) {
            return Err(MethodError::InvalidToken);
        }
        Ok(Self(s))
    }

    /// Create a `Method` without validation.
    ///
    /// # Safety
    ///
    /// The caller must ensure the string is a valid HTTP token. This is
    /// used internally for constructing standard method constants.
    #[inline]
    fn new_unchecked(value: &'static str) -> Self {
        Self(value.to_string())
    }

    /// Create a `Method` for `GET`.
    pub fn get() -> Self {
        Self::new_unchecked("GET")
    }

    /// Create a `Method` for `HEAD`.
    pub fn head() -> Self {
        Self::new_unchecked("HEAD")
    }

    /// Create a `Method` for `POST`.
    pub fn post() -> Self {
        Self::new_unchecked("POST")
    }

    /// Create a `Method` for `PUT`.
    pub fn put() -> Self {
        Self::new_unchecked("PUT")
    }

    /// Create a `Method` for `DELETE`.
    pub fn delete() -> Self {
        Self::new_unchecked("DELETE")
    }

    /// Create a `Method` for `PATCH`.
    pub fn patch() -> Self {
        Self::new_unchecked("PATCH")
    }

    /// Create a `Method` for `OPTIONS`.
    pub fn options() -> Self {
        Self::new_unchecked("OPTIONS")
    }

    /// Create a `Method` for `TRACE`.
    pub fn trace() -> Self {
        Self::new_unchecked("TRACE")
    }

    /// Create a `Method` for `CONNECT`.
    pub fn connect() -> Self {
        Self::new_unchecked("CONNECT")
    }

    /// Returns the method as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if this is `GET`.
    pub fn is_get(&self) -> bool {
        self.0 == "GET"
    }

    /// Returns `true` if this is `HEAD`.
    pub fn is_head(&self) -> bool {
        self.0 == "HEAD"
    }

    /// Returns `true` if the method is safe (does not modify resources).
    ///
    /// Safe methods are `GET`, `HEAD`, `OPTIONS`, and `TRACE` per RFC 9110
    /// section 9.2.1.
    pub fn is_safe(&self) -> bool {
        matches!(self.0.as_str(), "GET" | "HEAD" | "OPTIONS" | "TRACE")
    }

    /// Returns `true` if the method is idempotent.
    ///
    /// Idempotent methods are `GET`, `HEAD`, `PUT`, `DELETE`, and `OPTIONS`
    /// per RFC 9110 section 9.2.2. `TRACE` is also idempotent by definition.
    pub fn is_idempotent(&self) -> bool {
        matches!(
            self.0.as_str(),
            "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
        )
    }

    /// Returns `true` if this method permits static file resolution.
    ///
    /// The built-in static service only supports `GET` and `HEAD`. This
    /// method provides a policy helper without conflating method identity
    /// with server policy.
    pub fn permits_static_resolution(&self) -> bool {
        self.0 == "GET" || self.0 == "HEAD"
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Method {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Method {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Method {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// Check if a string is a valid HTTP token (RFC 9110 section 5.6.2).
fn is_http_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| matches!(b, 0x21 | 0x23..=0x27 | 0x2A | 0x2B | 0x2D..=0x2E | 0x30..=0x39 | 0x41..=0x5A | 0x5E..=0x7A | 0x7C | 0x7E))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_methods() {
        assert_eq!(Method::get().as_str(), "GET");
        assert_eq!(Method::head().as_str(), "HEAD");
        assert_eq!(Method::post().as_str(), "POST");
        assert_eq!(Method::put().as_str(), "PUT");
        assert_eq!(Method::delete().as_str(), "DELETE");
        assert_eq!(Method::patch().as_str(), "PATCH");
        assert_eq!(Method::options().as_str(), "OPTIONS");
        assert_eq!(Method::trace().as_str(), "TRACE");
        assert_eq!(Method::connect().as_str(), "CONNECT");
    }

    #[test]
    fn extension_method() {
        let m = Method::new("PURGE").unwrap();
        assert_eq!(m.as_str(), "PURGE");
    }

    #[test]
    fn case_preserved() {
        let m = Method::new("get").unwrap();
        assert_eq!(m.as_str(), "get");
        assert!(!m.is_get()); // case-sensitive
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(Method::new("").unwrap_err(), MethodError::Empty);
    }

    #[test]
    fn invalid_token_rejected() {
        assert_eq!(
            Method::new("GET POST").unwrap_err(),
            MethodError::InvalidToken
        );
        assert_eq!(Method::new("GET\t").unwrap_err(), MethodError::InvalidToken);
        assert_eq!(Method::new("").unwrap_err(), MethodError::Empty);
    }

    #[test]
    fn valid_tokens() {
        assert!(Method::new("X").is_ok());
        assert!(Method::new("x-y-z").is_ok());
        assert!(Method::new("!").is_ok());
        assert!(Method::new("#").is_ok());
        assert!(Method::new("0").is_ok());
    }

    #[test]
    fn is_safe_classification() {
        assert!(Method::get().is_safe());
        assert!(Method::head().is_safe());
        assert!(Method::options().is_safe());
        assert!(Method::trace().is_safe());
        assert!(!Method::post().is_safe());
        assert!(!Method::put().is_safe());
        assert!(!Method::delete().is_safe());
        assert!(!Method::patch().is_safe());
    }

    #[test]
    fn is_idempotent_classification() {
        assert!(Method::get().is_idempotent());
        assert!(Method::head().is_idempotent());
        assert!(Method::put().is_idempotent());
        assert!(Method::delete().is_idempotent());
        assert!(Method::options().is_idempotent());
        assert!(Method::trace().is_idempotent());
        assert!(!Method::post().is_idempotent());
        assert!(!Method::patch().is_idempotent());
    }

    #[test]
    fn permits_static_resolution() {
        assert!(Method::get().permits_static_resolution());
        assert!(Method::head().permits_static_resolution());
        assert!(!Method::post().permits_static_resolution());
        assert!(!Method::put().permits_static_resolution());
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", Method::get()), "GET");
        assert_eq!(format!("{}", Method::new("PURGE").unwrap()), "PURGE");
    }

    #[test]
    fn eq_str() {
        assert_eq!(Method::get(), "GET");
        assert_eq!(Method::get(), "GET");
        assert_ne!(Method::get(), "POST");
    }

    #[test]
    fn method_error_display() {
        assert!(!MethodError::Empty.to_string().is_empty());
        assert!(!MethodError::InvalidToken.to_string().is_empty());
    }

    #[test]
    fn method_error_is_error() {
        let err: &dyn std::error::Error = &MethodError::Empty;
        assert!(!err.to_string().is_empty());
    }
}
