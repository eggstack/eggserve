# HTTP Client Primitives

Low-level, Rust-backed HTTP client substrate for eggserve. Feature-gated behind `client` (and optionally `client-tls` for HTTPS).

## Scope

This is a **transport substrate**, not a high-level client library. It provides:

- HTTP/1.1 request/response over TCP
- Explicit timeout policy (connect, request)
- TLS verification by default (when `client-tls` is enabled)
- Structured errors with a 12-variant taxonomy
- Full buffering of response bodies with configurable max-bytes enforcement

## Non-goals

This is intentionally minimal. It does **not** provide:

- Cookie management
- Authentication helpers
- Automatic retries or backoff
- Redirect following
- Proxy support
- HTTP/2 or HTTP/3
- WebSocket
- Multipart/form helpers
- Caching
- Decompression

## Feature gates

| Feature | Enables | Dependencies |
|---------|---------|-------------|
| `client` | HTTP client (`HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse`) | `hyper/client`, `hyper-util/client-legacy` |
| `client-tls` | HTTPS support with TLS verification | `client` + `rustls`, `tokio-rustls`, `webpki-roots` |

The default build (no features) remains server-only with no client dependencies.

## Rust API

```rust
use eggserve_core::primitives::client::{
    HttpClient, ClientConfig, ClientRequest, ClientResponse,
    Method, Scheme, ParsedUrl, ClientError,
};

// Create client with default config
let client = HttpClient::new(ClientConfig::default());

// Or with custom config
let config = ClientConfig {
    connect_timeout: Duration::from_secs(5),
    request_timeout: Duration::from_secs(15),
    max_response_body_bytes: Some(5 * 1024 * 1024), // 5 MiB
    verify_tls: true,
};
let client = HttpClient::new(config);

// Builder pattern
let request = ClientRequest::builder()
    .method(Method::Get)
    .url("http://localhost:8080/data")?
    .header("Accept", "application/json")?
    .build()?;
let response = client.send(request)?;

// Response
println!("Status: {}", response.status);
println!("Content-Type: {}", response.content_type());
let text = response.text()?;
```

## Python API

```python
from eggserve import HttpClient, ClientConfig, Method

client = HttpClient(ClientConfig())
response = client.get("http://localhost:8080/index.html")
print(response.status)       # 200
print(response.content_type()) # text/html
print(response.text())       # "<html>..."

# With custom config
config = ClientConfig(
    connect_timeout=5.0,
    request_timeout=15.0,
    max_response_body_bytes=5 * 1024 * 1024,
    verify_tls=True,
)
client = HttpClient(config)

# Custom request
request = ClientRequest(
    method=Method.Get,
    url="http://localhost:8080/data",
    headers={"Accept": "application/json"},
)
response = client.send(request)
```

## URL grammar

URLs are hand-parsed (no new dependency). The grammar is intentionally narrow:

### Supported

- Schemes: `http://` and `https://` only
- Hosts: IPv4 (`127.0.0.1`), IPv6 bracketed (`[::1]`), domain names (`example.com`)
- Ports: default (80 for HTTP, 443 for HTTPS) or explicit (`:8080`)
- Path: always starts with `/`; percent-encoded paths are preserved
- Query: optional `?key=value` after path; `?` without `/` before it is rejected

### Rejected

- Non-HTTP/HTTPS schemes (e.g. `ftp://`, `file://`)
- Userinfo in URL (`http://user:pass@host`)
- Empty host or empty port (`http://host:` or `http://:80`)
- Control characters (0x00–0x1F, 0x7F) and spaces in URL
- CR/LF in URL
- Unicode/IDN hostnames (must be ASCII)
- Fragment components are stripped before sending (not sent to server)

## Timeout semantics

| Timeout | Default | Scope | Error |
|---------|---------|-------|-------|
| `connect_timeout` | 10s | TCP connection establishment + TLS handshake | `ClientError::Timeout` or `ClientError::ConnectError` |
| `request_timeout` | 30s | Full request lifecycle: HTTP handshake, headers, and body transfer | `ClientError::Timeout` |
| `max_response_body_bytes` | 10 MiB | Maximum response body size; exceeded → `ClientError::ResponseBodyTooLarge` | N/A (error variant) |

- **Connect timeout** covers TCP connection only. If the connection succeeds, the request timeout governs the rest.
- **Request timeout** wraps the entire hyper HTTP/1.1 handshake, request send, and response body collection.
- **TLS handshake** is bounded by `connect_timeout` (not `request_timeout`).
- Timeouts do not apply to DNS resolution (which uses system resolver).
- Zero or negative timeout values are not validated by the Rust API; the Python bindings validate at construction time.

## TLS behavior

When `client-tls` is enabled:

- Certificates are verified by default against Mozilla CA roots (`webpki-roots`)
- `verify_tls: false` disables all certificate verification (intentionally loud opt-in; uses `NoVerifier`)
- TLS handshake is bounded by `connect_timeout`
- Self-signed, expired, or hostname-mismatched certificates are rejected unless `verify_tls` is false
- When `client-tls` is **not** enabled, HTTPS URLs are rejected with `ClientError::TlsError` before any TCP connection is attempted
- HTTP URLs never enter TLS regardless of the `verify_tls` setting

## Error taxonomy

```rust
pub enum ClientError {
    InvalidUrl(String),
    UnsupportedScheme(String),
    MissingHost,
    InvalidHeader(String),
    BodyTooLarge { limit: u64, actual: u64 },
    Timeout(String),
    DnsError(String),
    ConnectError(String),
    TlsError(String),
    ProtocolError(String),
    ResponseBodyTooLarge { limit: u64 },
    Io(io::Error),
}
```

| Variant | When |
|---------|------|
| `InvalidUrl` | Malformed URL, empty host, control characters, userinfo |
| `UnsupportedScheme` | Non-HTTP/HTTPS scheme |
| `MissingHost` | URL has no host component |
| `InvalidHeader` | Header name is not an RFC 7230 token, value contains NUL/CR/LF |
| `BodyTooLarge` | Request body exceeds configured limit |
| `Timeout` | Connect or request timeout expired |
| `DnsError` | DNS resolution failed |
| `ConnectError` | TCP connection failed (refused, unreachable) |
| `TlsError` | TLS handshake failed, hostname mismatch, or HTTPS without `client-tls` |
| `ProtocolError` | HTTP protocol error (malformed response, non-data frame) |
| `ResponseBodyTooLarge` | Response body exceeded `max_response_body_bytes` |
| `Io` | Underlying I/O error |

All 12 variants map to Python `EggserveError` with the same structure.

## Tests

- **Rust**: `crates/eggserve-core/tests/client_integration.rs` — 23 tests for core client behavior. `crates/eggserve-core/tests/client_interop.rs` — 48 tests for interoperability edge cases. `crates/eggserve-core/tests/client_tls.rs` — 7 tests for TLS correctness (behind `client-tls`). No internet required.
- **Python**: `crates/eggserve-python/python/eggserve/test_client_primitives.py` — 10 tests for bindings and error handling.

## Stability

All client APIs are **experimental**. The interface may change in any release. Consumers should pin to a specific version. See [api-stability.md](api-stability.md) for the full classification.

## See also

- [http-primitives.md](http-primitives.md) — Server-side HTTP primitives
- [architecture/client.md](../architecture/client.md) — Implementation deep dive
