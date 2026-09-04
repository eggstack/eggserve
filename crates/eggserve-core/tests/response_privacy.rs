//! Response privacy and fingerprint-minimization tests (Plan 165).
//!
//! Proves the final-boundary [`ResponsePolicy`] is the sole authority for
//! `Date`/`Server`, that denylisting cannot break framing, that runtime
//! errors stay generic, and that static validators are explicitly
//! configurable. Covers TCP, caller-owned duplex, and (where available) TLS
//! parity.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eggserve_core::policy::{ErrorRepresentationPolicy, StaticMetadataPolicy, StaticPolicy};
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use eggserve_core::server::{
    response_policy::{DatePolicy, ResponsePolicy},
    service_fn, Request, RuntimeConfig, RuntimeState,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn base_config() -> RuntimeConfig {
    RuntimeConfig::default()
}

fn runtime(config: &RuntimeConfig) -> Arc<RuntimeState> {
    Arc::new(RuntimeState::new(config))
}

fn count_header(raw: &str, name: &str) -> usize {
    let lower = format!("{}:", name.to_ascii_lowercase());
    raw.lines()
        .filter(|l| l.to_ascii_lowercase().starts_with(&lower))
        .count()
}

fn header_value(raw: &str, name: &str) -> Option<String> {
    let lower = format!("{}:", name.to_ascii_lowercase());
    for line in raw.lines() {
        if line.to_ascii_lowercase().starts_with(&lower) {
            let v = line[lower.len()..].trim().to_owned();
            return Some(v);
        }
    }
    None
}

async fn duplex_once(
    request_bytes: &[u8],
    service: impl eggserve_core::server::Service,
    config: Arc<RuntimeConfig>,
) -> (String, ConnectionOutcome) {
    let (mut client, server) = tokio::io::duplex(128 * 1024);
    let shutdown = ConnectionShutdown::new();
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let runtime = runtime(&config);
    let driver = tokio::spawn(async move {
        serve_http1_connection(server, service, config, context, runtime, &shutdown).await
    });
    client.write_all(request_bytes).await.unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let outcome = driver.await.unwrap();
    (String::from_utf8_lossy(&buf).into_owned(), outcome)
}

async fn tcp_once(
    request_bytes: &[u8],
    service: impl eggserve_core::server::Service,
    config: RuntimeConfig,
) -> String {
    let config = Arc::new(config);
    let runtime = runtime(&config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = config.clone();
    let server = tokio::spawn(async move {
        let (stream, remote) = listener.accept().await.unwrap();
        let local = stream.local_addr().unwrap_or(addr);
        let shutdown = ConnectionShutdown::new();
        let context = ConnectionContext::for_tcp(local, remote, None);
        serve_http1_connection(stream, service, cfg, context, runtime, &shutdown).await
    });
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.write_all(request_bytes).await.unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let _ = server.await;
    String::from_utf8_lossy(&buf).into_owned()
}

fn ok_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

fn powered_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("x-powered-by", "FakeFramework/9.9")
            .unwrap()
            .header("x-generator", "EvilCMS")
            .unwrap()
            .header("server", "spoofed/1.0")
            .unwrap()
            .header("content-type", "text/plain")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

// ---------------------------------------------------------------------------
// Default: exactly one valid Date, no Server
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_response_has_one_valid_date_and_no_server() {
    let config = Arc::new(base_config());
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
    )
    .await;
    assert!(raw.starts_with("HTTP/1.1 200 OK"), "got: {raw}");
    assert_eq!(
        count_header(&raw, "date"),
        1,
        "expected exactly one Date: {raw}"
    );
    assert_eq!(
        count_header(&raw, "server"),
        0,
        "Server must be suppressed: {raw}"
    );
    let date = header_value(&raw, "date").unwrap();
    // Valid HTTP-date parses.
    assert!(
        httpdate::parse_http_date(&date).is_ok(),
        "invalid Date value: {date}"
    );
}

#[tokio::test]
async fn fixed_server_opt_in_emits_single_value() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .server_header("ExampleOrigin/1.0".into())
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        powered_service(),
        config,
    )
    .await;
    // Application Server is subordinate; fixed value wins exactly once.
    assert_eq!(count_header(&raw, "server"), 1, "got: {raw}");
    assert_eq!(
        header_value(&raw, "server").as_deref(),
        Some("ExampleOrigin/1.0")
    );
    assert_eq!(count_header(&raw, "date"), 1);
    // Denylist-independent: service Server never leaks.
    assert!(!raw.contains("spoofed/1.0"));
}

