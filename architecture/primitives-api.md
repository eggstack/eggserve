# Primitives API — Deep Dive

The `primitives` module is the intended public boundary for embedding consumers. It provides path validation, filesystem resolution, HTTP request validation, and response planning — all without depending on Hyper types for the public surface.

## Module Location

`eggserve-core::primitives/` — the `pub` facade for external consumers.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `primitives/mod.rs` | Re-exports all public types |
| `secure_root.rs` | `primitives/secure_root.rs` | `SecureRoot`, `ResolvedFile`, `ResolvedDirectory`, `ResolvedResource` |
| `http.rs` | `primitives/http.rs` | `ReadOnlyMethod`, request validation functions (legacy) |
| `method.rs` | `primitives/method.rs` | `Method`: validated HTTP method (standard + extension) |
| `version.rs` | `primitives/version.rs` | `HttpVersion`: HTTP/1.0, HTTP/1.1 |
| `header_block.rs` | `primitives/header_block.rs` | `HeaderBlock`: duplicate-preserving ordered headers |
| `request_target.rs` | `primitives/request_target.rs` | `RequestTarget`: validated origin-form target |
| `request_head.rs` | `primitives/request_head.rs` | `RequestHead`: canonical request head with Hyper conversion |
| `connection_info.rs` | `primitives/connection_info.rs` | `ConnectionInfo`: transport metadata |
| `planner.rs` | `primitives/planner.rs` | Response planning (conditional, range, ETag) |
| `response.rs` | `primitives/response.rs` | Planning types (`StaticResponsePlan`, `BodyPlan`, etc.) |
| `body.rs` | `primitives/body.rs` | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming |
| `client/` | `primitives/client/` | HTTP client primitives (feature-gated: `client`) |

## Public Types

### `SecureRoot` (`secure_root.rs`)

The primary entry point for filesystem resolution. Wraps a canonicalized root directory with a `StaticPolicy`.

```rust
pub struct SecureRoot {
    root: PathBuf,
    policy: StaticPolicy,
}
```

Methods:
- `new(root, policy)` — Construct with validation
- `resolve(&self, path: &ConfinedPath)` → `ResolvedResource`
- `resolve_uri(&self, uri: &str)` → `ResolvedResource` (convenience: parse + resolve)

### `ResolvedResource` (`secure_root.rs`)

```rust
pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(ResourceDeniedReason),
}
```

### `ResourceDeniedReason`

```rust
pub enum ResourceDeniedReason {
    SymlinkDenied,
    DotfileDenied,
    RootEscapeDenied,
    PolicyDenied(PathRejection),
}
```

### `ResolvedFile` (`secure_root.rs`)

A capability object — no public constructor. Obtained only through `SecureRoot::resolve()`. Wraps the internal `fs::ResolvedFile` which holds the open file handle and metadata.

```rust
pub struct ResolvedFile {
    inner: crate::fs::ResolvedFile,
}
```

Public methods:
- `len()` → `u64`
- `modified()` → `Option<SystemTime>`
- `content_type()` → `&str`
- `plan_response(...)` → `StaticResponsePlan`
- `plan_conditional_response(...)` → `StaticResponsePlan`
- `into_body(&StaticResponsePlan)` → `Result<BodySource, BodySourceError>`
- `into_range_body(start, end_inclusive)` → `Result<BodySource, BodySourceError>`
- `safe_relative_components()` → `Vec<String>`

Extraction methods (behind `python-bindings-internal` feature only):
- `into_std_file()` → `std::fs::File`
- `into_parts()` → `(std::fs::File, std::fs::Metadata)`
- `from_parts(file, metadata, content_type, total_len)` → `ResolvedFile`

### `ResolvedDirectory` (`secure_root.rs`)

```rust
pub struct ResolvedDirectory {
    pub(crate) path: PathBuf,
    pub(crate) dir_fd: Option<OwnedFd>,
}
```

Public methods:
- `list()` → `Vec<DirEntry>`
- `resolve_child(child)` → `ResolvedResource`

### HTTP Validation (`http.rs`)

Request validation without Hyper dependency:

