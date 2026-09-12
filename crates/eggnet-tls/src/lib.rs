//! Neutral rustls server identity, trust, client-authentication, and reload
//! configuration.
//!
//! This crate intentionally contains no EggServe, Eggress, HTTP runtime, proxy,
//! tracing, CLI, or Python dependencies. Applications own transport adapters,
//! filesystem watching, and operational logging.
//!
//! The crate provides bounded PEM parsing, validated SNI identity selection,
//! explicit WebPKI client authentication, and atomic replacement of immutable
//! snapshots for new handshakes. It does not provide certificate management,
//! filesystem watching, or application-level TLS policy.
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, RwLock};

use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer};

/// Maximum SNI identities per [`TlsServerConfig`] (exact + wildcard combined).
pub const MAX_TLS_IDENTITIES: usize = 64;
/// Maximum DNS length for an SNI identity (DNS limit, without `*.` prefix).
pub const MAX_SNI_LEN: usize = 253;
/// Maximum trust anchors for client authentication.
pub const MAX_TRUST_ROOTS: usize = 256;
/// Maximum CRLs for client authentication.
pub const MAX_CRLS: usize = 16;
/// Maximum certificates per server identity chain.
pub const MAX_IDENTITY_CHAIN: usize = 8;
/// Maximum PEM bytes accepted for trust-root/CRL parsing (1 MiB).
pub const MAX_TRUST_PEM_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub enum TlsError {
    CertFileNotFound(String),
    KeyFileNotFound(String),
    CertReadError(String),
    KeyReadError(String),
    NoCertificatesFound,
    NoPrivateKeyFound,
    MultiplePrivateKeysFound,
    InvalidKey(String),
    InvalidSniName(String),
    DuplicateIdentity(String),
    TooManyIdentities,
    NoIdentities,
    InvalidTrustRoots(String),
    TooManyTrustRoots,
    InvalidCrl(String),
    TooManyCrls,
    TrustRootsRequired,
}

impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CertFileNotFound(path) => write!(f, "certificate file not found: {path}"),
            Self::KeyFileNotFound(path) => write!(f, "key file not found: {path}"),
            Self::CertReadError(msg) => write!(f, "failed to read certificate: {msg}"),
            Self::KeyReadError(msg) => write!(f, "failed to read key: {msg}"),
            Self::NoCertificatesFound => {
                write!(f, "no valid certificates found in certificate file")
            }
            Self::NoPrivateKeyFound => write!(f, "no valid private key found in key file"),
            Self::MultiplePrivateKeysFound => {
                write!(f, "multiple private keys found; exactly one is required")
            }
            Self::InvalidKey(msg) => write!(f, "invalid private key: {msg}"),
            Self::InvalidSniName(msg) => write!(f, "invalid SNI identity: {msg}"),
            Self::DuplicateIdentity(name) => write!(f, "duplicate TLS identity: {name}"),
            Self::TooManyIdentities => {
                write!(f, "too many TLS identities (max {MAX_TLS_IDENTITIES})")
            }
            Self::NoIdentities => write!(f, "no TLS identities configured"),
            Self::InvalidTrustRoots(msg) => write!(f, "invalid trust roots: {msg}"),
            Self::TooManyTrustRoots => {
                write!(f, "too many trust roots (max {MAX_TRUST_ROOTS})")
            }
            Self::InvalidCrl(msg) => write!(f, "invalid CRL: {msg}"),
            Self::TooManyCrls => write!(f, "too many CRLs (max {MAX_CRLS})"),
            Self::TrustRootsRequired => {
                write!(f, "client authentication requires at least one trust root")
            }
        }
    }
}

impl std::error::Error for TlsError {}

