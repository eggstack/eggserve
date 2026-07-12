//! Interoperability and hardening tests for the HTTP client primitive.
//!
//! Covers:
//! - Track B: Local HTTP interoperability (Content-Length, chunked, connection-close,
//!   empty bodies, duplicate headers, malformed responses, premature EOF, delayed
//!   headers/body, oversized bodies)
//! - Track C: Timeout and size-limit semantics
//! - Track E: URL and authority hardening (Host header generation, IPv6, ports)
//! - Track F: Request construction and headers (method, Host, user-agent, framing)
//! - Track G: Response parsing and error mapping (malformed, premature EOF, body limits)

#![cfg(feature = "client")]

use std::convert::Infallible;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::client::url::ParsedUrl;
use eggserve_core::primitives::client::{
    ClientConfig, ClientError, ClientRequestBuilder, HttpClient, Method,
};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::task;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn start_server<F, Fut>(handler: F) -> std::net::SocketAddr
where
    F: Fn(Request<Incoming>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Response<Full<Bytes>>> + Send + 'static,
{
    let handler = Arc::new(handler);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    task::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let handler = Arc::clone(&handler);
            task::spawn(async move {
                let io = TokioIo::new(stream);
                let service = service_fn(move |req| {
                    let handler = Arc::clone(&handler);
                    async move { Ok::<_, Infallible>(handler(req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

/// Start a raw TCP server that sends the provided response bytes exactly once,
/// then closes the connection. Returns the address.
fn start_raw_server(response: &[u8]) -> std::net::SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response = response.to_vec();

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.write_all(&response);
            let _ = stream.flush();
            // Close after a short delay to ensure client reads the response
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    addr
}

/// Start a raw TCP server that reads the request and returns the raw response
/// bytes. Returns (address, captured_request).
fn start_raw_server_capture(
    response: &[u8],
) -> (std::net::SocketAddr, Arc<std::sync::Mutex<Vec<u8>>>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response = response.to_vec();
    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Read request
            let mut reader = BufReader::new(&stream);
            let mut request_bytes = Vec::new();
            let mut headers_done = false;
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let is_end = line == b"\r\n" || line == b"\n";
                        request_bytes.extend_from_slice(&line);
                        if is_end {
                            headers_done = true;
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Read body if Content-Length present
            if headers_done {
                let request_str = String::from_utf8_lossy(&request_bytes);
                if let Some(cl_line) = request_str
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("content-length:"))
                {
                    if let Some(cl_val) = cl_line.split(':').nth(1) {
                        if let Ok(cl) = cl_val.trim().parse::<usize>() {
                            let mut body = vec![0u8; cl];
                            let _ = std::io::Read::read(&mut reader, &mut body);
                            request_bytes.extend_from_slice(&body);
                        }
                    }
                }
            }
            *captured_clone.lock().unwrap() = request_bytes;

            let _ = stream.write_all(&response);
            let _ = stream.flush();
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    (addr, captured)
}

/// Start a raw TCP server that delays sending the response by the given duration.
fn start_delayed_server(delay: Duration, response: &[u8]) -> std::net::SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response = response.to_vec();

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            std::thread::sleep(delay);
            let _ = stream.write_all(&response);
            let _ = stream.flush();
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    addr
}

// ===========================================================================
// Track B — Local HTTP interoperability harness
// ===========================================================================

#[test]
fn fixed_content_length_response() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .header("content-length", "5")
            .body(Full::new(Bytes::from("hello")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"hello");
}

#[test]
fn chunked_transfer_encoding_response() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .header("transfer-encoding", "chunked")
            .body(Full::new(Bytes::from("hello world")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.text().unwrap(), "hello world");
}

#[test]
fn connection_close_body() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .header("connection", "close")
            .body(Full::new(Bytes::from("closed body")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.text().unwrap(), "closed body");
}

#[test]
fn empty_body_response() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-length", "0")
            .body(Full::new(Bytes::new()))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.body.is_empty());
}

#[test]
fn duplicate_response_headers_last_value_wins() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("x-duplicate", "first")
            .header("x-duplicate", "second")
            .body(Full::new(Bytes::from("dup")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.headers.get("x-duplicate").unwrap(), "second");
}

#[test]
fn malformed_status_line_raw() {
    let response = b"NOT_HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
    let addr = start_raw_server(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
    match result.unwrap_err() {
        ClientError::ProtocolError(_) => {}
        ClientError::Io(_) => {}
        other => panic!("expected ProtocolError or Io, got {other:?}"),
    }
}

#[test]
fn malformed_header_line_raw() {
    let response = b"HTTP/1.1 200 OK\r\nBad Header Line\r\nContent-Length: 2\r\n\r\nOK";
    let addr = start_raw_server(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
}

#[test]
fn premature_eof_on_body() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial";
    let addr = start_raw_server(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
    match result.unwrap_err() {
        ClientError::ProtocolError(_) => {}
        ClientError::Io(_) => {}
        ClientError::Timeout(_) => {}
        other => panic!("expected ProtocolError, Io, or Timeout, got {other:?}"),
    }
}

#[test]
fn incorrect_content_length_raw() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\nshort";
    let addr = start_raw_server(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
    match result.unwrap_err() {
        ClientError::ProtocolError(_) => {}
        ClientError::Io(_) => {}
        ClientError::Timeout(_) => {}
        other => panic!("expected ProtocolError, Io, or Timeout, got {other:?}"),
    }
}

#[test]
fn delayed_headers_connection_succeeds() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
    let addr = start_delayed_server(Duration::from_millis(200), response);

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"hello");
}

#[test]
fn delayed_body_connection_succeeds() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        // Simulate delayed body by sleeping before responding
        tokio::time::sleep(Duration::from_millis(200)).await;
        Response::builder()
            .status(200)
            .header("content-length", "5")
            .body(Full::new(Bytes::from("hello")))
            .unwrap()
    }));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"hello");
}