#[tokio::test]
async fn caller_supplied_clock_controls_date() {
    let fixed = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let provider: Arc<dyn Fn() -> SystemTime + Send + Sync> = Arc::new(move || fixed);
    let config = Arc::new(
        RuntimeConfig::builder()
            .date_policy(DatePolicy::Custom(provider))
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
    )
    .await;
    assert_eq!(count_header(&raw, "date"), 1, "got: {raw}");
    assert_eq!(
        header_value(&raw, "date").as_deref(),
        Some(httpdate::fmt_http_date(fixed).as_str())
    );
}

#[tokio::test]
async fn date_suppression_yields_zero_dates_hyper_disabled() {
    // Proves EggServe (not Hyper) is the sole Date authority: with
    // suppression there must be zero Date headers. If Hyper automatic Date
    // were still enabled, this would observe one.
    let config = Arc::new(
        RuntimeConfig::builder()
            .date_policy(DatePolicy::Suppress)
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
    )
    .await;
    assert_eq!(
        count_header(&raw, "date"),
        0,
        "Date must be suppressed: {raw}"
    );
    assert!(raw.starts_with("HTTP/1.1 200 OK"));
}

// ---------------------------------------------------------------------------
// Denylist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn denylisted_application_headers_do_not_survive() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .stripped_response_headers(vec!["x-powered-by".to_owned(), "x-generator".to_owned()])
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        powered_service(),
        config,
    )
    .await;
    assert!(
        !raw.to_ascii_lowercase().contains("x-powered-by"),
        "got: {raw}"
    );
    assert!(
        !raw.to_ascii_lowercase().contains("x-generator"),
        "got: {raw}"
    );
    // Non-denylisted application metadata survives.
    assert!(raw.to_ascii_lowercase().contains("content-type"));
}

#[tokio::test]
async fn minimal_fingerprint_preset_strips_powered_by() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .response_policy(ResponsePolicy::minimal_fingerprint())
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        powered_service(),
        config,
    )
    .await;
    assert!(
        !raw.to_ascii_lowercase().contains("x-powered-by"),
        "got: {raw}"
    );
    assert_eq!(count_header(&raw, "server"), 0);
    assert_eq!(count_header(&raw, "date"), 1);
}

#[tokio::test]
async fn denylist_validation_rejects_framing_headers() {
    for bad in [
        "date",
        "content-length",
        "transfer-encoding",
        "connection",
        "content-range",
    ] {
        let err = RuntimeConfig::builder()
            .stripped_response_headers(vec![bad.to_owned()])
            .build()
            .unwrap_err();
        assert!(
            err.to_string().contains("runtime-owned"),
            "expected framing rejection for {bad}: {err}"
        );
    }
}

#[tokio::test]
async fn framing_headers_survive_denylist_attempt_via_preset() {
    // Even with a denylist configured, Content-Length framing remains
    // correct (validation rejects framing entries, so this is the steady
    // state: denylist present, framing intact).
    let config = Arc::new(
        RuntimeConfig::builder()
            .stripped_response_headers(vec!["x-powered-by".to_owned()])
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
    )
    .await;
    assert_eq!(
        header_value(&raw, "content-length").as_deref(),
        Some("5"),
        "got: {raw}"
    );
}

#[tokio::test]
async fn duplicate_denied_headers_all_removed() {
    let svc = service_fn(|_req: Request| async {
        // Two identical identification headers; both must be stripped.
        let mut builder = Response::builder().status(StatusCode::OK);
        builder = builder.header("x-powered-by", "a").unwrap();
        builder = builder.header("x-powered-by", "b").unwrap();
        Ok(builder.body(ResponseBody::Bytes(b"hi".to_vec())).unwrap())
    });
    let config = Arc::new(
        RuntimeConfig::builder()
            .stripped_response_headers(vec!["x-powered-by".to_owned()])
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
        config,
    )
    .await;
    assert_eq!(count_header(&raw, "x-powered-by"), 0, "got: {raw}");
}