/// Client-certificate authentication policy (Plan 203 Track C).
///
/// Built-in verification uses rustls/WebPKI against explicit trust roots.
/// No Python callback is invoked during verification. Revocation is explicit:
/// when no CRL policy is configured, no revocation checking is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientAuthMode {
    /// No client authentication offered (default, backwards compatible).
    #[default]
    Disabled,
    /// Client authentication offered; unauthenticated clients allowed.
    /// Authenticated clients expose verified metadata.
    Optional,
    /// Handshake fails when no acceptable client certificate is supplied.
    Required,
}

/// Load a rustls server configuration from PEM certificate and key files.
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>, TlsError> {
    load_tls_config_with_http2(cert_path, key_path, cfg!(feature = "http2"))
}

/// Load a rustls server configuration and explicitly choose the advertised
/// application protocols.
///
/// Rust CLI callers built with `http2` normally use the default loader, which
/// advertises `h2` before `http/1.1`. Compatibility frontends can pass
/// `false` to keep their HTTP/1.1-only contract even when linked with a core
/// build that has H2 enabled.
pub fn load_tls_config_with_http2(
    cert_path: &Path,
    key_path: &Path,
    http2: bool,
) -> Result<Arc<ServerConfig>, TlsError> {
    let (certs, key) = load_identity(cert_path, key_path)?;
    build_single_cert_config(certs, key, http2)
}

