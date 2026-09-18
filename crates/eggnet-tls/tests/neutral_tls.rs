use std::sync::Arc;

use eggnet_tls::{
    http_alpn_protocols, load_tls_config_with_alpn, parse_crls_pem, parse_identity_pem,
    parse_trust_roots_pem, ClientAuthMode, TlsError, TlsReloadHandle, TlsServerConfig,
    MAX_ALPN_PROTOCOLS, MAX_ALPN_PROTOCOL_LEN, MAX_IDENTITY_CHAIN, MAX_TRUST_PEM_BYTES,
    MAX_TRUST_ROOTS,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls_pki_types::pem::PemObject;

fn init_tls() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

struct Identity {
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    cert_der: Vec<u8>,
}

fn identity(name: &str) -> Identity {
    init_tls();
    let key_pair = rcgen::KeyPair::generate().expect("key pair");
    let params = rcgen::CertificateParams::new(vec![name.to_owned()]).expect("certificate params");
    let cert = params.self_signed(&key_pair).expect("certificate");
    let cert_der: CertificateDer<'static> = cert.into();
    let cert_der_bytes = cert_der.as_ref().to_vec();
    let key = PrivatePkcs8KeyDer::from(key_pair.serialize_der()).into();
    Identity {
        certs: vec![cert_der],
        key,
        cert_der: cert_der_bytes,
    }
}

fn client_config(server: &Identity, client: Option<&Identity>) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(server.cert_der.clone()))
        .expect("server root");
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let mut config = match client {
        Some(client) => builder
            .with_client_auth_cert(client.certs.clone(), client.key.clone_key())
            .expect("client identity"),
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
}

async fn handshake(
    server: &Identity,
    tls: &TlsServerConfig,
    client: Option<&Identity>,
) -> Result<(), String> {
    let (server_io, client_io) = tokio::io::duplex(16 * 1024);
    let acceptor = tokio_rustls::TlsAcceptor::from(tls.server_config().clone());
    let connector = tokio_rustls::TlsConnector::from(client_config(server, client));
    let server_task = tokio::spawn(async move { acceptor.accept(server_io).await });
    let domain = ServerName::try_from("server.test".to_owned()).expect("server name");
    let client_result = connector.connect(domain, client_io).await;
    match (client_result, server_task.await.expect("server task")) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(err), _) => Err(err.to_string()),
        (Ok(_), Err(err)) => Err(err.to_string()),
    }
}

#[test]
fn valid_identity_and_pem_failures_are_checked_before_build() {
    let server = identity("server.test");
    let key_pem = PrivateKeyDer::from_pem_slice(b"not pem");
    assert!(key_pem.is_err());
    assert!(matches!(
        parse_identity_pem(b"not pem", b"not pem"),
        Err(TlsError::NoCertificatesFound)
    ));
    assert!(TlsServerConfig::builder()
        .single_identity(server.certs, server.key)
        .expect("valid identity")
        .build()
        .is_ok());
}

#[test]
fn identity_and_trust_bounds_are_enforced() {
    let server = identity("server.test");
    let too_many_chain = vec![server.certs[0].clone(); MAX_IDENTITY_CHAIN + 1];
    assert!(matches!(
        TlsServerConfig::builder().single_identity(too_many_chain, server.key.clone_key()),
        Err(TlsError::InvalidKey(_))
    ));
    assert!(matches!(
        TlsServerConfig::builder()
            .client_auth_required(vec![server.certs[0].clone(); MAX_TRUST_ROOTS + 1]),
        Err(TlsError::TooManyTrustRoots)
    ));
    assert!(matches!(
        parse_trust_roots_pem(&vec![0; MAX_TRUST_PEM_BYTES + 1]),
        Err(TlsError::InvalidTrustRoots(_))
    ));
    assert!(matches!(
        parse_crls_pem(b"-----BEGIN X509 CRL-----\nnot base64\n-----END X509 CRL-----"),
        Err(TlsError::InvalidCrl(_))
    ));
}

