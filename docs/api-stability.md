# API Stability Inventory

This document classifies every exported Rust and Python item by stability tier: **stable**, **experimental**, or **internal**. It is the authoritative reference for the release contract.

See [release-contract.md](release-contract.md) for the overall product surface and behavioral guarantees.

## Stability Tiers

| Tier | Meaning |
|------|---------|
| **stable** | Intentional public API. Patch releases must not break stable APIs. While pre-1.0, a minor release may break stable APIs only with explicit release notes and migration guidance. Semantic behavior identified in the release contract is covered by conformance tests. Unspecified formatting, debug output, log text, and internal implementation details are not stable unless explicitly documented. |
| **experimental** | May change in any non-patch release. Consumers should pin versions. Functionality is tested but the interface is not frozen. Experimental APIs may be omitted from language parity. |
| **internal** | Unavailable or unsupported for downstream use. No compatibility guarantee. Internal Python names are not exported through `__all__`. Internal Rust features do not become accidental default features. May be removed without notice. |

## Stability Rules

### Enum variant exhaustiveness

Stable enum variants are exhaustive unless documented otherwise. Adding a new variant to a stable enum is a breaking change.

### Thread safety (Send/Sync)

All stable canonical request types (`Method`, `HttpVersion`, `HeaderBlock`, `HeaderName`, `HeaderValue`, `HeaderField`, `RequestTarget`, `RequestHead`, `ConnectionInfo`, `Scheme`, `TlsInfo`, `StatusCode`, `ReadOnlyMethod`) implement `Send + Sync`. This means they can be safely shared between threads and sent across thread boundaries. This is a compile-time guarantee enforced by `public_api_consumers::canonical_types_are_send_and_sync`.

Error types (`MethodError`, `HttpVersionError`, `HeaderError`, `DuplicateHeaderError`, `RequestTargetError`, `RequestHeadError`, `ResponseConstructionError`, `RequestValidationError`) implement `Send` but not necessarily `Sync`, as they may contain `String` payloads.

`ResponseBody` implements `Send` (contains `Vec<u8>`). `Response` (head + body) implements `Send`.

Python wrapper types (`#[pyclass(frozen)]`) are frozen/immutable but are not `Send`/`Sync` in the Rust sense — Python GIL constraints apply.

### Exception classes, fields, and messages

Python exception classes and their field names are stable. Exception message strings are not stable and may change between releases.

### Header ordering and duplicate preservation

Rust `HeaderMapPlan` preserves insertion order and duplicate headers. This behavior is stable. Python `Response.headers` uses a `HashMap` and does not preserve duplicates — this is a known limitation, not a bug.

### Denial/error taxonomy variants

`PathRejection`, `RequestValidationError`, `ClientError`, and `ResourceDeniedReason` variants are stable. Adding a new variant is a breaking change.

### Serialization and repr output

`Debug` output, `Display` formatting, and Python `repr()` output are not stable unless explicitly documented as a contract.

### Deprecation

Deprecated stable items must remain functional for at least one minor release after deprecation is announced. Removal requires explicit release notes and migration guidance.

As of Plan 049, no items are deprecated. All legacy APIs (`ReadOnlyMethod`,
`validate_method()`, `validate_request_target()`, `StaticResponsePlan`,
`BodyPlan`, `HeaderMapPlan`, `ResponseStatus`) remain stable and functional.

## Rust API — `eggserve-core`

### Crate Root Modules

| Module | Visibility | Tier | Notes |
|--------|-----------|------|-------|
| `config` | pub | stable | `ServeConfig`, `StartupSummary`, `ServeState` |
| `limits` | pub | stable | `Limits` resource-limit configuration |
| `policy` | pub | stable | `StaticPolicy`, policy enums |
| `service` | pub | experimental | `handle_request()` — HTTP handler |
| `primitives` | pub | stable | Public facade for embedding consumers |
| `error` | pub(crate) | internal | Not externally visible |
| `fs` | pub(crate) | internal | Not externally visible |
| `mime` | pub(crate) | internal | Not externally visible |
| `path` | pub(crate) | internal | Not externally visible |
| `response` | pub(crate) | internal | Not externally visible |

### `config` Module

| Item | Tier | Notes |
|------|------|-------|
| `ServeConfig` | stable | All fields public: `bind`, `root`, `limits`, `static_policy` |
| `ServeConfig::default()` | stable | Binds to `127.0.0.1:8000`, serves `.` |
| `StartupSummary` | stable | Read-only summary after startup |
| `ServeState` | stable | Runtime state; fields are pub(crate), methods are public |

### `limits` Module

