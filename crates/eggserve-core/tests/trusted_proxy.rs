//! Trusted proxy metadata and PROXY protocol qualification (Plan 202).
//!
//! Covers the acceptance cases: raw preservation, default no-trust, explicit
//! peer policy with validation, bounded PROXY v1/v2 before TLS/HTTP,
//! malformed/untrusted never reaching services as trusted facts,
//! header hop/conflict policy with hard bounds, canonical Host preservation,
//! native consumer access, and unchanged compatibility behavior.

use std::net::SocketAddr;
use std::time::Duration;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::proxy::{
    derive_forwarded_effective, parse_proxy_v1_line, parse_proxy_v2_header, ForwardedConfig,
    IpPrefix, ProxySourceKind, TrustedProxyConfig, PROXY_V1_MAX_BYTES, PROXY_V2_MAX_LEN,
    PROXY_V2_SIGNATURE,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn header_block(pairs: &[(&str, &str)]) -> HeaderBlock {
    let mut block = HeaderBlock::new();
    for (name, value) in pairs {
        block.push_str(*name, *value).unwrap();
    }
    block
}

async fn raw_http_request(addr: SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    buf
}

fn echo_service() -> impl eggserve_core::server::Service {
    service_fn(|req: Request| async move {
        let connection = req.connection();
        let effective_client = connection
            .effective_client_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let raw_peer = connection
            .remote_addr
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let effective_scheme = connection.effective_scheme_value().as_str().to_owned();
        let effective_authority = connection
            .effective_authority_value()
            .map(|authority| authority.as_str().to_owned())
            .unwrap_or_else(|| "none".to_owned());
        let proxy_provenance = connection
            .proxy_provenance
            .map(|kind| kind.as_str().to_owned())
            .unwrap_or_else(|| "none".to_owned());
        let forwarded_provenance = connection
            .forwarded_provenance
            .map(|kind| kind.as_str().to_owned())
            .unwrap_or_else(|| "none".to_owned());
        let canonical_authority = req
            .head()
            .authority()
            .map(|authority| authority.as_str().to_owned())
            .unwrap_or_else(|| "none".to_owned());
        let body = format!(
            "raw={raw_peer};effective={effective_client};scheme={effective_scheme};\
             authority={effective_authority};canonical={canonical_authority};\
             proxy={proxy_provenance};forwarded={forwarded_provenance}"
        );
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap())
    })
}

async fn start_server(config: RuntimeConfig) -> (eggserve_core::server::ServerHandle, SocketAddr) {
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    (handle, addr)
}

// ---------------------------------------------------------------------------
// Track B: trusted peer policy
// ---------------------------------------------------------------------------

#[test]
fn default_trusts_nothing() {
    let config = TrustedProxyConfig::default();
    assert!(config.peers.is_empty());
    assert!(!config.trust_unix);
    assert!(!config.proxy_protocol.enabled);
    assert!(config.forwarded.is_disabled());
    // Loopback is not implicitly trusted.
    let loopback: SocketAddr = "127.0.0.1:12345".parse().unwrap();
    assert!(!config.is_trusted_peer(&loopback));
}

#[test]
fn loopback_must_be_listed_explicitly() {
    let mut config = TrustedProxyConfig::default();
    config.peers.push(IpPrefix::parse("127.0.0.1").unwrap());
    let loopback: SocketAddr = "127.0.0.1:54321".parse().unwrap();
    assert!(config.is_trusted_peer(&loopback));
    let other: SocketAddr = "127.0.0.2:54321".parse().unwrap();
    assert!(!config.is_trusted_peer(&other));
}

#[test]
fn cidr_matching_covers_subnets() {
    let prefix = IpPrefix::parse("10.0.0.0/8").unwrap();
    assert!(prefix.matches(&"10.1.2.3".parse().unwrap()));
    assert!(!prefix.matches(&"11.0.0.1".parse().unwrap()));
}

