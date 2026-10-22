//! Compatibility-vs-direct configuration projection parity (Plan 308).
//!
//! The compatibility [`RuntimeConfig`] carries the four direct-H1 ownership
//! knobs (`http1_request_target_mode`, `policy_ownership`,
//! `admission_ownership`, `runtime_rejection_presenter`) into the direct H1
//! driver instead of hardcoding them, so a compatibility consumer can reach
//! the same opt-ins a direct embedder can.
//!
//! This file holds
//!
//! 1. the explicit baseline capture of the default compatibility path
//!    (values captured before the change, at baseline `465b80c`), asserted
//!    against literal expected values; and
//! 2. the reachability proof for each knob.
//!
//! Defaults must not move: the baseline capture is the contract.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::request_target::RequestTargetForm;
use eggserve_core::server::config::{
    AdmissionOwner, AdmissionOwnership, H1PolicyOwnership, Http1RequestTargetMode, PolicyOwner,
    RuntimeErrorPresentation, RuntimeRejection, RuntimeRejectionKind, RuntimeRejectionPresenter,
};
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn count_header(raw: &str, name: &str) -> usize {
    let lower = format!("{}:", name.to_ascii_lowercase());
    raw.lines()
        .filter(|l| l.to_ascii_lowercase().starts_with(&lower))
        .count()
}

fn header_value(raw: &str, name: &str) -> Option<String> {
    let lower = format!("{}:", name.to_ascii_lowercase());
    raw.lines()
        .find(|l| l.to_ascii_lowercase().starts_with(&lower))
        .map(|l| l[lower.len()..].trim().to_owned())
}

fn status_line(raw: &str) -> String {
    raw.lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned()
}

fn runtime(config: &RuntimeConfig) -> Arc<RuntimeState> {
    Arc::new(RuntimeState::new(config))
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

async fn tcp_request(address: std::net::SocketAddr, wire: &[u8]) -> String {
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(wire).await.unwrap();
    let mut response = Vec::new();
    let mut byte = [0; 1];
    while !response.ends_with(b"\r\n\r\n") {
        client.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
    }
    String::from_utf8_lossy(&response).into_owned()
}

fn ok_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

/// Service that outlives the runtime handler deadline.
fn slow_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"slow".to_vec()))
            .unwrap())
    })
}

// ---------------------------------------------------------------------------
// A. Baseline capture (pre-change values, asserted literally)
// ---------------------------------------------------------------------------

/// Observable baseline of the default compatibility path.
///
/// Captured before the projection change: the compatibility projection
/// hardcoded the direct crate's defaults, so every value below is the
/// pre-change behavior and must stay identical afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BaselineCapture {
    /// Status of an absolute-form request target on the default path.
    absolute_form_status: String,
    /// Whether the default path dispatched an absolute-form request.
    absolute_form_dispatched: bool,
    /// `Date` header count on a successful default response.
    success_date_headers: usize,
    /// `Server` header count on a successful default response.
    success_server_headers: usize,
    /// Status of the second concurrent request under the default
    /// EggServe-owned service-call admission.
    admission_second_status: String,
    /// Whether the second concurrent request reached the service.
    admission_second_dispatched: bool,
}

/// The literal expected baseline.
fn expected_baseline() -> BaselineCapture {
    BaselineCapture {
        absolute_form_status: "400".to_owned(),
        absolute_form_dispatched: false,
        success_date_headers: 1,
        success_server_headers: 0,
        admission_second_status: "503".to_owned(),
        admission_second_dispatched: false,
    }
}

