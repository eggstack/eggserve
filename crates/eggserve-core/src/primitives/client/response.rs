//! Response model for the HTTP client.

use std::collections::HashMap;

/// An HTTP client response.
///
/// The body is collected into memory on receipt. For large responses, use
/// the streaming API on [`super::http_client::HttpClient`] directly.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ClientResponse {
    /// HTTP status code (e.g. 200, 404).
    pub status: u16,
    /// Response headers as a case-insensitive map.
    ///
    /// If multiple headers share the same name, only the last value is
    /// stored.
    pub headers: HashMap<String, String>,
    /// The response body bytes.
    pub body: Vec<u8>,
}

#[allow(dead_code)]
impl ClientResponse {
    /// Returns the Content-Length header value parsed as u64, if present.
    pub fn content_length(&self) -> Option<u64> {
        self.headers
            .get("content-length")
            .and_then(|v| v.trim().parse().ok())
    }

    /// Returns the Content-Type header value, if present.
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type").map(|s| s.as_str())
    }

    /// Returns true if the status code indicates success (2xx).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Returns the body as a string slice, if the body is valid UTF-8.
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }

    /// Returns the body bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }
}
