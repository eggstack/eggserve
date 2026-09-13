//! Direct-vs-compatibility H1 parity (Plan 215 Workstream J).
//!
//! Drives identical raw-HTTP scenarios through the direct
//! (`eggserve-server`) and compatibility (`eggserve-core::server`) H1 stacks
//! and asserts identical wire responses and connection outcomes. Tunnel
//! acceptance is excluded (Plan 216 owns it); every case below is ordinary
//! HTTP/1.1.
//!
//! The exchanges are sequential over a buffered `duplex` pair: the small
//! request fits the buffer, the driver runs inline to completion, then the
//! buffered response is read. No spawned task outlives a stack borrow.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DUPLEX_BUF: usize = 64 * 1024;
const DRIVE_TIMEOUT: Duration = Duration::from_secs(10);

fn short_timeout() -> Duration {
    Duration::from_millis(300)
}

fn long_timeout() -> Duration {
    Duration::from_secs(30)
}

fn direct_config() -> Arc<eggserve_server::config::RuntimeConfig> {
    Arc::new(
        eggserve_server::config::RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .build()
            .unwrap(),
    )
}

fn compat_config() -> Arc<eggserve_core::server::RuntimeConfig> {
    Arc::new(
        eggserve_core::server::RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .build()
            .unwrap(),
    )
}

fn direct_ctx() -> eggserve_server::connection::ConnectionContext {
    eggserve_server::connection::ConnectionContext::for_non_socket(
        eggserve_primitives::connection_info::Scheme::Http,
        None,
    )
}

fn compat_ctx() -> eggserve_core::server::connection::ConnectionContext {
    eggserve_core::server::connection::ConnectionContext::for_non_socket(
        eggserve_core::primitives::connection_info::Scheme::Http,
        None,
    )
}

fn split_response(raw: &[u8]) -> (String, Vec<u8>) {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    // Date varies by second across stacks; compare everything else.
    let head: Vec<&str> = head
        .lines()
        .filter(|line| !line.to_ascii_lowercase().starts_with("date:"))
        .collect();
    (head.join("\n"), body.as_bytes().to_vec())
}

fn ok_bytes() -> eggserve_primitives::canonical::Response {
    eggserve_primitives::canonical::Response::builder()
        .status(eggserve_primitives::canonical::StatusCode::OK)
        .body(eggserve_primitives::canonical::ResponseBody::Bytes(
            b"ok".to_vec(),
        ))
        .unwrap()
}

fn ok_bytes_compat() -> eggserve_core::primitives::canonical::Response {
    eggserve_core::primitives::canonical::Response::builder()
        .status(eggserve_core::primitives::canonical::StatusCode::OK)
        .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(
            b"ok".to_vec(),
        ))
        .unwrap()
}

async fn direct_ok(
    _req: eggserve_server::Request,
) -> Result<eggserve_primitives::canonical::Response, eggserve_server::ServiceError> {
    Ok(ok_bytes())
}

async fn compat_ok(
    _req: eggserve_core::server::Request,
) -> Result<eggserve_core::primitives::canonical::Response, eggserve_core::server::ServiceError> {
    Ok(ok_bytes_compat())
}

/// Drive one `Connection: close` exchange inline and return the response
/// head (minus `Date`), body, and outcome.
async fn full_exchange_direct(
    request: &[u8],
    service: impl eggserve_server::service::Service,
    config: Arc<eggserve_server::config::RuntimeConfig>,
) -> (String, Vec<u8>, String) {
    let (mut client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    client.write_all(request).await.unwrap();
    let state = Arc::new(eggserve_server::runtime::RuntimeState::new(&config));
    let shutdown = eggserve_server::connection::ConnectionShutdown::new();
    let outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_server::connection::serve_http1_connection(
            server_io,
            service,
            config,
            direct_ctx(),
            state,
            &shutdown,
        ),
    )
    .await
    .expect("direct driver must terminate")
    .to_string();
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let (head, body) = split_response(&raw);
    (head, body, outcome)
}

