# TLS Support Deep Dive

eggserve supports TLS via rustls, enabled through the `tls` feature flag. TLS is optional — eggserve defaults to plain HTTP for local development use cases.

## Feature Flags

| Feature | Crate | Purpose |
|---------|-------|---------|
| `tls` | `eggserve-core`, `eggserve-bin` | Server TLS via rustls/tokio-rustls |

## Dependencies

When `tls` is enabled in `eggserve-core`:

- `rustls` — TLS implementation
- `tokio-rustls` — Async TLS integration with tokio
- `rustls-pemfile` — PEM file parsing

`eggserve-bin` enables `eggserve-core/tls` and re-exports the module
(`bin/src/tls.rs` is `pub use eggserve_core::tls::*`). All loading
logic lives in `eggserve-core::tls`.

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

eggserve supports HTTP/1.1 ALPN only. The TLS configuration does not negotiate HTTP/2. This is intentional — eggserve is an HTTP/1.1 server.

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

1. **HTTP/1.1 only** — No HTTP/2 ALPN negotiation
2. **No OCSP stapling** — Not implemented
3. **No certificate management** — No ACME, no automatic renewal
4. **No custom trust stores** — Uses Mozilla's root bundle only
5. **No TLS session tickets** — Not configured by default
6. **No TLS 1.3 only mode** — Uses rustls defaults (TLS 1.2 + 1.3)

## Platform Support

| Platform | Status |
|----------|--------|
| Linux (x86_64, aarch64) | Supported |
| macOS (x86_64, aarch64) | Supported |
| Windows (x86_64) | Supported |

TLS support is platform-independent via rustls (pure Rust TLS implementation).