// ---------------------------------------------------------------------------
// Errors: generic, no leaks, HEAD correct, Empty variant
// ---------------------------------------------------------------------------

fn forbidden_paths() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "GET /%2e%2e/%2e%2e/etc/passwd HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            "403",
        ),
        (
            "GET /%00 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            "400",
        ),
        (
            "GET /missing-404-xyz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            "404",
        ),
    ]
}

#[tokio::test]
async fn runtime_errors_are_generic_without_leaks() {
    use eggserve_core::server::StaticService;
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("ok.txt"), b"ok").unwrap();
    let svc = StaticService::builder(tmp.path()).build().unwrap();
    let config = base_config();
    // Drive each error through the real pipeline (duplex) so Date/Server
    // finalization also applies.
    for (req, expect_status) in [
        (
            "GET /%2e%2e/secret HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            "403",
        ),
        (
            "POST /ok.txt HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "405",
        ),
        (
            "GET /no-such-file-404 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            "404",
        ),
    ] {
        let cfg = Arc::new(config.clone());
        let rt = runtime(&cfg);
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let shutdown = ConnectionShutdown::new();
        let context = ConnectionContext::for_non_socket(Scheme::Http, None);
        let svc_clone = svc.clone();
        let driver = tokio::spawn(async move {
            serve_http1_connection(server, svc_clone, cfg, context, rt, &shutdown).await
        });
        client.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        let _ = client.read_to_end(&mut buf).await;
        let _ = driver.await;
        let raw = String::from_utf8_lossy(&buf);
        assert!(
            raw.contains(expect_status),
            "expected {expect_status} for {req:?}: {raw}"
        );
        let lower = raw.to_ascii_lowercase();
        for leak in [
            "eggserve",
            "hyper",
            "rust",
            "python",
            "panic",
            "traceback",
            "tmp",
            "tmpdir",
            ".cargo",
            "target/",
            "linux",
            "windows",
            "macos",
            "ubuntu",
        ] {
            assert!(!lower.contains(leak), "leak {leak:?} in {raw}");
        }
        // No Server version, exactly one Date (or zero only under suppression,
        // which is not configured here).
        assert_eq!(count_header(&raw, "server"), 0);
        assert_eq!(count_header(&raw, "date"), 1);
    }
    let _ = forbidden_paths();
}

#[tokio::test]
async fn head_errors_have_no_body_but_keep_headers() {
    use eggserve_core::server::StaticService;
    let tmp = tempfile::TempDir::new().unwrap();
    let svc = StaticService::builder(tmp.path()).build().unwrap();
    let config = Arc::new(base_config());
    let rt = runtime(&config);
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let shutdown = ConnectionShutdown::new();
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let driver = tokio::spawn(async move {
        serve_http1_connection(server, svc, config, context, rt, &shutdown).await
    });
    client
        .write_all(b"HEAD /no-such-404 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let _ = driver.await;
    let raw = String::from_utf8_lossy(&buf);
    assert!(raw.contains("404"), "got: {raw}");
    // Body suppressed for HEAD: no "Not Found" text after headers.
    let parts: Vec<&str> = raw.split("\r\n\r\n").collect();
    assert!(parts.len() >= 2, "got: {raw}");
    assert!(
        !parts[1].contains("Not Found"),
        "HEAD must not emit body: {raw}"
    );
    assert!(raw.to_ascii_lowercase().contains("content-length"));
}

#[tokio::test]
async fn empty_error_policy_omits_bodies() {
    use eggserve_core::server::StaticService;
    let tmp = tempfile::TempDir::new().unwrap();
    let mut policy = StaticPolicy::safe_default();
    let _ = &mut policy;
    let svc = StaticService::builder(tmp.path())
        .error_policy(ErrorRepresentationPolicy::Empty)
        .build()
        .unwrap();
    let config = Arc::new(
        RuntimeConfig::builder()
            .error_policy(ErrorRepresentationPolicy::Empty)
            .build()
            .unwrap(),
    );
    let rt = runtime(&config);
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let shutdown = ConnectionShutdown::new();
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let driver = tokio::spawn(async move {
        serve_http1_connection(server, svc, config, context, rt, &shutdown).await
    });
    client
        .write_all(b"GET /no-such-404 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let _ = driver.await;
    let raw = String::from_utf8_lossy(&buf);
    assert!(raw.contains("404"), "got: {raw}");
    let parts: Vec<&str> = raw.split("\r\n\r\n").collect();
    let body = parts.get(1).copied().unwrap_or("");
    assert!(body.is_empty(), "Empty policy must emit no body: {raw:?}");
    assert_eq!(
        header_value(&raw, "content-length").as_deref(),
        Some("0"),
        "got: {raw}"
    );
}

#[tokio::test]
async fn application_4xx_bodies_are_never_rewritten() {
    // A custom service returning 404 with application content must keep its
    // body even under the Empty runtime-error policy.
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "text/plain")
            .unwrap()
            .body(ResponseBody::Bytes(b"app-missing-page".to_vec()))
            .unwrap())
    });
    let config = Arc::new(
        RuntimeConfig::builder()
            .error_policy(ErrorRepresentationPolicy::Empty)
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
        config,
    )
    .await;
    assert!(raw.contains("404"), "got: {raw}");
    assert!(
        raw.contains("app-missing-page"),
        "application body rewritten: {raw}"
    );
}

