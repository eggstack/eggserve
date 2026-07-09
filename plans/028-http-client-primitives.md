# Plan 028: HTTP Client Primitive Substrate

## Purpose

Add a low-level, Rust-backed HTTP client primitive substrate that Python users and downstream projects can build on. This should be conceptually closer to Python's `http.client` than `requests` or `httpx`: explicit request construction, timeout policy, TLS verification, response metadata, and response body streaming.

This plan should not be implemented until the server-side HTTP and body-source primitives are stable enough to avoid API churn. The client should be deliberately small and correct before it becomes convenient.

## Product boundary

The client substrate should let Python and Rust consumers perform basic outbound HTTP requests through Rust-owned networking. It should not become a full high-level client library in the first pass.

Downstream projects may later build higher-level clients on top. eggserve should provide the transport/protocol substrate and correctness primitives.

## Goals

- Provide a minimal Rust-backed HTTP/1.1 client primitive.
- Expose Python bindings for request construction and response streaming.
- Enforce explicit timeout policy.
- Verify TLS by default for HTTPS.
- Provide structured errors.
- Avoid high-level client features until the core is tested.
- Reuse request/header/body validation primitives where possible.

## Non-goals

- Do not build a `requests` replacement in this pass.
- Do not build an `httpx` replacement in this pass.
- Do not add cookie jar management.
- Do not add auth helpers.
- Do not add automatic retries.
- Do not add redirect following by default.
- Do not add proxy support.
- Do not add HTTP/2.
- Do not add HTTP/3.
- Do not add WebSocket.
- Do not add multipart/form helpers.
- Do not add caching.
- Do not add decompression unless explicitly required by Hyper/client stack and tested.

## Design constraints

### Explicit security defaults

- HTTPS must verify certificates by default.
- Insecure TLS must require an explicit opt-in flag with loud naming.
- Redirects must be disabled or explicitly configured in the first pass.
- Request body size and response body read limits must be explicit.
- Timeouts must default to conservative values.

### Streaming before convenience

The response body should be streamable. Python may have a convenience `read()` for bounded reads, but production callers must not be forced into loading full responses into memory.

### Structured errors

Errors should be typed enough for downstream clients:

- Invalid URL.
- Unsupported scheme.
- DNS/connect failure.
- TLS verification failure.
- Timeout.
- Invalid header.
- Body too large.
- Protocol error.
- Response stream error.

### Dependency discipline

Avoid adding a broad HTTP client dependency if the current Hyper/Tokio stack can support the minimal client. If an extra dependency is required for TLS roots or URL parsing, justify it in `docs/dependency-policy.md` and keep it narrow.

## Candidate Rust API

Names are illustrative:

```rust
pub struct ClientConfig {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub max_response_body_bytes: Option<u64>,
    pub verify_tls: bool,
}

pub struct HttpClient { ... }

pub struct ClientRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: RequestBody,
}

pub struct ClientResponse {
    pub status: u16,
    pub headers: HeaderMapPlan,
    pub body: ResponseStream,
}
```

If the repo avoids a URL dependency at first, expose a narrower authority/scheme/path builder and document limitations. However, robust URL parsing is security-sensitive; do not hand-roll more than necessary.

## Candidate Python API

Names are illustrative:

```python
from eggserve import HttpClient, ClientConfig, Request

client = HttpClient(ClientConfig())
response = client.request('GET', 'https://example.com/')
print(response.status)
print(response.headers)
chunk = response.read(8192)
```

Also support a more explicit builder:

```python
request = Request('GET', 'https://example.com/', headers={'accept': 'text/plain'})
response = client.send(request)
```

Keep initial body support minimal:

- No body for GET/HEAD by default.
- Bytes body for POST/PUT may be supported only if request body policy is implemented and tested.
- Streaming upload can be deferred.

## Implementation tasks

### 1. Decide crate/module placement

Prefer adding client primitives under `eggserve-core::primitives::client` only if dependency impact remains acceptable. If client dependencies would significantly affect the static-server crate, consider a separate crate such as `eggserve-client` in the workspace.

