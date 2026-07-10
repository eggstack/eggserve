//! HTTP client implementation backed by hyper-util.
//!
//! The [`HttpClient`] performs synchronous HTTP/1.1 requests by driving a
//! tokio runtime internally. Each call to [`HttpClient::send`] starts a
//! short-lived runtime for the duration of the request.

use std::collections::HashMap;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio::runtime::Runtime;
use tokio::time::timeout;

use super::error::ClientError;
use super::request::{ClientConfig, ClientRequest};
use super::response::ClientResponse;

/// A low-level HTTP/1.1 client backed by Rust networking.
///
/// Each instance shares a configuration. Requests are executed
/// synchronously by spinning up a tokio runtime for the duration of each
/// call.
#[allow(dead_code)]
pub struct HttpClient {
    config: ClientConfig,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("config", &self.config)
            .finish()
    }
}

impl HttpClient {
    /// Create a new client with the given configuration.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// Create a new client with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ClientConfig::default())
    }

    /// Execute an HTTP request and return the full response.
    ///
    /// The response body is collected into memory. For large responses,
    /// use lower-level streaming APIs.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on connection failure, timeout, TLS error,
    /// or protocol error.
    pub fn send(&self, request: &ClientRequest) -> Result<ClientResponse, ClientError> {
        let rt = Runtime::new().map_err(ClientError::Io)?;
        rt.block_on(self.send_async(request))
    }

    async fn send_async(&self, request: &ClientRequest) -> Result<ClientResponse, ClientError> {
        let url = &request.url;
        let addr = format!("{}:{}", url.host, url.port);

        // Connect with timeout
        let connect_future = TcpStream::connect(&addr);
        let tcp = timeout(self.config.connect_timeout, connect_future)
            .await
            .map_err(|_| ClientError::Timeout("connection timed out".into()))?
            .map_err(|e| ClientError::ConnectError(e.to_string()))?;

        let _ = tcp.set_nodelay(true);

        let io = TokioIo::new(tcp);

        // Build hyper request
        let mut builder = Request::builder();
        builder = builder.method(request.method.as_str());

        let authority = url.authority();
        let uri = format!("{}://{}{}", url.scheme, authority, url.path);
        builder = builder.uri(&uri);

        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }

        if !request.headers.contains_key("host") {
            builder = builder.header("host", &authority);
        }

        if !request.headers.contains_key("user-agent") {
            builder = builder.header("user-agent", "eggserve-client/0.1");
        }

        let body_bytes = request.body.as_deref().unwrap_or(&[]);
        let hyper_req: Request<Full<Bytes>> = builder
            .body(Full::new(Bytes::copy_from_slice(body_bytes)))
            .map_err(|e| ClientError::ProtocolError(e.to_string()))?;

        // Perform HTTP/1.1 handshake and send
        let response = timeout(self.config.request_timeout, async {
            let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
                .await
                .map_err(|e| ClientError::ProtocolError(e.to_string()))?;

            tokio::spawn(async move {
                let _ = conn.await;
            });

            sender
                .send_request(hyper_req)
                .await
                .map_err(|e| ClientError::ProtocolError(e.to_string()))
        })
        .await
        .map_err(|_| ClientError::Timeout("request timed out".into()))?;

        let response = response?;

        let (parts, body) = response.into_parts();

        let body_bytes = collect_body(body, self.config.max_response_body_bytes).await?;

        let mut headers = HashMap::new();
        for (name, value) in &parts.headers {
            if let Ok(v) = value.to_str() {
                headers.insert(name.as_str().to_lowercase(), v.to_string());
            }
        }

        Ok(ClientResponse {
            status: parts.status.as_u16(),
            headers,
            body: body_bytes,
        })
    }
}

async fn collect_body(body: Incoming, max_bytes: Option<u64>) -> Result<Vec<u8>, ClientError> {
    let mut collected = Vec::new();
    let mut total: u64 = 0;

    let mut body = body;
    loop {
        let chunk = body
            .frame()
            .await
            .transpose()
            .map_err(|e| ClientError::ProtocolError(format!("{e}")))?;

        match chunk {
            Some(data) => {
                let data = data
                    .into_data()
                    .map_err(|_| ClientError::ProtocolError("non-data frame in response".into()))?;
                total += data.len() as u64;
                if let Some(limit) = max_bytes {
                    if total > limit {
                        return Err(ClientError::ResponseBodyTooLarge { limit });
                    }
                }
                collected.extend_from_slice(&data);
            }
            None => break,
        }
    }

    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn client_creation() {
        let client = HttpClient::with_defaults();
        assert_eq!(client.config.connect_timeout, Duration::from_secs(10));
    }

    #[test]
    fn client_with_custom_config() {
        let config = ClientConfig {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(10),
            max_response_body_bytes: Some(1024),
            verify_tls: true,
        };
        let client = HttpClient::new(config);
        assert_eq!(client.config.connect_timeout, Duration::from_secs(5));
        assert_eq!(client.config.request_timeout, Duration::from_secs(10));
    }
}