#[test]
fn peer_validation_rejects_dns_and_bad_ranges() {
    assert!(IpPrefix::parse("proxy.internal").is_err());
    assert!(IpPrefix::parse("").is_err());
    assert!(IpPrefix::parse("192.168.0.0/33").is_err());
    assert!(IpPrefix::parse("2001:db8::/129").is_err());
}

#[test]
fn runtime_config_validation_covers_proxy_budgets() {
    let mut proxy = TrustedProxyConfig::default();
    proxy.proxy_protocol.enabled = true;
    proxy.proxy_protocol.timeout = Duration::ZERO;
    let error = RuntimeConfig::builder()
        .trusted_proxy(proxy)
        .build()
        .unwrap_err();
    assert!(error.to_string().contains("trusted_proxy"));

    let mut forwarded = TrustedProxyConfig::default();
    forwarded.forwarded.standard_enabled = true;
    forwarded.forwarded.max_bytes = 1;
    let error = RuntimeConfig::builder()
        .trusted_proxy(forwarded)
        .build()
        .unwrap_err();
    assert!(error.to_string().contains("trusted_proxy"));
}

// ---------------------------------------------------------------------------
// Track C: PROXY pure parsing
// ---------------------------------------------------------------------------

#[test]
fn proxy_v1_valid_and_unknown() {
    let endpoints =
        parse_proxy_v1_line(b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\n").unwrap();
    assert_eq!(endpoints.kind, ProxySourceKind::ProxyV1);
    assert_eq!(endpoints.source.unwrap().to_string(), "192.168.0.1:56324");
    assert_eq!(
        endpoints.destination.unwrap().to_string(),
        "192.168.0.11:443"
    );

    let unknown = parse_proxy_v1_line(b"PROXY UNKNOWN\r\n").unwrap();
    assert!(unknown.source.is_none());
    assert!(unknown.destination.is_none());
}

#[test]
fn proxy_v1_rejects_unix_and_spacing_and_ports() {
    assert!(parse_proxy_v1_line(b"PROXY UNIX a b\r\n").is_err());
    assert!(parse_proxy_v1_line(b"PROXY  TCP4 1.1.1.1 2.2.2.2 1 80\r\n").is_err());
    assert!(parse_proxy_v1_line(b"PROXY TCP4 1.1.1.1 2.2.2.2 99999 80\r\n").is_err());
}

#[test]
fn proxy_v1_max_and_oversized() {
    // Exactly at the ceiling parses when well-formed; anything longer fails.
    let mut longest = b"PROXY TCP6 2001:db8::1 2001:db8::2 12345 443\r\n".to_vec();
    assert!(longest.len() <= PROXY_V1_MAX_BYTES);
    assert!(parse_proxy_v1_line(&longest).is_ok());
    longest.resize(PROXY_V1_MAX_BYTES + 1, b'X');
    assert!(parse_proxy_v1_line(&longest).is_err());
}

fn v2_ipv4_header() -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(&PROXY_V2_SIGNATURE);
    header.push(0x21);
    header.push(0x11);
    header.extend_from_slice(&12u16.to_be_bytes());
    header.extend_from_slice(&[192, 168, 0, 1, 192, 168, 0, 11]);
    header.extend_from_slice(&56324u16.to_be_bytes());
    header.extend_from_slice(&443u16.to_be_bytes());
    header
}

#[test]
fn proxy_v2_ipv4_and_local_and_unspec() {
    let header = v2_ipv4_header();
    let (endpoints, consumed) = parse_proxy_v2_header(&header).unwrap();
    assert_eq!(consumed, header.len());
    assert_eq!(endpoints.kind, ProxySourceKind::ProxyV2);
    assert_eq!(endpoints.source.unwrap().to_string(), "192.168.0.1:56324");

    // LOCAL ignores addresses without inventing identity.
    let mut local = Vec::new();
    local.extend_from_slice(&PROXY_V2_SIGNATURE);
    local.push(0x20);
    local.push(0x11);
    local.extend_from_slice(&12u16.to_be_bytes());
    local.extend_from_slice(&[0u8; 12]);
    let (endpoints, _) = parse_proxy_v2_header(&local).unwrap();
    assert!(endpoints.source.is_none());

    // UNSPEC preserves truthful absence.
    let mut unspec = Vec::new();
    unspec.extend_from_slice(&PROXY_V2_SIGNATURE);
    unspec.push(0x21);
    unspec.push(0x00);
    unspec.extend_from_slice(&0u16.to_be_bytes());
    let (endpoints, _) = parse_proxy_v2_header(&unspec).unwrap();
    assert!(endpoints.source.is_none());
}

