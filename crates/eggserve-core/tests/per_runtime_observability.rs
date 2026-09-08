//! Per-runtime observability context tests (Plan 181 Track F).
//!
//! Proves that two runtimes in one process can own independent sinks,
//! counters, sink-failure accounting, and correlation-ID sequences, while
//! default construction keeps the historical process-global behavior.

use std::sync::{Arc, Mutex};

use eggserve_core::ops::{CompositeLogSink, Event, LogSink, OpsContext, OpsSnapshot, Severity};
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Shared recording sink: clones observe the same event log.
#[derive(Debug, Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<String>>>,
}

impl RecordingSink {
    fn lines(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl LogSink for RecordingSink {
    fn emit(&self, event: &Event) {
        let mut line = format!("{} {}", event.event, event.message);
        if let Some(cid) = event.connection_id {
            line.push_str(&format!(" conn={cid}"));
        }
        self.events.lock().unwrap().push(line);
    }

    fn flush(&self) {}
}

/// A sink that always fails, for failure-accounting isolation tests.
struct PanicSink;

impl LogSink for PanicSink {
    fn emit(&self, _event: &Event) {
        panic!("intentional sink failure for Plan 181 isolation test");
    }

    fn flush(&self) {}
}

fn ok_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

fn http_context() -> ConnectionContext {
    ConnectionContext::for_non_socket(Scheme::Http, None)
}

async fn drive_once(
    request_bytes: &[u8],
    service: impl eggserve_core::server::Service,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
) -> (Vec<u8>, ConnectionOutcome) {
    let (mut client, server) = tokio::io::duplex(128 * 1024);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            service,
            config,
            http_context(),
            runtime_state,
            &shutdown,
        )
        .await
    });
    client.write_all(request_bytes).await.unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let outcome = driver.await.unwrap();
    (buf, outcome)
}

fn snapshot(state: &RuntimeState) -> OpsSnapshot {
    state.ops_snapshot()
}

/// F1: events, counters, and correlation IDs stay per-runtime.
///
/// Runtime A serves a body-policy rejection (POST with a body against a
/// Reject service); runtime B serves a clean GET. A's rejection events and
/// counters must not leak into B, and both runtimes number their first
/// connection `conn=1` from independent ID sequences.
#[tokio::test]
async fn two_runtime_contexts_are_isolated() {
    let config = Arc::new(RuntimeConfig::default());

    let rec_a = RecordingSink::default();
    let rec_b = RecordingSink::default();
    let ops_a = OpsContext::with_sinks(vec![Box::new(rec_a.clone())]);
    let ops_b = OpsContext::with_sinks(vec![Box::new(rec_b.clone())]);
    let state_a = Arc::new(RuntimeState::with_ops(&config, ops_a).unwrap());
    let state_b = Arc::new(RuntimeState::with_ops(&config, ops_b).unwrap());

    // Runtime A: rejected POST (Reject policy is the `service_fn` default
    // with the zero body ceiling).
    let (buf_a, _) = drive_once(
        b"POST /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        ok_service(),
        config.clone(),
        state_a.clone(),
    )
    .await;
    assert!(
        String::from_utf8_lossy(&buf_a).starts_with("HTTP/1.1 413"),
        "expected 413 for rejected body, got: {}",
        String::from_utf8_lossy(&buf_a)
    );

    // Runtime B must be untouched by A's rejection.
    assert_eq!(rec_b.len(), 0, "B's sink must not observe A's traffic");
    let snap_b = snapshot(&state_b);
    assert_eq!(snap_b.body_rejections, 0);
    assert_eq!(snap_b.connections_accepted, 0);

    // A's rejection is accounted locally.
    let snap_a = snapshot(&state_a);
    assert_eq!(snap_a.body_rejections, 1);
    let lines_a = rec_a.lines();
    assert!(
        lines_a.iter().any(|l| l.contains("body_policy_rejection")),
        "A must log the rejection locally, got: {lines_a:?}"
    );

    // Runtime B: clean GET. Its first connection is `conn=1`, the same ID
    // A's connection used — coherent per context, not process-global.
    let (buf_b, _) = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config.clone(),
        state_b.clone(),
    )
    .await;
    assert!(
        String::from_utf8_lossy(&buf_b).starts_with("HTTP/1.1 200"),
        "expected 200, got: {}",
        String::from_utf8_lossy(&buf_b)
    );
    let lines_b = rec_b.lines();
    assert!(
        lines_b.iter().any(|l| l.contains("conn=1")),
        "B's first connection must be conn=1 in its own sequence, got: {lines_b:?}"
    );
    assert!(
        !lines_b.iter().any(|l| l.contains("rejection")),
        "B must not observe A's rejection, got: {lines_b:?}"
    );
    // A saw nothing of B's connection.
    assert_eq!(
        rec_a.len(),
        lines_a.len(),
        "A's sink must not grow from B's traffic"
    );
}