#[test]
fn sni_rules_are_exact_and_single_level() {
    let server = identity("server.test");
    assert!(TlsServerConfig::builder()
        .add_identity("example.test", server.certs.clone(), server.key.clone_key())
        .is_ok());
    for invalid in ["*", "*.com", "a.*.example.test", "bad_host!"] {
        assert!(matches!(
            TlsServerConfig::builder().add_identity(
                invalid,
                server.certs.clone(),
                server.key.clone_key()
            ),
            Err(TlsError::InvalidSniName(_))
        ));
    }
}

#[test]
fn reload_replacement_is_snapshot_based() {
    let a = identity("server.test");
    let b = identity("server.test");
    let tls_a = TlsServerConfig::builder()
        .single_identity(a.certs, a.key)
        .expect("identity A")
        .build()
        .expect("config A");
    let tls_b = TlsServerConfig::builder()
        .single_identity(b.certs, b.key)
        .expect("identity B")
        .build()
        .expect("config B");
    let reload = TlsReloadHandle::from_tls_server_config(&tls_a);
    let before = reload.current();
    reload.replace_tls_server_config(&tls_b);
    assert!(!Arc::ptr_eq(&before, &reload.current()));
}

#[tokio::test]
async fn client_auth_modes_have_the_documented_semantics() {
    let server = identity("server.test");
    let good_client = identity("client.test");
    let bad_client = identity("bad-client.test");

    let disabled = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("disabled identity")
        .build()
        .expect("disabled config");
    assert_eq!(disabled.client_auth(), ClientAuthMode::Disabled);
    handshake(&server, &disabled, None)
        .await
        .expect("disabled no cert");

    let optional = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("optional identity")
        .client_auth_optional(good_client.certs.clone())
        .expect("optional roots")
        .build()
        .expect("optional config");
    assert_eq!(optional.client_auth(), ClientAuthMode::Optional);
    handshake(&server, &optional, None)
        .await
        .expect("optional no cert");
    handshake(&server, &optional, Some(&good_client))
        .await
        .expect("optional valid cert");
    assert!(handshake(&server, &optional, Some(&bad_client))
        .await
        .is_err());

    let required = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("required identity")
        .client_auth_required(good_client.certs.clone())
        .expect("required roots")
        .build()
        .expect("required config");
    assert_eq!(required.client_auth(), ClientAuthMode::Required);
    assert!(handshake(&server, &required, None).await.is_err());
    handshake(&server, &required, Some(&good_client))
        .await
        .expect("required valid cert");
    assert!(handshake(&server, &required, Some(&bad_client))
        .await
        .is_err());
}