```rust
pub enum ReadOnlyMethod {
    Get,
    Head,
}

pub fn validate_method(method: &str) -> Result<ReadOnlyMethod, RequestValidationError>
pub fn validate_request_body(method: &str, has_body: bool) -> Result<(), RequestValidationError>
pub fn validate_request_target(target: &str) -> Result<ConfinedPath, RequestValidationError>
```

`RequestValidationError` maps to HTTP status codes (405, 400, etc.).

### Response Planning (`planner.rs`)

Pure functions for response planning:

```rust
pub fn plan_file_response(...) -> StaticResponsePlan
pub fn evaluate_conditional_headers(...) -> ConditionalRequestOutcome
pub fn evaluate_if_none_match(...) -> bool
pub fn evaluate_range_header(...) -> RangeRequestOutcome
pub fn evaluate_if_range(...) -> bool
pub fn generate_etag(metadata: &Metadata) -> String
pub fn plan_directory_listing(...) -> StaticResponsePlan
```

### Response Types (`response.rs`)

Framework-independent value objects:

```rust
pub struct StaticResponsePlan {
    pub status: ResponseStatus,
    pub headers: HeaderMapPlan,
    pub body: BodyPlan,
}

pub struct HeaderMapPlan { headers: Vec<ResponseHeader> }
pub struct ResponseHeader { pub name: String, pub value: String }
pub enum BodyPlan { Empty, FullBytes(Vec<u8>), FileFull, FileRange { start: u64, end_inclusive: u64 } }
pub struct ResponseStatus(pub u16); // associated constants: OK(200), NOT_MODIFIED(304), PARTIAL_CONTENT(206), etc.
```

## Usage Pattern

```rust
use eggserve_core::primitives::{
    SecureRoot, ConfinedPath, StaticPolicy,
    http::{validate_method, validate_request_target},
    planner::plan_file_response,
};

// 1. Validate request
let method = validate_method("GET")?;
let path = validate_request_target("/index.html")?;

// 2. Resolve filesystem
let root = SecureRoot::new("/srv/www", StaticPolicy::safe_default())?;
let resource = root.resolve(&path);

// 3. Plan response
match resource {
    ResolvedResource::File(file) => {
        let plan = file.plan_response(&method, None, None, None, None);
        // Use plan to construct HTTP response
    }
    _ => { /* handle other cases */ }
}
```

## Stability

The `primitives` module is the **stable** tier. Breaking changes bump the major version. Pre-1.0, minor versions may break. For the full API classification, see [api-stability.md](../docs/api-stability.md) and [release-contract.md](../docs/release-contract.md).

## Examples

### Duplicate request headers

```rust
use eggserve_core::primitives::header_block::HeaderBlock;

let mut headers = HeaderBlock::new();
headers.push_str("accept", "text/html").unwrap();
headers.push_str("accept", "application/json").unwrap();

// Get all values for a duplicate header
let all_accept = headers.get_all("accept");
assert_eq!(all_accept.len(), 2);

// get_unique fails on duplicates
assert!(headers.get_unique("accept").is_err());
```

### Safe unique-header access

```rust
use eggserve_core::primitives::header_block::HeaderBlock;

let mut headers = HeaderBlock::new();
headers.push_str("content-type", "text/html").unwrap();

// get_unique returns Ok(Some(value)) for single headers
let ct = headers.get_unique("content-type").unwrap().unwrap();
assert_eq!(ct.as_str(), "text/html");

// get_unique returns Ok(None) for absent headers
let missing = headers.get_unique("x-missing").unwrap();
assert!(missing.is_none());
```

### Connection metadata

```rust
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};

let info = ConnectionInfo {
    local_addr: "127.0.0.1:8000".parse().unwrap(),
    remote_addr: "127.0.0.1:12345".parse().unwrap(),
    scheme: Scheme::Https,
    tls: None,
};

assert_eq!(info.scheme, Scheme::Https);
assert_eq!(info.local_addr.port(), 8000);
```

### HEAD handling without handler special-casing

```rust
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};

// Build a response with a body
let resp = Response::builder()
    .status(StatusCode::OK)
    .header("content-type", "text/plain").unwrap()
    .body(ResponseBody::Bytes(b"hello".to_vec()))
    .unwrap();

// Normalize for HEAD — body is suppressed, headers preserved
let req = NormalizeRequest::new(true);
let head_resp = normalize_response(resp, &req).unwrap();

assert!(head_resp.body().unwrap().is_empty());
assert!(head_resp.headers().contains("content-type"));
```