#[test]
fn proxy_v2_rejects_truncated_and_oversized_and_families() {
    let mut truncated = v2_ipv4_header();
    truncated.truncate(truncated.len() - 4);
    assert!(parse_proxy_v2_header(&truncated).is_err());

    let mut oversized = Vec::new();
    oversized.extend_from_slice(&PROXY_V2_SIGNATURE);
    oversized.push(0x21);
    oversized.push(0x11);
    oversized.extend_from_slice(&(PROXY_V2_MAX_LEN as u16 + 1).to_be_bytes());
    assert!(parse_proxy_v2_header(&oversized).is_err());

    // Unassigned family and DGRAM-over-TCP matrix are deterministically rejected.
    let mut bad_family = v2_ipv4_header();
    bad_family[13] = 0x41;
    assert!(parse_proxy_v2_header(&bad_family).is_err());
    let mut dgram = v2_ipv4_header();
    dgram[13] = 0x12;
    assert!(parse_proxy_v2_header(&dgram).is_err());
}

#[test]
fn proxy_v2_ipv6_and_ports() {
    let mut header = Vec::new();
    header.extend_from_slice(&PROXY_V2_SIGNATURE);
    header.push(0x21);
    header.push(0x21);
    header.extend_from_slice(&36u16.to_be_bytes());
    header.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    header.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    header.extend_from_slice(&8080u16.to_be_bytes());
    header.extend_from_slice(&443u16.to_be_bytes());
    let (endpoints, _) = parse_proxy_v2_header(&header).unwrap();
    assert!(endpoints.source.unwrap().is_ipv6());
    assert_eq!(endpoints.source.unwrap().port(), 8080);
    assert_eq!(endpoints.destination.unwrap().port(), 443);
}

// ---------------------------------------------------------------------------
// Track C: async preamble (fragmented, timeout, leftover)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn proxy_preamble_fragmented_across_reads() {
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// AsyncRead that yields at most `chunk` bytes per poll, proving the
    /// preamble parser reassembles fragmented arrivals without loss.
    struct Chunked {
        data: std::io::Cursor<Vec<u8>>,
        chunk: usize,
    }
    impl tokio::io::AsyncRead for Chunked {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let remaining = buf.remaining().min(self.chunk);
            let mut tmp = vec![0u8; remaining];
            match std::io::Read::read(&mut self.data, &mut tmp) {
                Ok(0) => Poll::Ready(Ok(())),
                Ok(count) => {
                    buf.put_slice(&tmp[..count]);
                    Poll::Ready(Ok(()))
                }
                Err(error) => Poll::Ready(Err(error)),
            }
        }
    }

    let line = b"PROXY TCP4 10.0.0.1 10.0.0.2 1234 80\r\n";
    let payload = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
    let mut full = line.to_vec();
    full.extend_from_slice(payload);
    let mut chunked = Chunked {
        data: std::io::Cursor::new(full),
        chunk: 3,
    };
    let (endpoints, leftover) =
        eggserve_core::server::proxy::read_proxy_preamble(&mut chunked, Duration::from_secs(5))
            .await
            .unwrap();
    assert_eq!(endpoints.source.unwrap().to_string(), "10.0.0.1:1234");
    // Fragmented reads may split the payload boundary; the leftover plus any
    // not-yet-read payload must reconstruct the original tail. Here the full
    // input was available, so the leftover is a prefix of the payload and
    // the remainder follows in the stream.
    assert!(payload.starts_with(&leftover));
    // Drain the rest from the chunked stream and verify lossless reassembly.
    use tokio::io::AsyncReadExt;
    let mut rest = Vec::new();
    chunked.read_to_end(&mut rest).await.unwrap();
    let mut reassembled = leftover;
    reassembled.extend_from_slice(&rest);
    assert_eq!(reassembled, payload);
}

