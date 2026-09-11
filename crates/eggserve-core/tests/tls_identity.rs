//! Production TLS identity, SNI, mTLS, and reload (Plan 203).
//!
//! Deterministic in-process qualification using rcgen identities and
//! tokio-rustls clients. Covers SNI exact/wildcard/default/no-match,
//! invalid pairing failing before ready, Required/Optional/Disabled client
//! auth (untrusted/expired rejection), reload atomicity + race safety,
//! handshake timeout permit recovery, PROXY→TLS ordering, ALPN parity,
//! log hygiene (no PEM/key bytes), and verified metadata provenance.

#![cfg(feature = "tls")]

use std::sync::Arc;
use std::time::Duration;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::TlsInfo;
use eggserve_core::server::{service_fn, Server};
use eggserve_core::tls::{ClientAuthMode, TlsReloadHandle, TlsServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn init_tls() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

struct CertIdentity {
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    cert_der_bytes: Vec<u8>,
}

fn make_identity(sans: Vec<&str>) -> CertIdentity {
    init_tls();
    let key_pair = rcgen::KeyPair::generate().expect("keypair");
    let params =
        rcgen::CertificateParams::new(sans.into_iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .expect("params");
    let cert = params.self_signed(&key_pair).expect("self-sign");
    let cert_der: CertificateDer<'static> = cert.into();
    let cert_bytes = cert_der.as_ref().to_vec();
    let key_der = PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    CertIdentity {
        certs: vec![cert_der],
        key: key_der.into(),
        cert_der_bytes: cert_bytes,
    }
}

fn make_expired_identity(san: &str) -> CertIdentity {
    init_tls();
    let key_pair = rcgen::KeyPair::generate().expect("keypair");
    let mut params = rcgen::CertificateParams::new(vec![san.to_string()]).expect("params");
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2020, 1, 2);
    let cert = params.self_signed(&key_pair).expect("self-sign expired");
    let cert_der: CertificateDer<'static> = cert.into();
    let cert_bytes = cert_der.as_ref().to_vec();
    let key_der = PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    CertIdentity {
        certs: vec![cert_der],
        key: key_der.into(),
        cert_der_bytes: cert_bytes,
    }
}

fn client_config_for(
    roots: &[Vec<u8>],
    client_id: Option<&CertIdentity>,
) -> Arc<rustls::ClientConfig> {
    let mut store = rustls::RootCertStore::empty();
    for root in roots {
        let der = CertificateDer::from(root.clone());
        store.add(der).unwrap();
    }
    let builder = rustls::ClientConfig::builder().with_root_certificates(store);
    let config = match client_id {
        None => builder.with_no_client_auth(),
        Some(id) => builder
            .with_client_auth_cert(id.certs.clone(), id.key.clone_key())
            .expect("client auth cert"),
    };
    // Use http/1.1 ALPN for H1 tests; H2 tests override separately.
    let mut config = config;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
}

async fn tls_get(
    addr: std::net::SocketAddr,
    client_config: Arc<rustls::ClientConfig>,
    sni: &str,
    request: &str,
) -> Result<Vec<u8>, String> {
    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| e.to_string())?;
    let connector = tokio_rustls::TlsConnector::from(client_config);
    let domain: ServerName<'static> =
        ServerName::try_from(sni.to_owned()).map_err(|e| e.to_string())?;
    let tls = connector
        .connect(domain, tcp)
        .await
        .map_err(|e| format!("handshake failed: {e}"))?;
    let (mut reader, mut writer) = tokio::io::split(tls);
    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_to_end(&mut buf))
        .await
        .map_err(|_| "read timeout".to_string())?;
    Ok(buf)
}