Decision criteria:

- Does the client require TLS root dependencies?
- Does it require URL parsing dependencies?
- Does it increase binary size for users who only want static serving?
- Can it be feature-gated cleanly?

A feature gate such as `client` may be appropriate.

### 2. Define URL/scheme policy

Initial support:

- `http://`.
- `https://` if TLS client support is implemented.
- Reject unsupported schemes.
- Reject userinfo unless explicitly supported.
- Normalize/validate host and port.
- Preserve path/query correctly.
- Reject invalid header injection characters.

Document IPv6 literal behavior if supported.

### 3. Define request model

Implement request construction with:

- Method validation.
- URL validation.
- Header validation.
- Body policy.
- Timeout config.

Do not silently add unsafe defaults. User-agent can be omitted or set to a clear eggserve value if desired.

### 4. Define response model

Expose:

- Status code.
- Reason phrase only if available and stable.
- Headers.
- Content length if known.
- Body stream.
- Bounded `read_all(max_bytes=...)` helper.
- Iteration over chunks for Python if feasible.

Enforce max body read limits in convenience APIs.

### 5. Timeout behavior

Implement and test:

- Connect timeout.
- Header/read timeout.
- Body read timeout.
- Write timeout for request body if body support exists.

Timeout errors must be structured and should not leave ambiguous partial state to Python callers.

### 6. TLS behavior

If HTTPS is included in the first pass:

- Verify certificates by default.
- Use a maintained root store strategy.
- Document platform/root behavior.
- Add tests with local test certs where feasible.
- Add explicit insecure option only if absolutely necessary, named loudly.

If HTTPS is deferred, the first pass may support `http://` only, but the docs must say this is not production-complete for general internet clients.

### 7. Local integration tests

Use local test servers where possible to avoid network flakiness:

- eggserve static server for GET/HEAD.
- A small Hyper test server for headers/status/body behavior.
- Slow response server for timeout behavior.
- Malformed peer if feasible.

Do not depend on external internet access for CI tests.

## Python tests

Add tests for:

- Basic GET to local server.
- HEAD to local server.
- Response headers visible in Python.
- Bounded body read returns expected bytes.
- Chunked/streamed body read if supported.
- Invalid URL rejected.
- Unsupported scheme rejected.
- Invalid header rejected.
- Timeout produces structured error.
- Redirect is not followed by default if redirect handling is present.
- HTTPS verification behavior if HTTPS is implemented.

## Rust tests

Add tests for:

- Request builder validation.
- Header validation.
- URL/scheme validation.
- Response parsing from local server.
- Timeout behavior.
- Body read limits.
- Error taxonomy.

## Documentation changes

Add or update:

- `docs/http-client-primitives.md`.
- `docs/python-api.md` client section.
- `docs/dependency-policy.md` if new dependencies are added.
- `docs/extension-contract.md` to describe downstream high-level client use.
- `architecture/eggserve-core.md` or new architecture doc if adding a client module/crate.

Docs must state that this is a low-level substrate, not a high-level replacement for `requests` or `httpx` yet.

## Acceptance criteria

- Client primitive design is feature-gated or isolated if it adds meaningful dependencies.
- Python can issue a basic GET/HEAD to a local HTTP server through Rust-backed networking.
- Response body can be streamed or read with an explicit bound.
- Timeouts are configurable and tested.
- Invalid URLs and headers fail before network I/O.
- TLS is either verified by default or explicitly deferred with documentation.
- Redirects are not followed implicitly in the first pass.
- No cookie/auth/retry/proxy/framework features are added.
- CI does not depend on external internet access.
- Full validation passes.

## Validation commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
```

Packaging smoke:

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

## Handoff notes

This plan is intentionally conservative. The temptation will be to add redirects, cookies, retries, JSON helpers, auth, decompression, proxies, and a friendly high-level API. Do not do that in the first pass. The substrate should be correct, typed, timeout-aware, and testable. Higher-level clients can be built downstream or in later plans after the primitive boundary is stable.