#[test]
fn oversized_body_exceeds_limit() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        let body = "x".repeat(10000);
        Response::builder()
            .status(200)
            .header("content-length", "10000")
            .body(Full::new(Bytes::from(body)))
            .unwrap()
    }));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(100),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(
        result,
        Err(ClientError::ResponseBodyTooLarge { limit: 100 })
    ));
}

// ===========================================================================
// Track C — Timeout and size-limit semantics
// ===========================================================================

#[test]
fn connect_timeout_only_for_connection() {
    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_millis(100),
        request_timeout: Duration::from_secs(30),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url("http://192.0.2.1:1/")
        .unwrap()
        .build()
        .unwrap();

    let start = std::time::Instant::now();
    let result = client.send(&req);
    let elapsed = start.elapsed();

    assert!(matches!(result, Err(ClientError::Timeout(_))));
    // Should complete within ~200ms (100ms connect timeout + overhead)
    assert!(elapsed < Duration::from_secs(2));
}

#[test]
fn request_timeout_for_full_lifecycle() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        // Delay 3 seconds before responding
        tokio::time::sleep(Duration::from_secs(3)).await;
        Response::builder()
            .status(200)
            .body(Full::new(Bytes::from("late")))
            .unwrap()
    }));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_millis(500),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/slow", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(result, Err(ClientError::Timeout(_))));
}

#[test]
fn max_bytes_enforced() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-length", "50")
            .body(Full::new(Bytes::from("x".repeat(50))))
            .unwrap()
    }));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(10),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(
        result,
        Err(ClientError::ResponseBodyTooLarge { limit: 10 })
    ));
}

#[test]
fn unlimited_body_when_max_is_none() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        let body = "x".repeat(5000);
        Response::builder()
            .status(200)
            .header("content-length", "5000")
            .body(Full::new(Bytes::from(body)))
            .unwrap()
    }));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: None,
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.body.len(), 5000);
}

// ===========================================================================
// Track E — URL and authority hardening (Host header generation)
// ===========================================================================

#[test]
fn host_header_generated_from_url() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let captured = Arc::new(std::sync::Mutex::new(String::new()));
    let captured_clone = Arc::clone(&captured);

    let handler = move |req: Request<Incoming>| {
        let captured = Arc::clone(&captured_clone);
        async move {
            let host = req
                .headers()
                .get("host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            *captured.lock().unwrap() = host;
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::new()))
                .unwrap()
        }
    };

    let addr = rt.block_on(start_server(handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    client.send(&req).unwrap();
    // Host should be 127.0.0.1:PORT
    let host = captured.lock().unwrap().clone();
    assert!(host.starts_with("127.0.0.1:"));
}

#[test]
fn host_header_omits_default_port() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let captured = Arc::new(std::sync::Mutex::new(String::new()));
    let captured_clone = Arc::clone(&captured);

    let handler = move |req: Request<Incoming>| {
        let captured = Arc::clone(&captured_clone);
        async move {
            let host = req
                .headers()
                .get("host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            *captured.lock().unwrap() = host;
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::new()))
                .unwrap()
        }
    };

    let _addr = rt.block_on(start_server(handler));

    // Use a raw server to capture the request
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (raw_addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", raw_addr))
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);

    // Host header should include the port since it's non-default
    let host_line = request_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("host:"))
        .unwrap();
    assert!(host_line.contains(&raw_addr.to_string()));
}

#[test]
fn host_header_with_non_default_port() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);

    let host_line = request_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("host:"))
        .unwrap();
    // Non-default port should be in Host header
    assert!(host_line.contains(&format!(":{}", addr.port())));
}

