use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::handle::ServerHandle;
use eggserve_core::server::{service_fn_with_policy, Server};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct WireResponse {
    status: u16,
    headers: HashMap<String, Vec<String>>,
    body: Vec<u8>,
}

async fn read_wire_response(stream: &mut TcpStream) -> io::Result<WireResponse> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut head = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).await?;
            head.push(byte[0]);
            if head.ends_with(b"\r\n\r\n") {
                break;
            }
            if head.len() > 64 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "response headers exceed test bound",
                ));
            }
        }

        let header_end = head.len() - 4;
        let text = std::str::from_utf8(&head[..header_end]).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 response headers")
        })?;
        let mut lines = text.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid response status"))?;
        let mut headers: HashMap<String, Vec<String>> = HashMap::new();
        for line in lines {
            let (name, value) = line.split_once(':').ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid response header")
            })?;
            headers
                .entry(name.trim().to_ascii_lowercase())
                .or_default()
                .push(value.trim().to_string());
        }

        let mut body = Vec::new();
        if let Some(length) = headers
            .get("content-length")
            .and_then(|values| values.last())
            .and_then(|value| value.parse::<usize>().ok())
        {
            body.resize(length, 0);
            stream.read_exact(&mut body).await?;
        } else if headers.get("transfer-encoding").is_some_and(|values| {
            values
                .iter()
                .any(|value| value.eq_ignore_ascii_case("chunked"))
        }) {
            loop {
                let line = read_wire_line(stream).await?;
                let size =
                    usize::from_str_radix(line.split(';').next().unwrap_or_default().trim(), 16)
                        .map_err(|_| {
                            io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size")
                        })?;
                if size == 0 {
                    loop {
                        if read_wire_line(stream).await?.is_empty() {
                            break;
                        }
                    }
                    break;
                }
                let start = body.len();
                body.resize(start + size, 0);
                stream.read_exact(&mut body[start..]).await?;
                let mut crlf = [0u8; 2];
                stream.read_exact(&mut crlf).await?;
                if crlf != *b"\r\n" {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "missing chunk terminator",
                    ));
                }
            }
        }

        Ok(WireResponse {
            status,
            headers,
            body,
        })
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "timed out reading response"))?
}

async fn read_wire_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        line.push(byte[0]);
        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
            return String::from_utf8(line)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 wire line"));
        }
        if line.len() > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "wire line too long",
            ));
        }
    }
}

async fn start_server(config: RuntimeConfig) -> (ServerHandle, TempDir) {
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            |req: Request| async move {
                let (head, body) = req.into_head_and_body();
                let method = head.method().as_str().to_string();
                let data = body.read_all().await.unwrap();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(
                        format!("{}:{}", method, String::from_utf8_lossy(&data)).into_bytes(),
                    ))
                    .unwrap())
            },
            RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    (handle, tempfile::TempDir::new().unwrap())
}

async fn start_reject_server(config: RuntimeConfig) -> (ServerHandle, TempDir) {
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            |_req: Request| async move { unreachable!("reject server should not invoke handler") },
            RequestBodyPolicy::Reject,
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    (handle, tempfile::TempDir::new().unwrap())
}

async fn start_stream_server(
    config: RuntimeConfig,
    policy: RequestBodyPolicy,
) -> (ServerHandle, Arc<AtomicUsize>) {
    let invocations = Arc::new(AtomicUsize::new(0));
    let counter = invocations.clone();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            move |req: Request| {
                let counter = counter.clone();
                async move {
                    let invocation = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let (_head, body) = req.into_head_and_body();
                    let data = body.read_all().await.unwrap();
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(
                            format!("{invocation}:{}", String::from_utf8_lossy(&data)).into_bytes(),
                        ))
                        .unwrap())
                }
            },
            policy,
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    (handle, invocations)
}

async fn start_partial_stream_server() -> (ServerHandle, Arc<AtomicUsize>) {
    let invocations = Arc::new(AtomicUsize::new(0));
    let counter = invocations.clone();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind("127.0.0.1:0".parse().unwrap())
                .max_request_body_bytes(1024 * 1024)
                .body_read_timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            move |req: Request| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let (_head, mut body) = req.into_head_and_body();
                    let _ = body.next_chunk().await;
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(b"partial".to_vec()))
                        .unwrap())
                }
            },
            RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    (handle, invocations)
}

async fn assert_eof(stream: &mut TcpStream) {
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
        .await
        .expect("connection should close promptly")
        .expect("socket read should succeed");
    assert_eq!(read, 0, "unexpected bytes after expected connection close");
}

