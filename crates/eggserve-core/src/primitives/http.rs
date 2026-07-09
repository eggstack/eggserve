//! Request validation primitives for static/read-only serving.
//!
//! These types decouple HTTP method and body-framing validation from the
//! server loop so callers can pre-validate requests without depending on
//! Hyper types.

/// Supported read-only HTTP methods for static file serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadOnlyMethod {
    Get,
    Head,
}

impl ReadOnlyMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
        }
    }
}

impl std::fmt::Display for ReadOnlyMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors from request validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestValidationError {
    /// Method not supported for static serving.
    MethodNotAllowed,
    /// Malformed `Content-Length` header (non-numeric, empty, negative, or
    /// overflowing u64).
    InvalidContentLength,
    /// `Content-Length` exceeds the configured body size limit.
    BodyTooLarge,
    /// Non-empty `Transfer-Encoding` on a read-only request.
    UnsupportedTransferEncoding,
    /// Both `Content-Length` and `Transfer-Encoding` present.
    ConflictingBodyHeaders,
    /// Request target is not valid origin-form (must start with `/`).
    InvalidRequestTarget,
}

impl std::fmt::Display for RequestValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MethodNotAllowed => write!(f, "method not allowed for static serving"),
            Self::InvalidContentLength => write!(f, "malformed Content-Length"),
            Self::BodyTooLarge => write!(f, "request body too large"),
            Self::UnsupportedTransferEncoding => {
                write!(f, "unsupported Transfer-Encoding on read-only request")
            }
            Self::ConflictingBodyHeaders => {
                write!(f, "both Content-Length and Transfer-Encoding present")
            }
            Self::InvalidRequestTarget => write!(f, "request target is not valid origin-form"),
        }
    }
}

impl std::error::Error for RequestValidationError {}

/// Check if a method string is a supported read-only method.
pub fn validate_method(method: &str) -> Result<ReadOnlyMethod, RequestValidationError> {
    match method {
        "GET" => Ok(ReadOnlyMethod::Get),
        "HEAD" => Ok(ReadOnlyMethod::Head),
        _ => Err(RequestValidationError::MethodNotAllowed),
    }
}

/// Validate that a request has no body, as expected for GET/HEAD.
///
/// Checks:
/// - No `Content-Length` and `Transfer-Encoding` together
/// - No non-empty `Transfer-Encoding`
/// - `Content-Length` (if present) is zero or valid and within `max_body_bytes`
pub fn validate_request_body(
    content_length: Option<&str>,
    transfer_encoding: Option<&str>,
    max_body_bytes: u64,
) -> Result<(), RequestValidationError> {
    if content_length.is_some() && transfer_encoding.is_some() {
        return Err(RequestValidationError::ConflictingBodyHeaders);
    }

    if let Some(te) = transfer_encoding {
        if !te.trim().is_empty() {
            return Err(RequestValidationError::UnsupportedTransferEncoding);
        }
    }

    if let Some(cl) = content_length {
        let trimmed = cl.trim();
        if trimmed.is_empty() {
            return Err(RequestValidationError::InvalidContentLength);
        }
        if !trimmed.chars().all(|c| c.is_ascii_digit()) {
            return Err(RequestValidationError::InvalidContentLength);
        }
        let len: u64 = trimmed
            .parse()
            .map_err(|_| RequestValidationError::InvalidContentLength)?;
        if len > max_body_bytes {
            return Err(RequestValidationError::BodyTooLarge);
        }
    }

    Ok(())
}

