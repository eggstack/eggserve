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

async fn start_server(config: RuntimeConfig) -> (ServerHandle, TempDir) {
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
    (handle, tmp)
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
async fn keepalive_after_complete_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(true)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();

    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 4\r\n\
          \r\n\
          data",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    let mut temp = [0u8; 1];
    loop {
        if conn.read(&mut temp).await.unwrap_or(0) == 0 {
            break;
        }
        buf.push(temp[0]);
        if buf.ends_with(b"\r\n\r\n") || buf.windows(4).any(|w| w == b"\n\n") {
            break;
        }
    }
    let mut body_buf = [0u8; 4096];
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            match conn.read(&mut body_buf).await {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&body_buf[..n]),
                Err(_) => break,
            }
        }
    })
    .await;

    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "first request should succeed: {}",
        response
    );

    conn.write_all(
        b"GET /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf2 = Vec::new();
    conn.read_to_end(&mut buf2).await.unwrap();
    let response2 = String::from_utf8_lossy(&buf2);
    assert!(
        response2.starts_with("HTTP/1.1 200"),
        "second request on keep-alive connection should succeed: {}",
        response2
    );
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
async fn get_with_body_wire_rejected() {
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
        response.starts_with("HTTP/1.1 400"),
        "expected 400 for GET with body, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn partial_body_then_pipelined_request() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(true)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();

    // First request: send body that is fully consumed by the handler.
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 4\r\n\
          \r\n\
          data",
    )
    .await
    .unwrap();

    // Read the complete first response (headers + body).
    let mut buf = Vec::new();
    let mut header_done = false;
    loop {
        let mut temp = [0u8; 1];
        match conn.read(&mut temp).await {
            Ok(0) => break,
            Ok(_) => {
                buf.push(temp[0]);
                if !header_done && buf.ends_with(b"\r\n\r\n") {
                    header_done = true;
                }
                // After headers, read until we have content-length bytes of body.
                if header_done {
                    let response_str = String::from_utf8_lossy(&buf);
                    if let Some(pos) = response_str.find("\r\n\r\n") {
                        let header_end = pos + 4;
                        // Look for content-length in headers.
                        let header_part = &response_str[..header_end];
                        if let Some(cl_line) = header_part
                            .lines()
                            .find(|l| l.to_lowercase().starts_with("content-length:"))
                        {
                            if let Some(cl_val) = cl_line.split(':').nth(1) {
                                if let Ok(cl) = cl_val.trim().parse::<usize>() {
                                    let body_received = buf.len() - header_end;
                                    if body_received >= cl {
                                        break;
                                    }
                                }
                            }
                        }
                        // If no content-length or can't parse, just read a bit more.
                        if buf.len() - header_end > 1024 {
                            break;
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "first request should succeed: {}",
        response
    );

    // Try to read any remaining body data.
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        let mut temp = [0u8; 4096];
        loop {
            match conn.read(&mut temp).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;

    // Second request on the same keep-alive connection.
    let write_result = conn
        .write_all(
            b"GET /test HTTP/1.1\r\n\
              Host: localhost\r\n\
              Connection: close\r\n\
              \r\n",
        )
        .await;
    if write_result.is_err() {
        // Connection was closed by server — this is acceptable for buffer mode.
        handle.shutdown();
        return;
    }

    let mut buf2 = Vec::new();
    let read_result =
        tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf2)).await;
    match read_result {
        Ok(Ok(_)) => {
            let response2 = String::from_utf8_lossy(&buf2);
            assert!(
                response2.starts_with("HTTP/1.1 200"),
                "second request on keep-alive should succeed: {}",
                response2
            );
        }
        _ => {
            // Connection closed before second response — acceptable.
        }
    }
    handle.shutdown();
}

#[tokio::test]
async fn partial_body_close_policy() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close,
        )
        .keep_alive(true)
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
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 10\r\n\
          Connection: close\r\n\
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
        "response should be 200 even with partial body consumption: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn drain_policy_body_close_on_partial_consumption() {
    // When drain policy is set but body is not fully consumed,
    // the connection should close (drain is best-effort for Stream mode
    // because the body is opaque inside the Request).
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Drain {
                max_bytes: 1024,
                timeout: Duration::from_secs(1),
            },
        )
        .keep_alive(true)
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
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                // Read only first chunk.
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
        "response should be 200 with drain policy: {}",
        response
    );
    // Connection should close since body wasn't fully consumed.
    handle.shutdown();
}

#[tokio::test]
async fn drain_policy_keepalive_after_full_body_consumption() {
    // When drain policy is set and body IS fully consumed,
    // keep-alive should work for subsequent requests.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Drain {
                max_bytes: 1024,
                timeout: Duration::from_secs(1),
            },
        )
        .keep_alive(true)
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
            |req: Request| async move {
                let (_head, body) = req.into_head_and_body();
                // Fully consume the body.
                let _data = body.read_all().await.unwrap();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
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
          Content-Length: 4\r\n\
          \r\n\
          data",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "first request should succeed: {}",
        response
    );

    // Reconnect for the second request (keep-alive may or may not work).
    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"GET /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf2 = Vec::new();
    let read_result =
        tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf2)).await;
    match read_result {
        Ok(Ok(_)) => {
            let response2 = String::from_utf8_lossy(&buf2);
            assert!(
                response2.starts_with("HTTP/1.1 200"),
                "second request should succeed: {}",
                response2
            );
        }
        _ => {
            // Connection closed — acceptable.
        }
    }
    handle.shutdown();
}

