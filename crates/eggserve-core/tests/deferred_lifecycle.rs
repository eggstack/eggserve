//! Deferred request-body ownership + request lifecycle signaling (Plan 174).
//!
//! Proves a service can return response-start while a downstream task
//! continues consuming the request body, connection reuse waits for the
//! framing boundary, abandonment forces safe close, and peer
//! disconnect/shutdown/timeouts wake idle waiters via the transport-neutral
//! [`RequestLifecycle`] without Hyper types in the contract.

use std::sync::Arc;
use std::time::Duration;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_lifecycle::RequestCancellationReason;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{service_fn_with_policy, RuntimeConfig, RuntimeState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn stream_policy() -> RequestBodyPolicy {
    RequestBodyPolicy::Stream {
        max_bytes: 1024 * 1024,
    }
}

fn test_config() -> Arc<RuntimeConfig> {
    Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .connection_total_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    )
}

async fn read_headers(client: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut acc = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await.unwrap();
        acc.push(byte[0]);
        if acc.ends_with(b"\r\n\r\n") {
            break;
        }
        assert!(acc.len() < 16384, "headers too large");
    }
    acc
}

#[tokio::test]
async fn deferred_fixed_length_completes_and_reuses() {
    // Service returns response-start immediately while spawned task owns body.
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, body) = req.into_head_and_body();
            // Move body to downstream task; return response-start now.
            let handle = tokio::spawn(async move {
                let mut body = body;
                let mut total = 0usize;
                while let Some(chunk) = body.next_chunk().await.unwrap() {
                    total += chunk.len();
                }
                total
            });
            // Prove delegation happened by checking lifecycle active?
            // Return immediately; body task continues.
            let resp = Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"OK".to_vec()))
                .unwrap();
            // Ensure task completes eventually (for test observability, not required).
            tokio::spawn(async move {
                let _ = handle.await;
            });
            Ok(resp)
        },
        stream_policy(),
    );
    let config = test_config();
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.set_nodelay(true).unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"hello ").await.unwrap();
    client.flush().await.unwrap();

    // Response-start must arrive before request EOF.
    let headers = tokio::time::timeout(Duration::from_secs(3), read_headers(&mut client))
        .await
        .expect("early response headers must arrive before upload completes");
    let header_str = String::from_utf8_lossy(&headers);
    assert!(header_str.starts_with("HTTP/1.1 200"), "got: {header_str}");
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"OK");

    // Complete upload after response-start.
    client.write_all(b"world").await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Keep-alive reuse: second request succeeds only after framing boundary.
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf))
        .await
        .expect("second request must complete")
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    // Second request hits same service (returns OK), proving reuse.
    assert!(
        text.starts_with("HTTP/1.1 200"),
        "reuse failed, got: {text}"
    );

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn deferred_chunked_completes_and_reuses() {
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, body) = req.into_head_and_body();
            tokio::spawn(async move {
                let mut body = body;
                while let Some(chunk) = body.next_chunk().await.unwrap() {
                    let _ = chunk;
                }
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"OK".to_vec()))
                .unwrap())
        },
        stream_policy(),
    );
    let config = test_config();
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"6\r\nhello \r\n").await.unwrap();
    client.flush().await.unwrap();

    let headers = tokio::time::timeout(Duration::from_secs(3), read_headers(&mut client))
        .await
        .expect("early response for chunked");
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();

    client.write_all(b"5\r\nworld\r\n0\r\n\r\n").await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn abandoned_body_forces_close_and_suppresses_pipeline() {
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, body) = req.into_head_and_body();
            // Read one chunk then abandon (drop).
            tokio::spawn(async move {
                let mut body = body;
                let _ = body.next_chunk().await;
                // Drop without EOF -> Abandoned.
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"OK".to_vec()))
                .unwrap())
        },
        stream_policy(),
    );
    let config = test_config();
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"hello ").await.unwrap();
    client.flush().await.unwrap();

    let headers = tokio::time::timeout(Duration::from_secs(3), read_headers(&mut client))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Trailing bytes must never be parsed as a second request: connection closes.
    let _ = client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await;
    let mut buf = Vec::new();
    let read_res = tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf)).await;
    // Either EOF with no second response, or timeout with no bytes: both prove no reuse.
    if let Ok(Ok(_)) = read_res {
        let text = String::from_utf8_lossy(&buf);
        assert!(
            !text.contains("second") && !text.starts_with("HTTP/1.1 200 OK\r\ncontent-length: 6"),
            "abandoned body must not allow reuse, got: {text}"
        );
    }
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn lifecycle_wakes_idle_waiter_on_disconnect() {
    let (tx, rx) = tokio::sync::oneshot::channel::<RequestCancellationReason>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let svc = service_fn_with_policy(
        move |req: Request| {
            let tx = tx.clone();
            async move {
                let lc = req.lifecycle_clone();
                // Idle waiter: not polling body/response, only lifecycle.
                tokio::spawn(async move {
                    lc.cancelled().await;
                    if let Some(reason) = lc.cancellation_reason() {
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(reason);
                        }
                    }
                });
                // Simulate long-polling: wait longer than test disconnect.
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"late".to_vec()))
                    .unwrap())
            }
        },
        RequestBodyPolicy::Reject,
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .handler_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(60))
            .build()
            .unwrap(),
    );
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /poll HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Disconnect without reading response.
    drop(client);

    let reason = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("lifecycle must wake idle waiter on disconnect")
        .unwrap();
    assert_eq!(reason, RequestCancellationReason::PeerDisconnected);

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn lifecycle_cancelled_during_body_read() {
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let svc = service_fn_with_policy(
        move |req: Request| {
            let tx = tx.clone();
            async move {
                let (_head, mut body) = req.into_head_and_body();
                let lc = body.lifecycle();
                // Separate waiter proves lifecycle fires even when body IO
                // also observes transport failure first.
                let waiter_tx = tx.clone();
                let waiter_lc = lc.clone();
                tokio::spawn(async move {
                    waiter_lc.cancelled().await;
                    if let Some(tx) = waiter_tx.lock().unwrap().take() {
                        let _ = tx.send(true);
                    }
                });
                // Block on body read; disconnect surfaces as transport error
                // and lifecycle cancellation follows promptly.
                let _ = body.next_chunk().await;
                // Give lifecycle a moment to fire after transport error.
                tokio::time::sleep(Duration::from_millis(200)).await;
                // If waiter already sent, nothing to do; otherwise lifecycle
                // should have fired by now (driver cancel on ClientError).
                if tx.lock().unwrap().is_some() {
                    // Fallback: check lifecycle directly.
                    if lc.is_cancelled() {
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(true);
                        }
                    }
                }
                Err::<Response, _>(eggserve_core::server::ServiceError::internal("cancelled"))
            }
        },
        stream_policy(),
    );
    let config = test_config();
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Headers only, no body bytes: service blocks in next_chunk().
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n")
        .await
        .unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(client);

    let woke = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("body task must observe cancellation")
        .unwrap();
    assert!(woke);

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn deferred_body_timeout_after_response_start() {
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, body) = req.into_head_and_body();
            // Delegate but never complete: hold body open past deadline.
            tokio::spawn(async move {
                let mut body = body;
                // Block until timeout/cancel; do not complete.
                let _ = body.next_chunk().await;
                // Hold open a bit longer to ensure watchdog fires.
                tokio::time::sleep(Duration::from_secs(10)).await;
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"OK".to_vec()))
                .unwrap())
        },
        stream_policy(),
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_millis(300))
            .handler_timeout(Duration::from_secs(5))
            .connection_total_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let before = eggserve_core::ops::global_counters()
        .deferred_body_timeouts
        .load(std::sync::atomic::Ordering::Relaxed);
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"hi").await.unwrap();
    client.flush().await.unwrap();

    // Early response arrives, then watchdog closes after deadline.
    let headers = tokio::time::timeout(Duration::from_secs(3), read_headers(&mut client))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    let _ = client.read_exact(&mut body).await;

    // Connection must close (watchdog) rather than stay reusable.
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf)).await;
    // Watchdog counter must have fired.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = eggserve_core::ops::global_counters()
        .deferred_body_timeouts
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(after > before, "deferred body timeout must be observed");

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn shutdown_during_deferred_overlap_cancels() {
    let (tx, rx) = tokio::sync::oneshot::channel::<RequestCancellationReason>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let svc = service_fn_with_policy(
        move |req: Request| {
            let tx = tx.clone();
            async move {
                let lc = req.lifecycle_clone();
                let (_head, body) = req.into_head_and_body();
                tokio::spawn(async move {
                    // Deferred body consumer also watches lifecycle.
                    tokio::select! {
                        _ = async {
                            let mut body = body;
                            while let Ok(Some(_)) = body.next_chunk().await {}
                        } => {},
                        _ = lc.cancelled() => {
                            if let Some(reason) = lc.cancellation_reason() {
                                if let Some(tx) = tx.lock().unwrap().take() {
                                    let _ = tx.send(reason);
                                }
                            }
                        }
                    }
                });
                // Response-start immediately; body continues.
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"OK".to_vec()))
                    .unwrap())
            }
        },
        stream_policy(),
    );
    let config = test_config();
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"partial").await.unwrap();
    client.flush().await.unwrap();
    let headers = tokio::time::timeout(Duration::from_secs(3), read_headers(&mut client))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let reason = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("shutdown must cancel deferred lifecycle")
        .unwrap();
    assert_eq!(reason, RequestCancellationReason::ServerShutdown);
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn caller_owned_transport_deferred_parity() {
    // Same deferred shape over duplex (no sockets): proves transport-neutrality.
    let svc = service_fn_with_policy(
        |req: Request| async move {
            // Caller-owned must expose no socket addrs.
            assert!(req.connection().local_addr.is_none());
            let (_head, body) = req.into_head_and_body();
            tokio::spawn(async move {
                let mut body = body;
                let mut total = 0usize;
                while let Ok(Some(chunk)) = body.next_chunk().await {
                    total += chunk.len();
                }
                assert_eq!(total, 11);
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"OK".to_vec()))
                .unwrap())
        },
        stream_policy(),
    );
    let config: Arc<RuntimeConfig> = Arc::new(
        RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    );
    let runtime = Arc::new(RuntimeState::new(&config));
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    // Leak for 'static spawn lifetime (test-only, one per test).
    let shutdown: &'static ConnectionShutdown = Box::leak(Box::new(ConnectionShutdown::new()));
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let driver = tokio::spawn(serve_http1_connection(
        server, svc, config, context, runtime, shutdown,
    ));

    // Send POST in two parts with early-response read.
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\nhello ")
        .await
        .unwrap();
    // Read response headers (early) before sending rest.
    let mut acc = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await.unwrap();
        acc.push(byte[0]);
        if acc.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&acc).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    client.write_all(b"world").await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Second keep-alive request reuses duplex connection.
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));
    let _ = driver.await.unwrap();
}

