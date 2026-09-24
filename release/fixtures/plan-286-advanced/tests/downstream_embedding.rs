use std::time::Duration;

use eggserve_primitives::{RequestBodyPolicy, Response, ResponseBody, StatusCode};
use eggserve_server::{
    service_fn, service_fn_with_policy, AdmissionOwner, AdmissionOwnership, H1PolicyOwnership,
    Http1RequestTargetMode, PolicyOwner, Request, RuntimeConfig, Server, ServiceError,
    ShutdownResult,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug)]
struct BrandRejection;
impl eggserve_server::RuntimeRejectionPresenter for BrandRejection {
    fn present(
        &self,
        rejection: &eggserve_server::RuntimeRejection,
    ) -> Option<eggserve_server::RuntimeErrorPresentation> {
        assert_eq!(
            rejection.kind(),
            eggserve_server::RuntimeRejectionKind::RequestTargetTooLong
        );
        let mut headers = eggserve_primitives::HeaderBlock::new();
        headers.push_str("x-brand", "host-error").unwrap();
        headers.push_str("content-length", "999").unwrap();
        Some(eggserve_server::RuntimeErrorPresentation {
            headers,
            body: b"host body".to_vec(),
        })
    }
}

#[tokio::test]
async fn typed_rejection_presentation_cannot_change_status_or_framing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .max_request_target_bytes(128)
        .runtime_rejection_presenter(Arc::new(BrandRejection))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(b"GET /aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 414"));
    assert!(response
        .windows(b"x-brand: host-error".len())
        .any(|window| window.eq_ignore_ascii_case(b"x-brand: host-error")));
    assert!(response.ends_with(b"host body"));
    assert!(!response
        .windows(b"content-length: 999".len())
        .any(|window| window.eq_ignore_ascii_case(b"content-length: 999")));
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn caller_owned_tls_h1_stream_uses_direct_policy_api() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate = rustls::pki_types::CertificateDer::from(certified.cert.der().to_vec());
    let private_key =
        rustls::pki_types::PrivateKeyDer::Pkcs8(certified.key_pair.serialize_der().into());
    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], private_key)
        .unwrap();
    server_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
    let runtime = RuntimeConfig::default();
    let policy = Arc::new(runtime.h1_connection_policy().unwrap());
    let state = Arc::new(eggserve_server::RuntimeState::try_new(&runtime).unwrap());
    let shutdown = eggserve_server::ConnectionShutdown::new();
    let server_shutdown = shutdown.clone();
    let server_policy = policy.clone();
    let server_state = state.clone();
    let server = tokio::spawn(async move {
        let (tcp, peer) = listener.accept().await.unwrap();
        let tls = acceptor.accept(tcp).await.unwrap();
        assert_eq!(tls.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
        let ctx = eggserve_server::ConnectionContext::for_tcp(addr, peer, None);
        eggserve_server::serve_http1_connection_with_policy(
            tls,
            service_fn(|_request: Request| async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            }),
            server_policy,
            ctx,
            server_state,
            &server_shutdown,
        )
        .await
    });

    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let mut client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
    let tcp = TcpStream::connect(addr).await.unwrap();
    let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut client = connector.connect(name, tcp).await.unwrap();
    assert_eq!(client.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"), "{response:?}");
    assert!(response.ends_with(b"ok"));
    shutdown.shutdown();
    let _ = server.await.unwrap();
}

async fn get(stream: &mut TcpStream) {
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    let mut byte = [0; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
    }
    assert!(response.starts_with(b"HTTP/1.1 200"));
}

#[tokio::test]
async fn leaf_crate_supervision_with_unlimited_keep_alive() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .connection_total_timeout(Duration::from_millis(40))
        .disable_connection_total_timeout()
        .keep_alive_idle_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let (control, mut completion) = handle.into_parts();

    let mut client = TcpStream::connect(address).await.unwrap();
    get(&mut client).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    get(&mut client).await;
    drop(client);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let _ = shutdown_tx.send(());
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    tokio::pin!(shutdown);
    let result = tokio::select! {
        result = completion.wait() => result.unwrap(),
        _ = &mut shutdown => {
            control.shutdown();
            completion.wait().await.unwrap()
        }
    };
    assert_eq!(result, ShutdownResult::Clean);
    // The external supervisor owns the signal source independently of the
    // completion value, and requests shutdown without consuming it.
}

