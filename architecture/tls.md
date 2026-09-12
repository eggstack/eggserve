# TLS Support Deep Dive

eggserve supports TLS via rustls, enabled through the `tls` feature flag. TLS is optional — eggserve defaults to plain HTTP for local development use cases.

## Feature Flags

| Feature | Crate | Purpose |
|---------|-------|---------|
| `http2` | `eggserve-core`, `eggserve-bin` | Experimental HTTP/2 server path, bounded H2 prior knowledge and protocol config; see [HTTP/2 qualification](http2.md) |
| `http3` | `eggserve-core`, `eggserve-bin` | Experimental HTTP/3/QUIC server path; enables `tls`, h3 ALPN, and same-port UDP lifecycle; see [HTTP/3 boundary](http3.md) |
| `tls` | `eggnet-tls`, `eggserve-core`, `eggserve-bin` | Neutral rustls identity/trust policy plus EggServe async TLS transport |

## Dependencies

When `tls` is enabled in `eggserve-core`:

- `eggnet-tls` — neutral bounded identity, SNI, WebPKI client-auth, trust/CRL,
  and reload policy (runtime dependencies: `rustls` and `rustls-pki-types`)
- `rustls` / `tokio-rustls` — EggServe's transport-facing TLS and HTTP/3 QUIC
  assembly

`eggserve-bin` enables `eggserve-core/tls` and re-exports the module
(`bin/src/tls.rs` is `pub use eggserve_core::tls::*`). The historical
`eggserve_core::tls` module re-exports `eggnet_tls`; only HTTP/3-specific QUIC
assembly remains in the compatibility module.

The `http3` feature builds a separate Quinn rustls configuration from the
PEM identity supplied through `ServerBuilder::http3_identity`. It restricts
QUIC TLS to TLS 1.3, advertises only `h3`, and sets early data to zero. The
TCP rustls `ServerConfig` is never reused for QUIC because its ALPN and
protocol configuration are transport-specific.

## Server TLS

### Loading Configuration

**Location:** `eggserve-core::tls::load_tls_config()`

```rust
pub fn load_tls_config(
    cert_path: &Path,
    key_path: &Path,
) -> Result<Arc<ServerConfig>, TlsError>
```

The function:
1. Opens and reads the certificate file (PEM format)
2. Parses all certificates from the PEM stream
3. Opens and reads the key file (PEM format)
4. Parses the private key, supporting:
   - PKCS#1 (`RSA PRIVATE KEY`)
   - PKCS#8 (`PRIVATE KEY`)
   - SEC1 (`EC PRIVATE KEY`)
5. Validates exactly one private key is present
6. Builds a `rustls::ServerConfig` with no client authentication

### Key Formats

| Format | PEM Header | Supported |
|--------|-----------|-----------|
| PKCS#1 | `RSA PRIVATE KEY` | Yes |
| PKCS#8 | `PRIVATE KEY` | Yes |
| SEC1 | `EC PRIVATE KEY` | Yes |
| PKCS#12 | (binary) | No |

### Error Types

**Location:** `eggserve-core::tls::TlsError`

| Variant | Meaning |
|---------|---------|
| `CertFileNotFound` | Certificate file does not exist |
| `KeyFileNotFound` | Key file does not exist |
| `CertReadError` | Failed to read/parse certificate PEM |
| `KeyReadError` | Failed to read/parse key PEM |
| `NoCertificatesFound` | PEM file contains no valid certificates |
| `NoPrivateKeyFound` | PEM file contains no valid private key |
| `MultiplePrivateKeysFound` | PEM file contains multiple private keys |
| `InvalidKey` | Key does not match certificate or is invalid |

### CLI Usage

```sh
eggserve --directory /path/to/files --tls-cert /path/to/combined.pem
eggserve --directory /path/to/files \
    --tls-cert /path/to/cert.pem \
    --tls-key /path/to/key.pem
```

`--tls-cert` is required. If `--tls-key` is omitted, the certificate path is
also used as the key path, allowing a combined PEM file. A key-only
configuration remains invalid.

### Python Usage

```python
from eggserve.server import HTTPSServer, SimpleHTTPRequestHandler

server = HTTPSServer(
    ("127.0.0.1", 8443),
    SimpleHTTPRequestHandler,
    certfile="cert.pem",
    keyfile="key.pem",
    directory="/path/to/files",
)
server.serve_forever()
```

### Server Types

| Type | Purpose |
|------|---------|
| `HTTPSServer` | Single-threaded HTTPS server |
| `ThreadingHTTPSServer` | Multi-threaded HTTPS server |

Both accept `certfile` and `keyfile` keyword arguments.

## ALPN

