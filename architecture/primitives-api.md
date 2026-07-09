# Primitives API — Deep Dive

The `primitives` module is the intended public boundary for embedding consumers. It provides path validation, filesystem resolution, HTTP request validation, and response planning — all without depending on Hyper types for the public surface.

## Module Location

`eggserve-core::primitives/` — the `pub` facade for external consumers.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `primitives/mod.rs` | Re-exports all public types |
| `secure_root.rs` | `primitives/secure_root.rs` | `SecureRoot`, `ResolvedFile`, `ResolvedDirectory`, `ResolvedResource` |
| `http.rs` | `primitives/http.rs` | `ReadOnlyMethod`, request validation functions |
| `planner.rs` | `primitives/planner.rs` | Response planning (conditional, range, ETag) |
| `response.rs` | `primitives/response.rs` | Planning types (`StaticResponsePlan`, `BodyPlan`, etc.) |
| `body.rs` | `primitives/body.rs` | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming |

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
    DirectoryListingDenied,
    RootEscape,
    IoError(String),
}
```

### `ResolvedFile` (`secure_root.rs`)

A capability object — no public constructor. Obtained only through `SecureRoot::resolve()`.

```rust
pub struct ResolvedFile {
    pub(crate) file: std::fs::File,
    pub(crate) metadata: std::fs::Metadata,
    pub(crate) content_type: String,
    pub(crate) etag: String,
}
```

Public methods:
- `length()` → `u64`
- `modified()` → `Option<SystemTime>`
- `content_type()` → `&str`
- `etag()` → `&str`
- `plan_response(...)` → `StaticResponsePlan`
- `plan_conditional_response(...)` → `StaticResponsePlan`
- `into_body(&StaticResponsePlan)` → `Result<BodySource, BodySourceError>`
- `into_range_body(start, end_inclusive)` → `Result<BodySource, BodySourceError>`

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

The `primitives` module is the **stable** tier. Breaking changes bump the major version. Pre-1.0, minor versions may break.

## See Also

- [response-planning.md](response-planning.md) — Response planner details
- [filesystem-confinement.md](filesystem-confinement.md) — `SecureRoot` internals
- [path-confinement.md](path-confinement.md) — `ConfinedPath` construction
- [eggserve-python.md](eggserve-python.md) — Python bindings for primitives

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
| Planned Python server primitive | Python | Missing, planned | Rust must own socket I/O, timeouts, and file streaming | Dynamic server use in Python |
| Planned HTTP client primitive | Rust, Python | Missing, planned | TLS verification by default | Downstream client adapters |

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