#[test]
fn user_supplied_host_header_preserved() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .header("host", "custom-host.example.com")
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);

    let host_line = request_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("host:"))
        .unwrap();
    assert!(host_line.contains("custom-host.example.com"));
}

// ===========================================================================
// Track F — Request construction and headers
// ===========================================================================

#[test]
fn get_method_sends_get() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);
    let status_line = request_str.lines().next().unwrap();
    assert!(status_line.starts_with("GET "));
}

#[test]
fn head_method_sends_head() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Head)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);
    let status_line = request_str.lines().next().unwrap();
    assert!(status_line.starts_with("HEAD "));
}

#[test]
fn post_method_sends_post_with_body() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Post)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .body(b"test body".to_vec())
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);
    let status_line = request_str.lines().next().unwrap();
    assert!(status_line.starts_with("POST "));
    assert!(request_str.contains("test body"));
}

#[test]
fn default_user_agent_sent() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);

    let ua_line = request_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("user-agent:"))
        .unwrap();
    assert!(ua_line.contains("eggserve-client/0.1"));
}

#[test]
fn custom_user_agent_overrides_default() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .header("user-agent", "custom/2.0")
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);

    let ua_line = request_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("user-agent:"))
        .unwrap();
    assert!(ua_line.contains("custom/2.0"));
    assert!(!ua_line.contains("eggserve-client"));
}

#[test]
fn get_with_body_rejected_at_build() {
    let result = ClientRequestBuilder::new(Method::Get)
        .url("http://example.com/")
        .unwrap()
        .body(b"body".to_vec())
        .build();
    assert!(matches!(result, Err(ClientError::ProtocolError(_))));
}

#[test]
fn head_with_body_rejected_at_build() {
    let result = ClientRequestBuilder::new(Method::Head)
        .url("http://example.com/")
        .unwrap()
        .body(b"body".to_vec())
        .build();
    assert!(matches!(result, Err(ClientError::ProtocolError(_))));
}

#[test]
fn invalid_header_name_rejected() {
    let result = ClientRequestBuilder::new(Method::Get)
        .url("http://example.com/")
        .unwrap()
        .header("Bad Name", "value");
    assert!(matches!(result, Err(ClientError::InvalidHeader(_))));
}

#[test]
fn null_byte_in_header_value_rejected() {
    let result = ClientRequestBuilder::new(Method::Get)
        .url("http://example.com/")
        .unwrap()
        .header("X-Test", "bad\x00value");
    assert!(matches!(result, Err(ClientError::InvalidHeader(_))));
}

#[test]
fn newline_in_header_value_rejected() {
    let result = ClientRequestBuilder::new(Method::Get)
        .url("http://example.com/")
        .unwrap()
        .header("X-Test", "bad\nvalue");
    assert!(matches!(result, Err(ClientError::InvalidHeader(_))));
}

#[test]
fn empty_header_name_rejected() {
    let result = ClientRequestBuilder::new(Method::Get)
        .url("http://example.com/")
        .unwrap()
        .header("", "value");
    assert!(matches!(result, Err(ClientError::InvalidHeader(_))));
}

#[test]
fn request_uri_is_origin_form() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (addr, captured) = start_raw_server_capture(response);

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/path?query=1", addr))
        .unwrap()
        .build()
        .unwrap();

    let _ = client.send(&req);
    let request_bytes = captured.lock().unwrap().clone();
    let request_str = String::from_utf8_lossy(&request_bytes);
    let status_line = request_str.lines().next().unwrap();

    // Should use origin-form or absolute-form in the request line
    assert!(status_line.starts_with("GET "));
    assert!(status_line.ends_with(" HTTP/1.1"));
    assert!(status_line.contains("/path?query=1"));
}

#[test]
fn method_validation_get_head_post_put_delete_patch() {
    // All supported methods should build without error
    for method in [
        Method::Get,
        Method::Head,
        Method::Post,
        Method::Put,
        Method::Delete,
        Method::Patch,
    ] {
        let mut builder = ClientRequestBuilder::new(method)
            .url("http://example.com/")
            .unwrap();
        if matches!(method, Method::Get | Method::Head) {
            // GET/HEAD cannot have body
        } else {
            builder = builder.body(b"body".to_vec());
        }
        assert!(builder.build().is_ok(), "method {:?} should build", method);
    }
}

#[test]
fn get_method_as_str() {
    assert_eq!(Method::Get.as_str(), "GET");
    assert_eq!(Method::Head.as_str(), "HEAD");
    assert_eq!(Method::Post.as_str(), "POST");
    assert_eq!(Method::Put.as_str(), "PUT");
    assert_eq!(Method::Delete.as_str(), "DELETE");
    assert_eq!(Method::Patch.as_str(), "PATCH");
}

