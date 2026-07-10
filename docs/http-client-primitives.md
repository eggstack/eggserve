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
    max_response_body_bytes: 5 * 1024 * 1024, // 5 MiB
    verify_tls: true,
};
let client = HttpClient::new(config);

// Convenience methods
let response = client.get("http://localhost:8080/index.html")?;
let response = client.head("http://localhost:8080/style.css")?;
let response = client.post("http://localhost:8080/submit", body_bytes)?;
let response = client.put("http://localhost:8080/resource", body_bytes)?;
let response = client.delete("http://localhost:8080/resource")?;
let response = client.patch("http://localhost:8080/resource", patch_bytes)?;

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

## URL validation

URLs are hand-parsed (no new dependency). Supported:

- `http://` and `https://` schemes
- IPv4 and IPv6 literal hosts (`[::1]`)
- Default ports (80 for HTTP, 443 for HTTPS)
- Path and query preservation

Rejected:

- Non-HTTP/HTTPS schemes
- Userinfo in URL (`http://user:pass@host`)
- Empty host
- Invalid characters in host

## Timeout behavior

| Timeout | Default | Description |
|---------|---------|-------------|
| `connect_timeout` | 10s | TCP connection establishment |
| `request_timeout` | 30s | Full request lifecycle (headers + body) |
| `max_response_body_bytes` | 10 MiB | Maximum response body size |

Timeouts produce `ClientError::Timeout`. Connect timeouts produce `ClientError::ConnectError` or `ClientError::DnsError`.

## TLS behavior

When `client-tls` is enabled:

- Certificates are verified by default against Mozilla CA roots (`webpki-roots`)
- `verify_tls: false` disables verification (intentionally loud opt-in)
- TLS handshake is bounded by `connect_timeout`
- Self-signed certificates are rejected unless `verify_tls` is false

## Error taxonomy

```rust
pub enum ClientError {
    InvalidUrl(String),
    UnsupportedScheme(String),
    MissingHost,
    InvalidHeader(String),
    BodyTooLarge { max: u64 },
    Timeout(String),
    DnsError(String),
    ConnectError(String),
    TlsError(String),
    ProtocolError(String),
    ResponseBodyTooLarge { limit: u64 },
    Io(io::Error),
}
```

## Tests

- **Rust**: `crates/eggserve-core/tests/client_integration.rs` — 23 tests using local Hyper test servers. No internet required.
- **Python**: `crates/eggserve-python/python/eggserve/test_client_primitives.py` — 10 tests for bindings and error handling.

## See also

- [http-primitives.md](http-primitives.md) — Server-side HTTP primitives
- [architecture/client.md](../architecture/client.md) — Implementation deep dive
