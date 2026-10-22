//! Runtime-owned `Alt-Svc` advertisement equivalence (Plan 307).
//!
//! Proves that the compatibility H2/H3 path and the experimental H3
//! adapter advertise byte-identical `Alt-Svc` values, that the whole
//! decision matrix (H3 enabled/disabled x advertisement on/off x
//! denylisted/not) is enforced on the wire, and that an application can
//! never override the runtime advertisement.
//!
//! The advertisement policy itself (value format, window, denylist
//! handling) is owned by `eggserve_primitives::canonical`; this suite
//! proves both callers converge on that single authority.

#![cfg(all(feature = "http2", feature = "http3"))]

use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::server::{service_fn, Http3Config, Request, RuntimeConfig, Server};
use eggserve_primitives::canonical::{ALT_SVC_HEADER_NAME, ALT_SVC_MAX_AGE};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http2;
use hyper::Request as HyperRequest;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tempfile::TempDir;

/// A QUIC identity for the H3 endpoint. The H3 listener is experimental,
/// but the advertisement policy is observable on the H2 compatibility
/// path, so the endpoint must actually exist for the runtime to own the
/// `alt-svc` field.
fn write_identity(dir: &TempDir) -> (PathBuf, PathBuf) {
    let key_pair = rcgen::KeyPair::generate().expect("generate key pair");
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .expect("certificate parameters");
    let certificate = params.self_signed(&key_pair).expect("self-sign");
    let cert_path = dir.path().join("localhost.crt");
    let key_path = dir.path().join("localhost.key");
    std::fs::write(&cert_path, certificate.pem()).expect("write certificate");
    std::fs::write(&key_path, key_pair.serialize_pem()).expect("write key");
    (cert_path, key_path)
}

fn serve_config(root: &TempDir) -> Arc<ServeConfig> {
    Arc::new(ServeConfig {
        root: root.path().to_path_buf(),
        ..ServeConfig::default()
    })
}

/// Run one H2 request through the compatibility path and collect every
/// `alt-svc` value observed on the wire.
async fn observe_alt_svc(
    config: RuntimeConfig,
    identity: &(PathBuf, PathBuf),
    application_alt_svc: Option<&'static str>,
) -> (Vec<String>, u16) {
    let root = TempDir::new().unwrap();
    let service = service_fn(move |_request: Request| async move {
        let mut builder = Response::builder().status(StatusCode::OK);
        if let Some(value) = application_alt_svc {
            builder = builder.header(ALT_SVC_HEADER_NAME, value).unwrap();
        }
        Ok(builder.body(ResponseBody::Bytes(b"ok".to_vec())).unwrap())
    });
    let server = Server::builder()
        .runtime(config)
        .serve_config(serve_config(&root))
        .http3_identity(&identity.0, &identity.1)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let port = handle.local_addr().port();

    let stream = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let (mut sender, connection) =
        http2::handshake::<_, _, Full<Bytes>>(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
    let connection_task = tokio::spawn(connection);
    let request = HyperRequest::builder()
        .method("GET")
        .uri("http://example.test/")
        .header("host", "example.test")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    let values: Vec<String> = response
        .headers()
        .get_all(ALT_SVC_HEADER_NAME)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect();
    let _ = response.into_body().collect().await.unwrap().to_bytes();

    drop(sender);
    let _ = connection_task.await;
    handle.shutdown();
    handle.wait().await.unwrap();
    (values, port)
}

/// Build a compatibility-path config for one decision-matrix cell.
fn matrix_config(enabled: bool, advertise: bool, denylisted: bool) -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .http3(Http3Config {
            enabled,
            advertise_alt_svc: advertise,
            ..Http3Config::default()
        })
        .stripped_response_headers(if denylisted {
            vec![ALT_SVC_HEADER_NAME.to_owned()]
        } else {
            Vec::new()
        })
        .build()
        .unwrap()
}

