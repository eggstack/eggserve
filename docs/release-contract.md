# Release Contract

This document defines the exact product surface, behavioral guarantees, and compatibility commitments for eggserve's first public release. It is the normative reference for what eggserve ships, what is stable, what is experimental, and what is internal.

Version: 0.1.0 (pre-release)

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
- Absolute-form (`http://host/path`), authority-form (`host:port`), asterisk-form (`*`) → 400.
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
- Invalid status codes (outside 100–999) produce 500.
- Invalid header names (empty) or values (containing NUL, CR, LF) produce 500.
- Handler exceptions produce 500 without leaking tracebacks.
- Callback concurrency is bounded (default: 8 concurrent handler calls).
- File-backed responses are eagerly read to bytes before passing through the handler boundary.
- **HEAD is not special-cased in the Python handler path** — the handler returns the same response for GET and HEAD; hyper suppresses the body for HEAD.

### Buffered Client (feature-gated: `client`)

- One connection per request. No connection pooling.
- No redirect following.
- No cookie handling.
- No proxy support.
- No automatic decompression.
- No streaming response API — the full response body is buffered in memory.
- TLS verification is enabled by default.
- Timeout enforcement: connect timeout (default 10s) and request timeout (default 30s).
- Max response body: 10 MiB default.

## Header Representation

### Rust Core (`HeaderMapPlan`)

- `Vec<ResponseHeader>` — ordered list of `(name, value)` pairs.
- Duplicates are preserved. `get()` returns the first match (case-insensitive).
- No code path currently generates duplicate headers internally.

### Python Server (`Response.headers`)

- `HashMap<String, String>` — keys are unique, case-sensitive.
- Duplicate header names (e.g. multiple `Set-Cookie`) are **not representable** — only the last value for a given key survives.
- Request headers are also `HashMap<String, String>` — same limitation on the inbound side.

### Implication

Python handlers cannot emit duplicate response headers. If a handler needs multiple `Set-Cookie` headers, the handler must combine them into a single value or use the static-responder path which preserves duplicates through `HeaderMapPlan`.

## API Stability Tiers

Every exported Rust and Python item is classified into one of three tiers:

### Stable

Breaking changes bump the major version. Pre-1.0, minor versions may break. Stable items are reviewed and intentional. Changes require a plan update and migration guide.

### Experimental

Explicitly exempt from normal compatibility promises. The API may change in any release. Experimental items are functional but their interface is not yet finalized. Consumers should pin to a specific version.

### Internal

Not part of the public contract. Used only for cross-crate communication (e.g. Python bindings). Not documented as a user feature. May be removed or changed without notice.

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