/// Observe the default compatibility path.
async fn observe_baseline() -> BaselineCapture {
    // 1. Absolute-form request target on the default path.
    let dispatched = Arc::new(AtomicUsize::new(0));
    let observed = dispatched.clone();
    let service = service_fn(move |_req: Request| {
        observed.fetch_add(1, Ordering::SeqCst);
        async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"hello".to_vec()))
                .unwrap())
        }
    });
    let config = Arc::new(RuntimeConfig::default());
    let (raw, _) = duplex_once(
        b"GET http://example.test:8080/a?b=1 HTTP/1.1\r\nHost: example.test:8080\r\nConnection: close\r\n\r\n",
        service,
        config,
    )
    .await;
    let absolute_form_status = status_line(&raw);
    let absolute_form_dispatched = dispatched.load(Ordering::SeqCst) > 0;

    // 2. Date/Server on a successful default response.
    let config = Arc::new(RuntimeConfig::default());
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
    )
    .await;
    let success_date_headers = count_header(&raw, "date");
    let success_server_headers = count_header(&raw, "server");

    // 3. Default admission behavior (EggServe-owned service-call admission).
    let (first, second, second_dispatched) = admission_probe(
        RuntimeConfig::builder()
            .max_in_flight_requests(1)
            .build()
            .unwrap(),
    )
    .await;
    assert_eq!(status_line(&first), "200", "first request must succeed");
    let _ = second;

    BaselineCapture {
        absolute_form_status,
        absolute_form_dispatched,
        success_date_headers,
        success_server_headers,
        admission_second_status: status_line(&second),
        admission_second_dispatched: second_dispatched,
    }
}

/// Drive two concurrent requests through one runtime.
///
/// The first request parks inside the service; the second arrives while the
/// single in-flight permit is taken.
async fn admission_probe(config: RuntimeConfig) -> (String, String, bool) {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let entered_service = entered.clone();
    let release_service = release.clone();
    let call_count = calls.clone();
    let service = service_fn(move |_req: Request| {
        let index = call_count.fetch_add(1, Ordering::SeqCst);
        let entered = entered_service.clone();
        let release = release_service.clone();
        async move {
            if index == 0 {
                entered.notify_one();
                release.notified().await;
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }
    });
    let mut config = config;
    config.bind = "127.0.0.1:0".parse().unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let address = handle.local_addr();

    let first = tokio::spawn(async move {
        tcp_request(
            address,
            b"GET /first HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let second = tcp_request(
        address,
        b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    release.notify_one();
    let first = tokio::time::timeout(Duration::from_secs(5), first)
        .await
        .unwrap()
        .unwrap();
    let dispatched = calls.load(Ordering::SeqCst);
    handle.shutdown();
    let _ = handle.wait().await;
    (first, second, dispatched > 1)
}

#[tokio::test]
async fn baseline_capture_is_unchanged_by_the_projection_change() {
    assert_eq!(observe_baseline().await, expected_baseline());
}

// ---------------------------------------------------------------------------
// F1. Default compatibility path keeps the baseline behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_compatibility_path_rejects_absolute_form_and_owns_metadata() {
    let dispatched = Arc::new(AtomicUsize::new(0));
    let observed = dispatched.clone();
    let service = service_fn(move |_req: Request| {
        observed.fetch_add(1, Ordering::SeqCst);
        async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"hello".to_vec()))
                .unwrap())
        }
    });
    let config = Arc::new(RuntimeConfig::default());
    let (raw, _) = duplex_once(
        b"GET http://example.test:8080/a?b=1 HTTP/1.1\r\nHost: example.test:8080\r\nConnection: close\r\n\r\n",
        service,
        config.clone(),
    )
    .await;
    assert_eq!(status_line(&raw), "400", "got: {raw}");
    assert_eq!(
        dispatched.load(Ordering::SeqCst),
        0,
        "absolute-form must not reach the service: {raw}"
    );

    // The default compatibility configuration carries the direct defaults.
    assert_eq!(
        config.http1_request_target_mode,
        Http1RequestTargetMode::OriginOnly
    );
    assert_eq!(config.policy_ownership, H1PolicyOwnership::default());
    assert_eq!(config.admission_ownership, AdmissionOwnership::default());
    assert!(config.runtime_rejection_presenter.is_none());

    // Runtime-owned metadata on a successful response.
    let config = Arc::new(RuntimeConfig::default());
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
    )
    .await;
    assert_eq!(count_header(&raw, "date"), 1, "got: {raw}");
    assert_eq!(count_header(&raw, "server"), 0, "got: {raw}");
    let date = header_value(&raw, "date").unwrap();
    assert!(
        httpdate::parse_http_date(&date).is_ok(),
        "invalid Date value: {date}"
    );
}

