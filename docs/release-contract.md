# Release Contract

This document defines the exact product surface, behavioral guarantees, and compatibility commitments for eggserve's first public release. It is the normative reference for what eggserve ships, what is stable, what is experimental, and what is internal.

Version: 0.1.0 (pre-release)

The Python wheel compatibility declaration is CPython 3.14 only
(`>=3.14,<3.15`) on Linux, macOS, and Windows. The wheel bundles the matching
platform-native CLI binary; PyPy and free-threaded CPython are not supported.

## Release Artifacts

| Artifact | Description | Distribution |
|----------|-------------|--------------|
| `eggserve` binary | CLI static file server | `cargo install`, Python wheel `bin/` |
| `eggserve-core` crate | Rust library for path confinement, policy, response planning | crates.io (planned) |
| Python wheel `eggserve` | Python package wrapping the binary and Rust primitives | PyPI (planned) |

### Feature Gates

| Feature | Default | Enables |
|---------|---------|---------|
| (none) | Yes | Core server + primitives |
| `client` | No | `primitives::client` module — HTTP client substrate |
| `client-tls` | No | Implies `client`; HTTPS via rustls + webpki-roots |
| `python-bindings-internal` | No | `ResolvedFile` extraction methods for Python bindings only |

## Runtime Service Boundary (Experimental, Milestone 3)

**Stability**: All `server` module types are **experimental**. The interface may change in any release.

The `server` module provides a reusable, transport-owning HTTP runtime for embedding. It owns the TCP accept loop, connection management, optional TLS (feature-gated), and HTTP/1 connection handling. Downstream projects implement the `Service` trait and provide it to `Server`; the runtime handles transport concerns.

### Exposed Types

| Type | Description |
|------|-------------|
| `Server` | Main entry point; creates via `Server::builder()` |
| `ServerBuilder` | Configured builder for `Server`; supports `.bind()` and `.from_listener()` for existing listeners |
| `ServerHandle` | Control handle: `local_addr()`, `shutdown()`, `wait()`, `wait_timeout()`, `ready()`, `force_shutdown()`, `state()` |
| `RuntimeConfig` | Transport-level configuration (bind, limits, timeouts, keep-alive) |
| `Service` trait | Receives `RequestHead`, returns `Result<Response, ServiceError>` |
| `service_fn` | Create a `Service` from a closure |
| `StaticService` | Hardened static file service implementing `Service` |
| `ServiceError` | Per-request errors: Internal, Rejected, Panic, Timeout |
| `ServerError` | Startup/lifecycle errors: Bind, Config, AlreadyStarted, Accept, ShutdownTimeout, Startup, Terminal |
| `LifecycleState` | Lifecycle state machine: Startup → Running → Draining → Stopped |
| `ShutdownResult` | Returned by shutdown operations, carries final `LifecycleState` |

### Guarantees

- Canonical `RequestHead` (no Hyper types) is passed to services
- Canonical `Response` is returned by services; the runtime normalizes and sends it
- Hop-by-hop header stripping and content-length computation are runtime-owned
- Handler panics are caught at the tokio task boundary and map to `ServiceError::Panic`
- Handler timeouts map to `ServiceError::Timeout`
- Filesystem policy (symlinks, dotfiles, listing) belongs to the service, not the runtime
- `StaticService` provides all the security properties of the built-in static handler
- Lifecycle state transitions are race-safe: `shutdown()` and `force_shutdown()` are idempotent
- No permit leakage: all connection semaphore permits are released on connection drop or shutdown
- `ready()` resolves once the server is accepting connections (no false positives)

### Lifecycle guarantees

- Lifecycle state transitions are race-safe (atomic CAS)
- Double-start returns `ServerError::AlreadyStarted`
- Shutdown before start is a no-op
- Multiple shutdown calls are idempotent
- Dropping `ServerHandle` triggers graceful shutdown
- `ready().await` returns immediately if already running
- `ready().await` returns error if server failed during startup
- `force_shutdown(deadline)` returns `ShutdownResult::Forced` on deadline exceeded

## Supported Protocol

