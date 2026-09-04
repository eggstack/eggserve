# Migration Guide

This document covers migration paths for superseded APIs in eggserve. It is
intended for downstream consumers upgrading to releases that include canonical
HTTP types (Plans 047–049).

## Legacy → Canonical Type Mapping

### ReadOnlyMethod → Method

`ReadOnlyMethod` (GET/HEAD only) remains stable. `Method` (standard +
extension) is the canonical type for new code.

| Legacy | Canonical | Change |
|--------|-----------|--------|
| `ReadOnlyMethod::Get` | `Method::get()` | Same value, richer API |
| `ReadOnlyMethod::Head` | `Method::head()` | Same value, richer API |
| `validate_method("GET")?` | `Method::new("GET")?` | Unified constructor |

**Migration**: Replace `ReadOnlyMethod` with `Method` in new code. Existing
`ReadOnlyMethod` usage continues to work. `Method` supports extension methods
(e.g. `PURGE`) and provides `is_safe()`, `is_idempotent()`,
`permits_static_resolution()`.

### validate_request_target() → RequestTarget::parse()

| Legacy | Canonical | Change |
|--------|-----------|--------|
| `validate_request_target("/path")?` | `RequestTarget::parse("/path")?` | Typed errors, query support |

**Migration**: `RequestTarget::parse()` returns a typed `RequestTarget` with
`.path()` and `.query()` accessors. Error variants are more specific (Empty,
AbsoluteUri, AuthorityForm, AsteriskForm, ContainsWhitespace, NotOriginForm).

### Response planning types

The existing response planning types (`StaticResponsePlan`, `BodyPlan`,
`HeaderMapPlan`, `ResponseStatus`) remain stable. Canonical response types
(`StatusCode`, `Response`, `normalize_response`) are a parallel API for
constructing transport-independent responses.

| Use Case | Existing | Canonical |
|----------|----------|-----------|
| File response planning | `plan_file_response()` | N/A (planner is stable) |
| Custom response construction | `ResponsePlan` namedtuple (Python) | `Response::builder()` (Rust) |
| Status code | `ResponseStatus` (u16 newtype) | `StatusCode` (validated, classified) |

### Python header representation

| Legacy | Canonical | Limitation |
|--------|-----------|------------|
| `Response.headers: HashMap` | `HeaderBlock: Vec<HeaderField>` | HashMap loses duplicates |

**Migration**: Python handlers using `Response(headers={"Set-Cookie": "a=1"})`
cannot represent duplicate headers. For duplicate headers, use the
static-responder path which preserves duplicates through `HeaderMapPlan`.

## StatusCode Range Change

`StatusCode` now only accepts values in the 100–599 range (standard three-digit
HTTP status codes). Values below 100 and above 599 are no longer valid.

| Before | After | Impact |
|--------|-------|--------|
| `StatusCode` accepted 1–999 | `StatusCode` accepts 100–599 | `StatusCode::new(0)` through `StatusCode::new(99)` and `StatusCode::new(600)` through `StatusCode::new(999)` now return `Err(InvalidStatus)` |

This aligns with HTTP/1.1 syntax requirements: status codes are always
three-digit integers. Values below 100 are not defined by HTTP/1.1 and have no
semantic meaning in eggserve's response pipeline.

**Migration**: If you were using status codes below 100, replace them with
appropriate three-digit codes. The `normalize_metadata()` function enforces this
range for all response producers.

## Deprecation Policy

Deprecated stable items remain functional for at least one minor release after
deprecation is announced. Removal requires explicit release notes and migration
guidance.

### Currently Deprecated

None. All legacy APIs remain stable and functional.

### Internally Retained (not for downstream use)

| Item | Location | Reason |
|------|----------|--------|
| `ResolvedFile::into_std_file()` | `primitives::secure_root` | Python bindings only; behind `python-bindings-internal` feature |
| `ResolvedFile::into_parts()` | `primitives::secure_root` | Python bindings only; behind `python-bindings-internal` feature |
| `ResolvedFile::from_parts()` | `primitives::secure_root` | Python bindings only; behind `python-bindings-internal` feature |

These methods are disabled by default and are not part of the public contract.

## Plan 087: Structured Logging

### --log-format json behavior change

`--log-format json` now emits valid JSON Lines (one JSON object per line on stderr). Previous versions emitted a placeholder format that was not guaranteed to be parseable.

**Migration**: Any tooling parsing `--log-format json` output must accept standard JSON Lines. The schema includes `schema_version`, `severity`, `event`, `timestamp`, `message`, `connection_id`, `request_seq`, and `fields`.

### Operational events

