# HTTP Client Primitives — Deep Dive

Feature-gated HTTP client substrate in `eggserve-core::primitives::client`. Provides a low-level, Rust-backed HTTP/1.1 client with timeout policy, TLS verification, and structured errors.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `primitives/client/mod.rs` | Re-exports all public client types |
| `error.rs` | `primitives/client/error.rs` | `ClientError` — 12-variant error taxonomy |
| `url.rs` | `primitives/client/url.rs` | `Scheme`, `ParsedUrl` — hand-parsed URL validation |
| `request.rs` | `primitives/client/request.rs` | `ClientConfig`, `Method`, `ClientRequest`, `ClientRequestBuilder`, `validate_header` |
| `response.rs` | `primitives/client/response.rs` | `ClientResponse` — status, headers, buffered body |
| `http_client.rs` | `primitives/client/http_client.rs` | `HttpClient` — hyper client connection, timeout enforcement, TLS |

## Feature Gates

```toml
[features]
client = ["hyper/client", "hyper-util/client-legacy"]
client-tls = ["client", "dep:rustls", "dep:tokio-rustls", "dep:webpki-roots"]
```

## Key Types

### `HttpClient` (`http_client.rs`)

The core client. Wraps TCP connections with optional TLS (behind `client-tls` feature). HTTPS URLs are rejected when TLS is not compiled in.

```rust
pub struct HttpClient {
    config: ClientConfig,
}
```

Methods:
- `new(config)` — Create client
- `send(request)` — Send a constructed `ClientRequest`

Internal flow:
1. Parse URL via `ParsedUrl::parse()`
2. Connect with timeout via `tokio::time::timeout()`
3. For HTTPS: wrap TCP in TLS via `tokio-rustls` (when `client-tls` enabled), reject if not enabled
4. Set `Host` and `User-Agent` headers
5. Send request via hyper HTTP/1.1
6. Collect response body with max-bytes enforcement (buffered, not streaming)
7. Return `ClientResponse` (fully buffered)

### `ClientConfig` (`request.rs`)

```rust
pub struct ClientConfig {
    pub connect_timeout: Duration,       // default: 10s
    pub request_timeout: Duration,       // default: 30s
    pub max_response_body_bytes: Option<u64>,    // default: Some(10 MiB)
    pub verify_tls: bool,                // default: true
}
```

Note: `max_response_body_bytes` is `Option<u64>`. `Some(n)` enforces a limit; `None` allows unlimited response size (not recommended for untrusted servers).

### `ClientRequest` / `ClientRequestBuilder` (`request.rs`)

Builder pattern for constructing requests:

```rust
let request = ClientRequest::builder()
    .method(Method::Post)
    .url("http://localhost:8080/submit")?
    .header("Content-Type", "application/json")?
    .body(Some(json_bytes))?
    .build()?;
```

Validation:
- Method must be a valid HTTP method (any method, not just GET/HEAD)
- URL must parse as HTTP or HTTPS
- Header names must be valid RFC 7230 tokens
- Header values must not contain NUL, CR, or LF
- Body is optional `Vec<u8>`

### `ClientResponse` (`response.rs`)

```rust
pub struct ClientResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
```

Methods: `is_success()`, `content_length()`, `content_type()`, `text()`, `bytes()`

### `ClientError` (`error.rs`)

12-variant taxonomy:

| Variant | Fields | Meaning |
|---------|--------|---------|
| `InvalidUrl(String)` | message | URL parsing failed |
| `UnsupportedScheme(String)` | scheme | Not HTTP or HTTPS |
| `MissingHost` | — | URL has no host component |
| `InvalidHeader(String)` | message | Header name/value validation failed |
| `BodyTooLarge` | `limit: u64, actual: u64` | Request body exceeds limit |
| `Timeout(String)` | message | Connect or request timeout |
| `DnsError(String)` | message | DNS resolution failed |
| `ConnectError(String)` | message | TCP connection failed |
| `TlsError(String)` | message | TLS handshake or verification failed |
| `ProtocolError(String)` | message | HTTP protocol error |
| `ResponseBodyTooLarge` | `limit: u64` | Response body exceeds max_response_body_bytes |
| `Io(std::io::Error)` | error | Underlying I/O error |

## Dependencies

| Dependency | Feature gate | Purpose |
|------------|-------------|---------|
| `hyper` | `client` | HTTP/1.1 client connection |
| `hyper-util` | `client` | Client legacy connector, TokioIo bridge |
| `rustls` | `client-tls` | TLS implementation |
| `tokio-rustls` | `client-tls` | Async TLS stream |
| `webpki-roots` | `client-tls` | Mozilla CA root certificates |

All non-optional dependencies (`hyper`, `hyper-util`, `tokio`, `bytes`) are already in the default build. The `client` feature only enables client-specific feature flags on existing crates.

## Testing

Integration tests across three test files, all gated `#![cfg(feature = "client")]`:

**`crates/eggserve-core/tests/client_integration.rs`** — 23 tests using local Hyper test servers:
- GET, HEAD, POST, PUT, DELETE, PATCH, status codes, headers, body echo
- Timeouts, connect errors, TLS, URL/header validation, body size limits, server disconnect

**`crates/eggserve-core/tests/client_interop.rs`** — 48 tests covering interoperability edge cases:
- Fixed Content-Length, chunked Transfer-Encoding, connection-close bodies
- Empty bodies, duplicate response headers, malformed status/headers
- Premature EOF, incorrect Content-Length, delayed headers/body, oversized bodies
- Host header generation, User-Agent, method validation, header validation
- Response parsing, error mapping, status codes, content helpers

**`crates/eggserve-core/tests/client_tls.rs`** — 7 tests (behind `client-tls` feature):
- TLS verification (trusted/untrusted/self-signed certs), `verify_tls=false` bypass
- HTTP never enters TLS, TLS error variants, sequential HTTPS requests

## Python Bindings

`crates/eggserve-python/src/client.rs` wraps Rust types with PyO3:

- `PyHttpClient` — frozen class with `get()`, `head()`, `post()`, `put()`, `delete()`, `patch()`, `send()` methods. Convenience methods (`get()`, `head()`, etc.) are only available in the Python bindings; the Rust `HttpClient` only exposes `send()`.
- `PyClientConfig` — frozen dataclass with defaults and validation (rejects zero/negative timeouts, NaN, Inf)
- `PyClientRequest` — frozen, no public constructor (created by send methods)
- `PyClientResponse` — frozen, methods: `text()`, `bytes()`, `is_success()`, `content_length()`, `content_type()`
- `PyClientError` — enum mapped to Python `EggserveError`

## See Also

- [../docs/http-client-primitives.md](../docs/http-client-primitives.md) — Public API contract
- [primitives-api.md](primitives-api.md) — Overall primitives API boundary
- [eggserve-core.md](eggserve-core.md) — Core library overview

## API Stability

All client types are **experimental** in the first release. The client provides buffered, one-connection-per-request behavior with no pooling, redirects, cookies, proxies, retries, decompression, or streaming response API. See [api-stability.md](../docs/api-stability.md) for the full classification and [release-contract.md](../docs/release-contract.md) for behavioral guarantees.