- **HTTP/1.1 only** — no HTTP/2 or HTTP/3.
- **Read-only methods**: GET and HEAD. All other methods return 405.
- **Request target**: origin-form only (`/path?query`). Authority-form and absolute-form are rejected.
- **No request bodies**: GET and HEAD reject `Content-Length > 0`, invalid `Content-Length`, and any `Transfer-Encoding`.
- **Conditional requests**: `If-None-Match`, `If-Modified-Since`, `If-Range` are supported.
- **Range requests**: `Range` header with single byte ranges. Multi-range returns the full response.
- **HEAD parity**: HEAD responses include the same headers as GET with an empty body.

## Wire-level framing rules

These rules are enforced at the raw HTTP/1.1 socket boundary and are regression-tested in `crates/eggserve-core/tests/http_wire_correctness.rs`.

### Request rejection rules (eggserve policy)

- Non-GET/HEAD methods → 405 with `Allow: GET, HEAD`.
- Absolute-form (`http://host/path`) → 400.
- Authority-form (`host:port`), asterisk-form (`*`) → 405 (method check fires before target-form check).
- Empty or missing path → 400.
- Paths containing NUL bytes, backslashes, or encoded separators (`%2f`, `%5c`) → 400.
- Path traversal beyond root (`/../`) → 400 or 403.
- `Content-Length > 0` on GET/HEAD → 413.
- Invalid `Content-Length` (non-numeric, negative, overflow) → 400.
- `Transfer-Encoding` present on GET/HEAD → 400.
- Both `Content-Length` and `Transfer-Encoding` present → 400.
- `Transfer-Encoding` with unsupported codings (e.g. `chunked`) → 400.
- Duplicate `Content-Length` with conflicting values → 400.
- Duplicate `Content-Length` with identical values → treated as single `Content-Length`.
- Comma-joined `Content-Length` values (e.g. `Content-Length: 0, 10`) → 400.
- Request body bytes present without framing headers → connection closed (premature EOF).
- Truncated body mid-stream → connection closed, no response sent.

### Parser-level behavior (hyper)

These behaviors are determined by hyper's HTTP/1.1 parser, not eggserve policy:

- HTTP/1.0 requests are accepted (hyper accepts any `HTTP/x.y` version line).
- Bare LF (without CR) in header values is accepted by hyper's parser.
- CR+LF in header values is parsed as a header separator by hyper, resulting in two separate headers.
- Malformed header names or values that hyper cannot parse result in connection closure.
- Requests with invalid byte sequences in header names result in connection closure.

### Response guarantees (wire-tested)

- `Accept-Ranges: bytes` present on all file responses (200, 206, 416).
- `Content-Length` matches actual body bytes for all responses.
- HEAD responses include all headers but suppress body transfer.
- 304 responses include `ETag` and `Last-Modified` (when available), no body.
- 416 responses include `Content-Range: bytes */TOTAL` and `Content-Length: 0`.
- `X-Content-Type-Options: nosniff` present on all file responses.
- 405 responses include `Allow: GET, HEAD`.

## Behavioral Guarantees

### Safe Defaults (enforced at library level)

- Loopback bind (`127.0.0.1`) unless `--public` is passed
- Symlinks denied unless `--follow-symlinks` is passed
- Dotfiles denied unless `--allow-dotfiles` is passed
- Directory listing disabled unless `--directory-listing` is passed
- Unknown MIME types served as `application/octet-stream`
- Malformed request targets rejected

### Path Confinement

- No file is served outside the configured root directory.
- Path traversal, NUL bytes, ambiguous separators, Windows prefixes, reserved names, and ADS syntax are rejected.
- On Unix with safe defaults (symlinks denied): descriptor-relative traversal via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`. A symlink swapped between check and open is refused rather than followed.
- With `--follow-symlinks`: component-wise `symlink_metadata` checks. Weaker than descriptor-relative; explicitly outside the hardening guarantee.
- On Windows: parser-level checks only. Reparse-point/junction hardening is deferred. Do not use with untrusted public content on Windows.

### Resource Limits

| Limit | Default | Enforcement |
|-------|---------|-------------|
| Concurrent connections | 64 | Tokio semaphore |
| Concurrent file streams | 32 | Tokio semaphore |
| Request body size | 0 (rejected) | Header validation |
| Header read timeout | 10s | `tokio::time::timeout` |
| Response write timeout | 60s | `tokio::time::timeout` |
| Graceful shutdown | 10s | Drain after SIGTERM |

### Callback Server (Python `Server` with handler)

- Handler receives a `Request` object and must return a `Response` object.
- Invalid return types produce a generic 500 Internal Server Error.
- Invalid status codes (outside 100–999, non-three-digit) produce 500.
- Invalid header names (empty) or values (containing NUL, CR, LF) produce 500.
- Handler exceptions produce 500 without leaking tracebacks.
- Callback concurrency is bounded (default: 8 concurrent handler calls).
- File-backed responses retain their Rust-owned file capability and stream without copying the file through Python memory.
- HEAD requests still invoke the handler, but the runtime suppresses the body before acquiring file-stream resources.

### Buffered Client (feature-gated: `client`)

**Stability**: All client types are **experimental**. The interface may change in any release.

**Transport**: HTTP/1.1 only. One connection per request. No connection pooling.

**Supported URL schemes**: `http://` and `https://` only. Other schemes are rejected.