### Duplicate response headers

```rust
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};

let resp = Response::builder()
    .status(StatusCode::OK)
    .header("set-cookie", "a=1").unwrap()
    .header("set-cookie", "b=2").unwrap()
    .body(ResponseBody::Bytes(b"ok".to_vec()))
    .unwrap();

let all = resp.headers().get_all("set-cookie");
assert_eq!(all.len(), 2);
assert_eq!(all[0].as_str(), "a=1");
assert_eq!(all[1].as_str(), "b=2");
```

### File-backed response (conceptual)

```rust
use eggserve_core::primitives::response::{BodyPlan, StaticResponsePlan};
use eggserve_core::primitives::response::ResponseStatus;

// Static response planning produces a BodyPlan::FileFull
// that streams from disk without loading into memory.
let plan = StaticResponsePlan {
    status: ResponseStatus::OK,
    headers: Default::default(),
    body: BodyPlan::FileFull,
};

// The runtime resolves the file handle and streams it directly.
// No Python memory copy is involved for file-backed responses.
match &plan.body {
    BodyPlan::FileFull => { /* full file streaming */ }
    BodyPlan::FileRange { start, end_inclusive } => { /* range streaming */ }
    BodyPlan::FullBytes(data) => { /* in-memory body */ }
    BodyPlan::Empty => { /* no body */ }
}
```

### Invalid framing rejection

```rust
use eggserve_core::primitives::canonical::{ResponseConstructionError, StatusCode};

// Status code 0 is rejected
assert!(StatusCode::new(0).is_err());

// Status code 1000 is rejected
assert!(StatusCode::new(1000).is_err());

// Transfer-Encoding is forbidden in canonical responses
let err = ResponseConstructionError::ForbiddenFramingHeader("transfer-encoding".into());
assert!(err.to_string().contains("transfer-encoding"));
```

## See Also

- [response-planning.md](response-planning.md) — Response planner details
- [filesystem-confinement.md](filesystem-confinement.md) — `SecureRoot` internals
- [path-confinement.md](path-confinement.md) — `ConfinedPath` construction
- [eggserve-python.md](eggserve-python.md) — Python bindings for primitives
- [api-stability.md](../docs/api-stability.md) — API classification by stability tier
- [release-contract.md](../docs/release-contract.md) — Product surface and compatibility commitments

## API Classification