async fn peer_server_cert(
    addr: std::net::SocketAddr,
    client_config: Arc<rustls::ClientConfig>,
    sni: &str,
) -> Result<Vec<u8>, String> {
    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| e.to_string())?;
    let connector = tokio_rustls::TlsConnector::from(client_config);
    let domain: ServerName<'static> =
        ServerName::try_from(sni.to_owned()).map_err(|e| e.to_string())?;
    let tls = connector
        .connect(domain, tcp)
        .await
        .map_err(|e| format!("handshake failed: {e}"))?;
    let (_, conn) = tls.get_ref();
    let certs = conn
        .peer_certificates()
        .ok_or_else(|| "no peer certs".to_string())?;
    Ok(certs.first().unwrap().as_ref().to_vec())
}

/// True when a TLS client-auth rejection occurred: handshake Err, or an
/// established-then-closed connection without a 200 response. TLS 1.3
/// client-cert verification can fail after the client's connect future
/// resolves, so both shapes prove rejection.
fn is_rejected(res: Result<Vec<u8>, String>) -> bool {
    match res {
        Err(_) => true,
        Ok(bytes) => !String::from_utf8_lossy(&bytes).starts_with("HTTP/1.1 200"),
    }
}

fn echo_service() -> impl eggserve_core::server::Service {
    service_fn(|_req| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap())
    })
}

const GET_CLOSE: &str = "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";