async fn request(address: std::net::SocketAddr, wire: &'static [u8]) -> Vec<u8> {
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(wire).await.unwrap();
    let mut response = Vec::new();
    let mut byte = [0; 1];
    while !response.ends_with(b"\r\n\r\n") {
        client.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
    }
    response
}

#[tokio::test]
async fn absolute_form_is_explicit_and_projects_canonical_target_metadata() {
    for (mode, expect_dispatch) in [
        (Http1RequestTargetMode::OriginOnly, false),
        (Http1RequestTargetMode::OriginOrAbsolute, true),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let runtime = RuntimeConfig::builder()
            .http1_request_target_mode(mode)
            .build()
            .unwrap();
        let server = Server::builder()
            .runtime(runtime)
            .from_listener(listener)
            .build()
            .unwrap();
        let handle = server
            .start_with_service(service_fn(move |request: Request| {
                observed.fetch_add(1, Ordering::SeqCst);
                let target = request.head().target();
                assert_eq!(
                    target.form(),
                    eggserve_primitives::RequestTargetForm::Absolute
                );
                assert_eq!(target.scheme(), Some("http"));
                assert_eq!(
                    target.uri_authority().unwrap().as_str(),
                    "example.test:8080"
                );
                assert_eq!(target.path(), "/a");
                assert_eq!(target.query(), Some("b=1"));
                async {
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Empty)
                        .unwrap())
                }
            }))
            .await
            .unwrap();
        let response = request(address, b"GET http://example.test:8080/a?b=1 HTTP/1.1\r\nHost: example.test:8080\r\nConnection: close\r\n\r\n").await;
        assert_eq!(response.starts_with(b"HTTP/1.1 200"), expect_dispatch);
        assert_eq!(count.load(Ordering::SeqCst), usize::from(expect_dispatch));
        if expect_dispatch {
            let mismatch = request(address, b"GET http://example.test/a HTTP/1.1\r\nHost: other.test\r\nConnection: close\r\n\r\n").await;
            assert!(mismatch.starts_with(b"HTTP/1.1 400"));
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
        handle.shutdown();
        let _ = handle.wait().await;
    }
}