| API item | Language | Status | Security invariant | Downstream use case |
|----------|----------|--------|-------------------|---------------------|
| `StaticPolicy` | Rust, Python | Implemented and stable-ish | Denies all optional behaviors by default (symlinks, dotfiles, directory listing) | Policy configuration for downstream serving |
| `PathPolicy` | Rust, Python | Implemented and stable-ish | Controls dotfile acceptance and backslash rejection at parse time | Request-target filtering |
| `ConfinedPath` / `RequestTarget` | Rust, Python | Implemented and stable-ish | Rejects traversal, NUL, ambiguous separators, Windows prefixes, reserved names, ADS | Request-target validation |
| `SecureRoot` | Rust, Python | Implemented and stable-ish | Canonicalizes root, enforces policy, descriptor-relative resolution on Unix | Filesystem resolution entry point |
| `ResolvedResource` | Rust, Python | Implemented and stable-ish | Capability object — no public constructor, only obtainable through resolution | Serving decision (file, directory, denied, not-found) |
| `ResolvedFile` | Rust, Python | Implemented but provisional | File handle opened under policy during resolution; no public constructor | Metadata access and response planning |
| `ResolvedDirectory` | Rust, Python | Implemented but provisional | Directory listing filtered by policy; child resolution uses originating policy | Directory listing and navigation |
| `StaticResponsePlan` / `ResponsePlan` | Rust, Python | Implemented and stable-ish | Framework-independent value object; status, headers, body plan | Response construction |
| `HeaderMapPlan` | Rust | Implemented and stable-ish | Case-insensitive header storage | Response header construction |
| `validate_method` | Rust, Python | Implemented and stable-ish | Only GET/HEAD allowed; all others rejected | Request method validation |
| `validate_request_body` | Rust, Python | Implemented and stable-ish | Rejects non-empty bodies on GET/HEAD, invalid Content-Length, Transfer-Encoding | Body framing validation |
| `validate_request_target` | Rust, Python | Implemented and provisional | Coarse origin-form check (starts with `/`, no whitespace) | Pre-validation before full path parsing |
| `BodyPlan` | Rust | Implemented and provisional | Variants: Empty, FullBytes, FileFull, FileRange | Body source selection |
| `BodySource` | Rust, Python | Implemented | Owns resolved file handle; converts to Hyper body without path reopening | Safe body streaming for downstream servers |
| `BodyKind` | Rust, Python | Implemented | Discriminant: Empty, Bytes, FileFull, FileRange | Body type identification |
| `BodySourceError` | Rust, Python | Implemented | InvalidRange, AlreadyConsumed | Error handling for body conversion |
| `ResponseStatus` | Rust | Implemented and stable-ish | Associated constants for common HTTP status codes | Status code mapping |
| `Server` | Python | Implemented | Rust owns socket I/O, timeouts, file streaming; Python supplies optional handler callback | Dynamic server use in Python |
| `HttpClient` | Rust, Python | Implemented, experimental | Feature-gated (`client`). Uses hyper client, enforces timeouts, verifies TLS by default | Low-level outbound HTTP requests |
| `ClientConfig` | Rust, Python | Implemented, experimental | Timeout policy, max response body bytes, TLS verification flag | Client configuration |
| `ClientRequest` / `ClientRequestBuilder` | Rust, Python | Implemented, experimental | Method/URL/header/body validation before network I/O | Request construction |
| `ClientResponse` | Rust, Python | Implemented, experimental | Status, headers, body (fully buffered with max-bytes enforcement) | Response consumption |
| `ClientError` | Rust, Python | Implemented, experimental | 12-variant taxonomy: InvalidUrl, UnsupportedScheme, Timeout, TlsError, etc. | Structured error handling |
| `Scheme` / `ParsedUrl` | Rust | Implemented, experimental | Hand-parsed URL validation, no new dependency | URL validation |
| `Method` | Rust, Python | Implemented and stable | Validated HTTP method; standard + extension; token validation | Canonical method identity |
| `HttpVersion` | Rust, Python | Implemented and stable | HTTP/1.0, HTTP/1.1 | Canonical version identity |
| `HeaderBlock` | Rust, Python | Implemented and stable | Ordered Vec of HeaderField; case-insensitive lookup; duplicate preservation | Canonical header collection |
| `RequestTarget` | Rust, Python | Implemented and stable | Validated origin-form target (path + query) | Canonical request target |
| `RequestHead` | Rust, Python | Implemented and stable | Canonical request head with `try_from_hyper()` conversion | Transport-independent request inspection |
| `ConnectionInfo` | Rust, Python | Implemented and stable | Transport metadata (addrs, scheme, TLS); separate from headers | Connection-level metadata |
| `StatusCode` | Rust, Python | Implemented and stable | Validated HTTP status code (1–999 range) with classification helpers | Canonical status code |
| `ResponseHead` | Rust, Python | Implemented and stable | Status + HeaderBlock; transport-independent response metadata | Canonical response head |
| `ResponseBody` | Rust, Python | Implemented and stable | Body representation: Empty, Bytes | Canonical response body |
| `Response` | Rust, Python | Implemented and stable | Complete response with one-shot body consumption | Canonical complete response |
| `normalize_response()` | Rust | Implemented and stable | Single normalization path: HEAD suppression, body-forbidden enforcement, hop-by-hop stripping, content-length computation | Response normalization |

## Invariant checklist

- Safe defaults are shared across CLI, Rust primitives, and Python primitives
- Path parsing rejects traversal, NUL, ambiguous separators, Windows prefixes, reserved device names, and ADS syntax according to current policy
- Static filesystem resolution must not serve outside root
- Under Unix safe defaults, symlink denial is descriptor-relative
- `--follow-symlinks` is weaker and outside the descriptor-relative guarantee
- Python consumers must not reconstruct and reopen paths for static serving
- Future Python server APIs must keep socket I/O, timeout enforcement, and file streaming in Rust
- Future client APIs must verify TLS by default
- Unsupported behavior fails closed or is explicitly out of contract