async fn full_exchange_compat(
    request: &[u8],
    service: impl eggserve_core::server::Service,
    config: Arc<eggserve_core::server::RuntimeConfig>,
) -> (String, Vec<u8>, String) {
    let (mut client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    client.write_all(request).await.unwrap();
    let state = Arc::new(eggserve_core::server::RuntimeState::new(&config));
    let shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_core::server::connection::serve_http1_connection(
            server_io,
            service,
            config,
            compat_ctx(),
            state,
            &shutdown,
        ),
    )
    .await
    .expect("compat driver must terminate")
    .to_string();
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let (head, body) = split_response(&raw);
    (head, body, outcome)
}

fn assert_parity(direct: &(String, Vec<u8>, String), compat: &(String, Vec<u8>, String)) {
    assert_eq!(direct.0, compat.0, "status/head mismatch");
    assert_eq!(direct.1, compat.1, "body mismatch");
    assert_eq!(direct.2, compat.2, "outcome mismatch");
}

#[tokio::test]
async fn get_empty_200_matches() {
    let req = b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n";
    let direct =
        full_exchange_direct(req, eggserve_server::service_fn(direct_ok), direct_config()).await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn(compat_ok),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(direct.1, b"ok");
    assert_eq!(direct.2, "normal");
}

#[tokio::test]
async fn head_matches_with_length_and_no_body() {
    let req = b"HEAD / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n";
    let direct =
        full_exchange_direct(req, eggserve_server::service_fn(direct_ok), direct_config()).await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn(compat_ok),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 200 OK"));
    assert!(direct.1.is_empty());
    assert!(direct.0.contains("content-length: 2"));
}

#[tokio::test]
async fn post_reject_matches_413() {
    let req =
        b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
    let direct =
        full_exchange_direct(req, eggserve_server::service_fn(direct_ok), direct_config()).await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn(compat_ok),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 413"));
}

async fn direct_buffer_echo(
    req: eggserve_server::Request,
) -> Result<eggserve_primitives::canonical::Response, eggserve_server::ServiceError> {
    let (_head, body) = req.into_head_and_body();
    let bytes = body
        .read_all()
        .await
        .map_err(eggserve_server::ServiceError::from)?;
    Ok(eggserve_primitives::canonical::Response::builder()
        .status(eggserve_primitives::canonical::StatusCode::OK)
        .body(eggserve_primitives::canonical::ResponseBody::Bytes(
            bytes.to_vec(),
        ))
        .unwrap())
}

async fn compat_buffer_echo(
    req: eggserve_core::server::Request,
) -> Result<eggserve_core::primitives::canonical::Response, eggserve_core::server::ServiceError> {
    let (_head, body) = req.into_head_and_body();
    let bytes = body
        .read_all()
        .await
        .map_err(eggserve_core::server::ServiceError::from)?;
    Ok(eggserve_core::primitives::canonical::Response::builder()
        .status(eggserve_core::primitives::canonical::StatusCode::OK)
        .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(
            bytes.to_vec(),
        ))
        .unwrap())
}

