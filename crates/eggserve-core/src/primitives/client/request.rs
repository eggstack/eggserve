//! Request model for the HTTP client.

use std::collections::HashMap;
use std::time::Duration;

use super::error::ClientError;
use super::url::ParsedUrl;

/// Configuration for an HTTP client instance.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Timeout for establishing a TCP connection.
    pub connect_timeout: Duration,
    /// Overall timeout for the entire request (connect + headers + body).
    pub request_timeout: Duration,
    /// Maximum size in bytes for a response body read via [`ClientResponse::body`].
    pub max_response_body_bytes: Option<u64>,
    /// Whether to verify TLS certificates. Only applies when the `client-tls`
    /// feature is enabled and the URL uses `https://`.
    pub verify_tls: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            max_response_body_bytes: Some(10 * 1024 * 1024), // 10 MiB
            verify_tls: true,
        }
    }
}

/// HTTP method for client requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An HTTP client request.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ClientRequest {
    pub method: Method,
    pub url: ParsedUrl,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Builder for constructing a [`ClientRequest`].
#[derive(Debug)]
#[allow(dead_code)]
pub struct ClientRequestBuilder {
    method: Method,
    url: Option<ParsedUrl>,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
}

impl ClientRequestBuilder {
    pub fn new(method: Method) -> Self {
        Self {
            method,
            url: None,
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn url(mut self, url: &str) -> Result<Self, ClientError> {
        self.url = Some(ParsedUrl::parse(url)?);
        Ok(self)
    }

    pub fn header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ClientError> {
        let name = name.into();
        let value = value.into();
        validate_header(&name, &value)?;
        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    pub fn build(self) -> Result<ClientRequest, ClientError> {
        let url = self
            .url
            .ok_or_else(|| ClientError::InvalidUrl("URL not set".into()))?;

        if matches!(self.method, Method::Get | Method::Head) && self.body.is_some() {
            return Err(ClientError::ProtocolError(
                "GET and HEAD requests must not have a body".into(),
            ));
        }

        Ok(ClientRequest {
            method: self.method,
            url,
            headers: self.headers,
            body: self.body,
        })
    }
}

/// Validate a header name and value.
///
/// Header names must be non-empty ASCII tokens (no control characters,
/// spaces, or delimiters). Values must not contain null bytes or bare
/// newlines.
#[allow(dead_code)]
pub fn validate_header(name: &str, value: &str) -> Result<(), ClientError> {
    if name.is_empty() {
        return Err(ClientError::InvalidHeader("header name is empty".into()));
    }

    for byte in name.bytes() {
        if !is_token_byte(byte) {
            return Err(ClientError::InvalidHeader(format!(
                "invalid character in header name: {byte:#04x}"
            )));
        }
    }

    if value.contains('\0') {
        return Err(ClientError::InvalidHeader(
            "header value contains null byte".into(),
        ));
    }

    if value.contains('\n') || value.contains('\r') {
        return Err(ClientError::InvalidHeader(
            "header value contains CR or LF".into(),
        ));
    }

    Ok(())
}

/// RFC 7230 token character: visible ASCII except delimiters.
fn is_token_byte(byte: u8) -> bool {
    matches!(byte, b'!' | b'#'..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z')
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn default_config_has_safe_timeouts() {
        let config = ClientConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.max_response_body_bytes, Some(10 * 1024 * 1024));
        assert!(config.verify_tls);
    }

    #[test]
    fn method_as_str() {
        assert_eq!(Method::Get.as_str(), "GET");
        assert_eq!(Method::Post.as_str(), "POST");
        assert_eq!(Method::Put.as_str(), "PUT");
        assert_eq!(Method::Delete.as_str(), "DELETE");
        assert_eq!(Method::Patch.as_str(), "PATCH");
    }

    #[test]
    fn method_display() {
        assert_eq!(format!("{}", Method::Get), "GET");
    }

    #[test]
    fn request_builder_minimal() {
        let req = ClientRequestBuilder::new(Method::Get)
            .url("http://example.com/")
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.url.host, "example.com");
        assert!(req.body.is_none());
    }

    #[test]
    fn request_builder_with_header() {
        let req = ClientRequestBuilder::new(Method::Get)
            .url("http://example.com/")
            .unwrap()
            .header("accept", "text/plain")
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(req.headers.get("accept").unwrap(), "text/plain");
    }

    #[test]
    fn request_builder_with_body() {
        let req = ClientRequestBuilder::new(Method::Post)
            .url("http://example.com/")
            .unwrap()
            .body(b"hello".to_vec())
            .build()
            .unwrap();
        assert_eq!(req.body.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn request_builder_get_with_body_rejected() {
        let result = ClientRequestBuilder::new(Method::Get)
            .url("http://example.com/")
            .unwrap()
            .body(b"hello".to_vec())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn request_builder_head_with_body_rejected() {
        let result = ClientRequestBuilder::new(Method::Head)
            .url("http://example.com/")
            .unwrap()
            .body(b"hello".to_vec())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn request_builder_missing_url_rejected() {
        let result = ClientRequestBuilder::new(Method::Get).build();
        assert!(result.is_err());
    }

    #[test]
    fn validate_header_valid() {
        assert!(validate_header("Content-Type", "text/plain").is_ok());
        assert!(validate_header("X-Custom-Header", "value").is_ok());
    }

    #[test]
    fn validate_header_empty_name_rejected() {
        assert!(validate_header("", "value").is_err());
    }

    #[test]
    fn validate_header_control_char_rejected() {
        assert!(validate_header("Bad\x01Name", "value").is_err());
    }

    #[test]
    fn validate_header_space_in_name_rejected() {
        assert!(validate_header("Bad Name", "value").is_err());
    }

    #[test]
    fn validate_header_null_in_value_rejected() {
        assert!(validate_header("X-Test", "bad\x00value").is_err());
    }

    #[test]
    fn validate_header_newline_in_value_rejected() {
        assert!(validate_header("X-Test", "bad\nvalue").is_err());
        assert!(validate_header("X-Test", "bad\r\nvalue").is_err());
        assert!(validate_header("X-Test", "\rvalue").is_err());
    }

    #[test]
    fn is_token_byte_valid() {
        assert!(is_token_byte(b'A'));
        assert!(is_token_byte(b'0'));
        assert!(is_token_byte(b'-'));
        assert!(is_token_byte(b'_'));
        assert!(is_token_byte(b'!'));
    }

    #[test]
    fn is_token_byte_invalid() {
        assert!(!is_token_byte(b' '));
        assert!(!is_token_byte(b':'));
        assert!(!is_token_byte(b','));
        assert!(!is_token_byte(b'\x01'));
    }

    proptest::proptest! {
        #[test]
        fn validate_header_never_panics(name in ".*", value in ".*") {
            let _ = validate_header(&name, &value);
        }

        #[test]
        fn valid_header_name_accepted(name in "[!#-'*+-.0-9A-Z^_`a-z|~]+") {
            prop_assert!(validate_header(&name, "test-value").is_ok());
        }

        #[test]
        fn empty_name_rejected(name in "") {
            prop_assert!(validate_header(name, "value").is_err());
        }

        #[test]
        fn null_in_value_rejected(name in "[A-Za-z-]+") {
            prop_assert!(validate_header(&name, "bad\x00value").is_err());
        }

        #[test]
        fn newline_in_value_rejected(name in "[A-Za-z-]+") {
            prop_assert!(validate_header(&name, "bad\nvalue").is_err());
            prop_assert!(validate_header(&name, "bad\rvalue").is_err());
        }

        #[test]
        fn token_byte_correctness(byte in 0u8..=255) {
            let expected = matches!(byte,
                b'!' | b'#'..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
            );
            prop_assert_eq!(is_token_byte(byte), expected);
        }
    }
}