/// Parse one identity from PEM bytes (caller-supplied in-memory material).
///
/// Validates exactly one private key and at least one certificate; key/cert
/// pairing is checked via the active crypto provider before return.
pub fn parse_identity_pem(
    cert_pem: &[u8],
    key_pem: &[u8],
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TlsError> {
    use std::io::Cursor;
    let certs = CertificateDer::pem_reader_iter(Cursor::new(cert_pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::CertReadError(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificatesFound);
    }
    if certs.len() > MAX_IDENTITY_CHAIN {
        return Err(TlsError::InvalidKey(format!(
            "certificate chain too long (max {MAX_IDENTITY_CHAIN})"
        )));
    }
    let mut private_key = None;
    let mut key_count = 0;
    for item in PrivateKeyDer::pem_reader_iter(Cursor::new(key_pem)) {
        let key = item.map_err(|e| TlsError::KeyReadError(e.to_string()))?;
        key_count += 1;
        private_key = Some(key);
    }
    if key_count > 1 {
        return Err(TlsError::MultiplePrivateKeysFound);
    }
    let key = private_key.ok_or(TlsError::NoPrivateKeyFound)?;
    validate_key_pair(&certs, &key)?;
    Ok((certs, key))
}

/// Parse trust anchors from PEM bytes (bounded, validated at load).
pub fn parse_trust_roots_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    if pem.len() > MAX_TRUST_PEM_BYTES {
        return Err(TlsError::InvalidTrustRoots("trust PEM too large".into()));
    }
    use std::io::Cursor;
    let certs = CertificateDer::pem_reader_iter(Cursor::new(pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::InvalidTrustRoots(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError::InvalidTrustRoots("no certificates found".into()));
    }
    if certs.len() > MAX_TRUST_ROOTS {
        return Err(TlsError::TooManyTrustRoots);
    }
    // Validate each anchor parses as a trust anchor now (fail before ready).
    let mut store = rustls::RootCertStore::empty();
    for cert in &certs {
        store
            .add(cert.clone())
            .map_err(|e| TlsError::InvalidTrustRoots(e.to_string()))?;
    }
    Ok(certs)
}

/// Parse CRLs from PEM bytes (bounded, validated at load).
pub fn parse_crls_pem(pem: &[u8]) -> Result<Vec<CertificateRevocationListDer<'static>>, TlsError> {
    if pem.len() > MAX_TRUST_PEM_BYTES {
        return Err(TlsError::InvalidCrl("CRL PEM too large".into()));
    }
    use std::io::Cursor;
    let crls = CertificateRevocationListDer::pem_reader_iter(Cursor::new(pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::InvalidCrl(e.to_string()))?;
    if crls.len() > MAX_CRLS {
        return Err(TlsError::TooManyCrls);
    }
    Ok(crls)
}

fn build_single_cert_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    http2: bool,
) -> Result<Arc<ServerConfig>, TlsError> {
    if certs.len() > MAX_IDENTITY_CHAIN {
        return Err(TlsError::InvalidKey(format!(
            "certificate chain too long (max {MAX_IDENTITY_CHAIN})"
        )));
    }
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
    config.alpn_protocols = advertised_alpn(http2);
    // Conservative defaults: no 0-RTT, rustls default ticket policy (stateless
    // disabled / NeverProducesTickets). Set explicitly so upgrades cannot
    // silently enable early data.
    config.max_early_data_size = 0;
    Ok(Arc::new(config))
}

/// Load one certificate chain and exactly one private key from PEM files.
pub fn load_identity(
    cert_path: &Path,
    key_path: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TlsError> {
    let cert_file = File::open(cert_path)
        .map_err(|_| TlsError::CertFileNotFound(cert_path.display().to_string()))?;
    let cert_reader = BufReader::new(cert_file);
    let certs = CertificateDer::pem_reader_iter(cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::CertReadError(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificatesFound);
    }
    if certs.len() > MAX_IDENTITY_CHAIN {
        return Err(TlsError::InvalidKey(format!(
            "certificate chain too long (max {MAX_IDENTITY_CHAIN})"
        )));
    }

    let key_file = File::open(key_path)
        .map_err(|_| TlsError::KeyFileNotFound(key_path.display().to_string()))?;
    let key_reader = BufReader::new(key_file);
    let mut private_key = None;
    let mut key_count = 0;
    for item in PrivateKeyDer::pem_reader_iter(key_reader) {
        let key = item.map_err(|e| TlsError::KeyReadError(e.to_string()))?;
        key_count += 1;
        private_key = Some(key);
    }
    if key_count > 1 {
        return Err(TlsError::MultiplePrivateKeysFound);
    }
    let key = private_key.ok_or(TlsError::NoPrivateKeyFound)?;
    validate_key_pair(&certs, &key)?;
    Ok((certs, key))
}

/// Validate key/cert pairing early via the ring provider (never defer an
/// obvious mismatch to the first hostile connection).
fn validate_key_pair(
    certs: &[CertificateDer<'static>],
    key: &PrivateKeyDer<'static>,
) -> Result<(), TlsError> {
    let provider = rustls::crypto::ring::default_provider();
    let signing_key = provider
        .key_provider
        .load_private_key(key.clone_key())
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
    let ck = rustls::sign::CertifiedKey::new(certs.to_vec(), signing_key);
    ck.keys_match()
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
    Ok(())
}

/// Normalize and validate an SNI identity name.
///
/// Returns `(is_wildcard, normalized)` where wildcard names keep the `*.`
/// prefix lowercased (e.g. `*.example.com`). Exact names are lowercased DNS
/// names. Validation is DNS-conventional: ASCII, length bounds, label rules;
/// wildcard is single-level only (`*.example.com` matches one label, never
/// bare `example.com` or multi-level).
fn normalize_sni_name(name: &str) -> Result<(bool, String), TlsError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(TlsError::InvalidSniName("empty SNI name".into()));
    }
    if trimmed.len() > MAX_SNI_LEN + 2 {
        return Err(TlsError::InvalidSniName(format!(
            "SNI name too long (max {MAX_SNI_LEN})"
        )));
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("*.") {
        if rest.is_empty() || rest.contains('*') {
            return Err(TlsError::InvalidSniName(
                "wildcard must be exactly '*.suffix'".into(),
            ));
        }
        validate_dns_labels(rest)?;
        // Conventional wildcard: suffix should contain a dot (avoid `*.com`).
        if !rest.contains('.') {
            return Err(TlsError::InvalidSniName(
                "wildcard suffix must contain a dot (e.g. *.example.com)".into(),
            ));
        }
        if rest.len() > MAX_SNI_LEN {
            return Err(TlsError::InvalidSniName(format!(
                "wildcard suffix too long (max {MAX_SNI_LEN})"
            )));
        }
        Ok((true, format!("*.{rest}")))
    } else {
        if lower.contains('*') {
            return Err(TlsError::InvalidSniName(
                "wildcard only allowed as leading '*.'".into(),
            ));
        }
        validate_dns_labels(&lower)?;
        // Extra strictness via rustls DNS parsing for exact names.
        if rustls::pki_types::DnsName::try_from(lower.as_str()).is_err() {
            return Err(TlsError::InvalidSniName(format!(
                "invalid DNS name: {trimmed}"
            )));
        }
        Ok((false, lower))
    }
}

fn validate_dns_labels(name: &str) -> Result<(), TlsError> {
    if name.is_empty() || name.len() > MAX_SNI_LEN {
        return Err(TlsError::InvalidSniName(format!(
            "DNS name length must be 1..={MAX_SNI_LEN}"
        )));
    }
    if name.starts_with('.') || name.ends_with('.') || name.starts_with('-') {
        return Err(TlsError::InvalidSniName(
            "invalid DNS label placement".into(),
        ));
    }
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(TlsError::InvalidSniName("invalid DNS label length".into()));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(TlsError::InvalidSniName(
                "DNS label must not start/end with '-'".into(),
            ));
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(TlsError::InvalidSniName(
                "DNS labels must be ASCII alphanumeric or '-'".into(),
            ));
        }
    }
    Ok(())
}

