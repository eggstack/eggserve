//! Plan 179 Track E: configuration authority conformance.
//!
//! Proves each shared runtime constraint has identical semantics through the
//! supported construction paths without duplicating the matrix per frontend:
//! a shared case table is applied to `Limits::validate`,
//! `RuntimeConfigBuilder::build`, `try_from_serve_config`, and
//! `RuntimeConfig::validate` / runtime boundaries.

use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::limits::Limits;
use eggserve_core::server::{
    try_from_serve_config, ConnectionContext, ConnectionOutcome, ConnectionShutdown, RuntimeConfig,
    RuntimeState,
};

fn serve_with_limits(limits: Limits) -> Result<RuntimeConfig, String> {
    let serve = ServeConfig {
        limits,
        ..Default::default()
    };
    try_from_serve_config(&serve).map_err(|e| e.to_string())
}

#[test]
fn defaults_match_between_limits_bridge_and_runtime() {
    let limits = Limits::default();
    let runtime = RuntimeConfig::default();
    assert_eq!(limits.max_connections, runtime.max_connections);
    assert_eq!(limits.max_file_streams, runtime.max_file_streams);
    // `Limits::max_request_body_bytes` is `pub(crate)`; default 0 is covered
    // by unit tests and the bridge projection below. Integration checks the
    // public runtime default directly.
    assert_eq!(0, runtime.max_request_body_bytes);
    assert_eq!(limits.header_read_timeout, runtime.header_read_timeout);
    assert_eq!(limits.tls_handshake_timeout, runtime.tls_handshake_timeout);
    assert_eq!(
        limits.connection_total_timeout,
        runtime.connection_total_timeout
    );
    assert_eq!(limits.handler_timeout, runtime.handler_timeout);
    assert_eq!(limits.body_read_timeout, runtime.body_read_timeout);
    assert_eq!(
        limits.graceful_shutdown_timeout,
        runtime.graceful_shutdown_timeout
    );
    assert_eq!(limits.stream_chunk_size, runtime.stream_chunk_size);
    assert_eq!(limits.max_buf_size, runtime.max_buf_size);
    assert_eq!(limits.max_headers, runtime.max_headers);
    assert_eq!(limits.max_header_bytes, runtime.max_header_bytes);
    assert_eq!(
        limits.max_request_target_bytes,
        runtime.max_request_target_bytes
    );
    assert_eq!(
        limits.max_in_flight_requests,
        runtime.max_in_flight_requests
    );
    assert_eq!(
        limits.keep_alive_idle_timeout,
        runtime.keep_alive_idle_timeout
    );
    assert_eq!(
        limits.max_requests_per_connection,
        runtime.max_requests_per_connection
    );
    assert_eq!(
        limits.response_write_timeout,
        runtime.response_write_timeout
    );

    // Builder with no overrides must equal Default.
    let built = RuntimeConfig::builder().build().unwrap();
    assert_eq!(built.max_connections, runtime.max_connections);
    assert_eq!(built.max_buf_size, runtime.max_buf_size);

    // Default ServeConfig bridge must round-trip exactly.
    let bridged = try_from_serve_config(&ServeConfig::default()).unwrap();
    assert_eq!(bridged.max_connections, runtime.max_connections);
    assert_eq!(bridged.max_buf_size, runtime.max_buf_size);
}

#[test]
fn authoritative_defaults_match_kernel() {
    use eggserve_core::limits as lim;
    assert_eq!(lim::DEFAULT_MAX_CONNECTIONS, 64);
    assert_eq!(lim::DEFAULT_MAX_FILE_STREAMS, 32);
    assert_eq!(lim::DEFAULT_MAX_REQUEST_BODY_BYTES, 0);
    assert_eq!(lim::DEFAULT_HEADER_READ_TIMEOUT, Duration::from_secs(10));
    assert_eq!(
        lim::DEFAULT_CONNECTION_TOTAL_TIMEOUT,
        Duration::from_secs(60)
    );
    assert_eq!(lim::DEFAULT_HANDLER_TIMEOUT, Duration::from_secs(30));
    assert_eq!(lim::DEFAULT_BODY_READ_TIMEOUT, Duration::from_secs(30));
    assert_eq!(
        lim::DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT,
        Duration::from_secs(10)
    );
    assert_eq!(
        lim::DEFAULT_KEEP_ALIVE_IDLE_TIMEOUT,
        Duration::from_secs(60)
    );
    assert_eq!(lim::DEFAULT_RESPONSE_WRITE_TIMEOUT, Duration::from_secs(30));
    assert_eq!(lim::DEFAULT_STREAM_CHUNK_SIZE, 8192);
    assert_eq!(lim::DEFAULT_MAX_BUF_SIZE, 64 * 1024);
    assert_eq!(lim::DEFAULT_MAX_HEADERS, 100);
    assert_eq!(lim::DEFAULT_MAX_HEADER_BYTES, 32 * 1024);
    assert_eq!(lim::DEFAULT_MAX_REQUEST_TARGET_BYTES, 8192);
    assert_eq!(lim::DEFAULT_MAX_IN_FLIGHT_REQUESTS, 64);
}