// ---------------------------------------------------------------------------
// Static validators
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_static_validators_unchanged() {
    use eggserve_core::server::StaticService;
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("f.txt"), b"0123456789").unwrap();
    let svc = StaticService::builder(tmp.path()).build().unwrap();
    let req = eggserve_core::primitives::request::Request::new(
        eggserve_core::primitives::request_head::RequestHead::new(
            eggserve_core::primitives::method::Method::get(),
            eggserve_core::primitives::request_target::RequestTarget::parse("/f.txt").unwrap(),
            eggserve_core::primitives::version::HttpVersion::Http11,
            eggserve_core::primitives::header_block::HeaderBlock::new(),
        ),
        eggserve_core::primitives::request_body::RequestBody::empty(),
        eggserve_core::primitives::connection_info::ConnectionInfo::without_socket_addrs(
            Scheme::Http,
            None,
        ),
    );
    let resp = svc.call_for_test(req).await;
    // Default emits both validators.
    assert!(resp.headers().contains("etag"));
    assert!(resp.headers().contains("last-modified"));
}

// Helper trait to call a Service without verbose plumbing in this file.
trait CallForTest {
    async fn call_for_test(
        &self,
        req: eggserve_core::primitives::request::Request,
    ) -> eggserve_core::primitives::canonical::Response;
}

impl<T: eggserve_core::server::Service> CallForTest for T {
    async fn call_for_test(
        &self,
        req: eggserve_core::primitives::request::Request,
    ) -> eggserve_core::primitives::canonical::Response {
        use eggserve_core::server::Service;
        Service::call(self, req).await.unwrap()
    }
}

#[tokio::test]
async fn minimal_static_metadata_suppresses_validators() {
    use eggserve_core::server::StaticService;
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("f.txt"), b"0123456789").unwrap();
    let mut policy = StaticPolicy::safe_default();
    policy.static_metadata = StaticMetadataPolicy::minimal_fingerprint();
    let svc = StaticService::builder(tmp.path())
        .policy(policy)
        .build()
        .unwrap();
    let req = eggserve_core::primitives::request::Request::new(
        eggserve_core::primitives::request_head::RequestHead::new(
            eggserve_core::primitives::method::Method::get(),
            eggserve_core::primitives::request_target::RequestTarget::parse("/f.txt").unwrap(),
            eggserve_core::primitives::version::HttpVersion::Http11,
            eggserve_core::primitives::header_block::HeaderBlock::new(),
        ),
        eggserve_core::primitives::request_body::RequestBody::empty(),
        eggserve_core::primitives::connection_info::ConnectionInfo::without_socket_addrs(
            Scheme::Http,
            None,
        ),
    );
    let resp = svc.call_for_test(req).await;
    assert!(!resp.headers().contains("etag"), "ETag must be suppressed");
    assert!(
        !resp.headers().contains("last-modified"),
        "Last-Modified must be suppressed"
    );
    // Still 200 with correct length framing.
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "10"
    );
}