**URL grammar**:
- Hosts: IPv4, IPv6 bracketed (`[::1]`), domain names
- Ports: default (80/443) or explicit
- Path: starts with `/`; percent-encoded paths preserved
- Query: optional; `?` without `/` before it is rejected
- Fragments: stripped before sending
- Rejected: userinfo, empty host/port, control characters, spaces, CR/LF, Unicode/IDN

**Timeouts**:

| Timeout | Default | Scope |
|---------|---------|-------|
| `connect_timeout` | 10s | TCP connection + TLS handshake |
| `request_timeout` | 30s | Full request lifecycle (handshake, headers, body) |
| `max_response_body_bytes` | 10 MiB | Maximum response body size |

**TLS**:
- When `client-tls` is enabled: certificates verified by default against Mozilla CA roots
- `verify_tls: false` disables all certificate verification
- When `client-tls` is not enabled: HTTPS URLs rejected before TCP connection
- HTTP URLs never enter TLS

**Response handling**:
- Full response body buffered in memory (no streaming API)
- Duplicate response headers: last-value-wins (HashMap)
- No redirect following
- No cookie handling
- No proxy support
- No automatic decompression
- No HTTP/2 or HTTP/3

**Error taxonomy** (12 variants): `InvalidUrl`, `UnsupportedScheme`, `MissingHost`, `InvalidHeader`, `BodyTooLarge`, `Timeout`, `DnsError`, `ConnectError`, `TlsError`, `ProtocolError`, `ResponseBodyTooLarge`, `Io`.

**Request construction**:
- GET and HEAD requests must not have a body (enforced at builder level)
- Header names must be RFC 7230 tokens; values must not contain NUL/CR/LF
- Default `User-Agent: eggserve-client/0.1` (overridable)
- Default `Host` header from URL (overridable)

## Canonical HTTP Request Types

**Stability**: All canonical request types are **stable** after conformance completion in Plan 049.

The canonical request types provide transport-independent, Hyper-independent value types for inspecting HTTP requests. They are defined in `eggserve_core::primitives` and projected to Python through `eggserve._native`.

### Rust Types

| Type | Module | Description |
|------|--------|-------------|
| `Method` | `primitives::method` | Validated HTTP method (standard + extension). Case-sensitive. |
| `HttpVersion` | `primitives::version` | HTTP/1.0 or HTTP/1.1. |
| `HeaderBlock` | `primitives::header_block` | Ordered, duplicate-preserving header collection. |
| `HeaderName` | `primitives::header_block` | Validated header name (RFC 9110 token). |
| `HeaderValue` | `primitives::header_block` | Validated header value (no CR/LF/NUL). |
| `RequestTarget` | `primitives::request_target` | Validated origin-form target (path + query). |
| `RequestHead` | `primitives::request_head` | Canonical request head: method, target, version, headers. |
| `ConnectionInfo` | `primitives::connection_info` | Transport metadata: local/remote addrs, scheme, TLS. |

**Conversion**: `RequestHead::try_from_hyper()` converts a `hyper::Request<B>` into a canonical `RequestHead`. The conversion is fallible and typed — malformed or unsupported input is rejected before handlers. The resulting `RequestHead` contains no Hyper types.

**Legacy**: `ReadOnlyMethod` (GET/HEAD only) remains stable for existing consumers. `Method` is the canonical type for new code.

### Python Types