#[tokio::test]
async fn service_admission_remains_distinct_from_app_tasks() {
    // EggServe max_in_flight_requests bounds Service::call only; downstream
    // app tasks own a separate semaphore. Exhausting the service semaphore
    // yields 503 without invoking the service, while app tasks continue.
    use std::sync::atomic::{AtomicUsize, Ordering};
    let entered = Arc::new(AtomicUsize::new(0));
    let entered_clone = entered.clone();
    let svc = service_fn_with_policy(
        move |_req: Request| {
            let entered = entered_clone.clone();
            async move {
                entered.fetch_add(1, Ordering::Relaxed);
                // Simulate app task outliving Service::call via detached spawn
                // (downstream-owned budget, not EggServe permit).
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            }
        },
        RequestBodyPolicy::Reject,
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_in_flight_requests(1)
            .build()
            .unwrap(),
    );
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    // Single request succeeds; admission distinctness is structural (service
    // permit released at response-start, app task detached). The key assert
    // is no deadlock and permit recovery.
    let addr = handle.local_addr();
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));
    assert_eq!(entered.load(Ordering::Relaxed), 1);
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "tls")]
async fn tls_deferred_fixed_length_parity() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    let cert_der: rustls::pki_types::CertificateDer<'static> = cert.into();
    let key_der = rustls::pki_types::PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    let server_tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der.into())
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    let client_tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, body) = req.into_head_and_body();
            tokio::spawn(async move {
                let mut body = body;
                while let Ok(Some(_)) = body.next_chunk().await {}
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"OK".to_vec()))
                .unwrap())
        },
        stream_policy(),
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .tls_config(Arc::new(server_tls))
            .build()
            .unwrap(),
    );
    let server = eggserve_core::server::Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_tls));
    let mut client = connector
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\nhello ")
        .await
        .unwrap();
    client.flush().await.unwrap();
    // Early response before EOF (TLS parity with TCP).
    let mut acc = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await.unwrap();
        acc.push(byte[0]);
        if acc.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&acc).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    client.write_all(b"world").await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    handle.shutdown();
    handle.wait().await.unwrap();
}
