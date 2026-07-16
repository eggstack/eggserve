use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::{service_fn_with_policy, Server};
use serde::Deserialize;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Corpus deserialization
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Corpus {
    groups: HashMap<String, Group>,
}

#[derive(Deserialize)]
struct Group {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize, Clone)]
struct Fixture {
    id: String,
    description: String,
    input: FixtureInput,
    expected: FixtureExpected,
}

#[derive(Deserialize, Clone)]
struct FixtureInput {
    method: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    body_partial: Option<String>,
    #[serde(default)]
    body_hex: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    encoding: Option<String>,
    #[serde(default)]
    chunk_size: Option<usize>,
    policy: String,
    max_body_bytes: u64,
    #[serde(default)]
    handler_action: Option<String>,
}

#[derive(Deserialize, Clone)]
struct FixtureExpected {
    status: u16,
    #[serde(default)]
    handler_called: Option<bool>,
    #[serde(default)]
    echo_body: Option<String>,
    #[serde(default)]
    echo_len: Option<usize>,
    #[serde(default)]
    has_body: Option<bool>,
    #[serde(default)]
    body_data: Option<String>,
    #[serde(default)]
    handler_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_body_corpus() -> Corpus {
    let json = include_str!("../../../conformance/body_corpus.json");
    serde_json::from_str(json).expect("body corpus must be valid JSON")
}

fn group(name: &str) -> Vec<Fixture> {
    load_body_corpus()
        .groups
        .get(name)
        .unwrap_or_else(|| panic!("group '{name}' not found in body corpus"))
        .fixtures
        .clone()
}

fn parse_policy(input: &FixtureInput) -> RequestBodyPolicy {
    match input.policy.as_str() {
        "reject" => RequestBodyPolicy::Reject,
        "buffer" => RequestBodyPolicy::Buffer {
            max_bytes: input.max_body_bytes,
        },
        "stream" => RequestBodyPolicy::Stream {
            max_bytes: input.max_body_bytes,
        },
        _ => panic!("unknown policy: {}", input.policy),
    }
}

fn build_request_bytes(fixture: &FixtureInput, addr: &str) -> Vec<u8> {
    let body_bytes = if let Some(ref hex) = fixture.body_hex {
        decode_hex(hex)
    } else if let Some(ref body) = fixture.body {
        body.as_bytes().to_vec()
    } else if let Some(ref partial) = fixture.body_partial {
        partial.as_bytes().to_vec()
    } else {
        Vec::new()
    };

    let mut headers: Vec<String> = fixture
        .headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();

    // Ensure Host header
    if !headers.iter().any(|h| h.to_lowercase().starts_with("host:")) {
        headers.push(format!("Host: {addr}"));
    }

    // For chunked encoding, use Transfer-Encoding instead of Content-Length
    if fixture.encoding.as_deref() == Some("chunked") {
        if !headers
            .iter()
            .any(|h| h.to_lowercase().starts_with("transfer-encoding:"))
        {
            headers.push("Transfer-Encoding: chunked".to_string());
        }
    } else if !headers
        .iter()
        .any(|h| h.to_lowercase().starts_with("content-length:"))
    {
        // Only add Content-Length if not already provided and not empty body
        // For body_partial (premature EOF), use the declared length from headers
        if !fixture.headers.contains_key("Content-Length") && !body_bytes.is_empty() {
            headers.push(format!("Content-Length: {}", body_bytes.len()));
        }
    }

    headers.push("Connection: close".to_string());

    let header_str = headers.join("\r\n");
    let mut req = format!(
        "{} /test HTTP/1.1\r\n{}\r\n\r\n",
        fixture.method, header_str
    )
    .into_bytes();

    if fixture.encoding.as_deref() == Some("chunked") {
        // Build chunked body
        let chunk_size = fixture.chunk_size.unwrap_or(body_bytes.len());
        for chunk in body_bytes.chunks(chunk_size) {
            req.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            req.extend_from_slice(chunk);
            req.extend_from_slice(b"\r\n");
        }
        req.extend_from_slice(b"0\r\n\r\n");
    } else {
        req.extend_from_slice(&body_bytes);
    }

    req
}

fn parse_response_status(data: &[u8]) -> Option<u16> {
    let header_end = data.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_str = std::str::from_utf8(&data[..header_end]).ok()?;
    let status_line = header_str.lines().next()?;
    let status_part = status_line.split_whitespace().nth(1)?;
    status_part.parse().ok()
}

fn decode_hex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "hex string must have even length");
    s.as_bytes()
        .chunks(2)
        .map(|pair| {
            let hi = pair[0] as char;
            let lo = pair[1] as char;
            fn hex_val(c: char) -> u8 {
                match c {
                    '0'..='9' => c as u8 - b'0',
                    'a'..='f' => c as u8 - b'a' + 10,
                    'A'..='F' => c as u8 - b'A' + 10,
                    _ => panic!("invalid hex char: {c}"),
                }
            }
            (hex_val(hi) << 4) | hex_val(lo)
        })
        .collect()
}