#[tokio::test]
async fn post_buffer_echo_matches() {
    let req = b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world";
    let direct = full_exchange_direct(
        req,
        eggserve_server::service_fn_with_policy(
            direct_buffer_echo,
            eggserve_primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ),
        direct_config(),
    )
    .await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn_with_policy(
            compat_buffer_echo,
            eggserve_core::primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert_eq!(direct.1, b"hello world");
}

async fn direct_stream_echo(
    req: eggserve_server::Request,
) -> Result<eggserve_primitives::canonical::Response, eggserve_server::ServiceError> {
    let (_head, mut body) = req.into_head_and_body();
    let mut out = Vec::new();
    while let Some(chunk) = body
        .next_chunk()
        .await
        .map_err(eggserve_server::ServiceError::from)?
    {
        out.extend_from_slice(&chunk);
    }
    Ok(eggserve_primitives::canonical::Response::builder()
        .status(eggserve_primitives::canonical::StatusCode::OK)
        .body(eggserve_primitives::canonical::ResponseBody::Bytes(out))
        .unwrap())
}

async fn compat_stream_echo(
    req: eggserve_core::server::Request,
) -> Result<eggserve_core::primitives::canonical::Response, eggserve_core::server::ServiceError> {
    let (_head, mut body) = req.into_head_and_body();
    let mut out = Vec::new();
    while let Some(chunk) = body
        .next_chunk()
        .await
        .map_err(eggserve_core::server::ServiceError::from)?
    {
        out.extend_from_slice(&chunk);
    }
    Ok(eggserve_core::primitives::canonical::Response::builder()
        .status(eggserve_core::primitives::canonical::StatusCode::OK)
        .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(
            out,
        ))
        .unwrap())
}

#[tokio::test]
async fn post_stream_echo_matches() {
    let req = b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world";
    let direct = full_exchange_direct(
        req,
        eggserve_server::service_fn_with_policy(
            direct_stream_echo,
            eggserve_primitives::RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        ),
        direct_config(),
    )
    .await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn_with_policy(
            compat_stream_echo,
            eggserve_core::primitives::RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        ),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert_eq!(direct.1, b"hello world");
}

#[tokio::test]
async fn chunked_buffer_echo_matches() {
    let req = b"POST /echo HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    let direct = full_exchange_direct(
        req,
        eggserve_server::service_fn_with_policy(
            direct_buffer_echo,
            eggserve_primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ),
        direct_config(),
    )
    .await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn_with_policy(
            compat_buffer_echo,
            eggserve_core::primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert_eq!(direct.1, b"hello world");
}

async fn direct_trailer_echo(
    req: eggserve_server::Request,
) -> Result<eggserve_primitives::canonical::Response, eggserve_server::ServiceError> {
    let (_head, body) = req.into_head_and_body();
    let (bytes, trailers) = body
        .read_all_with_trailers()
        .await
        .map_err(eggserve_server::ServiceError::from)?;
    let text = format!("{}|{}", bytes.len(), trailers.is_some());
    Ok(eggserve_primitives::canonical::Response::builder()
        .status(eggserve_primitives::canonical::StatusCode::OK)
        .body(eggserve_primitives::canonical::ResponseBody::Bytes(
            text.into_bytes(),
        ))
        .unwrap())
}

async fn compat_trailer_echo(
    req: eggserve_core::server::Request,
) -> Result<eggserve_core::primitives::canonical::Response, eggserve_core::server::ServiceError> {
    let (_head, body) = req.into_head_and_body();
    let (bytes, trailers) = body
        .read_all_with_trailers()
        .await
        .map_err(eggserve_core::server::ServiceError::from)?;
    let text = format!("{}|{}", bytes.len(), trailers.is_some());
    Ok(eggserve_core::primitives::canonical::Response::builder()
        .status(eggserve_core::primitives::canonical::StatusCode::OK)
        .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(
            text.into_bytes(),
        ))
        .unwrap())
}

#[tokio::test]
async fn chunked_trailers_match() {
    let req = b"POST /echo HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\nTE: trailers\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\nx-sum: 42\r\n\r\n";
    let direct = full_exchange_direct(
        req,
        eggserve_server::service_fn_with_policy(
            direct_trailer_echo,
            eggserve_primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ),
        direct_config(),
    )
    .await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn_with_policy(
            compat_trailer_echo,
            eggserve_core::primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        ),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert_eq!(direct.1, b"5|true");
}

#[tokio::test]
async fn malformed_framing_matches_400() {
    let req = b"GARBAGE\r\n\r\n";
    let direct =
        full_exchange_direct(req, eggserve_server::service_fn(direct_ok), direct_config()).await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn(compat_ok),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 400"));
}