All operational events (connection lifecycle, request handling, listener errors, shutdown) now emit structured log events via the `ops` module. Previously, many of these events were not logged or used ad-hoc `eprintln!` output.

### Connection IDs

Connections are now assigned a unique 64-bit connection ID at accept time. This ID is included in all connection-scoped and request-scoped log events.

### Listener error backoff

Backoff for transient listener errors now resets on successful accepts. Previously, backoff accumulated without reset.

### Fatal accept errors

Fatal accept errors (unknown `io::ErrorKind` variants) now terminate the accept loop immediately. Previously, the loop retried these errors with backoff, which was incorrect for truly fatal conditions.

## Plan 077: Runtime Timeout Semantics and Structured Shutdown

### response_write_timeout → connection_total_timeout

The `response_write_timeout` field has been renamed to `connection_total_timeout` to accurately reflect its behavior. The field wraps the entire Hyper connection future (total connection lifetime), not individual response writes.

| Before | After | Change |
|--------|-------|--------|
| `Limits::response_write_timeout` | `Limits::connection_total_timeout` | Renamed; same default (60s) |
| `RuntimeConfig::response_write_timeout` | `RuntimeConfig::connection_total_timeout` | Renamed; same default (60s) |
| `RuntimeConfigBuilder::response_write_timeout()` | `RuntimeConfigBuilder::connection_total_timeout()` | Renamed; same default (60s) |
| `--response-write-timeout` (CLI) | `--connection-total-timeout` (CLI) | Renamed; same default (60s) |
| `response_write_timeout_secs` (Python) | `connection_total_timeout_secs` (Python) | Renamed; same default (60s) |

**Migration**: Replace all references to `response_write_timeout` with `connection_total_timeout`. The behavior is unchanged — it remains a total connection lifetime limit. If you were relying on this timeout to close stalled writes, note that it still functions as a hard deadline for the entire connection.