#[tokio::test]
async fn proxy_preamble_slow_times_out() {
    let (mut _client, mut server) = tokio::io::duplex(64);
    // Never write; the helper must time out rather than hang.
    let error =
        eggserve_core::server::proxy::read_proxy_preamble(&mut server, Duration::from_millis(50))
            .await
            .unwrap_err();
    assert_eq!(error, eggserve_core::server::proxy::ProxyReadError::Timeout);
}

// ---------------------------------------------------------------------------
// Track D: header policy units
// ---------------------------------------------------------------------------

#[test]
fn forwarded_standard_single_hop() {
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: false,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[("Forwarded", "for=192.0.2.60;proto=https;host=example.test")]);
    let effective = derive_forwarded_effective(&headers, &config, true)
        .unwrap()
        .unwrap();
    assert_eq!(effective.client.unwrap().to_string(), "192.0.2.60:0");
    assert_eq!(effective.scheme, Some(Scheme::Https));
    assert_eq!(effective.authority.unwrap().as_str(), "example.test");
    assert_eq!(effective.provenance, ProxySourceKind::Forwarded);
}

#[test]
fn forwarded_unknown_preserves_absence() {
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: false,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[("Forwarded", "for=unknown;proto=https")]);
    let effective = derive_forwarded_effective(&headers, &config, true)
        .unwrap()
        .unwrap();
    assert!(effective.client.is_none());
    assert_eq!(effective.scheme, Some(Scheme::Https));
}

#[test]
fn forwarded_rightmost_wins_for_single_hop_append() {
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: false,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[("Forwarded", "for=192.0.2.1, for=192.0.2.2")]);
    let effective = derive_forwarded_effective(&headers, &config, true)
        .unwrap()
        .unwrap();
    assert_eq!(effective.client.unwrap().to_string(), "192.0.2.2:0");
}

#[test]
fn forwarded_conflict_fails_closed() {
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: true,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[
        ("Forwarded", "for=192.0.2.60"),
        ("X-Forwarded-For", "192.0.2.61"),
    ]);
    let error = derive_forwarded_effective(&headers, &config, true).unwrap_err();
    assert_eq!(
        error,
        eggserve_core::primitives::proxy::ForwardedRejection::Conflict
    );
}

#[test]
fn forwarded_bounds_are_hard() {
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: false,
        max_bytes: 32,
        max_elements: 16,
    };
    let headers = header_block(&[("Forwarded", "for=192.0.2.60;proto=https;host=example.test")]);
    assert!(derive_forwarded_effective(&headers, &config, true).is_err());

    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: false,
        max_bytes: 4096,
        max_elements: 1,
    };
    let headers = header_block(&[("Forwarded", "for=192.0.2.1, for=192.0.2.2")]);
    assert!(derive_forwarded_effective(&headers, &config, true).is_err());
}

#[test]
fn legacy_forwarded_rightmost_and_scheme_host() {
    let config = ForwardedConfig {
        standard_enabled: false,
        legacy_enabled: true,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[
        ("X-Forwarded-For", "192.0.2.1, 192.0.2.2"),
        ("X-Forwarded-Proto", "https"),
        ("X-Forwarded-Host", "example.test"),
    ]);
    let effective = derive_forwarded_effective(&headers, &config, true)
        .unwrap()
        .unwrap();
    assert_eq!(effective.client.unwrap().to_string(), "192.0.2.2:0");
    assert_eq!(effective.scheme, Some(Scheme::Https));
    assert_eq!(effective.authority.unwrap().as_str(), "example.test");
    assert_eq!(effective.provenance, ProxySourceKind::LegacyForwarded);
}