/// The value the H3 adapter advertises for the same bind port and the
/// same decision-matrix cell.
fn adapter_value(port: u16, enabled: bool, advertise: bool, denylisted: bool) -> Option<String> {
    let server_config = eggserve_server::config::RuntimeConfig::builder()
        .bind(format!("127.0.0.1:{port}").parse().unwrap())
        .stripped_response_headers(if denylisted {
            vec![ALT_SVC_HEADER_NAME.to_owned()]
        } else {
            Vec::new()
        })
        .build()
        .unwrap();
    let h3_config = eggserve_h3::Http3Config {
        enabled,
        advertise_alt_svc: advertise,
        ..eggserve_h3::Http3Config::default()
    };
    let response = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Empty)
        .unwrap();
    let response = eggserve_h3::apply_alt_svc(response, &server_config, &h3_config);
    response
        .headers()
        .get_first(ALT_SVC_HEADER_NAME)
        .map(|value| value.to_str().unwrap().to_owned())
}

#[tokio::test]
async fn compat_path_and_h3_adapter_advertise_byte_identical_values() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let identity = write_identity(&identity_dir);

    let (values, port) = observe_alt_svc(matrix_config(true, true, false), &identity, None).await;
    let expected = format!("h3=\":{port}\"; {ALT_SVC_MAX_AGE}");
    assert_eq!(values, vec![expected.clone()]);

    // The adapter must produce the very same bytes for the same bind port.
    assert_eq!(
        adapter_value(port, true, true, false).as_deref(),
        Some(expected.as_str())
    );
}

#[tokio::test]
async fn compat_path_advertisement_decision_matrix_is_enforced_on_the_wire() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let identity = write_identity(&identity_dir);

    // (h3_enabled, advertise, denylisted, expect_advertisement)
    let matrix = [
        (true, true, false, true),
        (true, false, false, false),
        (true, true, true, false),
        (true, false, true, false),
        (false, true, false, false),
        (false, false, false, false),
        (false, true, true, false),
        (false, false, true, false),
    ];
    for (enabled, advertise, denylisted, expect) in matrix {
        let config = matrix_config(enabled, advertise, denylisted);
        let (values, port) = observe_alt_svc(config, &identity, None).await;
        if expect {
            assert_eq!(
                values,
                vec![format!("h3=\":{port}\"; {ALT_SVC_MAX_AGE}")],
                "h3_enabled={enabled} advertise={advertise} denylisted={denylisted}"
            );
            // The adapter agrees cell-for-cell.
            assert_eq!(
                adapter_value(port, enabled, advertise, denylisted).as_deref(),
                Some(format!("h3=\":{port}\"; {ALT_SVC_MAX_AGE}").as_str()),
                "h3_enabled={enabled} advertise={advertise} denylisted={denylisted}"
            );
        } else {
            assert!(
                values.is_empty(),
                "h3_enabled={enabled} advertise={advertise} denylisted={denylisted} \
                 must not advertise"
            );
            assert_eq!(
                adapter_value(port, enabled, advertise, denylisted),
                None,
                "h3_enabled={enabled} advertise={advertise} denylisted={denylisted} \
                 must not advertise"
            );
        }
    }
}

#[tokio::test]
async fn application_supplied_alt_svc_never_overrides_the_runtime_advertisement() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let identity = write_identity(&identity_dir);

    let (values, port) = observe_alt_svc(
        matrix_config(true, true, false),
        &identity,
        Some("h3=\":1\""),
    )
    .await;
    assert_eq!(
        values,
        vec![format!("h3=\":{port}\"; {ALT_SVC_MAX_AGE}")],
        "the application value must be replaced by the runtime advertisement"
    );
}

#[tokio::test]
async fn advertised_port_matches_the_bound_port() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let identity = write_identity(&identity_dir);

    let (values, port) = observe_alt_svc(matrix_config(true, true, false), &identity, None).await;
    assert_eq!(values.len(), 1);
    let advertised = values[0]
        .strip_prefix("h3=\"")
        .and_then(|rest| rest.split('"').next())
        .and_then(|raw| raw.strip_prefix(':'))
        .and_then(|raw| raw.parse::<u16>().ok());
    assert_eq!(
        advertised,
        Some(port),
        "the advertised port must be the bound port: {}",
        values[0]
    );
}
