//! Plan 321: forwarded-header trust follows the accepting transport.
//!
//! A caller-owned stream has no runtime-known peer. Even when it is driven
//! with a Unix-shaped [`ConnectionContext`], `trust_unix` must not adopt
//! attacker-supplied forwarding metadata. The runtime-owned Unix listener
//! case (which does adopt it) is covered by the compatibility suite, where
//! the Unix accept loop lives.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use eggserve_primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_primitives::connection_info::Scheme;
use eggserve_server::connection::{serve_http1_connection, ConnectionContext, ConnectionShutdown};
use eggserve_server::{service_fn, Request, RuntimeConfig, RuntimeState};

fn trusted_unix_config() -> Arc<RuntimeConfig> {
    let mut config = RuntimeConfig::default();
    config.trusted_proxy.trust_unix = true;
    config.trusted_proxy.forwarded.standard_enabled = true;
    Arc::new(config)
}

fn effective_client_echo() -> impl eggserve_server::Service {
    service_fn(|req: Request| async move {
        let connection = req.connection();
        let effective = connection
            .effective_client_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let raw_peer = connection
            .remote_addr
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let canonical = req
            .head()
            .authority()
            .map(|authority| authority.as_str().to_owned())
            .unwrap_or_else(|| "none".to_owned());
        let body = format!("raw={raw_peer};effective={effective};canonical={canonical}");
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap())
    })
}

/// Drive one request over a caller-owned duplex stream and return the body
/// plus the `forwarded_rejected` counter observed after the connection ends.
async fn drive(
    request: &[u8],
    context: ConnectionContext,
    config: Arc<RuntimeConfig>,
) -> (String, u64) {
    let runtime_state = Arc::new(RuntimeState::new(&config));
    let counter_state = Arc::clone(&runtime_state);
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            effective_client_echo(),
            config,
            context,
            runtime_state,
            &shutdown,
        )
        .await
    });

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    client.write_all(request).await.unwrap();
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut response)).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), driver).await;

    let text = String::from_utf8_lossy(&response);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_else(|| text.into_owned());
    let rejected = counter_state
        .ops()
        .counters()
        .forwarded_rejected
        .load(Ordering::Relaxed);
    (body, rejected)
}

const SPOOFED_REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: localhost\r\n\
Forwarded: for=203.0.113.9;proto=https;host=public.example\r\n\
X-Forwarded-For: 203.0.113.9\r\nConnection: close\r\n\r\n";

#[tokio::test]
async fn non_socket_caller_stream_ignores_trust_unix() {
    let config = trusted_unix_config();
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let (body, rejected) = drive(SPOOFED_REQUEST, context, config).await;

    assert!(body.contains("effective=none"), "body: {body}");
    assert!(body.contains("raw=none"), "body: {body}");
    assert!(body.contains("canonical=localhost"), "body: {body}");
    assert!(rejected >= 1, "untrusted peer must be counted as rejected");
}

#[cfg(unix)]
#[tokio::test]
async fn unix_shaped_context_on_public_driver_is_opaque() {
    let config = trusted_unix_config();
    // `for_unix()` describes a Unix transport, but this entry point is
    // caller-owned: the runtime did not accept the peer, so `trust_unix`
    // must not apply.
    let context = ConnectionContext::for_unix();
    let (body, rejected) = drive(SPOOFED_REQUEST, context, config).await;

    assert!(body.contains("effective=none"), "body: {body}");
    assert!(body.contains("raw=none"), "body: {body}");
    assert!(body.contains("canonical=localhost"), "body: {body}");
    assert!(rejected >= 1, "untrusted peer must be counted as rejected");
}