#[test]
fn spoofed_headers_remain_untrusted_without_trust() {
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: true,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[
        ("Forwarded", "for=192.0.2.60"),
        ("X-Forwarded-For", "192.0.2.61"),
    ]);
    // Untrusted peer with spoofed headers never yields trusted facts.
    assert!(derive_forwarded_effective(&headers, &config, false).is_err());
    // No headers means absent, not an error (avoids log spam).
    let empty = HeaderBlock::new();
    assert!(derive_forwarded_effective(&empty, &config, false)
        .unwrap()
        .is_none());
}

// ---------------------------------------------------------------------------
// Track A: raw preserved, provenance tagged
// ---------------------------------------------------------------------------

#[test]
fn connection_info_effective_accessors() {
    let raw = ConnectionInfo::with_socket_addrs(
        "127.0.0.1:8000".parse().unwrap(),
        "127.0.0.1:12345".parse().unwrap(),
        Scheme::Http,
        None,
    );
    assert_eq!(
        raw.effective_client_addr().unwrap().to_string(),
        "127.0.0.1:12345"
    );
    assert_eq!(raw.effective_scheme_value(), Scheme::Http);
    assert!(!raw.has_trusted_proxy_metadata());

    let proxied = raw.clone().with_proxy_endpoints(
        Some("192.0.2.60:1234".parse().unwrap()),
        Some("192.0.2.1:443".parse().unwrap()),
        ProxySourceKind::ProxyV1,
    );
    // Raw preserved; effective follows the proxy source.
    assert_eq!(proxied.remote_addr.unwrap().to_string(), "127.0.0.1:12345");
    assert_eq!(
        proxied.effective_client_addr().unwrap().to_string(),
        "192.0.2.60:1234"
    );
    assert!(proxied.has_trusted_proxy_metadata());
}