| Item | Tier | Notes |
|------|------|-------|
| `Limits` | stable | `max_connections`, `max_file_streams`, `header_read_timeout`, `response_write_timeout`, `graceful_shutdown_timeout` are pub; `max_request_body_bytes` is pub(crate) |
| `Limits::default()` | stable | Safe defaults |

### `policy` Module

| Item | Tier | Notes |
|------|------|-------|
| `DirectoryListingPolicy` | stable | Enum: `Disabled`, `Enabled` |
| `SymlinkPolicy` | stable | Enum: `Denied`, `Follow` |
| `DotfilePolicy` | stable | Enum: `Denied`, `Serve` |
| `StaticPolicy` | stable | All fields public |
| `PolicyMode` | internal | pub(crate) |

### `service` Module

| Item | Tier | Notes |
|------|------|-------|
| `handle_request()` | experimental | The HTTP handler. May change with hyper version or protocol updates |

### `primitives` Module — Path Types

| Item | Tier | Notes |
|------|------|-------|
| `ConfinedPath` | stable | Parsed request target |
| `PathDotfilePolicy` | stable | Alias for `path::DotfilePolicy` |
| `PathPolicy` | stable | Parse-time dotfile/backslash policy |
| `PathRejection` | stable | 16-variant rejection taxonomy |

### `primitives` Module — Policy Types (re-exported)

| Item | Tier | Notes |
|------|------|-------|
| `DirectoryListingPolicy` | stable | Re-export from `policy` |
| `DotfilePolicy` | stable | Re-export from `policy` |
| `StaticPolicy` | stable | Re-export from `policy` |
| `SymlinkPolicy` | stable | Re-export from `policy` |

### `primitives` Module — Secure Root

| Item | Tier | Notes |
|------|------|-------|
| `SecureRoot` | stable | Primary entry point for filesystem resolution |
| `ResolvedResource` | stable | Capability object — file, directory, not-found, denied |
| `ResolvedFile` | stable | File handle under policy. No public constructor |
| `ResolvedDirectory` | stable | Directory listing and child resolution |
| `ResourceDeniedReason` | stable | SymlinkDenied, DotfileDenied, RootEscapeDenied, PolicyDenied |
| `resolve_and_plan()` | stable | Combined resolve + plan convenience function |
| `ResolveAndPlanError` | stable | Error taxonomy for resolve_and_plan |

### `primitives` Module — HTTP Validation

| Item | Tier | Notes |
|------|------|-------|
| `ReadOnlyMethod` | stable | GET, HEAD only (legacy, prefer `Method`) |
| `RequestValidationError` | stable | 6-variant HTTP validation error |
| `validate_method()` | stable | Method string → ReadOnlyMethod (legacy) |
| `validate_request_body()` | stable | Body metadata validation |
| `validate_request_target()` | stable | Target string validation (legacy) |

### `primitives` Module — Canonical HTTP Request Types

**All canonical request types are stable** after conformance completion in Plan 049.

| Item | Tier | Notes |
|------|------|-------|
| `Method` | stable | Validated HTTP method; standard + extension support |
| `MethodError` | stable | Empty, InvalidToken |
| `HttpVersion` | stable | HTTP/1.0, HTTP/1.1 |
| `HttpVersionError` | stable | Unsupported version |
| `HeaderBlock` | stable | Duplicate-preserving ordered header collection |
| `HeaderName` | stable | Validated header name (token) |
| `HeaderValue` | stable | Validated header value (no CR/LF/NUL) |
| `HeaderError` | stable | InvalidName, InvalidValue, NameTooLong |
| `DuplicateHeaderError` | stable | Returned by get_unique() on duplicates |
| `HeaderField` | stable | `pub name: HeaderName`, `pub value: HeaderValue` |
| `RequestTarget` | stable | Validated origin-form target (path + query) |
| `RequestTargetError` | stable | 6-variant target validation error |
| `RequestHead` | stable | Canonical request head: method, target, version, headers |
| `RequestHeadError` | stable | Conversion error from Hyper |
| `ConnectionInfo` | stable | Transport metadata: addrs, scheme, TLS |
| `Scheme` | stable | Http, Https |
| `TlsInfo` | stable | Protocol version, server name |

### `primitives` Module — Response Planning

| Item | Tier | Notes |
|------|------|-------|
| `plan_file_response()` | stable | Full response plan for a file |
| `evaluate_conditional_headers()` | stable | If-None-Match / If-Modified-Since |
| `evaluate_if_none_match()` | stable | ETag comparison |
| `evaluate_range_header()` | stable | Range parsing |
| `evaluate_if_range()` | stable | If-Range evaluation |
| `generate_etag()` | stable | ETag from metadata |
| `plan_directory_listing()` | stable | Directory listing plan |

