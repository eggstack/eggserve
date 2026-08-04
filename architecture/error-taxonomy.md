# Error Taxonomy Deep Dive

eggserve uses seven distinct error layers, each scoped to a specific subsystem. This separation ensures that errors carry precise context and map cleanly to HTTP responses without leaking internal details.

## Error Layers Overview

| Layer | Type | Scope | Variants |
|-------|------|-------|----------|
| Path parsing | `PathRejection` | Request target validation | 16 |
| Top-level crate | `Error` | General eggserve operations | 9 |
| HTTP-level | `RequestValidationError` | Request framing and method | 6 |
| Server lifecycle | `ServerError` | Startup, bind, shutdown | 10 |
| Per-request | `ServiceError` | Service handler failures | 4 (kinds) |
| Body consumption | `RequestBodyError` | Request body reading | 12 |
| HTTP client | `ClientError` | Outbound HTTP requests | 12 |

---

## `PathRejection` — Path Parsing Errors

**Location:** `eggserve-core::path::rejected`

Returned by the 6-stage path validation pipeline when a request target fails any validation check. These are the first errors a request encounters and prevent any filesystem access.

| Variant | Meaning | Example Trigger |
|---------|---------|-----------------|
| `Empty` | Request target is empty | `GET /` with no path |
| `TooLong` | Path exceeds 8192 bytes | Extremely long URL |
| `UnsupportedUriForm` | Not origin-form | `GET http://example.com/` |
| `MalformedPercentEncoding` | Invalid `%XX` sequence | `GET /%zz` |
| `InvalidUtf8` | Path contains invalid UTF-8 | Raw bytes in URL |
| `NulByte` | Path contains `%00` | `GET /%00/etc/passwd` |
| `AbsolutePath` | Path starts with `/` after decode | Absolute path injection |
| `ParentComponent` | Path contains `..` | `GET /../../../etc/passwd` |
| `CurrentComponent` | Path contains `.` | `GET /./etc/passwd` |
| `SeparatorAmbiguity` | Backslash or mixed separators | `GET /foo\bar` |
| `DotfileDenied` | Dotfile denied by path policy | `GET /.env` |
| `WindowsPrefixDenied` | Windows drive prefix detected | `GET /C:/` |
| `WindowsReservedNameDenied` | Windows reserved filename | `GET /CON` |
| `WindowsAlternateStreamDenied` | Windows alternate data stream | `GET /file::$DATA` |
| `SymlinkDenied` | Symlink denied by policy | Symlink in path |
| `RootEscapeDenied` | Canonical path escapes root | Symlink to `/etc` |

**HTTP mapping:** All path rejections produce 404 (Not Found) or 403 (Forbidden) responses. The rejection reason is logged but never exposed to the client.

---

## `Error` — Top-Level Crate Error

**Location:** `eggserve-core::error`

The general-purpose error type for eggserve-core operations. Wraps lower-level errors and provides a unified error surface.

| Variant | Meaning | Source |
|---------|---------|--------|
| `PathEscape` | Path escapes configured root | `PathRejection` conversion |
| `PathNotAccessible(String)` | Path is not accessible | Filesystem errors |
| `Config(String)` | Configuration error | Invalid config values |
| `Bind(String)` | Bind error | Address parsing/binding |
| `Runtime(String)` | Runtime error | General runtime failures |
| `RequestRejected(String)` | Request was rejected | `PathRejection` → `Error` |
| `ResponseConstruction` | Response construction failed | `ResponseConstructionError` |
| `Io` | I/O error | `std::io::Error` |
| `Client` | Client error (feature-gated) | `ClientError` |

**Conversions:**
- `PathRejection` → `Error::RequestRejected`
- `ResponseConstructionError` → `Error::ResponseConstruction`
- `std::io::Error` → `Error::Io`

---

## `RequestValidationError` — HTTP-Level Errors

**Location:** `eggserve-core::primitives::http`

Returned during request framing validation, before any path parsing or filesystem access. Prevents request smuggling and body-policy violations.

| Variant | Meaning | HTTP Status |
|---------|---------|-------------|
| `MethodNotAllowed` | Method not supported (non-GET/HEAD) | 405 |
| `InvalidContentLength` | Malformed Content-Length header | 400 |
| `BodyTooLarge` | Content-Length exceeds limit | 413 |
| `UnsupportedTransferEncoding` | Non-empty Transfer-Encoding on read-only request | 400 |
| `ConflictingBodyHeaders` | Both Content-Length and Transfer-Encoding present | 400 |
| `InvalidRequestTarget` | Request target is not origin-form | 400 |

**Validation functions:**
- `validate_method()` — checks if method is GET or HEAD
- `validate_request_body()` — checks Content-Length/Transfer-Encoding consistency and limits
- `validate_request_target()` — checks origin-form validity

---

## `ServerError` — Server Lifecycle Errors

**Location:** `eggserve-core::server::errors`

Errors from server startup, lifecycle management, and shutdown. These are returned to the caller (CLI, Python facade) before or during serving.