/// Shared invalid cases: (name, limits mutation, builder mutation, expected field).
///
/// Each case must fail identically through `Limits::validate`,
/// `RuntimeConfigBuilder::build`, `try_from_serve_config`, and
/// hand-constructed `RuntimeConfig::validate`.
#[allow(clippy::type_complexity)]
fn invalid_cases() -> Vec<(
    &'static str,
    Box<dyn Fn(&mut Limits)>,
    Box<
        dyn Fn(
            eggserve_core::server::RuntimeConfigBuilder,
        ) -> eggserve_core::server::RuntimeConfigBuilder,
    >,
    &'static str,
)> {
    let max_permits = tokio::sync::Semaphore::MAX_PERMITS;
    vec![
        (
            "zero connections",
            Box::new(|l: &mut Limits| l.max_connections = 0),
            Box::new(|b| b.max_connections(0)),
            "max_connections",
        ),
        (
            "connection overflow",
            Box::new(move |l: &mut Limits| l.max_connections = max_permits + 1),
            Box::new(move |b| b.max_connections(max_permits + 1)),
            "max_connections",
        ),
        (
            "zero file streams",
            Box::new(|l: &mut Limits| l.max_file_streams = 0),
            Box::new(|b| b.max_file_streams(0)),
            "max_file_streams",
        ),
        (
            "zero in-flight",
            Box::new(|l: &mut Limits| l.max_in_flight_requests = 0),
            Box::new(|b| b.max_in_flight_requests(0)),
            "max_in_flight_requests",
        ),
        (
            "in-flight overflow",
            Box::new(move |l: &mut Limits| l.max_in_flight_requests = max_permits + 1),
            Box::new(move |b| b.max_in_flight_requests(max_permits + 1)),
            "max_in_flight_requests",
        ),
        (
            "zero header timeout",
            Box::new(|l: &mut Limits| l.header_read_timeout = Duration::ZERO),
            Box::new(|b| b.header_read_timeout(Duration::ZERO)),
            "header_read_timeout",
        ),
        (
            "header wider than total",
            Box::new(|l: &mut Limits| {
                l.header_read_timeout = Duration::from_secs(60);
                l.connection_total_timeout = Duration::from_secs(30);
            }),
            Box::new(|b| {
                b.header_read_timeout(Duration::from_secs(60))
                    .connection_total_timeout(Duration::from_secs(30))
            }),
            "header_read_timeout",
        ),
        (
            "handler wider than total",
            Box::new(|l: &mut Limits| {
                l.handler_timeout = Duration::from_secs(60);
                l.connection_total_timeout = Duration::from_secs(30);
            }),
            Box::new(|b| {
                b.handler_timeout(Duration::from_secs(60))
                    .connection_total_timeout(Duration::from_secs(30))
            }),
            "handler_timeout",
        ),
        (
            "body wider than total",
            Box::new(|l: &mut Limits| {
                l.body_read_timeout = Duration::from_secs(60);
                l.connection_total_timeout = Duration::from_secs(30);
            }),
            Box::new(|b| {
                b.body_read_timeout(Duration::from_secs(60))
                    .connection_total_timeout(Duration::from_secs(30))
            }),
            "body_read_timeout",
        ),
        (
            "buf below minimum",
            Box::new(|l: &mut Limits| {
                l.max_buf_size = eggserve_core::limits::MIN_MAX_BUF_SIZE - 1;
            }),
            Box::new(|b| b.max_buf_size(eggserve_core::limits::MIN_MAX_BUF_SIZE - 1)),
            "max_buf_size",
        ),
        (
            "buf above maximum",
            Box::new(|l: &mut Limits| {
                l.max_buf_size = eggserve_core::limits::MAX_MAX_BUF_SIZE + 1;
            }),
            Box::new(|b| b.max_buf_size(eggserve_core::limits::MAX_MAX_BUF_SIZE + 1)),
            "max_buf_size",
        ),
        (
            "zero max headers",
            Box::new(|l: &mut Limits| l.max_headers = 0),
            Box::new(|b| b.max_headers(0)),
            "max_headers",
        ),
        (
            "header-bytes below minimum",
            Box::new(|l: &mut Limits| {
                l.max_header_bytes = eggserve_core::limits::MIN_MAX_HEADER_BYTES - 1;
            }),
            Box::new(|b| b.max_header_bytes(eggserve_core::limits::MIN_MAX_HEADER_BYTES - 1)),
            "max_header_bytes",
        ),
        (
            "target below minimum",
            Box::new(|l: &mut Limits| {
                l.max_request_target_bytes =
                    eggserve_core::limits::MIN_MAX_REQUEST_TARGET_BYTES - 1;
            }),
            Box::new(|b| {
                b.max_request_target_bytes(eggserve_core::limits::MIN_MAX_REQUEST_TARGET_BYTES - 1)
            }),
            "max_request_target_bytes",
        ),
        (
            "zero max-requests-per-connection",
            Box::new(|l: &mut Limits| l.max_requests_per_connection = Some(0)),
            Box::new(|b| b.max_requests_per_connection(Some(0))),
            "max_requests_per_connection",
        ),
        (
            "zero keep-alive idle",
            Box::new(|l: &mut Limits| l.keep_alive_idle_timeout = Duration::ZERO),
            Box::new(|b| b.keep_alive_idle_timeout(Duration::ZERO)),
            "keep_alive_idle_timeout",
        ),
        (
            "zero response-write",
            Box::new(|l: &mut Limits| l.response_write_timeout = Duration::ZERO),
            Box::new(|b| b.response_write_timeout(Duration::ZERO)),
            "response_write_timeout",
        ),
        (
            "zero handler",
            Box::new(|l: &mut Limits| l.handler_timeout = Duration::ZERO),
            Box::new(|b| b.handler_timeout(Duration::ZERO)),
            "handler_timeout",
        ),
        (
            "zero body",
            Box::new(|l: &mut Limits| l.body_read_timeout = Duration::ZERO),
            Box::new(|b| b.body_read_timeout(Duration::ZERO)),
            "body_read_timeout",
        ),
    ]
}