/// SNI resolver with exact priority, single-level wildcard fallback, and an
/// optional default for no-SNI/no-match clients.
///
/// No blocking filesystem/network IO occurs in [`rustls::server::ResolvesServerCert::resolve`];
/// it is pure in-memory map lookup. SNI input is bounded (253 chars) before
/// use; key/cert bytes are never logged.
#[derive(Debug)]
struct SniResolver {
    exact: HashMap<String, Arc<rustls::sign::CertifiedKey>>,
    wildcards: Vec<(String, Arc<rustls::sign::CertifiedKey>)>,
    default: Option<Arc<rustls::sign::CertifiedKey>>,
}

impl rustls::server::ResolvesServerCert for SniResolver {
    fn resolve(
        &self,
        client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        if let Some(name) = client_hello.server_name() {
            if name.len() > MAX_SNI_LEN {
                return self.default.clone();
            }
            let lower = name.to_ascii_lowercase();
            if let Some(k) = self.exact.get(&lower) {
                return Some(k.clone());
            }
            // Single-level wildcard: `*.example.com` matches `foo.example.com`
            // only (one label, non-empty, no extra dots in the label).
            if let Some(dot) = lower.find('.') {
                let (label, suffix) = lower.split_at(dot);
                let suffix = &suffix[1..];
                if !label.is_empty() && !label.contains('.') {
                    for (w_suffix, key) in &self.wildcards {
                        if suffix == w_suffix {
                            return Some(key.clone());
                        }
                    }
                }
            }
            self.default.clone()
        } else {
            self.default.clone()
        }
    }
}

/// Validated production TLS identity configuration (Plan 203 Tracks A–C).
///
/// Immutable once built; reload via [`TlsReloadHandle`] (all-or-nothing for
/// new handshakes; established connections keep their session). Exposes only
/// sanitized metadata (identity names, client-auth mode, ALPN); never private
/// keys.
#[derive(Debug, Clone)]
pub struct TlsServerConfig {
    config: Arc<ServerConfig>,
    identities: Vec<String>,
    has_default: bool,
    client_auth: ClientAuthMode,
    alpn: Vec<Vec<u8>>,
}

impl TlsServerConfig {
    /// Builder for [`TlsServerConfig`].
    pub fn builder() -> TlsServerConfigBuilder {
        TlsServerConfigBuilder::new()
    }