// ---------------------------------------------------------------------------
// F2. `http1_request_target_mode` is reachable through the compatibility path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn origin_or_absolute_mode_dispatches_absolute_form_through_the_compat_h1_path() {
    type SeenTargets = Arc<std::sync::Mutex<Vec<(RequestTargetForm, String, Option<String>)>>>;
    let seen: SeenTargets = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = seen.clone();
    let service = service_fn(move |req: Request| {
        let observed = observed.clone();
        async move {
            let target = req.head().target();
            observed.lock().unwrap().push((
                target.form(),
                target.path().to_owned(),
                target.query().map(str::to_owned),
            ));
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"absolute".to_vec()))
                .unwrap())
        }
    });
    let config = Arc::new(
        RuntimeConfig::builder()
            .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
            .build()
            .unwrap(),
    );
    assert_eq!(
        config.http1_request_target_mode,
        Http1RequestTargetMode::OriginOrAbsolute
    );
    let (raw, _) = duplex_once(
        b"GET http://example.test:8080/a?b=1 HTTP/1.1\r\nHost: example.test:8080\r\nConnection: close\r\n\r\n",
        service,
        config,
    )
    .await;
    assert_eq!(status_line(&raw), "200", "got: {raw}");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "expected exactly one dispatch: {seen:?}");
    assert_eq!(seen[0].0, RequestTargetForm::Absolute);
    assert_eq!(seen[0].1, "/a");
    assert_eq!(seen[0].2.as_deref(), Some("b=1"));
}

#[tokio::test]
async fn origin_or_absolute_mode_still_validates_the_absolute_target() {
    // Parity with the direct path: a malformed/mismatched absolute-form
    // target is rejected before service dispatch.
    let dispatched = Arc::new(AtomicUsize::new(0));
    let observed = dispatched.clone();
    let service = service_fn(move |_req: Request| {
        observed.fetch_add(1, Ordering::SeqCst);
        async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }
    });
    let config = Arc::new(
        RuntimeConfig::builder()
            .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET http://example.test/a HTTP/1.1\r\nHost: other.test\r\nConnection: close\r\n\r\n",
        service,
        config,
    )
    .await;
    assert_eq!(status_line(&raw), "400", "got: {raw}");
    assert_eq!(dispatched.load(Ordering::SeqCst), 0, "got: {raw}");
}

// ---------------------------------------------------------------------------
// F3. `policy_ownership` is reachable through the compatibility path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn external_handler_deadline_is_honored_through_the_compatibility_path() {
    // EggServe-owned handler deadline: the runtime rejects the slow call.
    let config = Arc::new(
        RuntimeConfig::builder()
            .handler_timeout(Duration::from_millis(50))
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        slow_service(),
        config,
    )
    .await;
    assert_eq!(status_line(&raw), "504", "got: {raw}");

    // External handler deadline: the caller's own deadline governs and the
    // successful response still carries runtime-owned metadata.
    let config = Arc::new(
        RuntimeConfig::builder()
            .handler_timeout(Duration::from_millis(50))
            .policy_ownership(H1PolicyOwnership {
                handler_deadline: PolicyOwner::External,
                ..H1PolicyOwnership::default()
            })
            .build()
            .unwrap(),
    );
    assert_eq!(
        config.policy_ownership.handler_deadline,
        PolicyOwner::External
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        slow_service(),
        config,
    )
    .await;
    assert_eq!(status_line(&raw), "200", "got: {raw}");
    assert_eq!(count_header(&raw, "date"), 1, "got: {raw}");
    assert_eq!(count_header(&raw, "server"), 0, "got: {raw}");
}