/// Validate a request target is valid origin-form (starts with `/`).
pub fn validate_request_target(target: &str) -> Result<(), RequestValidationError> {
    if target.is_empty() || !target.starts_with('/') {
        return Err(RequestValidationError::InvalidRequestTarget);
    }
    if target.contains(char::is_whitespace) {
        return Err(RequestValidationError::InvalidRequestTarget);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_is_allowed() {
        assert_eq!(validate_method("GET").unwrap(), ReadOnlyMethod::Get);
    }

    #[test]
    fn head_is_allowed() {
        assert_eq!(validate_method("HEAD").unwrap(), ReadOnlyMethod::Head);
    }

    #[test]
    fn post_is_rejected() {
        assert_eq!(
            validate_method("POST").unwrap_err(),
            RequestValidationError::MethodNotAllowed
        );
    }

    #[test]
    fn put_is_rejected() {
        assert_eq!(
            validate_method("PUT").unwrap_err(),
            RequestValidationError::MethodNotAllowed
        );
    }

    #[test]
    fn delete_is_rejected() {
        assert_eq!(
            validate_method("DELETE").unwrap_err(),
            RequestValidationError::MethodNotAllowed
        );
    }

    #[test]
    fn patch_is_rejected() {
        assert_eq!(
            validate_method("PATCH").unwrap_err(),
            RequestValidationError::MethodNotAllowed
        );
    }

    #[test]
    fn options_is_rejected() {
        assert_eq!(
            validate_method("OPTIONS").unwrap_err(),
            RequestValidationError::MethodNotAllowed
        );
    }

    #[test]
    fn zero_content_length_allowed() {
        assert!(validate_request_body(Some("0"), None, 0).is_ok());
    }

    #[test]
    fn positive_content_length_rejected() {
        assert_eq!(
            validate_request_body(Some("1024"), None, 0).unwrap_err(),
            RequestValidationError::BodyTooLarge
        );
    }

    #[test]
    fn invalid_content_length_rejected() {
        assert_eq!(
            validate_request_body(Some("not-a-number"), None, 0).unwrap_err(),
            RequestValidationError::InvalidContentLength
        );
    }

    #[test]
    fn negative_content_length_rejected() {
        assert_eq!(
            validate_request_body(Some("-1"), None, 0).unwrap_err(),
            RequestValidationError::InvalidContentLength
        );
    }

    #[test]
    fn overflowing_content_length_rejected() {
        assert_eq!(
            validate_request_body(Some("99999999999999999999"), None, 0).unwrap_err(),
            RequestValidationError::InvalidContentLength
        );
    }

    #[test]
    fn nonempty_transfer_encoding_rejected() {
        assert_eq!(
            validate_request_body(None, Some("chunked"), 0).unwrap_err(),
            RequestValidationError::UnsupportedTransferEncoding
        );
    }

    #[test]
    fn content_length_and_transfer_encoding_rejected() {
        assert_eq!(
            validate_request_body(Some("0"), Some("chunked"), 0).unwrap_err(),
            RequestValidationError::ConflictingBodyHeaders
        );
    }

    #[test]
    fn empty_transfer_encoding_allowed() {
        assert!(validate_request_body(None, Some(""), 0).is_ok());
    }

    #[test]
    fn whitespace_transfer_encoding_allowed() {
        assert!(validate_request_body(None, Some("  "), 0).is_ok());
    }

    #[test]
    fn no_headers_allowed() {
        assert!(validate_request_body(None, None, 0).is_ok());
    }

    #[test]
    fn empty_content_length_rejected() {
        assert_eq!(
            validate_request_body(Some(""), None, 0).unwrap_err(),
            RequestValidationError::InvalidContentLength
        );
    }

    #[test]
    fn whitespace_only_content_length_rejected() {
        assert_eq!(
            validate_request_body(Some("  "), None, 0).unwrap_err(),
            RequestValidationError::InvalidContentLength
        );
    }

    #[test]
    fn content_length_with_max_body_bytes() {
        assert!(validate_request_body(Some("100"), None, 100).is_ok());
        assert_eq!(
            validate_request_body(Some("101"), None, 100).unwrap_err(),
            RequestValidationError::BodyTooLarge
        );
    }

    #[test]
    fn valid_request_target() {
        assert!(validate_request_target("/").is_ok());
        assert!(validate_request_target("/foo").is_ok());
        assert!(validate_request_target("/foo/bar").is_ok());
        assert!(validate_request_target("/file.txt").is_ok());
    }

    #[test]
    fn empty_request_target_rejected() {
        assert_eq!(
            validate_request_target("").unwrap_err(),
            RequestValidationError::InvalidRequestTarget
        );
    }

    #[test]
    fn absolute_uri_request_target_rejected() {
        assert_eq!(
            validate_request_target("http://example.com/").unwrap_err(),
            RequestValidationError::InvalidRequestTarget
        );
    }

    #[test]
    fn asterisk_request_target_rejected() {
        assert_eq!(
            validate_request_target("*").unwrap_err(),
            RequestValidationError::InvalidRequestTarget
        );
    }

    #[test]
    fn whitespace_in_request_target_rejected() {
        assert_eq!(
            validate_request_target("/foo bar").unwrap_err(),
            RequestValidationError::InvalidRequestTarget
        );
    }

    #[test]
    fn read_only_method_as_str() {
        assert_eq!(ReadOnlyMethod::Get.as_str(), "GET");
        assert_eq!(ReadOnlyMethod::Head.as_str(), "HEAD");
    }

    #[test]
    fn read_only_method_display() {
        assert_eq!(format!("{}", ReadOnlyMethod::Get), "GET");
        assert_eq!(format!("{}", ReadOnlyMethod::Head), "HEAD");
    }

    #[test]
    fn request_validation_error_is_display() {
        let err = RequestValidationError::MethodNotAllowed;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn request_validation_error_is_error() {
        let err: &dyn std::error::Error = &RequestValidationError::BodyTooLarge;
        assert!(!err.to_string().is_empty());
    }
}