    /// The validated rustls server configuration for new handshakes.
    pub fn server_config(&self) -> &Arc<ServerConfig> {
        &self.config
    }

    /// Clone the inner server configuration (for reload handles / acceptors).
    pub fn into_server_config(&self) -> Arc<ServerConfig> {
        self.config.clone()
    }

    /// Configured SNI identity names (exact and `*.` wildcards, lowercased).
    pub fn identities(&self) -> &[String] {
        &self.identities
    }

    /// Whether a default identity exists for no-SNI/no-match clients.
    pub fn has_default_identity(&self) -> bool {
        self.has_default
    }

    /// Configured client-authentication policy.
    pub fn client_auth(&self) -> ClientAuthMode {
        self.client_auth
    }

    /// Advertised ALPN protocols.
    pub fn alpn_protocols(&self) -> &[Vec<u8>] {
        &self.alpn
    }
}

/// Builder for [`TlsServerConfig`].
#[derive(Debug, Default)]
pub struct TlsServerConfigBuilder {
    identities: Vec<(String, Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>,
    default: Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>,
    client_auth: ClientAuthMode,
    roots: Vec<CertificateDer<'static>>,
    crls: Vec<CertificateRevocationListDer<'static>>,
    http2: bool,
}

impl TlsServerConfigBuilder {
    /// Create an empty builder (ALPN defaults to crate `http2` feature).
    pub fn new() -> Self {
        Self {
            http2: cfg!(feature = "http2"),
            ..Default::default()
        }
    }

    /// Advertise H2 before HTTP/1.1 when `true` (default follows the crate
    /// `http2` feature). Coherent with `RuntimeConfig::http2.enabled`.
    pub fn http2(mut self, enabled: bool) -> Self {
        self.http2 = enabled;
        self
    }