| Variant | Meaning | Category |
|---------|---------|----------|
| `Bind(io::Error)` | Failed to bind TCP listener | Startup |
| `Config(String)` | Invalid configuration | Startup |
| `AlreadyStarted` | Server already started | Lifecycle |
| `NotStarted` | Server not started | Lifecycle |
| `Accept(io::Error)` | Connection acceptance error | Runtime |
| `TlsSetup(String)` | TLS certificate/config error | Startup |
| `Transport(String)` | Response normalization/conversion error | Runtime |
| `ShutdownTimeout` | Graceful shutdown timed out | Runtime |
| `Startup(String)` | Fatal startup error | Startup |
| `Terminal(String)` | Terminal runtime error | Runtime |

**Error categories:**
- **Startup errors** (`Bind`, `Config`, `TlsSetup`) — returned before listener is ready
- **Lifecycle errors** (`AlreadyStarted`, `NotStarted`) — misuse of server handle
- **Runtime errors** (`Accept`, `ShutdownTimeout`, `Terminal`) — logged, not returned to callers
- **Transport errors** (`Transport`) — response normalization or body conversion failures

---

## `ServiceError` — Per-Request Errors

**Location:** `eggserve-core::server::service`

Errors from service handler invocation. The runtime converts these to HTTP responses without leaking internal details.

| Kind | HTTP Status | Meaning |
|------|-------------|---------|
| `Internal` | 500 | Unexpected internal failure |
| `Rejected(u16)` | (caller-specified) | Deliberate rejection |
| `Panic` | 500 | Handler panicked |
| `Timeout` | 504 | Handler timed out |

**Constructors:**
- `ServiceError::internal(msg)` — 500 error
- `ServiceError::rejected(status, msg)` — custom status code
- `ServiceError::panic(msg)` — handler panic (internal)
- `ServiceError::timeout(msg)` — handler timeout (internal)

**Safety:** Error messages are logged but never included in HTTP response bodies to prevent information leakage.

---

## `RequestBodyError` — Body Consumption Errors

**Location:** `eggserve-core::primitives::request_body_error`

Errors from request body reading. The runtime maps these to appropriate HTTP responses.

| Variant | HTTP Status | Meaning |
|---------|-------------|---------|
| `RejectedByPolicy` | 400 | Body rejected by policy (e.g. static service) |
| `DeclaredLengthTooLarge` | 413 | Content-Length exceeds limit |
| `LimitExceeded` | 413 | Body exceeded byte limit during consumption |
| `ReadTimeout` | 408 | Body read timed out |
| `PrematureEof` | 400 | Connection closed before body fully received |
| `LengthMismatch` | 400 | Actual body length != declared Content-Length |
| `InvalidChunkFraming` | 400 | Invalid Transfer-Encoding chunks |
| `Cancelled` | 499 | Body consumption cancelled |
| `Disconnected` | 499 | Client disconnected |
| `AlreadyConsumed` | 500 | Body already consumed (programmer error) |
| `MixedConsumptionMode` | 500 | Switched between read_all and streaming |
| `Transport(String)` | 500 | Transport-level error |

**Classification helpers:**
- `is_policy_rejection()` — policy rejection
- `is_limit_exceeded()` — limit violation (declared or consumed)
- `is_timeout()` — read timeout
- `is_disconnect()` — disconnect or premature EOF
- `is_consumption_state()` — already consumed or mixed mode

---

## `ClientError` — HTTP Client Errors

**Location:** `eggserve-core::primitives::client::error`

Feature-gated (`client`). Errors from outbound HTTP client operations.

| Variant | Meaning |
|---------|---------|
| `InvalidUrl(String)` | Malformed URL |
| `UnsupportedScheme(String)` | Non-http/https scheme |
| `MissingHost` | URL has no host component |
| `InvalidHeader(String)` | Invalid header name/value |
| `BodyTooLarge { limit, actual }` | Request body exceeds limit |
| `Timeout(String)` | Connection or request timeout |
| `DnsError(String)` | DNS resolution failed |
| `ConnectError(String)` | TCP connection failed |
| `TlsError(String)` | TLS handshake/verification failed |
| `ProtocolError(String)` | HTTP protocol error |
| `ResponseBodyTooLarge { limit }` | Response body exceeds limit |
| `Io(io::Error)` | I/O error |

---

## Error Conversion Flow

```
PathRejection ──→ Error::RequestRejected ──→ 404/403
RequestValidationError ──→ 400/405/413
ServerError ──→ process exit (startup) or log (runtime)
ServiceError ──→ 500/504
RequestBodyError ──→ 400/408/413/499/500
ClientError ──→ propagated to caller
```

## Design Principles

1. **No information leakage** — Error messages are logged but never sent to clients in response bodies.
2. **Precise HTTP mapping** — Each error variant maps to a specific HTTP status code.
3. **Layer separation** — Each subsystem has its own error type with appropriate context.
4. **Classification helpers** — Body errors provide `is_*()` methods for programmatic handling.
5. **Conversion chain** — Lower-level errors convert to higher-level errors with appropriate context loss.