#[test]
fn shared_invalid_cases_fail_identically_on_every_path() {
    for (name, mutate_limits, mutate_builder, field) in invalid_cases() {
        // Limits::validate
        let mut limits = Limits::default();
        mutate_limits(&mut limits);
        let errs = limits.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.field == field),
            "{name}: Limits::validate missing {field}: {errs:?}"
        );

        // RuntimeConfigBuilder::build
        let builder = RuntimeConfig::builder();
        let builder = mutate_builder(builder);
        let err = builder.build().unwrap_err().to_string();
        assert!(
            err.contains(field),
            "{name}: builder missing {field}: {err}"
        );

        // try_from_serve_config
        let mut limits2 = Limits::default();
        mutate_limits(&mut limits2);
        let err = serve_with_limits(limits2).unwrap_err();
        assert!(
            err.contains(field),
            "{name}: try_from_serve_config missing {field}: {err}"
        );

        // Hand-constructed RuntimeConfig::validate: project the mutated Limits
        // into a hand-built config via struct update (avoids reassign lint).
        let mut probe = Limits::default();
        mutate_limits(&mut probe);
        let hand = RuntimeConfig {
            max_connections: probe.max_connections,
            max_file_streams: probe.max_file_streams,
            max_request_body_bytes: RuntimeConfig::default().max_request_body_bytes,
            header_read_timeout: probe.header_read_timeout,
            tls_handshake_timeout: probe.tls_handshake_timeout,
            connection_total_timeout: probe.connection_total_timeout,
            handler_timeout: probe.handler_timeout,
            body_read_timeout: probe.body_read_timeout,
            graceful_shutdown_timeout: probe.graceful_shutdown_timeout,
            stream_chunk_size: probe.stream_chunk_size,
            max_buf_size: probe.max_buf_size,
            max_headers: probe.max_headers,
            max_header_bytes: probe.max_header_bytes,
            max_request_target_bytes: probe.max_request_target_bytes,
            max_in_flight_requests: probe.max_in_flight_requests,
            keep_alive_idle_timeout: probe.keep_alive_idle_timeout,
            max_requests_per_connection: probe.max_requests_per_connection,
            response_write_timeout: probe.response_write_timeout,
            ..RuntimeConfig::default()
        };
        let err = hand.validate().unwrap_err().to_string();
        assert!(
            err.contains(field),
            "{name}: RuntimeConfig::validate missing {field}: {err}"
        );
    }
}