fn parse_response_body(data: &[u8]) -> Vec<u8> {
    let header_end = match data.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => pos + 4,
        None => return Vec::new(),
    };
    let body = &data[header_end..];

    // Check for chunked transfer encoding
    let header_str = std::str::from_utf8(&data[..header_end]).unwrap_or("");
    if header_str
        .to_lowercase()
        .contains("transfer-encoding: chunked")
    {
        // Decode chunks
        let mut result = Vec::new();
        let mut pos = 0;
        while pos < body.len() {
            // Find end of chunk size line
            let size_end = match body[pos..].windows(2).position(|w| w == b"\r\n") {
                Some(p) => pos + p,
                None => break,
            };
            let size_str = std::str::from_utf8(&body[pos..size_end]).unwrap_or("0");
            let chunk_size = usize::from_str_radix(size_str.trim(), 16).unwrap_or(0);
            if chunk_size == 0 {
                break;
            }
            let data_start = size_end + 2;
            let data_end = data_start + chunk_size;
            if data_end <= body.len() {
                result.extend_from_slice(&body[data_start..data_end]);
            }
            pos = data_end + 2; // skip \r\n after chunk data
        }
        result
    } else {
        body.to_vec()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn body_conformance_policy_selection() {
    for fixture in group("body_policy_selection") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        if fixture.input.policy == "static" {
            // Static service test
            let tmp = TempDir::new().unwrap();
            std::fs::write(tmp.path().join("test.txt"), "ok").unwrap();
            let serve_config = Arc::new(ServeConfig {
                root: tmp.path().to_path_buf(),
                ..ServeConfig::default()
            });
            let server = Server::builder()
                .runtime(config)
                .serve_config(serve_config)
                .build()
                .unwrap();
            let handle = server.start().await.unwrap();
            let addr = handle.local_addr();

            let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
            let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
            conn.write_all(&req_bytes).await.unwrap();
            let mut buf = Vec::new();
            conn.read_to_end(&mut buf).await.unwrap();
            let status = parse_response_status(&buf).unwrap_or(0);
            assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);
            handle.shutdown();
        } else {
            let policy = parse_policy(&fixture.input);
            let handler_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let handler_called_clone = handler_called.clone();
            let handler_action = fixture.input.handler_action.clone();

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
                    move |req: Request| {
                        let handler_called = handler_called_clone.clone();
                        let handler_action = handler_action.clone();
                        async move {
                            handler_called.store(
                                true,
                                std::sync::atomic::Ordering::Relaxed,
                            );

                            let (head, mut body) = req.into_head_and_body();
                            let method = head.method().as_str().to_string();

                            // Handle special handler actions
                            if let Some(ref action) = handler_action {
                                use futures_util::StreamExt;
                                match action.as_str() {
                                    "double_read" => {
                                        let _ = body.next_chunk().await;
                                        let second = body.next_chunk().await;
                                        if second.is_err() || second.unwrap().is_some() {
                                            return Ok(Response::builder()
                                                .status(StatusCode::OK)
                                                .body(ResponseBody::Bytes(
                                                    b"consumed".to_vec(),
                                                ))
                                                .unwrap());
                                        }
                                    }
                                    "read_then_iter" => {
                                        let _ = body.next_chunk().await;
                                        let second = body.next_chunk().await;
                                        if second.is_err() || second.unwrap().is_some() {
                                            return Ok(Response::builder()
                                                .status(StatusCode::OK)
                                                .body(ResponseBody::Bytes(
                                                    b"consumed".to_vec(),
                                                ))
                                                .unwrap());
                                        }
                                    }
                                    "double_iter" => {
                                        while let Ok(Some(_)) = body.next_chunk().await {}
                                        let second = body.next_chunk().await;
                                        if second.is_err() || second.unwrap().is_some() {
                                            return Ok(Response::builder()
                                                .status(StatusCode::OK)
                                                .body(ResponseBody::Bytes(
                                                    b"consumed".to_vec(),
                                                ))
                                                .unwrap());
                                        }
                                    }
                                    _ => {}
                                }
                                return Ok(Response::builder()
                                    .status(StatusCode::OK)
                                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                                    .unwrap());
                            }

                            // For stream mode, collect via next_chunk; for buffer, use read_all
                            let mut all = Vec::new();
                            use futures_util::StreamExt;
                            loop {
                                match body.next_chunk().await {
                                    Ok(Some(chunk)) => all.extend_from_slice(&chunk),
                                    Ok(None) => break,
                                    Err(_) => break,
                                }
                            }
                            Ok(Response::builder()
                                .status(StatusCode::OK)
                                .body(ResponseBody::Bytes(
                                    format!("{}:{}", method, String::from_utf8_lossy(&all))
                                        .into_bytes(),
                                ))
                                .unwrap())
                        }
                    },
                    policy,
                ))
                .await
                .unwrap();
            handle.ready().await.unwrap();
            let addr = handle.local_addr();

            let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
            let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
            conn.write_all(&req_bytes).await.unwrap();
            let mut buf = Vec::new();
            conn.read_to_end(&mut buf).await.unwrap();
            let status = parse_response_status(&buf).unwrap_or(0);

            assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

            if let Some(expected_handler_called) = fixture.expected.handler_called {
                assert_eq!(
                    handler_called.load(std::sync::atomic::Ordering::Relaxed),
                    expected_handler_called,
                    "{}: handler_called",
                    fixture.id
                );
            }

            if let Some(ref expected_echo) = fixture.expected.echo_body {
                let resp_body = parse_response_body(&buf);
                let resp_str = String::from_utf8_lossy(&resp_body);
                assert!(
                    resp_str.contains(expected_echo),
                    "{}: echo_body expected '{}' got '{}'",
                    fixture.id,
                    expected_echo,
                    resp_str
                );
            }

            handle.shutdown();
        }
    }
}