| Type | Module | Description |
|------|--------|-------------|
| `Method` | `eggserve._native` | Canonical HTTP method. Frozen. |
| `HttpVersion` | `eggserve._native` | Canonical HTTP version. Frozen. |
| `HeaderBlock` | `eggserve._native` | Duplicate-preserving headers. Frozen. |
| `ConnectionInfo` | `eggserve._native` | Transport metadata. Frozen. |
| `CanonicalRequest` | `eggserve._native` | Canonical request head. Frozen. |

**Functions**: `parse_method(value)`, `parse_http_version(value)` — standalone constructors with typed errors.

**Exceptions**: `MethodError`, `HttpVersionError`, `HeaderError`, `DuplicateHeaderError` — child classes of `EggserveError`.

## Canonical Response Types

**Stability**: All canonical response types are **stable** after conformance completion in Plan 049.

The canonical response types provide transport-independent, Hyper-independent value types for constructing HTTP responses. They are defined in `eggserve_core::primitives::canonical` and enforce response normalization rules at construction and before transport conversion.

### Rust Types

| Type | Module | Description |
|------|--------|-------------|
| `StatusCode` | `primitives::canonical` | Validated HTTP status code (100–999, three-digit only). |
| `ResponseHead` | `primitives::canonical` | Status + validated `HeaderBlock`. |
| `ResponseBody` | `primitives::canonical` | Body representation: `Empty`, `Bytes`. |
| `Response` | `primitives::canonical` | Complete response: head + body. One-shot consumption. |
| `ResponseBuilder` | `primitives::canonical` | Validated builder for `Response`. |
| `NormalizeRequest` | `primitives::canonical` | Context for response normalization. |
| `ResponseConstructionError` | `primitives::canonical` | Error taxonomy for response construction. |

**Normalization functions**:

- `normalize_response(response, request)` applies the following rules before transport conversion:
  1. HEAD suppression — body discarded, representation headers preserved.
  2. Body-forbidden statuses — 1xx, 204, 304 bodies discarded.
  3. Hop-by-hop header stripping — `Transfer-Encoding` removed.
  4. Content-Length computation — set to actual body length.
  5. Duplicate end-to-end headers preserved.

- `normalize_metadata(status, headers, body_len, is_head)` is the shared normalization entry point for both in-memory and file-backed response producers. It applies the same framing rules (Transfer-Encoding stripping, Content-Length computation) without consuming a `Response` value. File-streaming producers call this directly.

**Conversion**: `to_hyper_response(response)` converts a normalized canonical `Response` into a Hyper `Response<BoxBody>`. This is the final step before sending on the wire.

### Unified Response Architecture

All response producers converge on `normalize_metadata()` for metadata normalization. This function is the shared normalization entry point for both in-memory and file-backed response producers. It applies:

1. Strip runtime-owned `Transfer-Encoding` — always removed regardless of status.
2. HEAD responses: suppress `Content-Length`.
3. Body-forbidden statuses (1xx, 204, 304): suppress `Content-Length`.
4. Normal payloads: set `Content-Length` to actual body length.
5. Preserve all other headers (including duplicates).

File and byte responses share the same framing policy: `Transfer-Encoding` is always stripped, and `Content-Length` is computed from actual body length. Handler-provided `Content-Length` is overwritten with the computed value.

`normalize_metadata()` is called by `normalize_response()` (for complete `Response` values) and directly by file-streaming producers (for file-backed responses that bypass the canonical `Response` type).

### Response Normalization Algorithm

The normalization algorithm is the single final path for all response producers. It is documented as normative behavior:

**Inputs**: request method/version, response status, response headers, response body metadata, connection policy.

**Rules** (applied in order):
1. HEAD transmits no body bytes while preserving representation headers.
2. 1xx, 204, and 304 transmit no payload body.
3. `Transfer-Encoding` is runtime-owned; handler-supplied values are removed.
4. `Content-Length` is computed by the runtime.
5. Duplicate end-to-end headers are preserved.
6. Error responses do not leak handler tracebacks or internals.

**Fail-versus-strip policy**: The normalization function strips runtime-owned headers (`Transfer-Encoding`) rather than rejecting, because these headers are commonly set by frameworks and rejection would break compatibility. Handler-provided `Content-Length` is overwritten with the computed value.

### Header Representation

#### Response Headers (`HeaderMapPlan`)