#[tokio::test]
async fn runtime_error_metadata_stays_runtime_owned_under_external_policy_ownership() {
    let failing = service_fn(|_req: Request| async {
        Err(eggserve_core::server::ServiceError::rejected(
            503,
            "service unavailable",
        ))
    });
    let config = Arc::new(
        RuntimeConfig::builder()
            .policy_ownership(H1PolicyOwnership {
                handler_deadline: PolicyOwner::External,
                request_body_deadline: PolicyOwner::External,
                keep_alive_idle_deadline: PolicyOwner::External,
                response_write_progress_deadline: PolicyOwner::External,
                global_request_body_ceiling: PolicyOwner::External,
                request_target_ceiling: PolicyOwner::External,
            })
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        failing,
        config,
    )
    .await;
    assert_eq!(status_line(&raw), "503", "got: {raw}");
    // Runtime-owned metadata on a runtime error response.
    assert_eq!(count_header(&raw, "date"), 1, "got: {raw}");
    assert_eq!(count_header(&raw, "server"), 0, "got: {raw}");
}

// ---------------------------------------------------------------------------
// F4. `admission_ownership` is reachable through the compatibility path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn external_service_admission_is_honored_through_the_compatibility_path() {
    let (first, second, dispatched) = admission_probe(
        RuntimeConfig::builder()
            .max_in_flight_requests(1)
            .admission_ownership(AdmissionOwnership {
                service_calls: AdmissionOwner::External,
                ..AdmissionOwnership::default()
            })
            .build()
            .unwrap(),
    )
    .await;
    assert_eq!(status_line(&first), "200", "first: {first}");
    assert_eq!(
        status_line(&second),
        "200",
        "external admission must not gate the second request: {second}"
    );
    assert!(dispatched, "both requests must reach the service");
}

#[tokio::test]
async fn eggserve_owned_admission_still_gates_the_compatibility_path() {
    let (first, second, dispatched) = admission_probe(
        RuntimeConfig::builder()
            .max_in_flight_requests(1)
            .build()
            .unwrap(),
    )
    .await;
    assert_eq!(status_line(&first), "200", "first: {first}");
    assert_eq!(
        status_line(&second),
        "503",
        "default admission must stay EggServe-owned: {second}"
    );
    assert!(!dispatched, "the gated request must not reach the service");
}

// ---------------------------------------------------------------------------
// F5. `runtime_rejection_presenter` is reachable through the compatibility path
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RecordingPresenter {
    kinds: std::sync::Mutex<Vec<RuntimeRejectionKind>>,
}

impl RuntimeRejectionPresenter for RecordingPresenter {
    fn present(&self, rejection: &RuntimeRejection) -> Option<RuntimeErrorPresentation> {
        self.kinds.lock().unwrap().push(rejection.kind());
        let mut headers = HeaderBlock::new();
        headers.push_str("x-presented", "yes").unwrap();
        Some(RuntimeErrorPresentation {
            headers,
            body: b"presented".to_vec(),
        })
    }
}

#[tokio::test]
async fn runtime_rejection_presenter_is_invoked_through_the_compatibility_path() {
    let presenter = Arc::new(RecordingPresenter {
        kinds: std::sync::Mutex::new(Vec::new()),
    });
    let installed = presenter.clone();
    let config = Arc::new(
        RuntimeConfig::builder()
            .handler_timeout(Duration::from_millis(50))
            .runtime_rejection_presenter(installed)
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        slow_service(),
        config,
    )
    .await;
    // The runtime keeps status and framing authority.
    assert_eq!(status_line(&raw), "504", "got: {raw}");
    assert!(
        raw.contains("presented"),
        "presenter body must be used: {raw}"
    );
    assert_eq!(
        header_value(&raw, "x-presented").as_deref(),
        Some("yes"),
        "got: {raw}"
    );
    // Runtime-owned metadata and framing survive the presenter.
    assert_eq!(count_header(&raw, "date"), 1, "got: {raw}");
    assert_eq!(count_header(&raw, "server"), 0, "got: {raw}");
    assert_eq!(
        header_value(&raw, "content-length").as_deref(),
        Some("9"),
        "got: {raw}"
    );
    let kinds = presenter.kinds.lock().unwrap().clone();
    assert_eq!(kinds, vec![RuntimeRejectionKind::HandlerTimeout]);
}