/// F1: a failing sink is contained and counted in its own context only.
///
/// The composite keeps delivering to the healthy sibling (Plan 178) while
/// `dropped_log_events` lands in the owning runtime's counters.
#[tokio::test]
async fn failing_sink_in_one_runtime_does_not_affect_other() {
    let config = Arc::new(RuntimeConfig::default());

    let rec_a = RecordingSink::default();
    let rec_b = RecordingSink::default();
    let ops_a = OpsContext::with_sinks(vec![Box::new(PanicSink), Box::new(rec_a.clone())]);
    let ops_b = OpsContext::with_sinks(vec![Box::new(rec_b.clone())]);
    let state_a = Arc::new(RuntimeState::with_ops(&config, ops_a).unwrap());
    let state_b = Arc::new(RuntimeState::with_ops(&config, ops_b).unwrap());

    let (buf, _) = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config.clone(),
        state_a.clone(),
    )
    .await;
    assert!(
        String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"),
        "response must succeed despite the failing sink"
    );

    // Healthy sibling still received events; failures counted locally.
    assert!(
        !rec_a.lines().is_empty(),
        "healthy sibling sink must still receive events"
    );
    let snap_a = snapshot(&state_a);
    assert!(
        snap_a.dropped_log_events >= 1,
        "A must count its own sink failures, got {}",
        snap_a.dropped_log_events
    );

    // Runtime B is fully unaffected.
    assert_eq!(rec_b.len(), 0);
    let snap_b = snapshot(&state_b);
    assert_eq!(snap_b.dropped_log_events, 0);
}

/// F1 (unit-level): `CompositeLogSink::with_failure_counters` routes
/// contained failures to the supplied counters instead of the global ones.
#[test]
fn composite_with_failure_counters_counts_locally() {
    use eggserve_core::ops::OpsCounters;

    let counters = Arc::new(OpsCounters::new());
    let composite =
        CompositeLogSink::with_failure_counters(vec![Box::new(PanicSink)], counters.clone());
    composite.emit(&Event::new(
        Severity::Info,
        eggserve_core::ops::EventKind::ProcessStarting,
        "test",
    ));
    assert_eq!(
        counters.snapshot().dropped_log_events,
        1,
        "explicit counters must observe the contained failure"
    );
}

/// F2: default construction keeps working with the global compat path.
#[tokio::test]
async fn default_construction_uses_global_compat_path() {
    // Caller-owned driver with the compatibility constructor.
    let config = Arc::new(RuntimeConfig::default());
    let runtime = Arc::new(RuntimeState::new(&config));
    let (buf, outcome) = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config.clone(),
        runtime.clone(),
    )
    .await;
    assert!(
        String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"),
        "default runtime must serve, got: {}",
        String::from_utf8_lossy(&buf)
    );
    assert_eq!(outcome, ConnectionOutcome::Normal);
    // Snapshot access works on the default state too.
    let _ = runtime.ops_snapshot();

    // Full TCP server with no explicit context.
    let server = Server::builder()
        .runtime(RuntimeConfig::default())
        .build()
        .unwrap();
    let handle = server.start_with_service(ok_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    assert!(
        String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"),
        "default server must serve, got: {}",
        String::from_utf8_lossy(&buf)
    );
    let snap = handle.ops_snapshot();
    assert_eq!(snap.connections_accepted, 1);
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// F3: two TCP servers run concurrently in one process with independent
/// observability.
#[tokio::test]
async fn two_servers_run_concurrently_with_independent_contexts() {
    let rec_a = RecordingSink::default();
    let rec_b = RecordingSink::default();
    let ops_a = OpsContext::with_sinks(vec![Box::new(rec_a.clone())]);
    let ops_b = OpsContext::with_sinks(vec![Box::new(rec_b.clone())]);

    let server_a = Server::builder()
        .runtime(RuntimeConfig::default())
        .bind("127.0.0.1:0".parse().unwrap())
        .ops_context(ops_a)
        .build()
        .unwrap();
    let server_b = Server::builder()
        .runtime(RuntimeConfig::default())
        .bind("127.0.0.1:0".parse().unwrap())
        .ops_context(ops_b)
        .build()
        .unwrap();
    let handle_a = server_a.start_with_service(ok_service()).await.unwrap();
    let handle_b = server_b.start_with_service(ok_service()).await.unwrap();
    handle_a.ready().await.unwrap();
    handle_b.ready().await.unwrap();

    async fn get(addr: std::net::SocketAddr) -> Vec<u8> {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        buf
    }

    // Concurrent traffic to both servers in the same process.
    let (buf_a, buf_b) = tokio::join!(get(handle_a.local_addr()), get(handle_b.local_addr()));
    assert!(String::from_utf8_lossy(&buf_a).starts_with("HTTP/1.1 200"));
    assert!(String::from_utf8_lossy(&buf_b).starts_with("HTTP/1.1 200"));

    // Per-server admission counters are independent (not merely sequential).
    let snap_a = handle_a.ops_snapshot();
    let snap_b = handle_b.ops_snapshot();
    assert_eq!(snap_a.connections_accepted, 1, "server A: {snap_a:?}");
    assert_eq!(snap_b.connections_accepted, 1, "server B: {snap_b:?}");

    // Event streams are disjoint: each sink saw exactly its own server.
    for (rec, name) in [(&rec_a, "A"), (&rec_b, "B")] {
        let lines = rec.lines();
        assert!(
            lines.iter().any(|l| l.contains("connection_accepted")),
            "server {name} must log its own accept, got: {lines:?}"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.contains("connection_accepted"))
                .count(),
            1,
            "server {name} must not observe the other server, got: {lines:?}"
        );
    }

    handle_a.shutdown();
    handle_b.shutdown();
    handle_a.wait().await.unwrap();
    handle_b.wait().await.unwrap();
}