- `Vec<ResponseHeader>` — ordered list of `(name, value)` pairs.
- Duplicates are preserved. `get()` returns the first match (case-insensitive).
- No code path currently generates duplicate headers internally.

#### Canonical Request Headers (`HeaderBlock`)

- `Vec<HeaderField>` — ordered list of `(HeaderName, HeaderValue)` pairs.
- Duplicates are preserved. Original field-name casing is preserved.
- Case-insensitive lookup by field name.
- `get_first(name)` returns the first value. `get_all(name)` returns all values. `get_unique(name)` returns an error if duplicates exist.
- Rejects empty names, names exceeding 256 bytes, and values containing CR, LF, or NUL bytes.

#### Python Server (`Response.headers`)

- `HashMap<String, String>` — keys are unique, case-sensitive.
- Duplicate header names (e.g. multiple `Set-Cookie`) are **not representable** — only the last value for a given key survives.
- Request headers are also `HashMap<String, String>` — same limitation on the inbound side.

#### Implication

Python handlers cannot emit duplicate response headers. If a handler needs multiple `Set-Cookie` headers, the handler must combine them into a single value or use the static-responder path which preserves duplicates through `HeaderMapPlan`.

## Conformance Corpus

Plan 049 establishes a conformance corpus for canonical HTTP type behavior. The corpus contains:

- **Request type conformance**: Method, HttpVersion, HeaderBlock, RequestTarget, RequestHead, and ConnectionInfo parsing and validation rules.
- **Response type conformance**: StatusCode, ResponseHead, ResponseBody, Response construction, and normalization rules.
- **Rust/Python parity tests**: Tests exercising identical behavior across Rust and Python bindings.
- **Normalization conformance**: normalize_response() rules (HEAD suppression, body-forbidden enforcement, hop-by-hop stripping, content-length computation).

The corpus is run by:
- `tests/canonical_conformance.rs` (Rust side)
- `python/eggserve/test_canonical_conformance.py` (Python side)

## API Stability Tiers

Every exported Rust and Python item is classified into one of three tiers:

### Stable

Breaking changes bump the major version. Pre-1.0, minor versions may break stable APIs only with explicit release notes and migration guidance. Patch releases must not break stable APIs. Stable names and signatures are intentionally supported. Semantic behavior identified in this release contract is covered by conformance tests. Unspecified formatting, debug output, log text, and internal implementation details are not stable unless explicitly documented.

### Experimental

May change in any non-patch release. Consumers should pin versions. Functionality is tested but the interface is not frozen. Experimental APIs may be omitted from language parity.

### Internal

Not part of the public contract. Used only for cross-crate communication (e.g. Python bindings). Internal Python names are not exported through `__all__`. Internal Rust features do not become accidental default features. May be removed without notice.

### Compatibility Rules

- **Enum variants** — Stable enum variants are exhaustive unless documented otherwise. Adding a new variant to a stable enum is a breaking change.
- **Exception classes** — Python exception classes and field names are stable. Message strings are not stable.
- **Header ordering** — Rust `HeaderMapPlan` preserves order and duplicates (stable). Python `Response.headers` uses `HashMap` and does not preserve duplicates (known limitation).
- **Error taxonomy** — `PathRejection`, `RequestValidationError`, `ClientError`, and `ResourceDeniedReason` variants are stable.
- **Serialization** — `Debug`, `Display`, and Python `repr()` output are not stable unless documented.
- **Deprecation** — Deprecated stable items remain functional for at least one minor release after announcement.

## Platforms

| Platform | Status | Security Level |
|----------|--------|---------------|
| Linux x86_64 | Supported, CI-tested | Full (descriptor-relative) |
| Linux aarch64 | Supported, CI-tested | Full (descriptor-relative) |
| macOS arm64 | Supported, CI-tested | Full (descriptor-relative) |
| macOS x86_64 | Supported, CI-tested | Full (descriptor-relative) |
| Windows x86_64 | Supported, CI-tested | Partial (parser-level only) |

## What This Document Does NOT Cover

- Framework abstractions, routing, middleware, ASGI/WSGI adapters — these are non-goals.
- Compatibility with `python -m http.server` beyond practical equivalence — see [compatibility.md](compatibility.md).
- Version freeze — this is a pre-release contract. The API surface may change before 1.0.