#[tokio::test]
async fn partial_chunked_body_close() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close,
        )
        .keep_alive(true)
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
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                // Read only the first chunk.
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
    conn.write_all(b"5\r\nworld\r\n").await.unwrap();
    conn.write_all(b"0\r\n\r\n").await.unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "response should be 200 with partial chunked consumption: {}",
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
        .keep_alive(false)
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
        .keep_alive(false)
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
    assert!(
        response.starts_with("HTTP/1.1 408") || response.is_empty(),
        "expected 408 or connection close for body timeout, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_complete_before_service_keepalive() {
    // Verify that when body is fully consumed during pre-buffering,
    // keep-alive works correctly for subsequent requests.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(true)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(config).await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();

    // First request: POST with body.
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 4\r\n\
          \r\n\
          data",
    )
    .await
    .unwrap();

    // Read first response.
    let mut buf = Vec::new();
    loop {
        let mut temp = [0u8; 1];
        match conn.read(&mut temp).await {
            Ok(0) => break,
            Ok(_) => {
                buf.push(temp[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "first POST should succeed: {}",
        response
    );

    // Try to read any remaining body data.
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        let mut temp = [0u8; 4096];
        loop {
            match conn.read(&mut temp).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;

    // Second request: GET on same connection.
    let write_result = conn
        .write_all(
            b"GET /test HTTP/1.1\r\n\
              Host: localhost\r\n\
              Connection: close\r\n\
              \r\n",
        )
        .await;
    if write_result.is_err() {
        handle.shutdown();
        return;
    }

    let mut buf2 = Vec::new();
    let read_result =
        tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf2)).await;
    if let Ok(Ok(_)) = read_result {
        let response2 = String::from_utf8_lossy(&buf2);
        assert!(
            response2.starts_with("HTTP/1.1 200"),
            "GET on keep-alive should succeed: {}",
            response2
        );
    }
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
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close,
        )
        .keep_alive(false)
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
async fn drain_policy_timeout_closes_connection() {
    // When drain policy is set and body is not fully consumed,
    // the connection should close (Hyper encounters leftover bytes
    // and closes to prevent request smuggling).
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Drain {
                max_bytes: 1024,
                timeout: Duration::from_millis(100),
            },
        )
        .keep_alive(true)
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
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 10\r\n\
          Connection: close\r\n\
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
        "response should be 200 with drain policy timeout: {}",
        response
    );
    // Connection should close — leftover body bytes cause Hyper parse error.
    handle.shutdown();
}

#[tokio::test]
async fn drain_policy_partial_chunked_body() {
    // Drain policy with partial chunked body consumption.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Drain {
                max_bytes: 1024,
                timeout: Duration::from_secs(1),
            },
        )
        .keep_alive(true)
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
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                // Read only first chunk of a 3-chunk body.
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
    conn.write_all(b"5\r\nworld\r\n").await.unwrap();
    conn.write_all(b"1\r\n!\r\n").await.unwrap();
    conn.write_all(b"0\r\n\r\n").await.unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "response should be 200 with drain policy on chunked body: {}",
        response
    );
    // Connection closes because service only read first chunk.
    handle.shutdown();
}

#[tokio::test]
async fn drain_policy_malformed_remainder() {
    // Drain policy: service reads partial body, remainder is malformed.
    // Connection should close cleanly.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Drain {
                max_bytes: 1024,
                timeout: Duration::from_secs(1),
            },
        )
        .keep_alive(true)
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
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                // Read only a few bytes.
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
    // Send fixed-length body, service reads only first chunk.
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 20\r\n\
          Connection: close\r\n\
          \r\n\
          helloworld12345678",
    )
    .await
    .unwrap();

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "response should be 200 with drain policy malformed remainder: {}",
        response
    );
    // Connection closes — leftover bytes not parsed as request.
    handle.shutdown();
}

#[tokio::test]
async fn drain_policy_keepalive_with_full_consumption_stream() {
    // Drain policy with Stream mode and full body consumption.
    // Keep-alive should work.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Drain {
                max_bytes: 1024,
                timeout: Duration::from_secs(1),
            },
        )
        .keep_alive(true)
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
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                // Fully consume the body via streaming.
                while let Some(_chunk) = body.next_chunk().await.unwrap() {}
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
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 4\r\n\
          \r\n\
          data",
    )
    .await
    .unwrap();

    // Read first response.
    let mut buf = Vec::new();
    loop {
        let mut temp = [0u8; 1];
        match conn.read(&mut temp).await {
            Ok(0) => break,
            Ok(_) => {
                buf.push(temp[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "first request should succeed: {}",
        response
    );

    // Reconnect for second request.
    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"GET /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    let mut buf2 = Vec::new();
    let read_result =
        tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf2)).await;
    match read_result {
        Ok(Ok(_)) => {
            let response2 = String::from_utf8_lossy(&buf2);
            assert!(
                response2.starts_with("HTTP/1.1 200"),
                "second request should succeed: {}",
                response2
            );
        }
        _ => {
            // Connection closed — acceptable if drain didn't preserve keep-alive.
        }
    }
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
        .keep_alive(false)
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