> **Plan 164 update**: the `response_write_timeout` name now exists again with
> different, no-progress semantics (close after 30s default with zero forward
> socket progress while a response is outstanding; steady progress never
> trips it). It is unrelated to the pre-077 total-lifetime field above. The
> old name was reused deliberately and documented in
> [the timeout reference](timeout-reference.md#6-connection-total-timeout).
> A progress-aware write timeout (inactivity-based) is implemented.

### Zero-duration timeout validation

`RuntimeConfigBuilder::build()` now rejects zero-duration values for all timeout fields. Previously, zero durations were silently accepted and could cause immediate request failures.

| Field | Minimum | Default | Error on zero |
|-------|---------|---------|---------------|
| `header_read_timeout` | > 0 | 10s | Yes |
| `connection_total_timeout` | > 0 | 60s | Yes |
| `handler_timeout` | > 0 | 30s | Yes |
| `body_read_timeout` | > 0 | 30s | Yes |
| `graceful_shutdown_timeout` | > 0 | 10s | Yes |

**Migration**: If you were setting any timeout to `Duration::ZERO`, choose a small positive value instead (e.g., `Duration::from_millis(1)`).

### Shutdown observability

The `ShutdownComplete` operational event now includes the abort count (`aborted=N`) when tasks are forcibly terminated. The `ForcedShutdownStarted` event is now emitted before `tasks.abort_all()` when the grace deadline expires.

**Migration**: If you are parsing operational log output, update parsers to handle the new `(aborted=N)` suffix in `ShutdownComplete` messages and the new `ForcedShutdownStarted` event type.

## Plan 078: Custom-Service Ownership and Connection Metadata

### Removed: `ServerBuilder::build_with_service()`

The `build_with_service()` method accepted a service value but silently discarded it. The service had to be supplied again at `start_with_service()`. This method has been removed.

| Before | After | Change |
|--------|-------|--------|
| `ServerBuilder::build_with_service(svc)` | `ServerBuilder::build()` + `.start_with_service(svc)` | Removed; use `start_with_service()` |

**Migration**: Replace `server.build_with_service(svc)` with `server.build()` and pass the service to `start_with_service()`.

### Python: `Request.local_addr` and `Request.scheme`

The Python `Request` object now includes `local_addr` (the server's local socket address) and `scheme` (`"http"` or `"https"`). The `remote_addr` field is now populated from the actual transport peer instead of being `None`.

| Field | Before | After |
|-------|--------|-------|
| `Request.remote_addr` | `None` (always) | Real peer socket address string (e.g., `"127.0.0.1:54321"`) |
| `Request.local_addr` | Not present | Real local socket address string (e.g., `"127.0.0.1:8000"`) |
| `Request.scheme` | Not present | `"http"` or `"https"` |

**Migration**: No action required for existing code. The new fields are additive. If you were working around `remote_addr` being `None`, the workaround is no longer needed.

### Connection metadata reflects transport peer

Connection metadata (`remote_addr`, `local_addr`, `scheme`) reflects the transport-level peer, not the end-client identity. When eggserve is behind a reverse proxy, `remote_addr` will be the proxy's address. End-client identity requires explicit proxy-header validation (see `docs/deployment.md`).

## Plan 163: Transport-neutral connection driver and ConnectionInfo evolution

### ConnectionInfo fields are now Optional

`ConnectionInfo` fields `local_addr` and `remote_addr` are now `Option<SocketAddr>` instead of mandatory `SocketAddr`. TCP transports provide `Some(addr)`; non-socket transports (e.g., memory channels, piped I/O) expose `None`. Scheme and TLS remain caller-asserted.

| Before | After | Change |
|--------|-------|--------|
| `ConnectionInfo.local_addr: SocketAddr` | `ConnectionInfo.local_addr: Option<SocketAddr>` | TCP = `Some`, non-socket = `None` |
| `ConnectionInfo.remote_addr: SocketAddr` | `ConnectionInfo.remote_addr: Option<SocketAddr>` | TCP = `Some`, non-socket = `None` |

New helpers: `with_socket_addrs()`, `without_socket_addrs()`, `socket_endpoints()` → `Option<SocketEndpoints>`, `has_socket_endpoints()` → `bool`. `SocketEndpoints` is a struct with `local`/`remote: SocketAddr`.

**Migration**: Wrap existing addr access in `.unwrap()` or `?` for TCP-only code. Use `socket_endpoints()` when both must be present, or `has_socket_endpoints()` as a guard.

### Transport-neutral connection driver

`eggserve-core::server::connection` now exposes a transport-neutral driver:

- `serve_http1_connection(io, service, config, context, runtime_state, shutdown)` — canonical HTTP/1 driver over any `AsyncRead + AsyncWrite`.
- `serve_http1_connection_with_id(..., conn_id)` — same, with explicit connection ID for log correlation.
- `ConnectionContext::for_tcp(local_addr, remote_addr, tls_info)` — TCP context.
- `ConnectionContext::for_non_socket(scheme, tls_info)` — non-socket context (no addresses).
- `ConnectionShutdown::new()` — shutdown token; clone for select.
- `ConnectionOutcome` — return type: `Normal`, `HeaderTimeout`, `ClientError`, `TotalTimeout`, `Shutdown`.
- `RuntimeState::new(&config)` — shared admission pool; construct once, `Arc::clone` per connection. `new_for_testing` is hidden.

Raw Hyper `serve_connection` is now crate-private. TCP `Server` shares the same pipeline via `serve_http1_connection_with_id`.

### Python: ConnectionInfo local/remote are Optional

Python `ConnectionInfo` `local_addr` and `remote_addr` are now `Optional[str]` (default `None`). Existing positional string construction still works. Callers must check for `None` instead of assuming present.

## Plan 165: Response privacy policy and `server_header` migration

`RuntimeConfig::server_header: Option<String>` is replaced by
`RuntimeConfig::response_policy: ResponsePolicy` (experimental; `server` module
may change pre-1.0). `StaticPolicy` gains `static_metadata: StaticMetadataPolicy`;
`ServeConfig` gains `error_policy: ErrorRepresentationPolicy`.

| Before | After | Change |
|--------|-------|--------|
| `config.server_header` | `config.response_policy.server_identification` / `config.server_header_value()` | Field moved into policy |
| `RuntimeConfigBuilder::server_header(..)` | same method (still exists) | Now sets `response_policy.server_identification` |
| `plan_file_response_with_preconditions(..)` | same + `plan_file_response_with_preconditions_and_metadata(.., StaticMetadataPolicy)` | Old function preserves defaults (emit both) |
| `StaticPolicy { directory_listing, symlinks, dotfiles }` | add `static_metadata: StaticMetadataPolicy::standard()` or `..Default` | New field, additive |
| `ServeConfig { .. }` without `error_policy` | add `error_policy: Minimal` or `..ServeConfig::default()` | New field, additive |

**Migration**: Replace `config.server_header` reads with
`config.server_header_value()` or `config.response_policy.server_identification`.
Builder `.server_header(..)` keeps working. For struct literals, add
`..Default::default()` / `..ServeConfig::default()` or the new fields
explicitly. Hyper automatic `Date` is now disabled (`auto_date_header(false)`);
EggServe `DatePolicy` (`SystemClock` default, `Custom(provider)`, `Suppress`)
is the sole authority. `ResponsePolicy::minimal_fingerprint()` +
`StaticMetadataPolicy::minimal_fingerprint()` is the generic
minimal-fingerprint profile (minimizes signals, does not claim
un-fingerprintability).

## Plan 173: octet-preserving canonical HTTP metadata

`HeaderValue` is now byte-preserving. `as_str() -> &str` (infallible) is
removed; use `as_bytes() -> &[u8]` for forwarding and `to_str() ->
Result<&str, HeaderValueTextError>` for checked text interpretation.

| Before | After | Change |
|--------|-------|--------|
| `HeaderValue::new("text")?.as_str()` | `HeaderValue::new("text")?.to_str()?` | Fallible text access |
| `block.get_first("x")?.as_str()` | `block.get_first("x")?.to_str()?` | Fallible at interpretation |
| `HeaderValue::new(s)` only | `HeaderValue::from_bytes(b)` / `from_static_bytes(b)` + `push_bytes(..)` | Opaque obs-text without UTF-8 coercion |
| `value.as_str().is_empty()` | `value.is_empty()` / `value.as_bytes().is_empty()` | Byte-length checks |

Validation matches `http::HeaderValue::from_bytes` (`HTAB`, `SP`–`~`,
obs-text `0x80`–`0xFF`; rejects `CR`/`LF`/`NUL`/`DEL`/`CTL`s). Leading/trailing
`SP`/`HTAB` are still stripped as a deliberate `OWS` invariant (RFC 9110
field-line parsing), for both text and byte constructors. Inbound conversion
(`RequestHead::try_from_hyper`, connection pipeline) and outbound conversion
(`to_hyper_response`) preserve exact octets; protocol headers (`Content-Length`,
`Connection` tokens, conditionals/range) perform checked `to_str()` at the
point of interpretation. `Display` for `HeaderValue`/`HeaderBlock` is lossy
diagnostic only — never use it for wire conversion.

`RequestTarget` gains truthful byte accessors (`raw_bytes()`, `path_bytes()`,
`query_bytes()`) over the accepted origin-form bytes. `/path` and `/path?`
deliberately canonicalize identically (`query() == None`).

Python stdlib-shaped surfaces remain text-only: opaque request headers are
omitted from `Request.headers`/`header_items` and `HeaderBlock` getters/iteration
rather than lossily coerced. Rust canonical primitives are byte-correct;
Python facades enforce the text subset.

**Migration**: replace `as_str()` on `HeaderValue` with `to_str()?` (or
`as_bytes()` for forwarding). Add `HeaderValueTextError` to imports where the
error type is named. No `Hyper` types enter the canonical API; the two
adapters remain `RequestHead::try_from_hyper()` and `to_hyper_response()`.

## Plan 171: outbound response conversion boundary

### Release note for the next `0.2.0` minor transition

Plan 169 changed the implementation of `primitives::to_hyper_response()` to
support the stable one-owner `ResponseStream` contract: producers are `Send`
but need not be `Sync`, and the internal body uses unsynchronized erasure.
The public function now returns a Hyper response with an opaque
`http_body::Body<Data = bytes::Bytes, Error = std::io::Error>` body instead of
promising the concrete `http_body_util::combinators::BoxBody` type.

The source-compatibility check covered four realistic consumers. Inference-only
calls and generic consumers constrained only by `http_body::Body` compile on
both the 0.1.x API shape and current `main`; an explicit
`hyper::Response<BoxBody<...>>` annotation and a helper returning that named
type compile on the 0.1.x API shape but fail on current `main`. This is an
intentional pre-1.0 breaking transition, not a patch-compatible change.

Migration:

```rust,no_run
use bytes::Bytes;
use eggserve_core::primitives::{to_hyper_response, Response};

fn send_response<B>(response: hyper::Response<B>)
where
    B: http_body::Body<Data = Bytes, Error = std::io::Error>,
{
    // Pass the response to the downstream transport/application server.
    let _ = response;
}

fn convert(response: Response) -> Result<(), Box<dyn std::error::Error>> {
    send_response(to_hyper_response(response)?);
    Ok(())
}
```

Do not name `BoxBody`, `UnsyncBoxBody`, or another concrete erased body type.
The `ResponseStream` producer remains one-shot and `Send + 'static`; `Sync`
is not required and concurrent polling is unsupported. The former public
`to_hyper_response_with_file_stream_semaphore()` helper is now runtime-internal
and is not a supported downstream API.

## Breaking Change Policy

Patch releases preserve stable source compatibility. Before 1.0, intentional
breaking changes to stable Rust APIs require an explicit minor-version
transition (for example `0.1.x` → `0.2.0`), release notes, and migration
guidance. Experimental APIs may change under their separately documented
policy. Enum variant additions to stable enums are breaking changes.