#[test]
fn request_body_ceiling_is_enforced_on_runtime_paths() {
    // `Limits::max_request_body_bytes` is `pub(crate)`; its overflow is pinned
    // by unit tests in `limits.rs`. Here we pin the public runtime paths.
    let err = RuntimeConfig::builder()
        .max_request_body_bytes(u64::MAX)
        .build()
        .unwrap_err()
        .to_string();
    assert!(err.contains("max_request_body_bytes"));

    let hand = RuntimeConfig {
        max_request_body_bytes: u64::MAX,
        ..RuntimeConfig::default()
    };
    assert!(hand
        .validate()
        .unwrap_err()
        .to_string()
        .contains("max_request_body_bytes"));

    // Maximum accepted value round-trips through the builder.
    let ok = RuntimeConfig::builder()
        .max_request_body_bytes(eggserve_core::limits::MAX_REQUEST_BODY_BYTES)
        .build()
        .unwrap();
    assert_eq!(
        ok.max_request_body_bytes,
        eggserve_core::limits::MAX_REQUEST_BODY_BYTES
    );
}

#[test]
fn parser_boundaries_are_accepted() {
    use eggserve_core::limits as lim;
    for size in [
        lim::MIN_MAX_BUF_SIZE,
        lim::DEFAULT_MAX_BUF_SIZE,
        lim::MAX_MAX_BUF_SIZE,
    ] {
        let mut limits = Limits::default();
        limits.max_buf_size = size;
        assert!(limits.validate().is_ok(), "buf {size}");
        let _ = RuntimeConfig::builder().max_buf_size(size).build().unwrap();
    }
    for n in [1, lim::DEFAULT_MAX_HEADERS, lim::MAX_MAX_HEADERS] {
        let mut limits = Limits::default();
        limits.max_headers = n;
        assert!(limits.validate().is_ok(), "headers {n}");
    }
    for b in [lim::MIN_MAX_HEADER_BYTES, lim::MAX_MAX_HEADER_BYTES] {
        let mut limits = Limits::default();
        limits.max_header_bytes = b;
        assert!(limits.validate().is_ok(), "header_bytes {b}");
    }
    for t in [
        lim::MIN_MAX_REQUEST_TARGET_BYTES,
        lim::MAX_MAX_REQUEST_TARGET_BYTES,
    ] {
        let mut limits = Limits::default();
        limits.max_request_target_bytes = t;
        assert!(limits.validate().is_ok(), "target {t}");
    }
}