#[tokio::test]
async fn target_too_long_matches_414() {
    let path = format!("/{}", "a".repeat(9000));
    let req = format!("GET {path} HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n");
    let direct = full_exchange_direct(
        req.as_bytes(),
        eggserve_server::service_fn(direct_ok),
        direct_config(),
    )
    .await;
    let compat = full_exchange_compat(
        req.as_bytes(),
        eggserve_core::server::service_fn(compat_ok),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 414"));
}

#[tokio::test]
async fn headers_too_large_matches_431() {
    let big = "v".repeat(2048);
    let req = format!("GET / HTTP/1.1\r\nHost: h\r\nX-Big: {big}\r\nConnection: close\r\n\r\n");
    let direct_cfg = Arc::new(
        eggserve_server::config::RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .max_header_bytes(1024)
            .build()
            .unwrap(),
    );
    let compat_cfg = Arc::new(
        eggserve_core::server::RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .max_header_bytes(1024)
            .build()
            .unwrap(),
    );
    let direct = full_exchange_direct(
        req.as_bytes(),
        eggserve_server::service_fn(direct_ok),
        direct_cfg,
    )
    .await;
    let compat = full_exchange_compat(
        req.as_bytes(),
        eggserve_core::server::service_fn(compat_ok),
        compat_cfg,
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 431"));
}

async fn direct_sleepy(
    _req: eggserve_server::Request,
) -> Result<eggserve_primitives::canonical::Response, eggserve_server::ServiceError> {
    tokio::time::sleep(Duration::from_secs(30)).await;
    Ok(ok_bytes())
}

async fn compat_sleepy(
    _req: eggserve_core::server::Request,
) -> Result<eggserve_core::primitives::canonical::Response, eggserve_core::server::ServiceError> {
    tokio::time::sleep(Duration::from_secs(30)).await;
    Ok(ok_bytes_compat())
}

#[tokio::test]
async fn handler_timeout_matches_504() {
    let req = b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n";
    let direct_cfg = Arc::new(
        eggserve_server::config::RuntimeConfig::builder()
            .handler_timeout(short_timeout())
            .connection_total_timeout(long_timeout())
            .build()
            .unwrap(),
    );
    let compat_cfg = Arc::new(
        eggserve_core::server::RuntimeConfig::builder()
            .handler_timeout(short_timeout())
            .connection_total_timeout(long_timeout())
            .build()
            .unwrap(),
    );
    let direct =
        full_exchange_direct(req, eggserve_server::service_fn(direct_sleepy), direct_cfg).await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn(compat_sleepy),
        compat_cfg,
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 504"));
}

#[tokio::test]
async fn service_panic_matches_500_without_detail_leak() {
    async fn boom(
        _req: eggserve_server::Request,
    ) -> Result<eggserve_primitives::canonical::Response, eggserve_server::ServiceError> {
        panic!("secret-boom-direct");
    }
    async fn boom_compat(
        _req: eggserve_core::server::Request,
    ) -> Result<eggserve_core::primitives::canonical::Response, eggserve_core::server::ServiceError>
    {
        panic!("secret-boom-compat");
    }
    let req = b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n";
    let direct =
        full_exchange_direct(req, eggserve_server::service_fn(boom), direct_config()).await;
    let compat = full_exchange_compat(
        req,
        eggserve_core::server::service_fn(boom_compat),
        compat_config(),
    )
    .await;
    assert_parity(&direct, &compat);
    assert!(direct.0.starts_with("HTTP/1.1 500"));
    assert!(!String::from_utf8_lossy(&direct.1).contains("secret"));
}

#[tokio::test]
async fn header_timeout_matches() {
    let direct_cfg = Arc::new(
        eggserve_server::config::RuntimeConfig::builder()
            .header_read_timeout(short_timeout())
            .connection_total_timeout(long_timeout())
            .build()
            .unwrap(),
    );
    let compat_cfg = Arc::new(
        eggserve_core::server::RuntimeConfig::builder()
            .header_read_timeout(short_timeout())
            .connection_total_timeout(long_timeout())
            .build()
            .unwrap(),
    );
    // Open the stream and send nothing; the header deadline must fire.
    let (_client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    let direct_state = Arc::new(eggserve_server::runtime::RuntimeState::new(&direct_cfg));
    let direct_shutdown = eggserve_server::connection::ConnectionShutdown::new();
    let direct_outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_server::connection::serve_http1_connection(
            server_io,
            eggserve_server::service_fn(direct_ok),
            direct_cfg,
            direct_ctx(),
            direct_state,
            &direct_shutdown,
        ),
    )
    .await
    .expect("direct driver must terminate")
    .to_string();

    let (_client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    let compat_state = Arc::new(eggserve_core::server::RuntimeState::new(&compat_cfg));
    let compat_shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let compat_outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_core::server::connection::serve_http1_connection(
            server_io,
            eggserve_core::server::service_fn(compat_ok),
            compat_cfg,
            compat_ctx(),
            compat_state,
            &compat_shutdown,
        ),
    )
    .await
    .expect("compat driver must terminate")
    .to_string();

    assert_eq!(direct_outcome, compat_outcome);
    assert_eq!(direct_outcome, "header-timeout");
}

#[tokio::test]
async fn stalled_body_matches_504() {
    let direct_cfg = Arc::new(
        eggserve_server::config::RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(short_timeout())
            .connection_total_timeout(long_timeout())
            .build()
            .unwrap(),
    );
    let compat_cfg = Arc::new(
        eggserve_core::server::RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(short_timeout())
            .connection_total_timeout(long_timeout())
            .build()
            .unwrap(),
    );
    // Send headers plus a partial body, then stall: the collapsed
    // body/handler deadline fires while the stalled body is still owned by
    // the timed-out service future, which reads as a handler-side timeout
    // (504) on both stacks. The wire status must agree exactly.
    let head = b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 1024\r\nConnection: close\r\n\r\npartial";
    let (mut direct_client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    direct_client.write_all(head).await.unwrap();
    let direct_state = Arc::new(eggserve_server::runtime::RuntimeState::new(&direct_cfg));
    let direct_shutdown = eggserve_server::connection::ConnectionShutdown::new();
    let direct_outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_server::connection::serve_http1_connection(
            server_io,
            eggserve_server::service_fn_with_policy(
                direct_stream_echo,
                eggserve_primitives::RequestBodyPolicy::Stream {
                    max_bytes: 1024 * 1024,
                },
            ),
            direct_cfg,
            direct_ctx(),
            direct_state,
            &direct_shutdown,
        ),
    )
    .await
    .expect("direct driver must terminate")
    .to_string();

    let (mut compat_client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    compat_client.write_all(head).await.unwrap();
    let compat_state = Arc::new(eggserve_core::server::RuntimeState::new(&compat_cfg));
    let compat_shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let compat_outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_core::server::connection::serve_http1_connection(
            server_io,
            eggserve_core::server::service_fn_with_policy(
                compat_stream_echo,
                eggserve_core::primitives::RequestBodyPolicy::Stream {
                    max_bytes: 1024 * 1024,
                },
            ),
            compat_cfg,
            compat_ctx(),
            compat_state,
            &compat_shutdown,
        ),
    )
    .await
    .expect("compat driver must terminate")
    .to_string();

    assert_eq!(direct_outcome, compat_outcome);
    for (client, outcome) in [
        (direct_client, direct_outcome),
        (compat_client, compat_outcome),
    ] {
        let _ = outcome;
        let mut client = client;
        let mut raw = Vec::new();
        client.read_to_end(&mut raw).await.unwrap();
        let (head, _body) = split_response(&raw);
        assert!(
            head.starts_with("HTTP/1.1 504"),
            "stalled body must yield 504 on both stacks, got: {head}"
        );
    }
}

#[tokio::test]
async fn max_requests_per_connection_matches_close() {
    // With max 1, a single keep-alive GET completes with `Connection: close`
    // and the driver then ends normally. Sequential: the small request fits
    // the buffer, the driver responds and closes, then the buffered reply
    // is read to EOF.
    let req = b"GET / HTTP/1.1\r\nHost: h\r\n\r\n";
    let direct_cfg = Arc::new(
        eggserve_server::config::RuntimeConfig::builder()
            .max_requests_per_connection(Some(1))
            .build()
            .unwrap(),
    );
    let (mut client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    client.write_all(req).await.unwrap();
    let direct_state = Arc::new(eggserve_server::runtime::RuntimeState::new(&direct_cfg));
    let direct_shutdown = eggserve_server::connection::ConnectionShutdown::new();
    let direct_outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_server::connection::serve_http1_connection(
            server_io,
            eggserve_server::service_fn(direct_ok),
            direct_cfg,
            direct_ctx(),
            direct_state,
            &direct_shutdown,
        ),
    )
    .await
    .expect("direct driver must terminate")
    .to_string();
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let (head, _body) = split_response(&raw);
    assert!(
        head.to_ascii_lowercase().contains("connection: close"),
        "max-requests response must close, got: {head}"
    );
    assert_eq!(direct_outcome, "normal");

    let compat_cfg = Arc::new(
        eggserve_core::server::RuntimeConfig::builder()
            .max_requests_per_connection(Some(1))
            .build()
            .unwrap(),
    );
    let (mut client, server_io) = tokio::io::duplex(DUPLEX_BUF);
    client.write_all(req).await.unwrap();
    let compat_state = Arc::new(eggserve_core::server::RuntimeState::new(&compat_cfg));
    let compat_shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let compat_outcome = tokio::time::timeout(
        DRIVE_TIMEOUT,
        eggserve_core::server::connection::serve_http1_connection(
            server_io,
            eggserve_core::server::service_fn(compat_ok),
            compat_cfg,
            compat_ctx(),
            compat_state,
            &compat_shutdown,
        ),
    )
    .await
    .expect("compat driver must terminate")
    .to_string();
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let (head, _body) = split_response(&raw);
    assert!(
        head.to_ascii_lowercase().contains("connection: close"),
        "max-requests response must close, got: {head}"
    );
    assert_eq!(compat_outcome, "normal");
}

#[tokio::test]
async fn invalid_config_rejected_identically() {
    let direct_err = eggserve_server::config::RuntimeConfig::builder()
        .max_connections(0)
        .build()
        .unwrap_err()
        .to_string();
    let compat_err = eggserve_core::server::RuntimeConfig::builder()
        .max_connections(0)
        .build()
        .unwrap_err()
        .to_string();
    assert_eq!(direct_err, compat_err);

    // Shared-kernel cross-field rule projects identically too.
    let direct_err = eggserve_server::config::RuntimeConfig::builder()
        .handler_timeout(Duration::from_secs(60))
        .connection_total_timeout(Duration::from_secs(30))
        .build()
        .unwrap_err()
        .to_string();
    let compat_err = eggserve_core::server::RuntimeConfig::builder()
        .handler_timeout(Duration::from_secs(60))
        .connection_total_timeout(Duration::from_secs(30))
        .build()
        .unwrap_err()
        .to_string();
    assert_eq!(direct_err, compat_err);

    // And hand-constructed validation agrees.
    let direct_cfg = eggserve_server::config::RuntimeConfig {
        max_file_streams: 0,
        ..eggserve_server::config::RuntimeConfig::default()
    };
    let compat_cfg = eggserve_core::server::RuntimeConfig {
        max_file_streams: 0,
        ..eggserve_core::server::RuntimeConfig::default()
    };
    assert_eq!(
        direct_cfg.validate().unwrap_err().to_string(),
        compat_cfg.validate().unwrap_err().to_string()
    );
}