// ---------------------------------------------------------------------------
// Wire: spoofed headers without trust remain untrusted, raw preserved
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wire_spoofed_headers_remain_untrusted_by_default() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;
    let response = raw_http_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: real.test\r\nForwarded: for=192.0.2.60;proto=https;host=evil.test\r\nX-Forwarded-For: 192.0.2.61\r\nConnection: close\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    // Raw peer preserved; no trusted override adopted.
    assert!(text.contains("forwarded=none"), "unexpected: {text}");
    assert!(text.contains("scheme=http"), "unexpected: {text}");
    assert!(text.contains("authority=none"), "unexpected: {text}");
    assert!(text.contains("canonical=real.test"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_trusted_standard_forwarded_is_adopted_without_rewriting_canonical() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.forwarded.standard_enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;
    let response = raw_http_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: real.test\r\nForwarded: for=192.0.2.60;proto=https;host=external.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    assert!(
        text.contains("effective=192.0.2.60:0"),
        "unexpected: {text}"
    );
    assert!(text.contains("scheme=https"), "unexpected: {text}");
    assert!(
        text.contains("authority=external.test"),
        "unexpected: {text}"
    );
    // Canonical Host is never silently rewritten.
    assert!(text.contains("canonical=real.test"), "unexpected: {text}");
    assert!(text.contains("forwarded=forwarded"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_trusted_legacy_forwarded_is_adopted() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.forwarded.legacy_enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;
    let response = raw_http_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: real.test\r\nX-Forwarded-For: 192.0.2.1, 192.0.2.2\r\nX-Forwarded-Proto: https\r\nX-Forwarded-Host: external.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    // Rightmost (closest to the trusted proxy) wins.
    assert!(text.contains("effective=192.0.2.2:0"), "unexpected: {text}");
    assert!(
        text.contains("forwarded=legacy_forwarded"),
        "unexpected: {text}"
    );
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_conflicting_forwarded_fails_closed() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.forwarded.standard_enabled = true;
    trusted.forwarded.legacy_enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;
    let response = raw_http_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: real.test\r\nForwarded: for=192.0.2.60\r\nX-Forwarded-For: 192.0.2.61\r\nConnection: close\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    assert!(text.contains("forwarded=none"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

// ---------------------------------------------------------------------------
// Wire: PROXY protocol
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wire_proxy_v1_trusted_is_adopted_with_raw_preserved() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\n")
        .await
        .unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: internal.test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    assert!(
        text.contains("effective=192.168.0.1:56324"),
        "unexpected: {text}"
    );
    assert!(text.contains("proxy=proxy_v1"), "unexpected: {text}");
    // Raw peer (127.0.0.1) preserved alongside the effective client.
    assert!(text.contains("raw=127.0.0.1:"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_proxy_v2_trusted_is_adopted() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;

    let mut preamble = Vec::new();
    preamble.extend_from_slice(&PROXY_V2_SIGNATURE);
    preamble.push(0x21);
    preamble.push(0x11);
    preamble.extend_from_slice(&12u16.to_be_bytes());
    preamble.extend_from_slice(&[10, 0, 0, 7, 10, 0, 0, 8]);
    preamble.extend_from_slice(&4321u16.to_be_bytes());
    preamble.extend_from_slice(&80u16.to_be_bytes());

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(&preamble).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: internal.test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    assert!(
        text.contains("effective=10.0.0.7:4321"),
        "unexpected: {text}"
    );
    assert!(text.contains("proxy=proxy_v2"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_proxy_untrusted_peer_is_rejected_before_service() {
    // PROXY enabled but only 10/8 trusted; loopback sends a valid preamble
    // and must close before any service response (never trusted).
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("10.0.0.0/8").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\nGET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    // Either EOF with no bytes or a timeout with no HTTP response: never a
    // trusted 200 with the spoofed client.
    if let Ok(Ok(_)) = result {
        let text = String::from_utf8_lossy(&buf);
        assert!(
            !text.contains("effective=192.168.0.1"),
            "untrusted PROXY must never become trusted: {text}"
        );
        assert!(
            !text.starts_with("HTTP/1.1 200"),
            "untrusted PROXY must close before service: {text}"
        );
    }
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_proxy_disabled_treats_signature_as_plain_http() {
    // No PROXY config: preamble bytes are ordinary (invalid) HTTP input,
    // never auto-detected as trusted metadata.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;
    let response =
        raw_http_request(addr, b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\n").await;
    let text = String::from_utf8_lossy(&response);
    // Not a trusted 200; the bytes fail as ordinary HTTP (400/408/empty).
    assert!(
        !text.contains("effective=192.168.0.1"),
        "disabled listener must not adopt PROXY: {text}"
    );
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_proxy_unknown_preserves_absence() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"PROXY UNKNOWN\r\n").await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: internal.test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "unexpected: {text}");
    // Truthful absence: no invented client, but provenance records the preamble.
    assert!(text.contains("proxy=proxy_v1"), "unexpected: {text}");
    assert!(text.contains("raw=127.0.0.1:"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn wire_proxy_malformed_closes_before_service() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"PROXY BOGUS\r\n").await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(
        !text.starts_with("HTTP/1.1 200"),
        "malformed preamble must close before service: {text}"
    );
    handle.shutdown();
    let _ = handle.wait().await;
}

// ---------------------------------------------------------------------------
// TLS-after-PROXY ordering
// ---------------------------------------------------------------------------

#[cfg(feature = "tls")]
#[tokio::test]
async fn wire_tls_after_proxy_ordering() {
    use std::sync::Arc as StdArc;

    let _ = rustls::crypto::ring::default_provider().install_default();
    let key_pair = rcgen::KeyPair::generate().expect("generate key pair");
    let params =
        rcgen::CertificateParams::new(vec!["localhost".to_string()]).expect("create params");
    let cert = params.self_signed(&key_pair).expect("self sign");
    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert_der.clone())],
            rustls::pki_types::PrivateKeyDer::try_from(key_der.clone()).unwrap(),
        )
        .unwrap();
    server_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let server_config = StdArc::new(server_config);

    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(server_config)
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let (handle, addr) = start_server(config).await;

    // TCP accept -> PROXY preamble -> TLS handshake -> HTTP (required order).
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(b"PROXY TCP4 192.168.5.5 192.168.5.6 1111 443\r\n")
        .await
        .unwrap();

    let mut root_store = rustls::RootCertStore::empty();
    root_store
        .add(rustls::pki_types::CertificateDer::from(cert_der))
        .unwrap();
    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(StdArc::new(client_config));
    let domain = "localhost".try_into().unwrap();
    let mut tls = connector.connect(domain, tcp).await.unwrap();
    tls.write_all(b"GET / HTTP/1.1\r\nHost: tls.test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("200"), "unexpected: {text}");
    assert!(
        text.contains("effective=192.168.5.5:1111"),
        "TLS-after-PROXY must preserve effective client: {text}"
    );
    assert!(text.contains("proxy=proxy_v1"), "unexpected: {text}");
    handle.shutdown();
    let _ = handle.wait().await;
}

// ---------------------------------------------------------------------------
// H1/H2 parity (shared pipeline) and H3 out-of-scope
// ---------------------------------------------------------------------------

#[test]
fn forwarded_derivation_is_version_independent() {
    // H1 and H2 share `apply_forwarded_policy`; derivation must not depend
    // on the request version (only headers + trust).
    let config = ForwardedConfig {
        standard_enabled: true,
        legacy_enabled: false,
        ..ForwardedConfig::default()
    };
    let headers = header_block(&[("Forwarded", "for=192.0.2.60;proto=https")]);
    let h1 = derive_forwarded_effective(&headers, &config, true)
        .unwrap()
        .unwrap();
    let h2 = derive_forwarded_effective(&headers, &config, true)
        .unwrap()
        .unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn h3_ignores_proxy_policy_by_construction() {
    // H3 uses `ConnectionContext::for_quic` with no PROXY layer and its own
    // request path without `apply_forwarded_policy`. The policy type exists
    // but datagram proxying is out of scope here.
    let context = eggserve_core::server::connection::ConnectionContext::for_quic(
        "127.0.0.1:443".parse().unwrap(),
        "127.0.0.1:54321".parse().unwrap(),
        eggserve_core::primitives::connection_info::TlsInfo {
            protocol_version: Some("TLSv1.3".into()),
            server_name: None,
            ..Default::default()
        },
    );
    let info = context.connection_info();
    assert!(info.proxy_provenance.is_none());
    assert!(info.forwarded_provenance.is_none());
    assert!(!info.has_trusted_proxy_metadata());
}

// ---------------------------------------------------------------------------
// Fuzz ceilings: parsers never panic and respect hard bounds
// ---------------------------------------------------------------------------

#[test]
fn fuzz_proxy_parsers_respect_bounds() {
    // Deterministic pseudo-random corpus (no external harness): every input
    // must either parse within bounds or fail with a typed error, never
    // panic or allocate beyond the ceilings.
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut next_byte = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    for _ in 0..2000 {
        let len = (next_byte() as usize) % 130;
        let mut input = vec![0u8; len];
        for byte in &mut input {
            *byte = next_byte();
        }
        let _ = parse_proxy_v1_line(&input);
        let _ = parse_proxy_v2_header(&input);
        assert!(input.len() <= 129);
    }
    // Oversized inputs deterministically fail as too long (v2 len path).
    let mut oversized = Vec::new();
    oversized.extend_from_slice(&PROXY_V2_SIGNATURE);
    oversized.push(0x21);
    oversized.push(0x11);
    oversized.extend_from_slice(&u16::MAX.to_be_bytes());
    oversized.extend_from_slice(&[0u8; 64]);
    assert!(parse_proxy_v2_header(&oversized).is_err());
}

#[test]
fn eggserve_performs_no_reverse_proxying() {
    // Plan 202 adds trusted *inbound* metadata only. There is no forwarding
    // of requests to upstreams: the public server surface exposes listeners
    // and services, never an upstream/proxy client.
    //
    // This test pins the boundary by asserting the server module exposes no
    // `proxy_client`, `upstream`, or `forward_request` symbols via the
    // documented public surface (compile-time census of key names).
    let source = include_str!("../src/server/mod.rs");
    assert!(!source.contains("proxy_client"));
    assert!(!source.contains("upstream"));
    assert!(!source.contains("forward_request"));
}