The default `tls` build advertises HTTP/1.1 only. Builds with both `http2` and
`tls` advertise `h2` before `http/1.1`; the completed rustls handshake selects
the corresponding strict Hyper driver, so an ALPN/state-machine mismatch is
not silently accepted. Python compatibility builds do not enable `http2` and
remain HTTP/1.1-only.

Cleartext H2 uses prior knowledge, not HTTP/1 `Upgrade: h2c`. A caller-owned
multi-protocol connection uses a bounded preface classifier so the existing
HTTP/1 header timeout still applies while protocol selection is pending.

## Security Considerations

### What TLS Protects

- Encrypts request/response data in transit
- Prevents passive network sniffing
- Provides server identity via certificate

### What TLS Does Not Protect

- Does not provide end-to-end encryption (reverse proxies terminate TLS)
- Does not protect against application-level attacks

### Deployment Profiles

| Profile | TLS | Use Case |
|---------|-----|----------|
| Local development | No | `eggserve` on localhost |
| Reverse proxy | Proxy terminates | Production behind nginx/Caddy |
| Direct HTTPS | Yes | Public-facing, no reverse proxy |

See [docs/deployment.md](../docs/deployment.md) for deployment guidance.

## Production identity (Plan 203)

`eggnet_tls::TlsServerConfigBuilder` / `TlsServerConfig` (re-exported by
`eggserve-core::tls`)
(multi-identity SNI via a custom `ResolvesServerCert`: exact priority, then
single-level `*.suffix`, then optional default; no IO in `resolve`), WebPKI
client auth (`Disabled` / `Optional` / `Required` + bounded roots/CRLs), and
`TlsReloadHandle` (atomic `Arc<RwLock<Arc<ServerConfig>>>` snapshot for new
handshakes). Construction validates DNS syntax, key/cert pairing
(`keys_match`), and trust/CRLs before readiness; SNI is bounded (253) before
observability; key bytes never enter logs. `RuntimeConfig.tls_reload_handle`
wins over legacy `tls_config` when both are set; `tls_expose_peer_chain`
(default `false`) gates the bounded DER chain (8 × 64 KiB) in the extended
`TlsInfo` (`protocol_version` / `server_name` / `alpn` / `client_authenticated`
/ `peer_certificates_present` / `peer_certificate_chain`). Accept order is
`TCP → PROXY → TLS deadline → ALPN → HTTP`; ALPN derives from
`http2.enabled`; `max_early_data_size = 0` and `NeverProducesTickets` are
explicit. H3 keeps a separate TLS 1.3/`h3` QUIC config; TCP reload does not
atomically rotate H3 (endpoint replacement/drain required). Qualification:
`crates/eggserve-core/tests/tls_identity.rs`.

## Limitations

1. **Experimental H2 scope** — Plans 186, 190, and 191 keep H2 experimental. The
   deterministic suite and Linux wire checks pass, while browser/platform
   evidence, trailer-scope determinism, and a public safe per-stream reset
   hook remain open; HTTP/1 upgrades remain unavailable, and HTTP/3 is separately
   feature-gated and experimental. See
   [the qualification record](../release/plan-191-http2-supported-tier-qualification.md).
2. **Experimental HTTP/3 scope** — The `http3` feature creates a separate
   TLS 1.3/`h3` QUIC configuration and disables application 0-RTT. Plans 188,
   190, 192, and 193 close with H3 still experimental because independent-client,
   adversarial-wire, and cross-platform runtime evidence is incomplete, and
   Plan 192 additionally blocks on upstream `hyperium/h3#338` (no released
   fix) and the `#262` stream-drop remainder; Plan 193 re-checked both issues
   and retained the experimental tier. See
   [the Plan 192 readiness record](../release/plan-192-http3-dependency-readiness.md)
   and [the Plan 193 promotion record](../release/plan-193-http3-supported-tier-qualification.md).
3. **No OCSP stapling** — Not implemented (and no implied revocation without CRLs)
4. **No certificate management** — No ACME, renewal, discovery, secret storage, KMS, or watcher
5. **No Mozilla root bundle dependency for serving** — Server identities and mTLS trust are operator-supplied; no implicit system roots
5. **No stateful session tickets / no 0-RTT** — Explicit `max_early_data_size = 0`, `NeverProducesTickets` default
6. **TCP TLS protocol mode** — Direct TCP TLS uses rustls defaults (TLS 1.2 +
   1.3); the separate QUIC/H3 configuration is TLS 1.3-only.
7. **CLI single-identity** — Multi-identity/SNI/mTLS/reload are Rust `TlsServerConfig` APIs; CLI and Python `HTTPSServer` stay single-identity compatible.

## Platform Support

| Platform | Status |
|----------|--------|
| Linux (x86_64, aarch64) | Supported |
| macOS (x86_64, aarch64) | Supported |
| Windows (x86_64) | Supported |

TLS support is platform-independent via rustls (pure Rust TLS implementation).