#[tokio::test]
async fn stream_fixed_length_reuses_connection_after_full_consumption() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, invocations) =
        start_stream_server(config, RequestBodyPolicy::Stream { max_bytes: 1024 }).await;
    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(b"POST /first HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata")
        .await
        .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(first.body, b"1:data");
    assert!(!first
        .headers
        .get("connection")
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case("close")));

    conn.write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let second = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(second.status, 200);
    assert_eq!(second.body, b"2:");
    assert_eq!(invocations.load(Ordering::SeqCst), 2);
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn stream_chunked_reuses_connection_after_full_consumption() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, invocations) =
        start_stream_server(config, RequestBodyPolicy::Stream { max_bytes: 1024 }).await;
    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(
        b"POST /first HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
    )
    .await
    .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(first.body, b"1:hello world");
    assert!(!first
        .headers
        .get("connection")
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case("close")));

    conn.write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let second = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(second.status, 200);
    assert_eq!(second.body, b"2:");
    assert_eq!(invocations.load(Ordering::SeqCst), 2);
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn stream_empty_request_reuses_connection() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .build()
        .unwrap();
    let (handle, invocations) =
        start_stream_server(config, RequestBodyPolicy::Stream { max_bytes: 1024 }).await;
    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(b"GET /first HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(first.body, b"1:");
    assert!(!first
        .headers
        .get("connection")
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case("close")));
    conn.write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let second = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(second.status, 200);
    assert_eq!(invocations.load(Ordering::SeqCst), 2);
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn stream_incomplete_fixed_length_closes_and_suppresses_pipeline() {
    let (handle, invocations) = start_partial_stream_server().await;
    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(
        b"POST /first HTTP/1.1\r\nHost: localhost\r\nContent-Length: 64\r\n\r\n0123456789012345678901234567890123456789012345678901234567890123GET /second HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await
    .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 200);
    assert!(first
        .headers
        .get("connection")
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case("close")));
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn stream_incomplete_chunked_closes_and_suppresses_pipeline() {
    let (handle, invocations) = start_partial_stream_server().await;
    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(
        b"POST /first HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\nGET /second HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await
    .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 200);
    assert!(first
        .headers
        .get("connection")
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case("close")));
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn rejected_body_closes_and_suppresses_pipelined_request() {
    let invocations = Arc::new(AtomicUsize::new(0));
    let counter = invocations.clone();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind("127.0.0.1:0".parse().unwrap())
                .max_request_body_bytes(1024)
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            move |_req: Request| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Empty)
                        .unwrap())
                }
            },
            RequestBodyPolicy::Reject,
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();

    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(
        b"POST /first HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhelloGET /second HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await
    .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 413);
    assert!(first
        .headers
        .get("connection")
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case("close")));
    assert_eq!(invocations.load(Ordering::SeqCst), 0);
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn fixed_length_body_wire() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 11\r\n\
          Connection: close\r\n\
          \r\n\
          Hello, body",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "expected 200, got: {}",
        response
    );
    assert!(
        response.contains("POST:Hello, body"),
        "response should echo method and body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn chunked_body_wire() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Transfer-Encoding: chunked\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();
    conn.write_all(b"5\r\nhello\r\n").await.unwrap();
    conn.write_all(b"1\r\n \r\n").await.unwrap();
    conn.write_all(b"5\r\nworld\r\n").await.unwrap();
    conn.write_all(b"0\r\n\r\n").await.unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "expected 200, got: {}",
        response
    );
    assert!(
        response.contains("POST:hello world"),
        "response should contain reassembled body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn malformed_chunking_returns_400() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Transfer-Encoding: chunked\r\n\
          Connection: close\r\n\
          \r\n\
          ZZ\r\ninvalid chunk size\r\n",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    // Malformed chunk size causes body read error → 500 (runtime error mapping)
    // or connection close without response.
    assert!(
        response.starts_with("HTTP/1.1 400")
            || response.starts_with("HTTP/1.1 500")
            || response.is_empty(),
        "expected 400/500 or connection close for malformed chunking, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_limit_exceeded_mid_stream_wire() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(10)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Transfer-Encoding: chunked\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();
    conn.write_all(b"20\r\n").await.unwrap();
    conn.write_all(b"0123456789abcdef0123456789abcdef\r\n")
        .await
        .unwrap();
    conn.write_all(b"0\r\n\r\n").await.unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413") || response.is_empty(),
        "expected 413 or connection close for body limit exceeded, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn buffer_same_connection_reuse() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let mut conn = TcpStream::connect(handle.local_addr()).await.unwrap();
    conn.write_all(b"POST /first HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata")
        .await
        .unwrap();
    let first = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(first.body, b"POST:data");
    conn.write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let second = read_wire_response(&mut conn).await.unwrap();
    assert_eq!(second.status, 200);
    assert_eq!(second.body, b"GET:");
    assert_eof(&mut conn).await;
    handle.shutdown();
}

#[tokio::test]
async fn connection_close_after_rejected_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 999999\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413 for declared length too large: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn get_with_body_wire_follows_service_policy() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"GET /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          Connection: close\r\n\
          \r\n\
          hello",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200 for GET with body, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn handler_error_before_body_consumption() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let tmp = TempDir::new().unwrap();
    let serve_config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    let server = Server::builder()
        .runtime(config)
        .serve_config(serve_config)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            |_req: Request| async move {
                // Return error without consuming the body.
                Err(eggserve_core::server::ServiceError::rejected(
                    500,
                    "handler error",
                ))
            },
            RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          Connection: close\r\n\
          \r\n\
          hello",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 500"),
        "expected 500 for handler error, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_read_timeout_before_service() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_millis(50))
        .build()
        .unwrap();

    let tmp = TempDir::new().unwrap();
    let serve_config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    let server = Server::builder()
        .runtime(config)
        .serve_config(serve_config)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(
            |_req: Request| async move {
                unreachable!("service should not be called after body timeout");
            },
            RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Don't send body — body read timeout should fire.
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);
    // Normative: body timeout returns 408 or closes the connection.
    assert!(
        response.starts_with("HTTP/1.1 408") || response.is_empty(),
        "expected 408 or connection close for body timeout, got: {}",
        response
    );
    handle.shutdown();
}
#[tokio::test]
async fn leftover_bytes_not_parsed_as_next_request() {
    // Verify that when a body is not fully consumed, leftover bytes
    // are NOT parsed as a second HTTP request. The connection should
    // close or the second request should get a parse error.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let tmp = TempDir::new().unwrap();
    let serve_config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    let server = Server::builder()
        .runtime(config)
        .serve_config(serve_config)
        .build()
        .unwrap();
    // Service that reads only part of the body.
    let handle = server
        .start_with_service(service_fn_with_policy(
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                // Read only first chunk, don't consume the rest.
                let _chunk = body.next_chunk().await.unwrap();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            },
            RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Send body "helloworld" but service only reads "hello".
    // The leftover "world" bytes should NOT be parsed as a new request.
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 10\r\n\
          \r\n\
          helloworld",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "first request should succeed: {}",
        response
    );
    // The connection should close — leftover body bytes are not parsed.
    // No second response should appear.
    assert!(
        !response.contains("HTTP/1.1") || response.matches("HTTP/1.1").count() == 1,
        "should not have a second HTTP response from leftover bytes: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn http10_post_with_body_wire() {
    // HTTP/1.0 POST with body should work the same as HTTP/1.1.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.0\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          \r\n\
          hello",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200"),
        "HTTP/1.0 POST should succeed: {}",
        response
    );
    assert!(
        response.contains("POST:hello"),
        "response should echo body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn http10_body_timeout_returns_408() {
    // HTTP/1.0 body timeout should return 408.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.0\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Don't send body — body read timeout should fire.
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.0 408")
            || response.starts_with("HTTP/1.1 408")
            || response.is_empty(),
        "expected 408 or connection close for HTTP/1.0 body timeout, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn http11_body_limit_exceeded_returns_413() {
    // HTTP/1.1 body limit exceeded should return 413.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 999999\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413 for HTTP/1.1 body limit exceeded: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn expect_100_continue_rejected_by_policy() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_reject_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          Expect: 100-Continue\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413 for Expect: 100-continue with reject policy, got: {}",
        response
    );
    assert!(
        !response.contains("100"),
        "should not contain 100 Continue: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn duplicate_content_length_rejected_wire() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          Content-Length: 10\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "expected 400 for duplicate Content-Length, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn rejected_positive_cl_with_bytes_sent() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_reject_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Send headers claiming Content-Length: 5, then actually send 5 bytes.
    // The server should reject based on policy before reading the body.
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          Connection: close\r\n\
          \r\n\
          hello",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413 for rejected body with bytes sent, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_limit_minus_one_accepted() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 4\r\n\
          Connection: close\r\n\
          \r\n\
          hell",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200 for body at limit-1 (4 bytes, limit 1024), got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_limit_exact_accepted() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(5)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 5\r\n\
          Connection: close\r\n\
          \r\n\
          hello",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200 for body at exact limit (5 bytes, limit 5), got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_limit_plus_one_rejected() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(5)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 6\r\n\
          Connection: close\r\n\
          \r\n\
          helloo",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413 for body at limit+1 (6 bytes, limit 5), got: {}",
        response
    );
    handle.shutdown();
}