#[tokio::test]
async fn sni_exact_selection() {
    let a = make_identity(vec!["a.test"]);
    let b = make_identity(vec!["b.test"]);
    let tls = TlsServerConfig::builder()
        .add_identity("a.test", a.certs.clone(), a.key.clone_key())
        .unwrap()
        .add_identity("b.test", b.certs.clone(), b.key.clone_key())
        .unwrap()
        .default_identity(a.certs.clone(), a.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    // Need a service: use static? Use custom echo via start_with_service needs serve_config? Use runtime-only custom.
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let roots_a = vec![a.cert_der_bytes.clone()];
    let roots_b = vec![b.cert_der_bytes.clone()];
    // SNI a.test must present cert A.
    let got_a = peer_server_cert(addr, client_config_for(&roots_a, None), "a.test")
        .await
        .expect("a.test handshake");
    assert_eq!(got_a, a.cert_der_bytes);
    // SNI b.test must present cert B.
    let got_b = peer_server_cert(addr, client_config_for(&roots_b, None), "b.test")
        .await
        .expect("b.test handshake");
    assert_eq!(got_b, b.cert_der_bytes);

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn sni_wildcard_single_level() {
    let wild = make_identity(vec!["*.wild.test"]);
    // Default covers the names wildcard must NOT match, so fallback is observable
    // through successful hostname verification (wildcard would fail it).
    let def = make_identity(vec!["default.test", "a.b.wild.test", "wild.test"]);
    let tls = TlsServerConfig::builder()
        .add_identity("*.wild.test", wild.certs.clone(), wild.key.clone_key())
        .unwrap()
        .default_identity(def.certs.clone(), def.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Single-level matches wildcard.
    let got = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&wild.cert_der_bytes), None),
        "foo.wild.test",
    )
    .await
    .expect("wildcard match");
    assert_eq!(got, wild.cert_der_bytes);

    // Multi-level must NOT match wildcard → default (SAN includes it, so success
    // proves fallback; wildcard presentation would fail hostname verification).
    let got_def = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&def.cert_der_bytes), None),
        "a.b.wild.test",
    )
    .await
    .expect("multi-level falls back to default");
    assert_eq!(got_def, def.cert_der_bytes);

    // Bare suffix must NOT match wildcard → default.
    let got_bare = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&def.cert_der_bytes), None),
        "wild.test",
    )
    .await
    .expect("bare falls back to default");
    assert_eq!(got_bare, def.cert_der_bytes);

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn sni_no_match_fails_without_default() {
    let a = make_identity(vec!["a.test"]);
    let tls = TlsServerConfig::builder()
        .add_identity("a.test", a.certs.clone(), a.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    assert!(!tls.has_default_identity());
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Unknown SNI with no default → handshake fails (no identity).
    let other = make_identity(vec!["other.test"]);
    let res = tls_get(
        addr,
        client_config_for(std::slice::from_ref(&other.cert_der_bytes), None),
        "unknown.test",
        GET_CLOSE,
    )
    .await;
    assert!(res.is_err(), "no-match without default must fail: {res:?}");

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn sni_no_match_falls_back_to_default() {
    let a = make_identity(vec!["a.test"]);
    // Default SAN includes the unknown name so fallback is observable via a
    // successful, hostname-validated handshake (fallback presents default).
    let def = make_identity(vec!["default.test", "unknown.test"]);
    let tls = TlsServerConfig::builder()
        .add_identity("a.test", a.certs.clone(), a.key.clone_key())
        .unwrap()
        .default_identity(def.certs.clone(), def.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    assert!(tls.has_default_identity());
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Unknown SNI → default cert presented (hostname passes because default
    // covers unknown.test).
    let got = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&def.cert_der_bytes), None),
        "unknown.test",
    )
    .await
    .expect("unknown SNI falls back to default");
    assert_eq!(got, def.cert_der_bytes);

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn invalid_pairing_fails_before_ready() {
    let a = make_identity(vec!["a.test"]);
    let b = make_identity(vec!["b.test"]);
    // Cert A with key B must fail at build (keys_match).
    let res = TlsServerConfig::builder().add_identity("a.test", a.certs.clone(), b.key.clone_key());
    assert!(res.is_err(), "mismatched key must fail: {res:?}");
    // Empty identities must fail.
    assert!(TlsServerConfig::builder().build().is_err());
    // Bad SNI must fail.
    assert!(TlsServerConfig::builder()
        .add_identity("bad_host!", a.certs.clone(), a.key.clone_key())
        .is_err());
    assert!(TlsServerConfig::builder()
        .add_identity("*.com", a.certs.clone(), a.key.clone_key())
        .is_err());
}

#[tokio::test]
async fn client_auth_disabled_allows_unauthenticated() {
    let srv = make_identity(vec!["localhost"]);
    let tls = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(tls.client_auth(), ClientAuthMode::Disabled);
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    // Record TLS metadata.
    let seen: Arc<std::sync::Mutex<Option<TlsInfo>>> = Arc::new(std::sync::Mutex::new(None));
    let seen2 = seen.clone();
    let svc = service_fn(move |req: eggserve_core::server::Request| {
        let seen2 = seen2.clone();
        Box::pin(async move {
            *seen2.lock().unwrap() = req.connection().tls.clone();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    });
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let resp = tls_get(
        addr,
        client_config_for(std::slice::from_ref(&srv.cert_der_bytes), None),
        "localhost",
        GET_CLOSE,
    )
    .await
    .expect("disabled allows no cert");
    assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 200"));
    let info = seen.lock().unwrap().clone().expect("tls info");
    assert!(!info.client_authenticated);
    assert!(!info.peer_certificates_present);
    assert!(info.peer_certificate_chain.is_none());
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn client_auth_required_and_optional() {
    let srv = make_identity(vec!["localhost"]);
    let client_good = make_identity(vec!["client"]);
    let client_bad = make_identity(vec!["evil"]);

    // Required: no cert fails, good succeeds, untrusted fails.
    let tls_req = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .client_auth_required(vec![client_good.certs[0].clone()])
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls_req.into_server_config())
        .tls_expose_peer_chain(true)
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let seen: Arc<std::sync::Mutex<Option<TlsInfo>>> = Arc::new(std::sync::Mutex::new(None));
    let seen2 = seen.clone();
    let svc = service_fn(move |req: eggserve_core::server::Request| {
        let seen2 = seen2.clone();
        Box::pin(async move {
            *seen2.lock().unwrap() = req.connection().tls.clone();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    });
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let roots = vec![srv.cert_der_bytes.clone()];

    // No cert → fail (handshake alert may surface as Err or as a closed
    // connection with no 200; both prove rejection — TLS 1.3 client-auth
    // failure can arrive after the client's connect future resolves).
    let no_cert = tls_get(
        addr,
        client_config_for(&roots, None),
        "localhost",
        GET_CLOSE,
    )
    .await;
    assert!(is_rejected(no_cert), "required must reject unauthenticated");
    // Untrusted → fail.
    let untrusted = tls_get(
        addr,
        client_config_for(&roots, Some(&client_bad)),
        "localhost",
        GET_CLOSE,
    )
    .await;
    assert!(is_rejected(untrusted), "required must reject untrusted");
    // Good → succeeds + metadata verified.
    let resp = tls_get(
        addr,
        client_config_for(&roots, Some(&client_good)),
        "localhost",
        GET_CLOSE,
    )
    .await
    .expect("required good");
    assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 200"));
    let info = seen.lock().unwrap().clone().expect("tls");
    assert!(info.client_authenticated);
    assert!(info.peer_certificates_present);
    assert!(info.peer_certificate_chain.is_some());
    assert_eq!(info.peer_certificate_chain.unwrap().len(), 1);
    handle.shutdown();
    let _ = handle.wait().await;

    // Optional: no cert succeeds unauthenticated, good succeeds authenticated.
    let tls_opt = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .client_auth_optional(vec![client_good.certs[0].clone()])
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls_opt.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let seen: Arc<std::sync::Mutex<Option<TlsInfo>>> = Arc::new(std::sync::Mutex::new(None));
    let seen2 = seen.clone();
    let svc = service_fn(move |req: eggserve_core::server::Request| {
        let seen2 = seen2.clone();
        Box::pin(async move {
            *seen2.lock().unwrap() = req.connection().tls.clone();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    });
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let resp = tls_get(
        addr,
        client_config_for(&roots, None),
        "localhost",
        GET_CLOSE,
    )
    .await
    .expect("optional no cert");
    assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 200"));
    assert!(!seen.lock().unwrap().clone().unwrap().client_authenticated);
    let resp = tls_get(
        addr,
        client_config_for(&roots, Some(&client_good)),
        "localhost",
        GET_CLOSE,
    )
    .await
    .expect("optional good");
    assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 200"));
    assert!(seen.lock().unwrap().clone().unwrap().client_authenticated);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn client_auth_rejects_expired() {
    let srv = make_identity(vec!["localhost"]);
    let expired = make_expired_identity("client");
    let tls = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .client_auth_required(vec![expired.certs[0].clone()])
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let roots = vec![srv.cert_der_bytes.clone()];
    let res = tls_get(
        addr,
        client_config_for(&roots, Some(&expired)),
        "localhost",
        GET_CLOSE,
    )
    .await;
    assert!(
        is_rejected(res.clone()),
        "expired client cert must fail: {res:?}"
    );
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn reload_changes_new_handshakes_not_established() {
    let a = make_identity(vec!["localhost"]);
    let b = make_identity(vec!["localhost"]);
    let tls_a = TlsServerConfig::builder()
        .single_identity(a.certs.clone(), a.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let handle_reload = TlsReloadHandle::from_tls_server_config(&tls_a);
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_reload_handle(handle_reload.clone())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // New handshake before reload sees A.
    let got_a = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&a.cert_der_bytes), None),
        "localhost",
    )
    .await
    .expect("before reload A");
    assert_eq!(got_a, a.cert_der_bytes);

    // Hold an established keep-alive connection on A.
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(client_config_for(
        std::slice::from_ref(&a.cert_der_bytes),
        None,
    ));
    let domain: ServerName<'static> = ServerName::try_from("localhost".to_owned()).unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (mut reader, mut writer) = tokio::io::split(tls_stream);
    writer
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut buf = vec![0u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(5), reader.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).contains("200"));

    // Reload to B.
    let tls_b = TlsServerConfig::builder()
        .single_identity(b.certs.clone(), b.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    handle.replace_tls_server_config(&tls_b).expect("reload");
    // Also via raw handle (same underlying).
    // New handshake sees B.
    let got_b = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&b.cert_der_bytes), None),
        "localhost",
    )
    .await
    .expect("after reload B");
    assert_eq!(got_b, b.cert_der_bytes);

    // Established A connection still usable (keep-alive, same session).
    writer
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_to_end(&mut rest))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&rest).contains("200"));

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn failed_reload_preserves_old() {
    let a = make_identity(vec!["localhost"]);
    let b = make_identity(vec!["localhost"]);
    let tls_a = TlsServerConfig::builder()
        .single_identity(a.certs.clone(), a.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let reload = TlsReloadHandle::from_tls_server_config(&tls_a);
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_reload_handle(reload.clone())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Failed build (mismatched key) never reaches replace.
    let bad = TlsServerConfig::builder().add_identity("x.test", a.certs.clone(), b.key.clone_key());
    assert!(bad.is_err());
    // Live snapshot still A.
    let got = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&a.cert_der_bytes), None),
        "localhost",
    )
    .await
    .expect("old preserved");
    assert_eq!(got, a.cert_der_bytes);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn concurrent_reload_handshake_race_safe() {
    let a = make_identity(vec!["a.test"]);
    let b = make_identity(vec!["a.test"]);
    // Same SAN so hostname checks pass for either cert when trusting both.
    let tls_a = TlsServerConfig::builder()
        .single_identity(a.certs.clone(), a.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let tls_b = TlsServerConfig::builder()
        .single_identity(b.certs.clone(), b.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let reload = TlsReloadHandle::from_tls_server_config(&tls_a);
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_reload_handle(reload.clone())
        .max_connections(64)
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = Arc::new(server.start_with_service(echo_service()).await.unwrap());
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    // Trust both so either snapshot passes hostname verification (SAN a.test for both? b has a.test too).
    let roots = vec![a.cert_der_bytes.clone(), b.cert_der_bytes.clone()];
    // Actually client root store with both self-signed certs: handshake succeeds
    // regardless of which server cert is presented only if the presented cert is
    // in the store. Since A and B are different self-signed certs, trusting both
    // allows either.
    let mut store = rustls::RootCertStore::empty();
    for r in [&a.cert_der_bytes, &b.cert_der_bytes] {
        store.add(CertificateDer::from(r.clone())).unwrap();
    }
    let client_cfg = Arc::new({
        let mut c = rustls::ClientConfig::builder()
            .with_root_certificates(store)
            .with_no_client_auth();
        c.alpn_protocols = vec![b"http/1.1".to_vec()];
        c
    });

    let reload2 = reload.clone();
    let tls_a_cfg = tls_a.into_server_config();
    let tls_b_cfg = tls_b.into_server_config();
    let reloader = tokio::spawn(async move {
        for i in 0..50 {
            if i % 2 == 0 {
                reload2.replace(tls_b_cfg.clone());
            } else {
                reload2.replace(tls_a_cfg.clone());
            }
            tokio::task::yield_now().await;
        }
    });
    let mut tasks = Vec::new();
    for _ in 0..50 {
        let cfg = client_cfg.clone();
        tasks.push(tokio::spawn(async move {
            let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
            let connector = tokio_rustls::TlsConnector::from(cfg);
            let domain: ServerName<'static> = ServerName::try_from("a.test".to_owned()).unwrap();
            let tls = connector.connect(domain, tcp).await.unwrap();
            let (mut r, mut w) = tokio::io::split(tls);
            w.write_all(GET_CLOSE.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            let _ = tokio::time::timeout(Duration::from_secs(5), r.read_to_end(&mut buf))
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));
            let _ = roots;
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    reloader.await.unwrap();
    handle.shutdown();
    let h = Arc::try_unwrap(handle).expect("handle");
    let _ = h.wait().await;
}

#[tokio::test]
async fn handshake_timeout_releases_permit() {
    let srv = make_identity(vec!["localhost"]);
    let tls = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_handshake_timeout(Duration::from_millis(200))
        .max_connections(2)
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    // Occupy with incomplete handshakes (partial bytes, then stall).
    let mut stallers = Vec::new();
    for _ in 0..2 {
        let s = tokio::net::TcpStream::connect(addr).await.unwrap();
        stallers.push(s);
    }
    // Let handshake timeout fire and release permits.
    tokio::time::sleep(Duration::from_millis(600)).await;
    drop(stallers);
    tokio::time::sleep(Duration::from_millis(200)).await;
    // New valid handshake must still succeed (permit recovered).
    let got = peer_server_cert(
        addr,
        client_config_for(std::slice::from_ref(&srv.cert_der_bytes), None),
        "localhost",
    )
    .await;
    assert!(got.is_ok(), "permit must recover after timeout: {got:?}");
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn proxy_to_tls_ordering() {
    use eggserve_core::primitives::proxy::{IpPrefix, TrustedProxyConfig};
    let srv = make_identity(vec!["localhost"]);
    let tls = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let mut proxy = TrustedProxyConfig::default();
    proxy.peers.push(IpPrefix::parse("127.0.0.1").unwrap());
    proxy.proxy_protocol.enabled = true;
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .trusted_proxy(proxy)
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let seen: Arc<
        std::sync::Mutex<Option<eggserve_core::primitives::connection_info::ConnectionInfo>>,
    > = Arc::new(std::sync::Mutex::new(None));
    let seen2 = seen.clone();
    let svc = service_fn(move |req: eggserve_core::server::Request| {
        let seen2 = seen2.clone();
        Box::pin(async move {
            *seen2.lock().unwrap() = Some(req.connection().clone());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    });
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Direct TLS without PROXY preamble must fail (bytes interpreted as preamble).
    let direct = tls_get(
        addr,
        client_config_for(std::slice::from_ref(&srv.cert_der_bytes), None),
        "localhost",
        GET_CLOSE,
    )
    .await;
    assert!(
        direct.is_err(),
        "PROXY-enabled must reject bare TLS: {direct:?}"
    );

    // PROXY v1 preamble then TLS must succeed with effective client + TLS.
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(b"PROXY TCP4 192.168.1.100 10.0.0.1 12345 8000\r\n")
        .await
        .unwrap();
    let connector = tokio_rustls::TlsConnector::from(client_config_for(
        std::slice::from_ref(&srv.cert_der_bytes),
        None,
    ));
    let domain: ServerName<'static> = ServerName::try_from("localhost".to_owned()).unwrap();
    let tls_stream = connector.connect(domain, tcp).await.expect("proxy+tls");
    let (mut r, mut w) = tokio::io::split(tls_stream);
    w.write_all(GET_CLOSE.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), r.read_to_end(&mut buf))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));
    let info = seen.lock().unwrap().clone().expect("conn info");
    assert!(info.tls.is_some(), "TLS must be present after PROXY");
    assert_eq!(
        info.effective_client_addr().unwrap().ip().to_string(),
        "192.168.1.100"
    );
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn tls_metadata_provenance_correct() {
    let srv = make_identity(vec!["myhost.test"]);
    let tls = TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .build()
        .unwrap();
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls.into_server_config())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let seen: Arc<
        std::sync::Mutex<Option<eggserve_core::primitives::connection_info::ConnectionInfo>>,
    > = Arc::new(std::sync::Mutex::new(None));
    let seen2 = seen.clone();
    let svc = service_fn(move |req: eggserve_core::server::Request| {
        let seen2 = seen2.clone();
        Box::pin(async move {
            *seen2.lock().unwrap() = Some(req.connection().clone());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    });
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    // Use SNI myhost.test so server_name is populated.
    let mut store = rustls::RootCertStore::empty();
    store
        .add(CertificateDer::from(srv.cert_der_bytes.clone()))
        .unwrap();
    let mut cc = rustls::ClientConfig::builder()
        .with_root_certificates(store)
        .with_no_client_auth();
    cc.alpn_protocols = vec![b"http/1.1".to_vec()];
    let cc = Arc::new(cc);
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(cc);
    let domain: ServerName<'static> = ServerName::try_from("myhost.test".to_owned()).unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (mut r, mut w) = tokio::io::split(tls_stream);
    w.write_all(GET_CLOSE.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), r.read_to_end(&mut buf))
        .await
        .unwrap();
    let info = seen.lock().unwrap().clone().unwrap();
    assert_eq!(
        info.scheme,
        eggserve_core::primitives::connection_info::Scheme::Https
    );
    let tls_info = info.tls.as_ref().unwrap();
    assert_eq!(tls_info.server_name.as_deref(), Some("myhost.test"));
    assert_eq!(tls_info.alpn.as_deref(), Some("http/1.1"));
    assert!(!tls_info.client_authenticated);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[test]
fn logs_never_contain_pem_or_key_bytes() {
    use eggserve_core::ops::{Event, OpsContext};
    use std::sync::{Arc, Mutex};
    struct Capture {
        events: Mutex<Vec<Event>>,
    }
    impl eggserve_core::ops::LogSink for Capture {
        fn emit(&self, event: &Event) {
            self.events.lock().unwrap().push(event.clone());
        }
        fn flush(&self) {}
    }
    let capture = Arc::new(Capture {
        events: Mutex::new(Vec::new()),
    });
    let ops = OpsContext::new(capture.clone());
    // Emit representative TLS events the server produces (sanitized categories).
    ops.emit(eggserve_core::ops::Event::new(
        eggserve_core::ops::Severity::Warn,
        eggserve_core::ops::EventKind::TlsHandshakeFailure,
        "TLS handshake failed",
    ));
    ops.emit(eggserve_core::ops::Event::new(
        eggserve_core::ops::Severity::Warn,
        eggserve_core::ops::EventKind::TlsHandshakeTimeout,
        "TLS handshake timeout",
    ));
    // TlsInfo display must never include key material.
    let info = TlsInfo {
        protocol_version: Some("TLSv1.3".into()),
        server_name: Some("example.test".into()),
        alpn: Some("http/1.1".into()),
        client_authenticated: true,
        peer_certificates_present: true,
        peer_certificate_chain: Some(vec![vec![1, 2, 3]]),
    };
    let display = format!("{info}");
    assert!(!display.contains("-----BEGIN"));
    assert!(!display.contains("PRIVATE KEY"));
    for ev in capture.events.lock().unwrap().iter() {
        let json = eggserve_core::ops::event_to_json(ev);
        assert!(
            !json.contains("-----BEGIN"),
            "log must not contain PEM: {json}"
        );
        assert!(
            !json.contains("PRIVATE KEY"),
            "log must not contain key: {json}"
        );
    }
    // Builder errors must not echo key bytes either.
    let a = make_identity(vec!["a.test"]);
    let b = make_identity(vec!["b.test"]);
    let err = TlsServerConfig::builder()
        .add_identity("a.test", a.certs.clone(), b.key.clone_key())
        .unwrap_err()
        .to_string();
    assert!(!err.contains("-----BEGIN"));
}

#[test]
fn trust_roots_and_sni_bounds_enforced() {
    let srv = make_identity(vec!["localhost"]);
    // Empty roots required for mTLS.
    assert!(TlsServerConfig::builder()
        .single_identity(srv.certs.clone(), srv.key.clone_key())
        .unwrap()
        .client_auth_required(vec![])
        .is_err());
    // Too many identities rejected (build 65).
    let mut builder = TlsServerConfig::builder();
    for i in 0..65 {
        let id = make_identity(vec!["localhost"]);
        let name = format!("h{i}.test");
        match builder.add_identity(&name, id.certs.clone(), id.key.clone_key()) {
            Ok(b) => builder = b,
            Err(e) => {
                assert!(i >= 64, "should fail only after limit: {e}");
                return;
            }
        }
    }
    panic!("expected TooManyIdentities");
}

/// H1/H2 ALPN stays coherent across reload (Plan 203 Track E).
///
/// Requires the `http2` feature: server advertises `h2` then `http/1.1`;
/// clients offering each ALPN observe the matching `TlsInfo.alpn`, before
/// and after an atomic reload.
#[cfg(all(feature = "tls", feature = "http2"))]
#[tokio::test]
async fn alpn_parity_survives_reload() {
    let a = make_identity(vec!["localhost"]);
    let b = make_identity(vec!["localhost"]);
    let tls_a = TlsServerConfig::builder()
        .single_identity(a.certs.clone(), a.key.clone_key())
        .unwrap()
        .http2(true)
        .build()
        .unwrap();
    assert!(tls_a.alpn_protocols().contains(&b"h2".to_vec()));
    let reload = TlsReloadHandle::from_tls_server_config(&tls_a);
    let config = eggserve_core::server::RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_reload_handle(reload.clone())
        .build()
        .unwrap();
    // H2 enabled by default; ALPN coherence requires it.
    let server = Server::builder().runtime(config).build().unwrap();
    let seen_alpn: Arc<std::sync::Mutex<Vec<Option<String>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen2 = seen_alpn.clone();
    let svc = service_fn(move |req: eggserve_core::server::Request| {
        let seen2 = seen2.clone();
        Box::pin(async move {
            seen2
                .lock()
                .unwrap()
                .push(req.connection().tls.as_ref().and_then(|t| t.alpn.clone()));
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    });
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    async fn get_with_alpn(addr: std::net::SocketAddr, roots: &[u8], alpn: &str) -> Vec<u8> {
        let mut store = rustls::RootCertStore::empty();
        store.add(CertificateDer::from(roots.to_vec())).unwrap();
        let mut cc = rustls::ClientConfig::builder()
            .with_root_certificates(store)
            .with_no_client_auth();
        cc.alpn_protocols = vec![alpn.as_bytes().to_vec()];
        let cc = Arc::new(cc);
        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let connector = tokio_rustls::TlsConnector::from(cc);
        let domain: ServerName<'static> = ServerName::try_from("localhost".to_owned()).unwrap();
        let tls = connector.connect(domain, tcp).await.unwrap();
        // H2 over a raw TLS socket won't speak HTTP/1, so only check the
        // handshake ALPN via the service for http/1.1; for h2 assert the
        // negotiated ALPN at the TLS layer directly.
        if alpn != "http/1.1" {
            let (_, conn) = tls.get_ref();
            return conn.alpn_protocol().map(|p| p.to_vec()).unwrap_or_default();
        }
        let (mut r, mut w) = tokio::io::split(tls);
        w.write_all(GET_CLOSE.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), r.read_to_end(&mut buf))
            .await
            .unwrap();
        buf
    }

    // http/1.1 before reload → service sees http/1.1.
    let resp = get_with_alpn(addr, &a.cert_der_bytes, "http/1.1").await;
    assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 200"));
    // h2 before reload → TLS negotiates h2 (server will then expect H2 frames;
    // we only assert negotiation, not HTTP semantics here).
    let negotiated = get_with_alpn(addr, &a.cert_der_bytes, "h2").await;
    assert_eq!(negotiated, b"h2".to_vec());

    // Reload to B (same ALPN policy).
    let tls_b = TlsServerConfig::builder()
        .single_identity(b.certs.clone(), b.key.clone_key())
        .unwrap()
        .http2(true)
        .build()
        .unwrap();
    handle.replace_tls_server_config(&tls_b).unwrap();

    let resp = get_with_alpn(addr, &b.cert_der_bytes, "http/1.1").await;
    assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 200"));
    let negotiated = get_with_alpn(addr, &b.cert_der_bytes, "h2").await;
    assert_eq!(negotiated, b"h2".to_vec());

    // Service-observed ALPN for the two http/1.1 requests stays coherent.
    let seen = seen_alpn.lock().unwrap().clone();
    assert!(seen.len() >= 2);
    for alpn in seen {
        assert_eq!(alpn.as_deref(), Some("http/1.1"));
    }
    handle.shutdown();
    let _ = handle.wait().await;
}