#[tokio::test]
async fn runtime_rejection_presenter_is_not_invoked_without_configuration() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .handler_timeout(Duration::from_millis(50))
            .build()
            .unwrap(),
    );
    let (raw, _) = duplex_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        slow_service(),
        config,
    )
    .await;
    assert_eq!(status_line(&raw), "504", "got: {raw}");
    assert!(!raw.contains("presented"), "got: {raw}");
    assert_eq!(count_header(&raw, "date"), 1, "got: {raw}");
}

// ---------------------------------------------------------------------------
// F2 (continued). The compatibility H2 path stays unaffected
// ---------------------------------------------------------------------------

#[cfg(feature = "http2")]
mod h2_unaffected {
    use std::sync::Arc;

    use bytes::Bytes;
    use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
    use eggserve_core::primitives::connection_info::Scheme;
    use eggserve_core::server::config::Http1RequestTargetMode;
    use eggserve_core::server::connection::{
        serve_http_connection, ConnectionContext, ConnectionShutdown,
    };
    use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState};
    use http_body_util::Full;
    use hyper::client::conn::http2;
    use hyper::Request as HyperRequest;
    use hyper_util::rt::{TokioExecutor, TokioIo};

    /// Drive one HTTP/2 request through the compatibility H2 path.
    async fn h2_status(config: Arc<RuntimeConfig>, uri: &str) -> (u16, String) {
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = seen.clone();
        let service = service_fn(move |req: Request| {
            let observed = observed.clone();
            async move {
                observed
                    .lock()
                    .unwrap()
                    .push(req.head().target().path().to_owned());
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"h2".to_vec()))
                    .unwrap())
            }
        });
        let (client, server) = tokio::io::duplex(64 * 1024);
        let shutdown = ConnectionShutdown::new();
        let context = ConnectionContext::for_non_socket(Scheme::Http, None);
        let runtime = Arc::new(RuntimeState::new(&config));
        let driver = tokio::spawn(async move {
            serve_http_connection(server, service, config, context, runtime, &shutdown).await
        });
        let (mut sender, connection) =
            http2::handshake::<_, _, Full<Bytes>>(TokioExecutor::new(), TokioIo::new(client))
                .await
                .unwrap();
        let connection_task = tokio::spawn(connection);
        sender.ready().await.unwrap();
        let request = HyperRequest::builder()
            .method("GET")
            .uri(uri)
            .header("host", "example.test")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let status = match sender.send_request(request).await {
            Ok(response) => response.status().as_u16(),
            Err(_) => 0,
        };
        drop(sender);
        let _ = connection_task.await;
        let _ = driver.await;
        let seen = seen.lock().unwrap().clone();
        (status, seen.join(","))
    }

    #[tokio::test]
    async fn h2_path_is_unaffected_by_the_h1_request_target_mode() {
        let config = Arc::new(
            RuntimeConfig::builder()
                .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
                .build()
                .unwrap(),
        );
        // Ordinary H2 request: unaffected by the H1-only opt-in.
        let (status, seen) = h2_status(config.clone(), "http://example.test/a").await;
        assert_eq!(status, 200, "H2 path must keep working");
        assert_eq!(seen, "/a");
        // The H2 path keeps its own scheme validation: a conflicting scheme is
        // rejected regardless of the H1 request-target mode.
        let (status, _) = h2_status(config, "https://example.test/a").await;
        assert_eq!(status, 400, "H2 scheme validation must stay active");
    }
}
