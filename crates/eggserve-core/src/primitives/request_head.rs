//! Canonical HTTP request head.
//!
//! [`RequestHead`] is the transport-independent, immutable value type
//! representing the head of an HTTP request (method, target, version,
//! headers). It contains no Hyper types.

use crate::primitives::header_block::HeaderBlock;
use crate::primitives::method::Method;
use crate::primitives::request_target::{RequestTarget, RequestTargetError};
use crate::primitives::version::HttpVersion;

/// Errors from converting a Hyper request to a canonical [`RequestHead`].
#[derive(Debug)]
pub enum RequestHeadError {
    /// The request target could not be parsed.
    Target(RequestTargetError),
    /// The HTTP version is not supported.
    Version(crate::primitives::version::HttpVersionError),
    /// A header name is invalid.
    HeaderName(crate::primitives::header_block::HeaderError),
    /// A header value is invalid.
    HeaderValue(crate::primitives::header_block::HeaderError),
    /// The request uses an authority-form or absolute-form URI.
    AbsoluteForm,
}

impl std::fmt::Display for RequestHeadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Target(e) => write!(f, "invalid request target: {e}"),
            Self::Version(e) => write!(f, "unsupported HTTP version: {e}"),
            Self::HeaderName(e) => write!(f, "invalid header name: {e}"),
            Self::HeaderValue(e) => write!(f, "invalid header value: {e}"),
            Self::AbsoluteForm => write!(f, "absolute-form URI not supported"),
        }
    }
}

impl std::error::Error for RequestHeadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Target(e) => Some(e),
            Self::Version(e) => Some(e),
            Self::HeaderName(e) => Some(e),
            Self::HeaderValue(e) => Some(e),
            Self::AbsoluteForm => None,
        }
    }
}

impl From<RequestTargetError> for RequestHeadError {
    fn from(e: RequestTargetError) -> Self {
        Self::Target(e)
    }
}

impl From<crate::primitives::version::HttpVersionError> for RequestHeadError {
    fn from(e: crate::primitives::version::HttpVersionError) -> Self {
        Self::Version(e)
    }
}

/// A canonical, transport-independent HTTP request head.
///
/// Carries the method, request target, HTTP version, and headers from
/// an HTTP request. Immutable after construction.
///
/// # Hyper independence
///
/// No Hyper type appears in this struct or its public API. Downstream
/// code can inspect requests using only public eggserve types.
///
/// # Construction
///
/// Build through validated constructors or from the runtime adapter
/// (Hyper → canonical conversion).
#[derive(Debug, Clone)]
pub struct RequestHead {
    method: Method,
    target: RequestTarget,
    version: HttpVersion,
    headers: HeaderBlock,
}

impl RequestHead {
    /// Create a new request head.
    ///
    /// Prefer using [`Self::try_from_hyper`] for converting from Hyper
    /// requests. This constructor is for direct construction in tests or
    /// downstream code that already has validated components.
    pub fn new(
        method: Method,
        target: RequestTarget,
        version: HttpVersion,
        headers: HeaderBlock,
    ) -> Self {
        Self {
            method,
            target,
            version,
            headers,
        }
    }

    /// Returns the request method.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Returns the request target.
    pub fn target(&self) -> &RequestTarget {
        &self.target
    }

    /// Returns the HTTP version.
    pub fn version(&self) -> HttpVersion {
        self.version
    }

    /// Returns the header block.
    pub fn headers(&self) -> &HeaderBlock {
        &self.headers
    }

    /// Returns `true` if this is a HEAD request.
    pub fn is_head(&self) -> bool {
        self.method.is_head()
    }

    /// Returns `true` if this is a GET request.
    pub fn is_get(&self) -> bool {
        self.method.is_get()
    }

    /// Returns `true` if the method permits static file resolution.
    pub fn permits_static_resolution(&self) -> bool {
        self.method.permits_static_resolution()
    }

    /// Convert a Hyper request into a canonical [`RequestHead`].
    ///
    /// Extracts method, URI, version, and headers from the Hyper request
    /// without consuming the body. The conversion is fallible and typed:
    /// malformed or unsupported input is rejected before handlers.
    ///
    /// # Errors
    ///
    /// Returns [`RequestHeadError`] if the request target, version, or
    /// headers are invalid.
    pub fn try_from_hyper<B>(req: &hyper::Request<B>) -> Result<Self, RequestHeadError> {
        let method =
            Method::new(req.method().as_str()).map_err(|_| RequestHeadError::AbsoluteForm)?;

        let uri = req.uri();
        if uri.authority().is_some() {
            return Err(RequestHeadError::AbsoluteForm);
        }
        let target_str = uri.to_string();
        let target = RequestTarget::parse(target_str)?;

        let version = HttpVersion::from(&req.version());

        let mut headers = HeaderBlock::with_capacity(req.headers().len());
        for (name, value) in req.headers().iter() {
            let header_name = crate::primitives::header_block::HeaderName::new(name.as_str())
                .map_err(RequestHeadError::HeaderName)?;
            let value_str = value.to_str().unwrap_or("").to_string();
            let header_value = crate::primitives::header_block::HeaderValue::new(value_str)
                .map_err(RequestHeadError::HeaderValue)?;
            headers.push(header_name, header_value);
        }

        Ok(Self::new(method, target, version, headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::method::Method;
    use crate::primitives::request_target::RequestTarget;
    use crate::primitives::version::HttpVersion;

    fn make_head(method: &str, target: &str) -> RequestHead {
        RequestHead::new(
            Method::new(method).unwrap(),
            RequestTarget::parse(target).unwrap(),
            HttpVersion::Http11,
            HeaderBlock::new(),
        )
    }

    #[test]
    fn basic_construction() {
        let head = make_head("GET", "/");
        assert_eq!(head.method().as_str(), "GET");
        assert_eq!(head.target().path(), "/");
        assert_eq!(head.version(), HttpVersion::Http11);
        assert!(head.headers().is_empty());
    }

    #[test]
    fn is_head() {
        let head = make_head("HEAD", "/foo");
        assert!(head.is_head());
        assert!(!head.is_get());
    }

    #[test]
    fn is_get() {
        let head = make_head("GET", "/foo");
        assert!(head.is_get());
        assert!(!head.is_head());
    }

    #[test]
    fn permits_static_resolution() {
        let head = make_head("GET", "/");
        assert!(head.permits_static_resolution());

        let head = make_head("HEAD", "/");
        assert!(head.permits_static_resolution());

        let head = make_head("POST", "/");
        assert!(!head.permits_static_resolution());
    }

    #[test]
    fn with_headers() {
        let mut headers = HeaderBlock::new();
        headers.push_str("content-type", "text/html").unwrap();
        let head = RequestHead::new(
            Method::get(),
            RequestTarget::parse("/").unwrap(),
            HttpVersion::Http11,
            headers,
        );
        assert!(head.headers().contains("content-type"));
    }

    #[test]
    fn clone_preserves_values() {
        let head = make_head("GET", "/foo?bar");
        let cloned = head.clone();
        assert_eq!(cloned.method().as_str(), "GET");
        assert_eq!(cloned.target().path(), "/foo");
        assert_eq!(cloned.target().query(), Some("bar"));
    }
}
