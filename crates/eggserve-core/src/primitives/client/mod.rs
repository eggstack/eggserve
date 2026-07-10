//! HTTP client primitive substrate.
//!
//! This module provides a low-level, Rust-backed HTTP/1.1 client. It is
//! feature-gated behind `client` and optionally `client-tls` for HTTPS
//! support.
//!
//! # Design
//!
//! The client is deliberately minimal: no cookie jar, no redirect
//! following, no proxy support, no HTTP/2. It is a substrate for
//! downstream projects to build higher-level clients on.
//!
//! # Behavior
//!
//! The client buffers the complete response body in memory. The
//! `max_response_body_bytes` config option enforces an upper bound.
//! Streaming responses are not yet supported and are planned for a
//! future release.
//!
//! # Usage
//!
//! ```rust,no_run
//! use eggserve_core::primitives::client::{
//!     ClientConfig, HttpClient, ClientRequestBuilder, Method,
//! };
//!
//! let client = HttpClient::with_defaults();
//! let request = ClientRequestBuilder::new(Method::Get)
//!     .url("http://localhost:8000/")
//!     .unwrap()
//!     .build()
//!     .unwrap();
//! let response = client.send(&request).unwrap();
//! assert!(response.is_success());
//! ```

pub mod error;
pub mod http_client;
pub mod request;
pub mod response;
pub mod url;

pub use error::ClientError;
pub use http_client::HttpClient;
pub use request::{validate_header, ClientConfig, ClientRequest, ClientRequestBuilder, Method};
pub use response::ClientResponse;
pub use url::{ParsedUrl, Scheme};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_error_display() {
        let err = ClientError::InvalidUrl("bad".into());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn client_error_is_error() {
        let err: &dyn std::error::Error = &ClientError::Timeout("test".into());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn parsed_url_roundtrip() {
        let url = ParsedUrl::parse("http://example.com:8080/api").unwrap();
        assert_eq!(url.to_string(), "http://example.com:8080/api");
    }
}