### `primitives` Module — Response Types

| Item | Tier | Notes |
|------|------|-------|
| `ResponseStatus` | stable | Newtype u16 with associated constants |
| `ResponseHeader` | stable | `pub name: String`, `pub value: String` |
| `HeaderMapPlan` | stable | Ordered Vec of ResponseHeader; preserves duplicates |
| `FileRange` | stable | `pub start: u64`, `pub end_inclusive: u64` |
| `BodyPlan` | stable | Enum: Empty, FullBytes, FileFull, FileRange |
| `StaticResponsePlan` | stable | Status + headers + body plan |
| `ConditionalRequestOutcome` | stable | NotModified, FullResponse, Malformed |
| `RangeRequestOutcome` | stable | Satisfiable, NotSatisfiable, MalformedOrUnsupported, MultipleRanges |

### `primitives` Module — Canonical Response Types

**All canonical response types are stable** after conformance completion in Plan 049.

| Item | Tier | Notes |
|------|------|-------|
| `StatusCode` | stable | Validated HTTP status code (1–999) |
| `ResponseHead` | stable | Status + `HeaderBlock`; transport-independent response metadata |
| `ResponseBody` | stable | Body representation: Empty, Bytes |
| `Response` | stable | Complete response: head + body; one-shot consumption |
| `ResponseBuilder` | stable | Validated builder for Response |
| `NormalizeRequest` | stable | Context for response normalization (is_head flag) |
| `ResponseConstructionError` | stable | InvalidStatus, InvalidHeader, ForbiddenFramingHeader, BodyAlreadyConsumed, ContentLengthMismatch |
| `normalize_response()` | stable | Applies HEAD suppression, body-forbidden enforcement, hop-by-hop stripping, content-length computation |
| `to_hyper_response()` | stable | Converts canonical Response to Hyper Response |

### `primitives` Module — Body Types

| Item | Tier | Notes |
|------|------|-------|
| `BodySource` | stable | Owned body: Empty, Bytes, FileFull, FileRange |
| `BodyKind` | stable | Discriminant for BodySource |
| `BodySourceError` | stable | InvalidRange, AlreadyConsumed |

### `primitives::client` Module (feature-gated: `client`)

**All client items are experimental.**

| Item | Tier | Notes |
|------|------|-------|
| `HttpClient` | experimental | Buffered, no pooling, no redirects, no streaming |
| `ClientConfig` | experimental | Timeout and TLS config |
| `ClientRequest` | experimental | Request value object |
| `ClientRequestBuilder` | experimental | Builder pattern |
| `Method` | experimental | Get, Head, Post, Put, Delete, Patch |
| `validate_header()` | experimental | Header name/value validation |
| `ClientResponse` | experimental | Buffered response |
| `ClientError` | experimental | 12-variant error taxonomy |
| `Scheme` | experimental | Http, Https |
| `ParsedUrl` | experimental | Hand-parsed URL |

## Python API — `eggserve` Package

### `eggserve.__init__` — Always Available

| Item | Tier | Notes |
|------|------|-------|
| `__version__` | stable | Version string |
| `ServeConfig` | stable | Server configuration dataclass |
| `ServerProcess` | stable | Subprocess lifecycle manager |
| `serve_directory()` | stable | Blocking convenience function |
| `ResponsePlan` | stable | Namedtuple for response plan |
| `NATIVE_AVAILABLE` | stable | Whether native module loaded |

### `eggserve.server` — `__all__`

| Item | Tier | Notes |
|------|------|-------|
| `StaticPolicy` | stable | Policy dataclass (directory_listing, follow_symlinks, allow_dotfiles) |
| `ServeConfig` | stable | Config dataclass |
| `ServerProcess` | stable | Subprocess lifecycle |
| `serve_directory()` | stable | Blocking server |

### `eggserve._native` — Primitives (when NATIVE_AVAILABLE)

