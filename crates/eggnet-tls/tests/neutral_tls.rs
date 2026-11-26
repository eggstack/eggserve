//! Integration coverage for the neutral TLS substrate.

use std::sync::Arc;

use eggnet_tls::{
    http_alpn_protocols, load_identity, load_tls_config_with_alpn, parse_crls_pem,
    parse_identity_pem, parse_trust_roots_pem, ClientAuthMode, TlsError, TlsReloadHandle,
    TlsServerConfig, MAX_ALPN_PROTOCOLS, MAX_ALPN_PROTOCOL_LEN, MAX_IDENTITY_CHAIN,
    MAX_IDENTITY_PEM_BYTES, MAX_TLS_IDENTITIES, MAX_TRUST_PEM_BYTES, MAX_TRUST_ROOTS,
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

/// The named-identity ceiling is order-independent and excludes the optional
/// default identity; a 65th named identity is rejected.
#[test]
fn identity_ceiling_excludes_the_optional_default() {
    let shared = identity("shared.test");

    let mut builder = TlsServerConfig::builder()
        .single_identity(shared.certs.clone(), shared.key.clone_key())
        .expect("default identity");
    for i in 0..MAX_TLS_IDENTITIES {
        builder = builder
            .add_identity(
                &format!("h{i}.test"),
                shared.certs.clone(),
                shared.key.clone_key(),
            )
            .expect("identity within the documented ceiling");
    }
    let built = builder.build().expect("ceiling build");
    assert_eq!(built.identities().len(), MAX_TLS_IDENTITIES);
    assert!(built.has_default_identity());

    let mut overflow = TlsServerConfig::builder();
    for i in 0..MAX_TLS_IDENTITIES {
        overflow = overflow
            .add_identity(
                &format!("o{i}.test"),
                shared.certs.clone(),
                shared.key.clone_key(),
            )
            .expect("identity within the documented ceiling");
    }
    assert!(matches!(
        overflow.add_identity(
            "overflow.test",
            shared.certs.clone(),
            shared.key.clone_key()
        ),
        Err(TlsError::TooManyIdentities)
    ));
}

/// Plan 324: identity PEM input is bounded before parsing, and a valid
/// identity at the ceiling still parses.
#[test]
fn identity_pem_size_bound_is_enforced_before_parsing() {
    use std::io::Write;

    init_tls();
    let key_pair = rcgen::KeyPair::generate().expect("key pair");
    let params =
        rcgen::CertificateParams::new(vec!["bounded.test".to_owned()]).expect("certificate params");
    let cert = params.self_signed(&key_pair).expect("certificate");
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // One byte over the ceiling: rejected before the PEM readers run.
    let mut oversized_cert = cert_pem.clone();
    oversized_cert.push_str(&"\n".repeat(MAX_IDENTITY_PEM_BYTES + 1 - oversized_cert.len()));
    assert_eq!(oversized_cert.len(), MAX_IDENTITY_PEM_BYTES + 1);
    assert!(matches!(
        parse_identity_pem(oversized_cert.as_bytes(), key_pem.as_bytes()),
        Err(TlsError::CertReadError(_))
    ));

    let mut oversized_key = key_pem.clone();
    oversized_key.push_str(&"\n".repeat(MAX_IDENTITY_PEM_BYTES + 1 - oversized_key.len()));
    assert!(matches!(
        parse_identity_pem(cert_pem.as_bytes(), oversized_key.as_bytes()),
        Err(TlsError::KeyReadError(_))
    ));

    // Exactly at the ceiling: padding is ignored by the PEM reader and the
    // identity still parses and pairs.
    let mut at_ceiling = cert_pem.clone();
    at_ceiling.push_str(&"\n".repeat(MAX_IDENTITY_PEM_BYTES - at_ceiling.len()));
    assert_eq!(at_ceiling.len(), MAX_IDENTITY_PEM_BYTES);
    let parsed = parse_identity_pem(at_ceiling.as_bytes(), key_pem.as_bytes())
        .expect("identity at the ceiling must parse");
    assert_eq!(parsed.0.len(), 1);

    // File path: the same ceiling applies without relying on metadata.
    let dir =
        std::env::temp_dir().join(format!("eggnet-tls-identity-bound-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    let write = |path: &std::path::Path, contents: &str| {
        let mut file = std::fs::File::create(path).expect("create fixture");
        file.write_all(contents.as_bytes()).expect("write fixture");
    };

    write(&cert_path, &cert_pem);
    write(&key_path, &key_pem);
    let loaded = load_identity(&cert_path, &key_path).expect("normal identity loads");
    assert_eq!(loaded.0.len(), 1);

    write(&cert_path, &oversized_cert);
    assert!(matches!(
        load_identity(&cert_path, &key_path),
        Err(TlsError::CertReadError(_))
    ));

    write(&cert_path, &cert_pem);
    write(&key_path, &oversized_key);
    assert!(matches!(
        load_identity(&cert_path, &key_path),
        Err(TlsError::KeyReadError(_))
    ));

    write(&key_path, &key_pem);
    assert!(load_identity(&cert_path, &key_path).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}