#[test]
fn valid_non_default_round_trips_exactly() {
    // Note: `Limits::max_request_body_bytes` is `pub(crate)`; non-default
    // body ceilings are pinned via `RuntimeConfigBuilder` in
    // `request_body_ceiling_is_enforced_on_runtime_paths` and unit tests.
    // Here the bridge preserves the default 0 ceiling plus all public fields.
    // `Limits` cannot be struct-literal-built externally (private body field),
    // so mutate defaults.
    let mut limits = Limits::default();
    limits.max_connections = 99;
    limits.max_file_streams = 77;
    limits.header_read_timeout = Duration::from_secs(5);
    limits.tls_handshake_timeout = Duration::from_secs(7);
    limits.connection_total_timeout = Duration::from_secs(120);
    limits.handler_timeout = Duration::from_secs(42);
    limits.body_read_timeout = Duration::from_secs(99);
    limits.graceful_shutdown_timeout = Duration::from_secs(11);
    limits.stream_chunk_size = 4096;
    limits.max_buf_size = 16384;
    limits.max_headers = 50;
    limits.max_header_bytes = 4096;
    limits.max_request_target_bytes = 2048;
    limits.max_in_flight_requests = 16;
    limits.keep_alive_idle_timeout = Duration::from_secs(25);
    limits.max_requests_per_connection = Some(100);
    limits.response_write_timeout = Duration::from_secs(12);
    assert!(limits.validate().is_ok());
    let serve = ServeConfig {
        limits: limits.clone(),
        ..Default::default()
    };
    let runtime = try_from_serve_config(&serve).unwrap();
    assert_eq!(runtime.max_connections, 99);
    assert_eq!(runtime.max_file_streams, 77);
    assert_eq!(runtime.max_request_body_bytes, 0);
    assert_eq!(runtime.header_read_timeout, Duration::from_secs(5));
    assert_eq!(runtime.tls_handshake_timeout, Duration::from_secs(7));
    assert_eq!(runtime.connection_total_timeout, Duration::from_secs(120));
    assert_eq!(runtime.handler_timeout, Duration::from_secs(42));
    assert_eq!(runtime.body_read_timeout, Duration::from_secs(99));
    assert_eq!(runtime.graceful_shutdown_timeout, Duration::from_secs(11));
    assert_eq!(runtime.stream_chunk_size, 4096);
    assert_eq!(runtime.max_buf_size, 16384);
    assert_eq!(runtime.max_headers, 50);
    assert_eq!(runtime.max_header_bytes, 4096);
    assert_eq!(runtime.max_request_target_bytes, 2048);
    assert_eq!(runtime.max_in_flight_requests, 16);
    assert_eq!(runtime.keep_alive_idle_timeout, Duration::from_secs(25));
    assert_eq!(runtime.max_requests_per_connection, Some(100));
    assert_eq!(runtime.response_write_timeout, Duration::from_secs(12));
    // Full-config validation must accept the projection.
    runtime.validate().unwrap();
}

#[test]
fn static_only_limits_stay_outside_runtime() {
    // Listing budgets are service concerns: they validate in Limits but have
    // no counterpart in RuntimeConfig.
    let mut limits = Limits::default();
    limits.max_listing_entries = 0;
    let errs = limits.validate().unwrap_err();
    assert!(errs.iter().any(|e| e.field == "max_listing_entries"));

    // A valid RuntimeConfig does not carry listing fields at all (compile-time
    // separation); the default runtime still validates.
    RuntimeConfig::default().validate().unwrap();
}

#[test]
fn hand_constructed_invalid_is_rejected_at_runtime_boundaries() {
    // ServerBuilder rejects hand-built invalid configs instead of panicking.
    let bad = RuntimeConfig {
        max_connections: 0,
        ..RuntimeConfig::default()
    };
    assert!(bad.validate().is_err());
    let err = match eggserve_core::server::Server::builder()
        .runtime(bad)
        .build()
    {
        Ok(_) => panic!("expected ServerBuilder to reject max_connections=0"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("max_connections"));

    // RuntimeState::try_new returns the error; new() panics with context.
    let bad2 = RuntimeConfig {
        max_file_streams: 0,
        ..RuntimeConfig::default()
    };
    assert!(RuntimeState::try_new(&bad2).is_err());
    let panicked =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| RuntimeState::new(&bad2)));
    assert!(panicked.is_err());

    // Invalid response policy is also rejected at the boundary.
    let bad3 = RuntimeConfig {
        response_policy: eggserve_core::server::ResponsePolicy {
            server_identification: Some("bad\r\nvalue".into()),
            ..Default::default()
        },
        ..RuntimeConfig::default()
    };
    assert!(bad3.validate().is_err());
}

#[tokio::test]
async fn caller_owned_entry_rejects_invalid_config_without_panic() {
    use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
    use eggserve_core::server::{service_fn, Request};
    use std::sync::Arc;

    let bad = Arc::new(RuntimeConfig {
        max_buf_size: 1,
        ..RuntimeConfig::default()
    });
    // try_new would already fail; serve entry must not panic either.
    assert!(RuntimeState::try_new(&bad).is_err() || bad.validate().is_err());
    let good_state = Arc::new(RuntimeState::new(&RuntimeConfig::default()));
    let shutdown = ConnectionShutdown::new();
    let context = ConnectionContext::for_non_socket(
        eggserve_core::primitives::connection_info::Scheme::Http,
        None,
    );
    let (client, server) = tokio::io::duplex(1024);
    drop(client);
    let outcome = eggserve_core::server::serve_http1_connection(
        server,
        service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"hi".to_vec()))
                .unwrap())
        }),
        bad,
        context,
        good_state,
        &shutdown,
    )
    .await;
    assert_eq!(outcome, ConnectionOutcome::Internal);
}
