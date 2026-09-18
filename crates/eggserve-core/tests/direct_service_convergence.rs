//! Direct service/request convergence fixture (Plan 217).
//!
//! Proves the converged contract: one downstream type implementing only
//! `eggserve_server::Service` drives both the direct H1 driver
//! (`eggserve_server::connection::serve_http1_connection`) and the
//! compatibility H1/H2 driver
//! (`eggserve_core::server::connection::serve_http_connection`, H2-gated).
//! Canonical request/response/context types are owned by
//! `eggserve-primitives`; `eggserve-core` re-exports them (no second
//! envelope, no second error taxonomy, no second tunnel state machine).
//!
//! H2 wire mechanics stay compatibility-owned (explicit transport glue);
//! dispatch uses the same canonical service contract, including tunnel
//! intent/acceptance via `Service::call_with_tunnel`.

use std::sync::Arc;

use eggserve_primitives::{
    canonical::{Response, ResponseBody, StatusCode},
    connection_info::Scheme,
};
use eggserve_server::{service_fn, Service};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Type identity: core and direct imports name the identical types.
// ---------------------------------------------------------------------------

#[test]
fn core_and_direct_request_types_are_identical() {
    // If these were nominally distinct duplicates, the assignments below
    // would fail to compile. They compile because the core modules are
    // facades over the direct authorities.
    fn takes_direct(_: eggserve_primitives::request::Request) {}
    fn takes_core(_: eggserve_core::primitives::request::Request) {}

    // Same for context, head, body, lifecycle, response, and errors.
    fn ctx_direct(_: eggserve_primitives::request_context::RequestContext) {}
    fn ctx_core(_: eggserve_core::primitives::request_context::RequestContext) {}
    fn head_direct(_: eggserve_primitives::request_head::RequestHead) {}
    fn head_core(_: eggserve_core::primitives::request_head::RequestHead) {}
    fn body_direct(_: eggserve_primitives::request_body::RequestBody) {}
    fn body_core(_: eggserve_core::primitives::request_body::RequestBody) {}
    fn resp_direct(_: eggserve_primitives::canonical::Response) {}
    fn resp_core(_: eggserve_core::primitives::canonical::Response) {}

    // Service errors share one taxonomy (status + message + panic/timeout).
    let direct_err = eggserve_server::ServiceError::rejected(429, "busy");
    let core_err: eggserve_core::server::ServiceError = direct_err;
    assert_eq!(core_err.status_code().as_u16(), 429);

    // A service written against the direct contract satisfies the
    // compatibility bound without adaptation (single trait via re-export).
    fn assert_core_service<S: eggserve_core::server::Service>(_: &S) {}
    let svc = service_fn(|_req: eggserve_server::Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Empty)
            .unwrap())
    });
    assert_core_service(&svc);
    fn assert_direct_service<S: Service>(_: &S) {}
    assert_direct_service(&svc);

    let _ = (
        takes_direct,
        takes_core,
        ctx_direct,
        ctx_core,
        head_direct,
        head_core,
        body_direct,
        body_core,
        resp_direct,
        resp_core,
    );
}

// ---------------------------------------------------------------------------
// One downstream service, direct H1 driver.
// ---------------------------------------------------------------------------

fn echo_service() -> impl Service {
    service_fn(|req: eggserve_server::Request| async move {
        let path = req.head().target().path().to_string();
        let body = format!("direct:{path}");
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap())
    })
}

#[tokio::test]
async fn direct_service_drives_direct_h1() {
    let config = Arc::new(eggserve_server::RuntimeConfig::default());
    let state = Arc::new(eggserve_server::RuntimeState::new(&config));
    let shutdown = eggserve_server::connection::ConnectionShutdown::new();
    let context =
        eggserve_server::connection::ConnectionContext::for_non_socket(Scheme::Http, None);

    let (client, server) = tokio::io::duplex(64 * 1024);
    let serve = eggserve_server::connection::serve_http1_connection(
        server,
        echo_service(),
        config,
        context,
        state,
        &shutdown,
    );
    let mut client = client;
    client
        .write_all(b"GET /hello HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    // Drive the server concurrently with the client read to avoid duplex
    // deadlock (both halves are bounded).
    let (outcome, _) = tokio::join!(serve, async move {
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
        assert!(text.ends_with("direct:/hello"), "got: {text}");
    });
    let _ = outcome;
}

// ---------------------------------------------------------------------------
// Same service value, compatibility H1 driver (single contract, no adapter).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn direct_service_drives_compatibility_h1() {
    // `eggserve_core::server::Service` IS `eggserve_server::Service`
    // (re-export); the same closure value drives the compatibility driver
    // without translation. Request/response types are identical via facades.
    let service = echo_service();
    // Prove the bound statically: the direct service satisfies the
    // compatibility driver bound.
    fn assert_compat<S: eggserve_core::server::Service>(_: &S) {}
    assert_compat(&service);

    let config = Arc::new(eggserve_core::server::RuntimeConfig::default());
    let state = Arc::new(eggserve_core::server::RuntimeState::new(&config));
    let shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let context = eggserve_core::server::connection::ConnectionContext::for_non_socket(
        eggserve_core::primitives::connection_info::Scheme::Http,
        None,
    );

    let (client, server) = tokio::io::duplex(64 * 1024);
    let serve = eggserve_core::server::connection::serve_http1_connection(
        server, service, config, context, state, &shutdown,
    );
    let mut client = client;
    client
        .write_all(b"GET /world HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let (outcome, _) = tokio::join!(serve, async move {
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
        assert!(text.ends_with("direct:/world"), "got: {text}");
    });
    let _ = outcome;
}

// ---------------------------------------------------------------------------
// Same service value, compatibility H1/H2 auto driver (H2 transport glue,
// same service contract). Requires the `http2` feature for
// `serve_http_connection`.
// ---------------------------------------------------------------------------

#[cfg(feature = "http2")]
#[tokio::test]
async fn direct_service_drives_compatibility_h2_path() {
    let service = echo_service();
    fn assert_compat<S: eggserve_core::server::Service>(_: &S) {}
    assert_compat(&service);

    let config = Arc::new(eggserve_core::server::RuntimeConfig::default());
    let state = Arc::new(eggserve_core::server::RuntimeState::new(&config));
    let shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let context = eggserve_core::server::connection::ConnectionContext::for_non_socket(
        eggserve_core::primitives::connection_info::Scheme::Http,
        None,
    );

    // Auto classifier: H1 bytes take the H1 path through the shared
    // canonical pipeline; the point is the same `Service` value (with
    // `call_with_tunnel`) drives the compatibility transport, not a second
    // application model. A real H2 prior-knowledge exchange is covered by
    // the existing H2 suites; here we prove contract convergence.
    let (client, server) = tokio::io::duplex(64 * 1024);
    let serve = eggserve_core::server::connection::serve_http_connection(
        server, service, config, context, state, &shutdown,
    );
    let mut client = client;
    client
        .write_all(b"GET /h2path HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let (outcome, _) = tokio::join!(serve, async move {
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
        assert!(text.ends_with("direct:/h2path"), "got: {text}");
    });
    let _ = outcome;
}