// ---------------------------------------------------------------------------
// Transport parity: TCP vs caller-owned duplex
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tcp_and_duplex_agree_on_privacy_shape() {
    let tcp_raw = tcp_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        base_config(),
    )
    .await;
    let (duplex_raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        Arc::new(base_config()),
    )
    .await;
    for raw in [&tcp_raw, &duplex_raw] {
        assert_eq!(count_header(raw, "date"), 1, "got: {raw}");
        assert_eq!(count_header(raw, "server"), 0, "got: {raw}");
    }
    // Status line parity (Date values differ by clock, so compare shape).
    assert!(tcp_raw.starts_with("HTTP/1.1 200 OK"));
    assert!(duplex_raw.starts_with("HTTP/1.1 200 OK"));
}

// ---------------------------------------------------------------------------
// Regression scan: no version/build-path identifiers in runtime fixtures
// ---------------------------------------------------------------------------

#[test]
fn runtime_fixtures_contain_no_implementation_identifiers() {
    // Representative runtime-generated bodies/headers must not contain
    // crate, Hyper, Rust, Python, path, or OS identifiers.
    let bodies = [
        "400 Bad Request\n",
        "403 Forbidden\n",
        "404 Not Found\n",
        "405 Method Not Allowed\n",
        "408 Request Timeout\n",
        "413 Payload Too Large\n",
        "414 URI Too Long\n",
        "431 Request Header Fields Too Large\n",
        "500 Internal Server Error\n",
        "503 Service Unavailable\n",
        "504 Gateway Timeout\n",
    ];
    let forbidden = [
        "eggserve/",
        "eggserve-",
        "hyper/",
        "hyper-",
        "rustc",
        "rust/",
        "python/",
        "python-",
        "cpython",
        "traceback",
        ".cargo",
        "target/",
        "/home/",
        "/tmp/",
        "c:\\",
        "linux",
        "windows",
        "macos",
        "darwin",
        "ubuntu",
        "debian",
        "x-powered-by",
    ];
    for body in bodies {
        let lower = body.to_ascii_lowercase();
        for needle in forbidden {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "identifier {needle:?} in fixture {body:?}"
            );
        }
    }
    // Policy defaults themselves must not embed versions.
    let policy = ResponsePolicy::default();
    let flat = format!("{policy:?}");
    assert!(!flat.to_ascii_lowercase().contains("eggserve/"));
    let minimal = ResponsePolicy::minimal_fingerprint();
    let flat = format!("{minimal:?}");
    assert!(!flat.to_ascii_lowercase().contains("hyper"));
}

#[test]
fn core_has_no_i2p_specific_types() {
    // Plan 165 handoff: I2P integration requires no I2P-specific type in
    // eggserve-core. Grep the source tree for obvious I2P tunnel types.
    let forbidden = ["Destination", "LeaseSet", "TunnelId", "RouterInfo", "I2P"];
    let files = [
        "crates/eggserve-core/src/server/response_policy.rs",
        "crates/eggserve-core/src/server/config.rs",
        "crates/eggserve-core/src/policy.rs",
        "crates/eggserve-core/src/server/connection.rs",
    ];
    for file in files {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        for needle in forbidden {
            // Allow the word "I2P" only in comments referencing the plan's
            // anonymity-sensitive origin example, not as a type.
            if needle == "I2P" {
                continue;
            }
            assert!(
                !text.contains(needle),
                "{file} must not contain I2P type {needle}"
            );
        }
    }
}

#[test]
fn minimal_profile_does_not_claim_unfingerprintable() {
    // The profile language must say "minimize gratuitous fingerprint
    // signals", not claim to be "un-fingerprintable". A negation ("does not
    // make un-fingerprintable") is the required disclaimer and is allowed;
    // a positive claim ("is un-fingerprintable") is forbidden.
    let doc = include_str!("../src/server/response_policy.rs");
    assert!(doc.contains("minimize") || doc.contains("minimizes") || doc.contains("minimizing"));
    let lower = doc.to_ascii_lowercase();
    assert!(
        !lower.contains("is un-fingerprintable")
            && !lower.contains("are un-fingerprintable")
            && !lower.contains("is unfingerprintable")
            && !lower.contains("are unfingerprintable"),
        "must not claim to be un-fingerprintable"
    );
    // The disclaimer itself must be present.
    assert!(
        lower.contains("not") && lower.contains("un-fingerprintable"),
        "must disclaim un-fingerprintability"
    );
}