    /// Add one SNI identity (exact DNS or `*.suffix` wildcard).
    pub fn add_identity(
        mut self,
        name: &str,
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self, TlsError> {
        if self.identities.len() + usize::from(self.default.is_some()) >= MAX_TLS_IDENTITIES {
            return Err(TlsError::TooManyIdentities);
        }
        let (is_wildcard, normalized) = normalize_sni_name(name)?;
        let _ = is_wildcard;
        if self.identities.iter().any(|(n, _, _)| n == &normalized) {
            return Err(TlsError::DuplicateIdentity(normalized));
        }
        if certs.is_empty() {
            return Err(TlsError::NoCertificatesFound);
        }
        if certs.len() > MAX_IDENTITY_CHAIN {
            return Err(TlsError::InvalidKey(format!(
                "certificate chain too long (max {MAX_IDENTITY_CHAIN})"
            )));
        }
        validate_key_pair(&certs, &key)?;
        self.identities.push((normalized, certs, key));
        Ok(self)
    }

    /// Set the default identity for no-SNI/no-match clients.
    pub fn default_identity(
        mut self,
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self, TlsError> {
        if certs.is_empty() {
            return Err(TlsError::NoCertificatesFound);
        }
        if certs.len() > MAX_IDENTITY_CHAIN {
            return Err(TlsError::InvalidKey(format!(
                "certificate chain too long (max {MAX_IDENTITY_CHAIN})"
            )));
        }
        validate_key_pair(&certs, &key)?;
        self.default = Some((certs, key));
        Ok(self)
    }

    /// Convenience for a single identity (default-only, SNI-agnostic).
    pub fn single_identity(
        self,
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self, TlsError> {
        self.default_identity(certs, key)
    }

    /// Disable client authentication (default).
    pub fn client_auth_disabled(mut self) -> Self {
        self.client_auth = ClientAuthMode::Disabled;
        self.roots.clear();
        self.crls.clear();
        self
    }

    /// Require client certificates verified against `roots`.
    pub fn client_auth_required(
        mut self,
        roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, TlsError> {
        if roots.is_empty() {
            return Err(TlsError::TrustRootsRequired);
        }
        if roots.len() > MAX_TRUST_ROOTS {
            return Err(TlsError::TooManyTrustRoots);
        }
        self.client_auth = ClientAuthMode::Required;
        self.roots = roots;
        Ok(self)
    }

    /// Offer client authentication; allow unauthenticated clients.
    pub fn client_auth_optional(
        mut self,
        roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, TlsError> {
        if roots.is_empty() {
            return Err(TlsError::TrustRootsRequired);
        }
        if roots.len() > MAX_TRUST_ROOTS {
            return Err(TlsError::TooManyTrustRoots);
        }
        self.client_auth = ClientAuthMode::Optional;
        self.roots = roots;
        Ok(self)
    }

    /// Attach CRLs to the current client-auth policy (explicit revocation).
    ///
    /// Without CRLs no revocation checking is implied. Bounded and validated
    /// at build time.
    pub fn with_crls(
        mut self,
        crls: Vec<CertificateRevocationListDer<'static>>,
    ) -> Result<Self, TlsError> {
        if crls.len() > MAX_CRLS {
            return Err(TlsError::TooManyCrls);
        }
        self.crls = crls;
        Ok(self)
    }

    /// Build the validated [`TlsServerConfig`].
    ///
    /// Fails before readiness on invalid names, key/cert mismatch, empty
    /// trust, or unparsable CRLs. Never defers obvious pairing errors to the
    /// first hostile connection.
    pub fn build(self) -> Result<TlsServerConfig, TlsError> {
        if self.identities.is_empty() && self.default.is_none() {
            return Err(TlsError::NoIdentities);
        }
        if self.client_auth != ClientAuthMode::Disabled && self.roots.is_empty() {
            return Err(TlsError::TrustRootsRequired);
        }
        let provider = rustls::crypto::ring::default_provider();
        let mut exact: HashMap<String, Arc<rustls::sign::CertifiedKey>> = HashMap::new();
        let mut wildcards: Vec<(String, Arc<rustls::sign::CertifiedKey>)> = Vec::new();
        let mut identity_names: Vec<String> = Vec::new();
        for (name, certs, key) in self.identities {
            let signing_key = provider
                .key_provider
                .load_private_key(key)
                .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
            let ck = Arc::new(rustls::sign::CertifiedKey::new(certs, signing_key));
            ck.keys_match()
                .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
            if let Some(suffix) = name.strip_prefix("*.") {
                wildcards.push((suffix.to_owned(), ck));
            } else {
                exact.insert(name.clone(), ck);
            }
            identity_names.push(name);
        }
        let default = match self.default {
            Some((certs, key)) => {
                let signing_key = provider
                    .key_provider
                    .load_private_key(key)
                    .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
                let ck = Arc::new(rustls::sign::CertifiedKey::new(certs, signing_key));
                ck.keys_match()
                    .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
                Some(ck)
            }
            None => None,
        };
        let has_default = default.is_some();
        let resolver = Arc::new(SniResolver {
            exact,
            wildcards,
            default,
        });
        let verifier = build_client_verifier(self.client_auth, &self.roots, self.crls)?;
        let mut config = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_cert_resolver(resolver);
        let alpn = advertised_alpn(self.http2);
        config.alpn_protocols = alpn.clone();
        config.max_early_data_size = 0;
        identity_names.sort();
        Ok(TlsServerConfig {
            config: Arc::new(config),
            identities: identity_names,
            has_default,
            client_auth: self.client_auth,
            alpn,
        })
    }
}

fn build_client_verifier(
    mode: ClientAuthMode,
    roots: &[CertificateDer<'static>],
    crls: Vec<CertificateRevocationListDer<'static>>,
) -> Result<Arc<dyn rustls::server::danger::ClientCertVerifier>, TlsError> {
    use rustls::server::WebPkiClientVerifier;
    match mode {
        ClientAuthMode::Disabled => Ok(WebPkiClientVerifier::no_client_auth()),
        ClientAuthMode::Optional | ClientAuthMode::Required => {
            if roots.is_empty() {
                return Err(TlsError::TrustRootsRequired);
            }
            let mut store = rustls::RootCertStore::empty();
            for root in roots {
                store
                    .add(root.clone())
                    .map_err(|e| TlsError::InvalidTrustRoots(e.to_string()))?;
            }
            let builder = WebPkiClientVerifier::builder(Arc::new(store));
            let builder = if crls.is_empty() {
                builder
            } else {
                builder.with_crls(crls)
            };
            let builder = match mode {
                ClientAuthMode::Optional => builder.allow_unauthenticated(),
                _ => builder,
            };
            builder
                .build()
                .map_err(|e| TlsError::InvalidTrustRoots(e.to_string()))
        }
    }
}

/// Atomic reload handle for TLS identity/trust state (Plan 203 Track F).
///
/// Immutable validated [`ServerConfig`] snapshots are swapped atomically;
/// new handshakes read the current snapshot while established connections
/// keep their session. Reload is all-or-nothing: build the replacement
/// [`TlsServerConfig`] first (which validates), then [`Self::replace`];
/// a failed build never touches the live snapshot.
///
/// No filesystem watcher is provided; operators load PEM and call replace
/// when they decide files changed.
#[derive(Clone, Debug)]
pub struct TlsReloadHandle {
    inner: Arc<RwLock<Arc<ServerConfig>>>,
}

impl TlsReloadHandle {
    /// Create a handle around an initial validated configuration.
    pub fn new(initial: Arc<ServerConfig>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(initial)),
        }
    }

    /// Create a handle from a validated [`TlsServerConfig`].
    pub fn from_tls_server_config(config: &TlsServerConfig) -> Self {
        Self::new(config.into_server_config())
    }

    /// Current snapshot for the next handshake (cheap `Arc` clone).
    pub fn current(&self) -> Arc<ServerConfig> {
        self.inner
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// Atomically replace the snapshot for new handshakes.
    ///
    /// Existing connections are unaffected. Never fails; the caller must
    /// validate via [`TlsServerConfigBuilder::build`] before calling so a
    /// failed build cannot corrupt live state.
    pub fn replace(&self, next: Arc<ServerConfig>) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = next;
        }
    }

    /// Replace from a validated [`TlsServerConfig`].
    pub fn replace_tls_server_config(&self, next: &TlsServerConfig) {
        self.replace(next.into_server_config());
    }
}

fn advertised_alpn(http2: bool) -> Vec<Vec<u8>> {
    if http2 {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    } else {
        vec![b"http/1.1".to_vec()]
    }
}

#[cfg(test)]
mod tests {
    use super::{advertised_alpn, normalize_sni_name};

    #[test]
    fn h2_alpn_is_preferred_only_when_enabled() {
        assert_eq!(
            advertised_alpn(true),
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
        assert_eq!(advertised_alpn(false), vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn sni_names_validate() {
        assert!(normalize_sni_name("example.com").is_ok());
        assert!(normalize_sni_name("LOCALHOST").is_ok());
        assert!(normalize_sni_name("*.example.com").is_ok());
        assert!(normalize_sni_name("").is_err());
        assert!(normalize_sni_name("*").is_err());
        assert!(normalize_sni_name("*.com").is_err());
        assert!(normalize_sni_name("*.example.com.evil.*").is_err());
        assert!(normalize_sni_name("bad_host!").is_err());
        let (wild, norm) = normalize_sni_name("*.Example.COM").unwrap();
        assert!(wild);
        assert_eq!(norm, "*.example.com");
        let (wild, norm) = normalize_sni_name("Example.COM").unwrap();
        assert!(!wild);
        assert_eq!(norm, "example.com");
    }
}