#[tokio::test]
async fn body_conformance_empty_body() {
    for fixture in group("empty_body") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
        let handler_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

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
                move |req: Request| {
                    let handler_called = handler_called_clone.clone();
                    async move {
                        handler_called.store(true, std::sync::atomic::Ordering::Relaxed);
                        let (_head, body) = req.into_head_and_body();
                        let data = body.read_all().await.unwrap_or_default();
                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(ResponseBody::Bytes(data.to_vec()))
                            .unwrap())
                    }
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);
        assert!(
            handler_called.load(std::sync::atomic::Ordering::Relaxed),
            "{}: handler should be called",
            fixture.id
        );

        if let Some(ref expected_body) = fixture.expected.body_data {
            let resp_body = parse_response_body(&buf);
            assert_eq!(
                String::from_utf8_lossy(&resp_body),
                *expected_body,
                "{}: body_data",
                fixture.id
            );
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_fixed_length_exact() {
    for fixture in group("fixed_length_exact") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
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
                    let data = body.read_all().await.unwrap_or_default();
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(data.to_vec()))
                        .unwrap())
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

        if let Some(ref expected_echo) = fixture.expected.echo_body {
            let resp_body = parse_response_body(&buf);
            assert_eq!(
                String::from_utf8_lossy(&resp_body),
                *expected_echo,
                "{}: echo_body",
                fixture.id
            );
        }

        if let Some(expected_len) = fixture.expected.echo_len {
            let resp_body = parse_response_body(&buf);
            assert_eq!(resp_body.len(), expected_len, "{}: echo_len", fixture.id);
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_fixed_length_over_limit() {
    for fixture in group("fixed_length_over_limit") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
        let handler_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

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
                move |req: Request| {
                    let handler_called = handler_called_clone.clone();
                    async move {
                        handler_called.store(true, std::sync::atomic::Ordering::Relaxed);
                        let (_head, body) = req.into_head_and_body();
                        let _ = body.read_all().await;
                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(ResponseBody::Bytes(b"ok".to_vec()))
                            .unwrap())
                    }
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

        if let Some(expected_handler_called) = fixture.expected.handler_called {
            assert_eq!(
                handler_called.load(std::sync::atomic::Ordering::Relaxed),
                expected_handler_called,
                "{}: handler_called",
                fixture.id
            );
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_chunked_exact() {
    for fixture in group("chunked_exact") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
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
                    let mut all = Vec::new();
                    while let Some(chunk) = body.next_chunk().await.unwrap() {
                        all.extend_from_slice(&chunk);
                    }
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(all))
                        .unwrap())
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

        if let Some(ref expected_echo) = fixture.expected.echo_body {
            let resp_body = parse_response_body(&buf);
            assert_eq!(
                String::from_utf8_lossy(&resp_body),
                *expected_echo,
                "{}: echo_body",
                fixture.id
            );
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_chunked_over_limit() {
    for fixture in group("chunked_over_limit") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
        let handler_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

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
                move |req: Request| {
                    let handler_called = handler_called_clone.clone();
                    async move {
                        handler_called.store(true, std::sync::atomic::Ordering::Relaxed);
                        let (_head, mut body) = req.into_head_and_body();
                        let mut all = Vec::new();
                        let mut hit_limit = false;
                        use futures_util::StreamExt;
                        loop {
                            match body.next_chunk().await {
                                Ok(Some(chunk)) => all.extend_from_slice(&chunk),
                                Ok(None) => break,
                                Err(_) => {
                                    hit_limit = true;
                                    break;
                                }
                            }
                        }
                        if hit_limit {
                            return Ok(Response::builder()
                                .status(StatusCode::PAYLOAD_TOO_LARGE)
                                .body(ResponseBody::Bytes(b"limit exceeded".to_vec()))
                                .unwrap());
                        }
                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(ResponseBody::Bytes(all))
                            .unwrap())
                    }
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

        if let Some(expected_handler_called) = fixture.expected.handler_called {
            assert_eq!(
                handler_called.load(std::sync::atomic::Ordering::Relaxed),
                expected_handler_called,
                "{}: handler_called",
                fixture.id
            );
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_one_shot_consumption() {
    for fixture in group("one_shot_consumption") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
        let handler_action = fixture.input.handler_action.clone();
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
                move |req: Request| {
                    let action = handler_action.clone();
                    async move {
                        let (_head, mut body) = req.into_head_and_body();
                        let _action = action.unwrap_or_default();

                        // For one-shot tests, always try consuming and return
                        // "consumed" if it fails
                        let first = body.next_chunk().await;
                        if first.is_err() || first.unwrap().is_none() {
                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .body(ResponseBody::Bytes(b"consumed".to_vec()))
                                .unwrap());
                        }

                        // Try second consumption
                        let second = body.next_chunk().await;
                        if second.is_err() || second.unwrap().is_some() {
                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .body(ResponseBody::Bytes(b"consumed".to_vec()))
                                .unwrap());
                        }

                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(ResponseBody::Bytes(b"ok".to_vec()))
                            .unwrap())
                    }
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

        if let Some(ref expected_error) = fixture.expected.handler_error {
            let resp_body = parse_response_body(&buf);
            let resp_str = String::from_utf8_lossy(&resp_body);
            assert!(
                resp_str.contains(expected_error),
                "{}: expected handler_error '{}' got '{}'",
                fixture.id,
                expected_error,
                resp_str
            );
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_get_with_body_rejected() {
    for fixture in group("get_with_body_rejected") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
        let handler_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

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
                move |req: Request| {
                    let handler_called = handler_called_clone.clone();
                    async move {
                        handler_called.store(true, std::sync::atomic::Ordering::Relaxed);
                        let (_head, body) = req.into_head_and_body();
                        let _ = body.read_all().await;
                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(ResponseBody::Bytes(b"ok".to_vec()))
                            .unwrap())
                    }
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let status = parse_response_status(&buf).unwrap_or(0);

        assert_eq!(status, fixture.expected.status, "{}: status", fixture.id);

        if let Some(expected_handler_called) = fixture.expected.handler_called {
            assert_eq!(
                handler_called.load(std::sync::atomic::Ordering::Relaxed),
                expected_handler_called,
                "{}: handler_called",
                fixture.id
            );
        }

        handle.shutdown();
    }
}

#[tokio::test]
async fn body_conformance_premature_eof() {
    for fixture in group("fixed_length_premature_eof") {
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(fixture.input.max_body_bytes)
            .body_read_timeout(Duration::from_secs(5))
            .build();

        let policy = parse_policy(&fixture.input);
        let handler_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

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
                move |req: Request| {
                    let handler_called = handler_called_clone.clone();
                    async move {
                        handler_called.store(true, std::sync::atomic::Ordering::Relaxed);
                        let (_head, body) = req.into_head_and_body();
                        let _ = body.read_all().await;
                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(ResponseBody::Bytes(b"ok".to_vec()))
                            .unwrap())
                    }
                },
                policy,
            ))
            .await
            .unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();

        let req_bytes = build_request_bytes(&fixture.input, &addr.to_string().as_str());
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        conn.write_all(&req_bytes).await.unwrap();
        // Don't send the full body - close early for premature EOF
        // (body_partial already contains partial data)
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
        let status = parse_response_status(&buf);

        // Premature EOF should result in an error status or connection close
        if let Some(s) = status {
            assert_eq!(s, fixture.expected.status, "{}: status", fixture.id);
        }
        // Handler should not be called on premature EOF
        assert!(
            !handler_called.load(std::sync::atomic::Ordering::Relaxed),
            "{}: handler should NOT be called on premature EOF",
            fixture.id
        );

        handle.shutdown();
    }
}
