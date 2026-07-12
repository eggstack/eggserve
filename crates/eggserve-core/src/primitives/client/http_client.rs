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

#[cfg(feature = "client-tls")]
use std::sync::Arc;
#[cfg(feature = "client-tls")]
use tokio_rustls::{rustls, TlsConnector};

#[cfg(feature = "client-tls")]
#[derive(Debug)]
struct NoVerifier;

#[cfg(feature = "client-tls")]
impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

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
    /// The response body is fully buffered in memory, up to
    /// `max_response_body_bytes`. Streaming is not yet supported.
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

        #[cfg(not(feature = "client-tls"))]
        if url.is_https() {
            return Err(ClientError::TlsError(
                "TLS support not enabled; enable the client-tls feature".into(),
            ));
        }

        let addr = if url.host.contains(':') {
            format!("[{}]:{}", url.host, url.port)
        } else {
            format!("{}:{}", url.host, url.port)
        };

        // Connect with timeout
        let connect_future = TcpStream::connect(&addr);
        let tcp = timeout(self.config.connect_timeout, connect_future)
            .await
            .map_err(|_| ClientError::Timeout("connection timed out".into()))?
            .map_err(|e| ClientError::ConnectError(e.to_string()))?;

        let _ = tcp.set_nodelay(true);

        let authority = url.authority();

        #[cfg(feature = "client-tls")]
        if url.is_https() {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

            let config = if self.config.verify_tls {
                rustls::ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth()
            } else {
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(NoVerifier))
                    .with_no_client_auth()
            };

            let connector = TlsConnector::from(Arc::new(config));
            let domain = rustls::pki_types::ServerName::try_from(url.host.as_str())
                .map_err(|e| ClientError::TlsError(format!("invalid server name: {e}")))?;
            let tls_stream = timeout(
                self.config.connect_timeout,
                connector.connect(domain.to_owned(), tcp),
            )
            .await
            .map_err(|_| ClientError::Timeout("TLS handshake timed out".into()))?
            .map_err(|e| ClientError::TlsError(format!("TLS handshake failed: {e}")))?;

            return send_request_over_io(
                TokioIo::new(tls_stream),
                request,
                &authority,
                self.config.request_timeout,
                self.config.max_response_body_bytes,
            )
            .await;
        }

        #[cfg(not(feature = "client-tls"))]
        {
            send_request_over_io(
                TokioIo::new(tcp),
                request,
                &authority,
                self.config.request_timeout,
                self.config.max_response_body_bytes,
            )
            .await
        }

        #[cfg(feature = "client-tls")]
        {
            send_request_over_io(
                TokioIo::new(tcp),
                request,
                &authority,
                self.config.request_timeout,
                self.config.max_response_body_bytes,
            )
            .await
        }
    }
}

async fn send_request_over_io<I>(
    io: I,
    request: &ClientRequest,
    authority: &str,
    request_timeout: std::time::Duration,
    max_response_body_bytes: Option<u64>,
) -> Result<ClientResponse, ClientError>
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let url = &request.url;

    // Build hyper request
    let mut builder = Request::builder();
    builder = builder.method(request.method.as_str());

    builder = builder.uri(&url.path);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    if !request.headers.contains_key("host") {
        builder = builder.header("host", authority);
    }

    if !request.headers.contains_key("user-agent") {
        builder = builder.header("user-agent", "eggserve-client/0.1");
    }

    let body_bytes = request.body.as_deref().unwrap_or(&[]);
    let hyper_req: Request<Full<Bytes>> = builder
        .body(Full::new(Bytes::copy_from_slice(body_bytes)))
        .map_err(|e| ClientError::ProtocolError(e.to_string()))?;

    // Perform HTTP/1.1 handshake, send, and collect body within timeout
    let (parts, body_bytes) = timeout(request_timeout, async {
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| ClientError::ProtocolError(e.to_string()))?;

        tokio::spawn(async move {
            let _ = conn.await;
        });

        let response = sender
            .send_request(hyper_req)
            .await
            .map_err(|e| ClientError::ProtocolError(e.to_string()))?;

        let (parts, body) = response.into_parts();

        let body_bytes = collect_body(body, max_response_body_bytes).await?;

        Ok::<_, ClientError>((parts, body_bytes))
    })
    .await
    .map_err(|_| ClientError::Timeout("request timed out".into()))??;

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
    #[allow(unused_imports)]
    use crate::primitives::client::{ClientRequestBuilder, Method};
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

    #[cfg(not(feature = "client-tls"))]
    #[test]
    fn client_rejects_https_without_tls() {
        let client = HttpClient::with_defaults();
        let req = ClientRequestBuilder::new(Method::Get)
            .url("https://example.com/")
            .unwrap()
            .build()
            .unwrap();
        let result = client.send(&req);
        assert!(result.is_err());
        match result.unwrap_err() {
            ClientError::TlsError(_) => {}
            other => panic!("expected TlsError, got {other:?}"),
        }
    }
}