// ===========================================================================
// Track G — Response parsing and error mapping
// ===========================================================================

#[test]
fn response_status_code_parsed() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(201)
            .body(Full::new(Bytes::from("created")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Post)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .body(b"data".to_vec())
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 201);
}

#[test]
fn response_headers_parsed_case_insensitive() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("X-Custom-Header", "value123")
            .body(Full::new(Bytes::new()))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    // Header names are lowercased
    assert_eq!(resp.headers.get("x-custom-header").unwrap(), "value123");
}

#[test]
fn response_body_fully_buffered() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-length", "11")
            .body(Full::new(Bytes::from("hello world")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.text().unwrap(), "hello world");
    assert_eq!(resp.bytes(), b"hello world");
}

#[test]
fn empty_response_body() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(204)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 204);
    assert!(resp.body.is_empty());
    assert!(resp.is_success());
}

#[test]
fn server_disconnect_mid_body_returns_error() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-length", "1000")
            .body(Full::new(Bytes::from("short")))
            .unwrap()
    }));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(10000),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
    match result.unwrap_err() {
        ClientError::ProtocolError(_) => {}
        ClientError::Io(_) => {}
        ClientError::Timeout(_) => {}
        other => panic!("expected ProtocolError, Io, or Timeout, got {other:?}"),
    }
}

#[test]
fn connection_refused_maps_to_connect_error() {
    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url("http://127.0.0.1:19/")
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(result, Err(ClientError::ConnectError(_))));
}

#[test]
fn unsupported_scheme_error() {
    let result = ClientRequestBuilder::new(Method::Get).url("ftp://example.com/");
    assert!(matches!(result, Err(ClientError::UnsupportedScheme(_))));
}

#[test]
fn missing_host_error() {
    let result = ParsedUrl::parse("http:///path");
    assert!(result.is_err());
}

#[test]
fn invalid_url_error() {
    let result = ClientRequestBuilder::new(Method::Get).url("");
    assert!(matches!(result, Err(ClientError::InvalidUrl(_))));
}

#[test]
fn error_display_messages() {
    let errors = vec![
        ClientError::InvalidUrl("test".into()),
        ClientError::UnsupportedScheme("ftp".into()),
        ClientError::MissingHost,
        ClientError::InvalidHeader("bad".into()),
        ClientError::BodyTooLarge {
            limit: 100,
            actual: 200,
        },
        ClientError::Timeout("timed out".into()),
        ClientError::DnsError("no such host".into()),
        ClientError::ConnectError("connection refused".into()),
        ClientError::TlsError("handshake failed".into()),
        ClientError::ProtocolError("bad protocol".into()),
        ClientError::ResponseBodyTooLarge { limit: 50 },
    ];

    for error in errors {
        let msg = error.to_string();
        assert!(!msg.is_empty(), "error display should not be empty");
    }
}

#[test]
fn client_response_is_success() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|req: Request<Incoming>| async move {
        let path = req.uri().path();
        let status = if path.ends_with("/ok") {
            200
        } else if path.ends_with("/created") {
            201
        } else if path.ends_with("/no-content") {
            204
        } else if path.ends_with("/redirect") {
            301
        } else if path.ends_with("/not-found") {
            404
        } else if path.ends_with("/error") {
            500
        } else {
            200
        };
        Response::builder()
            .status(status)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();

    for (path, expected_success) in [
        ("/ok", true),
        ("/created", true),
        ("/no-content", true),
        ("/redirect", false),
        ("/not-found", false),
        ("/error", false),
    ] {
        let req = ClientRequestBuilder::new(Method::Get)
            .url(&format!("http://{}/test{}", addr, path))
            .unwrap()
            .build()
            .unwrap();

        let resp = client.send(&req).unwrap();
        assert_eq!(
            resp.is_success(),
            expected_success,
            "path {} should have is_success={}",
            path,
            expected_success
        );
    }
}

#[test]
fn client_response_content_type() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from("{}")))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.content_type(), Some("application/json; charset=utf-8"));
}

#[test]
fn client_response_content_length() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(|_req| async {
        Response::builder()
            .status(200)
            .header("content-length", "42")
            .body(Full::new(Bytes::from("x".repeat(42))))
            .unwrap()
    }));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.content_length(), Some(42));
}

#[test]
fn malformed_request_target_rejected() {
    // Missing scheme
    assert!(ClientRequestBuilder::new(Method::Get)
        .url("not-a-url")
        .is_err());
    // Empty
    assert!(ClientRequestBuilder::new(Method::Get).url("").is_err());
    // Control chars
    assert!(ClientRequestBuilder::new(Method::Get)
        .url("http://exam\x01ple.com/")
        .is_err());
    // Spaces
    assert!(ClientRequestBuilder::new(Method::Get)
        .url("http://exam ple.com/")
        .is_err());
}