| Item | Tier | Notes |
|------|------|-------|
| `EggserveError` | stable | Base exception |
| `PathPolicyError` | stable | Child of EggserveError |
| `RequestTargetError` | stable | Child of EggserveError |
| `RequestValidationError` | stable | Child of EggserveError |
| `SecureRootError` | stable | Child of EggserveError |
| `BodySourceError` | stable | Child of EggserveError |
| `LifecycleError` | stable | Raised on lifecycle violations (double start, stop before start) |
| `ResponseConstructionError` | stable | Raised when handler returns an invalid Response object |
| `MethodError` | stable | Invalid HTTP method |
| `HttpVersionError` | stable | Unsupported HTTP version |
| `HeaderError` | stable | Invalid header name or value |
| `DuplicateHeaderError` | stable | Duplicate header on unique access |
| `PathPolicy` | stable | Frozen, mirrors Rust PathPolicy |
| `StaticPolicy` | stable | Frozen, mirrors Rust StaticPolicy |
| `RequestTarget` | stable | Frozen, mirrors ConfinedPath |
| `SecureRoot` | stable | Resolution entry point |
| `ResolvedResource` | stable | Capability object |
| `ResolvedFile` | stable | File metadata + plan_response/body_for_plan |
| `ResolvedDirectory` | stable | list() + resolve_child() |
| `BodySource` | stable | Body read access |
| `validate_method()` | stable | Method validation |
| `validate_request_body()` | stable | Body validation |
| `validate_request_target()` | stable | Target validation |
| `generate_etag()` | stable | ETag generation |
| `parse_method()` | stable | Create validated Method |
| `parse_http_version()` | stable | Create validated HttpVersion |
| `Method` | stable | Canonical HTTP method |
| `HttpVersion` | stable | Canonical HTTP version |
| `HeaderBlock` | stable | Duplicate-preserving headers |
| `ConnectionInfo` | stable | Transport metadata |
| `CanonicalRequest` | stable | Canonical request head |

### `eggserve._native` — Server Types (when NATIVE_AVAILABLE)

| Item | Tier | Notes |
|------|------|-------|
| `Request` | stable | Frozen request object |
| `Response` | stable | Frozen response builder |
| `StaticResponder` | stable | Static file responder |
| `StaticPolicyWrapper` | stable | Frozen policy wrapper |
| `ServerSecureRoot` | stable | Frozen secure root |
| `ServerBodySource` | stable | Body read + to_response |
| `ServerRequestError` | stable | Raised as ValueError |
| `Server` | stable | Rust-owned HTTP server |

### `eggserve._native` — Client Types (when NATIVE_AVAILABLE, feature-gated: `client`)

**All client types are experimental.**

| Item | Tier | Notes |
|------|------|-------|
| `HttpClient` | experimental | Buffered HTTP client |
| `ClientConfig` | experimental | Timeout/TLS config |
| `ClientRequest` | experimental | Request value |
| `ClientResponse` | experimental | Buffered response |
| `ClientError` | experimental | Error enum |
| `Method` | experimental | HTTP method enum |

### Internal Names (not in `__all__`)

| Name | Location | Tier |
|------|----------|------|
| `_find_binary()` | `_bin.py` | internal |
| `main()` | `_bin.py` | internal |
| `_parse_bind()` | `server.py` | internal |
| `_config_to_argv()` | `server.py` | internal |
| `_VALID_LOG_FORMATS` | `server.py` | internal |

## Internal Bridge APIs

The `python-bindings-internal` feature gate enables:

| Rust Item | Gate | Tier |
|-----------|------|------|
| `ResolvedFile::into_std_file()` | `python-bindings-internal` | internal |
| `ResolvedFile::into_parts()` | `python-bindings-internal` | internal |
| `ResolvedFile::from_parts()` | `python-bindings-internal` | internal |

These methods:
- Are disabled by default
- Are not documented as a user feature
- Are used only by the Python crate
- Do not appear in default Rust docs or package examples
- Are unavailable under `default` or `client` feature builds

## Key Design Decisions

### Header Representation

- **Rust**: `HeaderMapPlan` is an ordered `Vec<ResponseHeader>`. Duplicates are preserved.
- **Python**: `Response.headers` is `HashMap<String, String>`. Duplicates are lost.
- **Decision**: The Rust core preserves duplicates. The Python surface uses a flat dict for simplicity. This is an acceptable trade-off for the initial release. A future revision may add ordered-pair support if duplicate headers become a common use case.

### Response Contract

- Python handlers must return a `Response` object (or duck-typed equivalent).
- Invalid returns produce 500 without leaking tracebacks.
- HEAD is not special-cased in the Python handler path.
- 204 and informational statuses are not special-cased — the handler is responsible.
- Hop-by-hop headers are not filtered — the handler should not emit them.
- File-backed responses retain their Rust-owned capability and stream without an eager Python-memory copy.

### Client Stability

The client is **experimental** in the first release. It provides:
- Buffered, one-connection-per-request behavior
- No pooling, redirects, cookies, proxies, retries, decompression, or streaming
- TLS verification by default
- Timeout enforcement