#[tokio::test]
async fn absolute_form_target_limit_counts_full_uri_before_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let runtime = RuntimeConfig::builder()
        .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
        .max_request_target_bytes(128)
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(move |_request: Request| {
            observed.fetch_add(1, Ordering::SeqCst);
            async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Empty)
                    .unwrap())
            }
        }))
        .await
        .unwrap();
    let response = request(
        address,
        b"GET http://example.test/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 414"));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn external_target_ceiling_keeps_parser_and_target_validation_active() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
        .max_request_target_bytes(128)
        .policy_ownership(H1PolicyOwnership {
            request_target_ceiling: PolicyOwner::External,
            ..H1PolicyOwnership::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|request: Request| async move {
            assert_eq!(
                request.head().target().form(),
                eggserve_primitives::RequestTargetForm::Absolute
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let response = request(address, b"GET http://example.test/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n").await;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn absolute_form_composes_with_streamed_body_and_terminal_trailers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
        .max_request_body_bytes(2)
        .body_read_timeout(Duration::from_millis(10))
        .policy_ownership(H1PolicyOwnership {
            request_body_deadline: PolicyOwner::External,
            global_request_body_ceiling: PolicyOwner::External,
            ..H1PolicyOwnership::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let service = service_fn_with_policy(
        |request: Request| async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let target_is_absolute =
                request.head().target().form() == eggserve_primitives::RequestTargetForm::Absolute;
            let (_, body, _) = request.into_parts();
            let (bytes, trailers) = body
                .read_all_with_trailers()
                .await
                .map_err(|error| ServiceError::internal(error.to_string()))?;
            assert!(target_is_absolute);
            assert_eq!(bytes.as_ref(), b"hello");
            assert_eq!(
                trailers
                    .unwrap()
                    .as_block()
                    .get_first("x-check")
                    .unwrap()
                    .as_bytes(),
                b"yes"
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 1024 },
    );
    let handle = server.start_with_service(service).await.unwrap();
    let response = request(
        address,
        b"POST http://example.test/upload HTTP/1.1\r\nHost: example.test\r\nTransfer-Encoding: chunked\r\nTrailer: x-check\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\nx-check: yes\r\n\r\n",
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn external_handler_deadline_overrides_only_the_handler_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let ownership = H1PolicyOwnership {
        handler_deadline: PolicyOwner::External,
        ..H1PolicyOwnership::default()
    };
    let runtime = RuntimeConfig::builder()
        .handler_timeout(Duration::from_millis(10))
        .policy_ownership(ownership)
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let response = request(
        address,
        b"GET / HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn external_service_admission_skips_eggserve_gate() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let count = Arc::new(AtomicUsize::new(0));
    let ownership = AdmissionOwnership {
        service_calls: AdmissionOwner::External,
        ..AdmissionOwnership::default()
    };
    let runtime = RuntimeConfig::builder()
        .max_in_flight_requests(1)
        .admission_ownership(ownership)
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let entered_service = entered.clone();
    let release_service = release.clone();
    let call_count = count.clone();
    let handle = server
        .start_with_service(service_fn(move |_request: Request| {
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
        }))
        .await
        .unwrap();
    let first = tokio::spawn(request(address, b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"));
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    let second = request(
        address,
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(second.starts_with(b"HTTP/1.1 200"));
    release.notify_one();
    let first = tokio::time::timeout(Duration::from_secs(1), first)
        .await
        .unwrap()
        .unwrap();
    assert!(first.starts_with(b"HTTP/1.1 200"));
    assert_eq!(count.load(Ordering::SeqCst), 2);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn external_idle_deadline_keeps_idle_connection_available() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .keep_alive_idle_timeout(Duration::from_millis(10))
        .policy_ownership(H1PolicyOwnership {
            keep_alive_idle_deadline: PolicyOwner::External,
            ..H1PolicyOwnership::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let mut client = TcpStream::connect(address).await.unwrap();
    get(&mut client).await;
    tokio::time::sleep(Duration::from_millis(40)).await;
    get(&mut client).await;
    drop(client);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn external_write_deadline_does_not_abort_a_slow_reader() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .response_write_timeout(Duration::from_millis(10))
        .policy_ownership(H1PolicyOwnership {
            response_write_progress_deadline: PolicyOwner::External,
            ..H1PolicyOwnership::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(vec![b'x'; 16 * 1024 * 1024]))
                .unwrap())
        }))
        .await
        .unwrap();
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(
        response.len(),
        16 * 1024 * 1024
            + response
                .windows(b"\r\n\r\n".len())
                .position(|w| w == b"\r\n\r\n")
                .unwrap()
            + 4
    );
    handle.shutdown();
    let _ = handle.wait().await;
}

#[test]
fn ordinary_direct_runtime_keeps_hardened_ownership_defaults() {
    use eggserve_server::{AdmissionOwnership, H1PolicyOwnership, Http1RequestTargetMode};
    let config = RuntimeConfig::default();
    assert_eq!(config.http1_request_target_mode, Http1RequestTargetMode::OriginOnly);
    assert_eq!(config.policy_ownership, H1PolicyOwnership::eggserve_owned());
    assert_eq!(config.admission_ownership, AdmissionOwnership::eggserve_owned());
    assert_eq!(config.max_request_body_bytes, 0);
    assert!(config.max_request_target_bytes > 0);
}

#[tokio::test]
async fn canonical_service_receives_duplicate_header_fields_in_order() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Server::builder()
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|request: Request| async move {
            let values: Vec<_> = request
                .head()
                .headers()
                .get_all("x-duplicate")
                .into_iter()
                .map(|value| value.as_bytes().to_vec())
                .collect();
            assert_eq!(values, [b"first".to_vec(), b"second".to_vec()]);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let response = request(address, b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Duplicate: first\r\nX-Duplicate: second\r\nConnection: close\r\n\r\n").await;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    handle.shutdown();
    let _ = handle.wait().await;
}