/// The neutral ALPN hook (Plan 222) lets non-HTTP transports advertise their
/// own protocols; the `http2` convenience stays HTTP-only.
#[test]
fn alpn_hook_is_neutral_and_validated() {
    let server = identity("server.test");

    // HTTP convenience keeps its documented shape.
    assert_eq!(
        http_alpn_protocols(true),
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    );
    assert_eq!(http_alpn_protocols(false), vec![b"http/1.1".to_vec()]);

    // Explicit protocols override the HTTP default; empty means no ALPN.
    let custom = vec![b"eggress-tunnel".to_vec()];
    let tls = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("identity")
        .alpn_protocols(custom.clone())
        .expect("custom ALPN")
        .build()
        .expect("custom config");
    assert_eq!(tls.alpn_protocols(), custom.as_slice());
    let none = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("identity")
        .alpn_protocols(Vec::new())
        .expect("empty ALPN")
        .build()
        .expect("empty config");
    assert!(none.alpn_protocols().is_empty());

    // Bounds fail before readiness.
    assert!(matches!(
        TlsServerConfig::builder()
            .single_identity(server.certs.clone(), server.key.clone_key())
            .expect("identity")
            .alpn_protocols(vec![vec![b'x'; MAX_ALPN_PROTOCOL_LEN + 1]]),
        Err(TlsError::InvalidAlpn(_))
    ));
    assert!(matches!(
        TlsServerConfig::builder()
            .single_identity(server.certs.clone(), server.key.clone_key())
            .expect("identity")
            .alpn_protocols(vec![Vec::new()]),
        Err(TlsError::InvalidAlpn(_))
    ));
    assert!(matches!(
        TlsServerConfig::builder()
            .single_identity(server.certs.clone(), server.key.clone_key())
            .expect("identity")
            .alpn_protocols(vec![b"p".to_vec(); MAX_ALPN_PROTOCOLS + 1]),
        Err(TlsError::InvalidAlpn(_))
    ));

    // Last call wins between the HTTP convenience and the neutral hook.
    let http_wins = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("identity")
        .alpn_protocols(custom.clone())
        .expect("custom ALPN")
        .http2(true)
        .build()
        .expect("http-wins config");
    assert_eq!(
        http_wins.alpn_protocols(),
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    );
    let custom_wins = TlsServerConfig::builder()
        .single_identity(server.certs, server.key.clone_key())
        .expect("identity")
        .http2(true)
        .alpn_protocols(custom.clone())
        .expect("custom ALPN")
        .build()
        .expect("custom-wins config");
    assert_eq!(custom_wins.alpn_protocols(), custom.as_slice());
}

/// The file loader with explicit ALPN validates protocols before touching
/// identity material, so non-HTTP transports fail fast on bad ALPN.
#[test]
fn file_loader_with_alpn_validates_before_identity() {
    use std::path::Path;
    // Invalid ALPN is rejected even when the paths do not exist.
    assert!(matches!(
        load_tls_config_with_alpn(
            Path::new("/nonexistent/cert.pem"),
            Path::new("/nonexistent/key.pem"),
            vec![Vec::new()],
        ),
        Err(TlsError::InvalidAlpn(_))
    ));
    // Missing files still report missing files (not ALPN) for valid ALPN.
    assert!(matches!(
        load_tls_config_with_alpn(
            Path::new("/nonexistent/cert.pem"),
            Path::new("/nonexistent/key.pem"),
            vec![b"eggress-tunnel".to_vec()],
        ),
        Err(TlsError::CertFileNotFound(_))
    ));
}

/// A non-HTTP ALPN negotiates end to end over the neutral configuration.
#[tokio::test]
async fn custom_alpn_negotiates_end_to_end() {
    let server = identity("server.test");
    let tls = TlsServerConfig::builder()
        .single_identity(server.certs.clone(), server.key.clone_key())
        .expect("identity")
        .alpn_protocols(vec![b"eggress-tunnel".to_vec()])
        .expect("custom ALPN")
        .build()
        .expect("custom config");

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(server.cert_der.clone()))
        .expect("server root");
    let mut client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"eggress-tunnel".to_vec()];
    let client = Arc::new(client);

    let (server_io, client_io) = tokio::io::duplex(16 * 1024);
    let acceptor = tokio_rustls::TlsAcceptor::from(tls.server_config().clone());
    let connector = tokio_rustls::TlsConnector::from(client);
    let server_task = tokio::spawn(async move { acceptor.accept(server_io).await });
    let domain = ServerName::try_from("server.test".to_owned()).expect("server name");
    let client_conn = connector
        .connect(domain, client_io)
        .await
        .expect("client handshake");
    let server_conn = server_task
        .await
        .expect("server task")
        .expect("server handshake");
    assert_eq!(
        client_conn.get_ref().1.alpn_protocol(),
        Some(b"eggress-tunnel".as_slice())
    );
    assert_eq!(
        server_conn.get_ref().1.alpn_protocol(),
        Some(b"eggress-tunnel".as_slice())
    );
}
