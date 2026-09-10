# TLS Support Deep Dive

eggserve supports TLS via rustls, enabled through the `tls` feature flag. TLS is optional — eggserve defaults to plain HTTP for local development use cases.

## Feature Flags

| Feature | Crate | Purpose |
|---------|-------|---------|
| `http2` | `eggserve-core`, `eggserve-bin` | Experimental HTTP/2 server path, bounded H2 prior knowledge and protocol config; see [HTTP/2 qualification](http2.md) |
| `http3` | `eggserve-core`, `eggserve-bin` | Experimental HTTP/3/QUIC server path; enables `tls`, h3 ALPN, and same-port UDP lifecycle; see [HTTP/3 boundary](http3.md) |
| `tls` | `eggserve-core`, `eggserve-bin` | Server TLS via rustls/tokio-rustls |

## Dependencies

When `tls` is enabled in `eggserve-core`:

- `rustls` — TLS implementation
- `tokio-rustls` — Async TLS integration with tokio
- `rustls-pki-types` — PEM certificate and private-key parsing

`eggserve-bin` enables `eggserve-core/tls` and re-exports the module
(`bin/src/tls.rs` is `pub use eggserve_core::tls::*`). All loading
logic lives in `eggserve-core::tls`.

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

## Limitations

1. **Experimental H2 scope** — Plans 186, 190, and 191 keep H2 experimental. The
   deterministic suite and Linux wire checks pass, while browser/platform
   evidence, trailer-scope determinism, and a public safe per-stream reset
   hook remain open; HTTP/1 upgrades remain unavailable, and HTTP/3 is separately
   feature-gated and experimental. See
   [the qualification record](../release/plan-191-http2-supported-tier-qualification.md).
2. **Experimental HTTP/3 scope** — The `http3` feature creates a separate
   TLS 1.3/`h3` QUIC configuration and disables application 0-RTT. Plans 188,
   190, and 192 close with H3 still experimental because independent-client,
   adversarial-wire, and cross-platform runtime evidence is incomplete, and
   Plan 192 additionally blocks on upstream `hyperium/h3#338` (no released
   fix) and the `#262` stream-drop remainder; see
   [the Plan 192 readiness record](../release/plan-192-http3-dependency-readiness.md).
3. **No OCSP stapling** — Not implemented
4. **No certificate management** — No ACME, no automatic renewal
5. **No custom trust stores** — Uses Mozilla's root bundle only
5. **No TLS session tickets** — Not configured by default
6. **TCP TLS protocol mode** — Direct TCP TLS uses rustls defaults (TLS 1.2 +
   1.3); the separate QUIC/H3 configuration is TLS 1.3-only.

## Platform Support

| Platform | Status |
|----------|--------|
| Linux (x86_64, aarch64) | Supported |
| macOS (x86_64, aarch64) | Supported |
| Windows (x86_64) | Supported |

TLS support is platform-independent via rustls (pure Rust TLS implementation).
