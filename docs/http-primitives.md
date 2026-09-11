# HTTP Primitives Contract

The static service still accepts only GET/HEAD semantics and rejects request
content, but the reusable runtime does not globally reject
GET/HEAD/DELETE/OPTIONS/extension content. Custom services declare Buffer or
Stream policy for the actual method within the runtime body ceiling. TRACE
content remains transport-rejected.

eggserve exposes a documented, reusable HTTP primitive contract for downstream
projects. The default listener/runtime and Python facade are HTTP/1.1-only;
Rust builds with the opt-in `http2` feature also provide bounded H2 through the
experimental server boundary. The separate opt-in `http3` feature provides an
experimental native QUIC/H3 server adapter; canonical metadata alone never
silently enables either wire protocol.

Plans 186, 190, and 191 keep the native H2 path experimental. The deterministic suite,
two-family interop, h2spec classification, and Linux wire/flow-control/load
qualification pass; browser/platform evidence, trailer-scope determinism,
and a public safe per-stream reset hook are not yet release-complete. See
[`architecture/http2.md`](../architecture/http2.md) and the
[`qualification record`](../release/plan-191-http2-supported-tier-qualification.md).
For H2 Reject policy, DATA presence is taken from Hyper's protocol body state,
not from `Content-Length`; the response timeout observes application-body poll
progress rather than guaranteed stream-level wire progress.
The H3 transport boundary is documented in
[`architecture/http3.md`](../architecture/http3.md). Plans 188, 190, 192, 193, 194, and 195 close H3 as
experimental: deterministic bounded checks pass (including per-stream producer
no-progress timeout with stream reset under Plan 194, correctively qualified
under Plan 195), but independent-client,
adversarial-wire, and cross-platform runtime evidence remains incomplete and
Plan 192 remains `BLOCKED` on upstream `h3#338`/`h3#262`. See
the [`qualification record`](../release/plan-190-multiprotocol-corrective-qualification.md), the
[`readiness record`](../release/plan-192-http3-dependency-readiness.md), and the
[`Plan 194 correction`](../release/plan-194-http3-producer-timeout-correction.md).

## Supported protocol subset

- HTTP/1.1 server behavior through Hyper; optional native Rust HTTP/2 through
  the feature-gated server runtime.
- GET and HEAD for the static CLI path.
- Explicit method validation primitive for downstream code (`ReadOnlyMethod`).
- Origin-form request targets for static path parsing.
- No request bodies for the static CLI path.
- Configurable body metadata validation primitive for downstream code.
- Static full-file responses.
- Static range responses.
- Empty responses.
- Byte responses for small dynamic bodies.
- Streaming responses via `ResponseBody::Stream` (`ResponseStream` with
  optional known length). Known lengths emit runtime-generated
  `Content-Length`; unknown lengths omit it and let HTTP/1 select chunked
  framing. Empty chunks are skipped, large chunks are split (not rejected),
  and `HEAD`/1xx/204/205/304 never poll the producer. Producers must be
  `Send` and one-shot, but need not be `Sync`; a single connection task owns
  polling.
- Conditional GET/HEAD via `If-None-Match` and `If-Modified-Since`.
- Range requests via `Range` and `If-Range`.
- Generic 400/403/404/405/413/416/500/503 behavior for the CLI path.

### Unsupported in this contract

- Request body streaming into Python callbacks.
- Multipart range responses.
- Manual chunked construction (`Transfer-Encoding` stays runtime-owned;
  services use `ResponseStream::new` and let the runtime frame).
- Upgrade semantics except via Plan 199 generic tunnel (`take_tunnel()`/`accept`/`TunnelIo`; H1 `101` / `200` otherwise; denial ordinary HTTP).
- Absolute-form proxy requests.
- Authority-form CONNECT except as Plan 199 `Connect` tunnel (validated, bounded; denial 405/400 for static).
- Asterisk-form OPTIONS requests.

### Canonical trailers and interim responses (Plan 198)

Initial headers, trailers, interim responses, and final responses are distinct:

- **Initial headers** arrive with the request head / final response head.
- **Trailers** are terminal metadata (`Trailers`, distinct from `HeaderBlock`
  by type, duplicate/order-preserving, byte-preserving). Validation reuses
  canonical header rules plus a conservative denylist (framing/routing:
  `content-length`, `transfer-encoding`, `trailer`, `te`, `connection`,
  `keep-alive`, `proxy-*`, `upgrade`, `host`, `expect`). Limits default to
  `32` fields / `8 KiB` aggregate, enforced before unbounded allocation and
  before exposing data to services. One canonical validator serves H1/H2/H3;
  no adapter maintains a second policy.
- **Request trailers** become available only after content completion:
  `while next_chunk() {}; body.trailers().await?`, or
  `read_all_with_trailers()` for buffered bodies (`read_all()` discards by
  type, documented). Byte limits stay byte limits; trailer bounds are separate.
  Malformed/oversized trailers fail with `InvalidTrailers` (400) and mark the
  lifecycle failed. Dropping before trailers preserves abandoned-body safety.
  H1 without valid chunked-trailer framing cannot inject (adapters populate
  only from protocol trailer frames; repeated/data-after-trailers fail).
- **Response trailers** attach as one terminal source:
  `ResponseStream::with_trailers(stream, future)` /
  `with_known_length_and_trailers` (known length counts data only). Exactly one
  block, no data after (adapter polls bytes to completion, then the future
  once, then ends). `HEAD`/body-forbidden never poll either producer.
  Cancellation/drop is deterministic. Adapters map without buffering the body.
- **H1 policy**: application code never sets `Transfer-Encoding`/`Trailer`
  (runtime-owned, stripped). Response trailers emit only when the request
  signals `TE: trailers`; otherwise suppressed with diagnostics. `Trailer`
  header naming is omitted when not knowable (allowed). HTTP/1.0 never carries
  trailers (suppressed). Trailer producer failure after commitment truncates
  (H1 close / H2 stream reset / H3 stream reset, siblings survive), never a
  second HTTP error.
- **H2/H3**: terminal HEADERS / terminal field section via protocol-native
  frames, same canonical validator. Failures are stream-local, never widen to
  siblings.
- **Interim 1xx** use a bounded request-scoped `InterimSender`
  (`request.context().interim()`): only 1xx (no `101`, no `200+`), headers only
  (no body/trailers by type), no interim after final commitment, bounded
  `4` messages / `8 KiB` aggregate, runtime-owned fields normalized, HTTP/1.0
  suppressed (validated/counted, no wire bytes). `103 Early Hints` allowed
  generically; EggServe invents no preload policy.
- **`100 Continue`**: `Reject` rejects without inviting the body (413, no `100`);
  `Buffer`/`Stream` accept and Hyper owns wire `100` when the body is polled.
  Unknown `Expect` values fail with `417`. At most one application `100` per
  request via interim (second fails `DuplicateContinue`); runtime `100` is the
  wire authority and never duplicated by application interims on the wire in
  the current Hyper pipeline (interims validated/recorded; Hyper server APIs
  own emission where permitted — no raw-socket fallback).
- **Python**: `validate_trailers` / `validate_interim` text-only bounded helpers
  project the capability to `eggserve.lowlevel` without changing the synchronous
  `http.server` surface.

### Ecosystem interoperability (Plan 200, implemented)

Optional `http` / `http-body` / Tower adapters live behind `http-interop`
and `tower` features (never in default builds). See
[http-interop.md](http-interop.md): loss-aware metadata conversions with
`RawTargetExt` / `ConnectionInfoExt` / `AuthorityExt` / `LifecycleExt`,
`RequestBody: http_body::Body` (data + trailers, backpressured, sanitized),
`response_from_http_body` (framing-authoritative, validated trailers),
`TowerToEggserve` (per-request clones, no shared mutex) and
`EggserveToTower` (adapter-local readiness). Interim/tunnel capabilities
never enter `http::Extensions`.

### Generic tunnels (Plan 199)

Validated H1 `Upgrade`, `CONNECT`, and H2/H3 Extended `CONNECT` yield a
one-shot, transport-backed `TunnelCapability` on `RequestContext`
(`take_tunnel()`; second take `None`; `AfterCommit` after final commitment).
`TunnelRequest` carries `TunnelKind` (`Http1Upgrade`/`Connect`/
`ExtendedConnect`), optional bounded `ProtocolName` (`token`, 1–64, generic —
no hard-coded `WebSocket` variant), and optional `Authority`.
Pseudo-headers never appear as ordinary headers; protocol bytes are bounded
before allocation; ordinary header construction cannot fabricate a capability.
`accept(headers, handler)` consumes the capability and returns a handshake
`Response` (`101` H1 with runtime-added `Upgrade`/`Connection`, `200`
otherwise; framing rejected, hop-by-hop stripped, 32 fields / 8 KiB bound;
runtime — not the application — writes transition bytes, no raw socket) plus
bounded single-owner `TunnelIo` (`AsyncRead + AsyncWrite`, 32 KiB duplex,
`tokio::io::split` for explicit split, lifecycle-aware, no payload logged).
Denial is ordinary HTTP (unused capability dropped). H1 preserves read-ahead
bytes; H2/H3 respect flow control (stream-local, siblings survive); H3 generic
`:protocol` (e.g. `websocket`) is blocked by `h3` 0.0.8 (only
`webtransport`/`connect-udp` plus plain `CONNECT` supported). Tunnels hold a
server-wide `max_active_tunnels` permit (default 64, 503 on exhaustion,
released exactly once); ordinary `response_write_timeout` does not apply after
transition; hard `connection_total_timeout` remains the outer bound; graceful
shutdown drains within the deadline then aborts (no detached survivors).

## Request method validation

`primitives::http` provides a `ReadOnlyMethod` enum restricted to `GET` and `HEAD`:

```rust
pub enum ReadOnlyMethod {
    Get,
    Head,
}
```

`validate_method(method: &str)` returns `Ok(ReadOnlyMethod)` for `"GET"` and `"HEAD"`, or `Err(RequestValidationError::MethodNotAllowed)` for all other methods.

### Error mapping

| Method | Result |
|--------|--------|
| `GET` | `ReadOnlyMethod::Get` |
| `HEAD` | `ReadOnlyMethod::Head` |
| Any other | `RequestValidationError::MethodNotAllowed` → HTTP 405 |

## Request target validation

`RequestTarget::parse(target)` is the authoritative request-target classifier:

- Must start with `/`
- Must not be empty
- Must not contain whitespace
- Rejects absolute-form (`http://...`), authority-form (`host:port`), and asterisk-form (`*`)

Error: `RequestValidationError::InvalidRequestTarget` → HTTP 400 (via path parsing layer).

`ConfinedPath::from_path_component()` then performs only path security
validation (traversal, dotfiles, percent-encoding, and platform rules).
`ConfinedPath::parse()` remains a compatibility adapter that delegates target
classification to `RequestTarget`.

### Parser-level target behavior

Target validation happens at two layers:

1. **Hyper's parser** handles HTTP version parsing and request-line splitting. Hyper accepts HTTP/1.0 version lines, bare LF in header values, and certain malformed inputs that eggserve does not actively reject.
2. **eggserve's validation** (`RequestTarget::parse()` plus
   `ConfinedPath::from_path_component()`) rejects non-origin-form targets, path
   traversal, NUL bytes, and encoded separators.

The wire-correctness tests in `http_wire_correctness.rs` document exactly which rejections come from which layer.

## Request body metadata policy

`validate_request_body()` validates body-framing headers for GET/HEAD requests:

```rust
pub fn validate_request_body(
    content_length: Option<&str>,
    transfer_encoding: Option<&str>,
    max_body_bytes: u64,
) -> Result<(), RequestValidationError>
```

### Behavior under zero-body policy (max_body_bytes = 0)

| Input | Result |
|-------|--------|
| No headers | OK |
| `Content-Length: 0` | OK |
| `Content-Length: 1024` | `BodyTooLarge` → HTTP 413 |
| `Content-Length: not-a-number` | `InvalidContentLength` → HTTP 400 |
| `Content-Length: -1` | `InvalidContentLength` → HTTP 400 |
| `Content-Length: 99999999999999999999` | `InvalidContentLength` → HTTP 400 |
| `Transfer-Encoding: chunked` | `UnsupportedTransferEncoding` → HTTP 400 |
| `Content-Length: 0` + `Transfer-Encoding: chunked` | `ConflictingBodyHeaders` → HTTP 400 |
| `Transfer-Encoding: ` (empty/whitespace) | `UnsupportedTransferEncoding` → HTTP 400 |

### Configurable body limits

The `max_body_bytes` parameter allows downstream projects to set non-zero limits. When set to a positive value, `Content-Length` values up to that limit are accepted; values above it trigger `BodyTooLarge`.

## Header handling rules (octet-preserving, Plan 173)

Canonical `HeaderValue` stores validated field-value octets without UTF-8
interpretation (`from_bytes`/`from_static_bytes`/`as_bytes()`; fallible
`to_str()`). Validation matches `http::HeaderValue::from_bytes` (`HTAB`,
`SP`–`~`, obs-text `0x80`–`0xFF`; rejects `CR`/`LF`/`NUL`/`DEL`/`CTL`s).
Leading/trailing `SP`/`HTAB` are stripped as a deliberate `OWS` invariant for
both text and byte constructors. Inbound (`RequestHead::try_from_hyper`,
connection pipeline) and outbound (`to_hyper_response`) conversions preserve
exact octets; `Content-Length`, `Connection` tokens, and conditional/range
headers perform checked `to_str()` at interpretation. `Display` is lossy
diagnostic only. Aggregate header-byte limits count bytes, not Unicode
scalars. `HeaderBlock` remains ordered, duplicate-preserving, and
case-insensitive for lookup, with `push_bytes(..)` for opaque values.

`RequestTarget` exposes truthful byte accessors (`raw_bytes()`,
`path_bytes()`, `query_bytes()`) over accepted origin-form bytes; `/path` and
`/path?` deliberately canonicalize to `query() == None`. If the transport
normalizes a target before EggServe sees it, downstream `raw_path` should be
omitted rather than fabricated.

Python stdlib-shaped surfaces stay text-only: opaque request headers are
omitted from `Request.headers`/`header_items` and `HeaderBlock`
getters/iteration instead of being coerced.

Response headers are constructed as a `HeaderMapPlan` (ordered list of name/value pairs). The planner produces these headers:

### Full response (200 OK)

- `Content-Length` — file size
- `Content-Type` — MIME type from file extension
- `Accept-Ranges: bytes`
- `X-Content-Type-Options: nosniff`
- `Last-Modified` — from file metadata (when available)
- `ETag` — weak validator from size + mtime secs + mtime nanos (when available)

Generated ETags use `W/"<size>-<mtime-secs>-<mtime-nanos>"`. For a
pre-epoch mtime, the seconds component is negative, so the serialized form
contains two adjacent hyphens (for example, `W/"100--86400-0"`); this is
intentional and consumers should treat the ETag as an opaque value.

**Platform limitations:** The ETag currently incorporates file size, mtime seconds, and mtime nanoseconds. It does not include Unix device/inode identity or Windows volume serial/file identifier. This means two distinct files with identical size and nanosecond-precision mtime may produce the same ETag. The weak validator is acceptable for static files where byte-for-byte identity semantics are not required. Content-based hashing is explicitly out of scope.

### Range response (206 Partial Content)

- `Content-Length` — range size
- `Content-Type` — MIME type
- `Content-Range: bytes START-END/TOTAL`
- `Accept-Ranges: bytes`
- `X-Content-Type-Options: nosniff`
- `Last-Modified` — (when available)
- `ETag` — (when available)

### Not modified (304)

- `ETag` — current validator
- `Last-Modified` — (when available)

### Not range satisfiable (416)

- `Content-Length: 0`
- `Accept-Ranges: bytes`
- `Content-Range: bytes */TOTAL`

### Method not allowed (405)

- `Allow: GET, HEAD`

## Static response planning

The response planner (`primitives::planner`) is a pure function with no Hyper dependency. It produces `StaticResponsePlan` values from file metadata and request headers.

### Planning flow

```
Request with conditional headers
    │
    ▼
┌─────────────────────────────────┐
│ If-None-Match (ETag)           │
│  Match? → 304 Not Modified     │
└─────────────────┬───────────────┘
                  │ No match
                  ▼
┌─────────────────────────────────┐
│ If-Modified-Since               │
│  Not modified? → 304            │
└─────────────────┬───────────────┘
                  │ Modified
                  ▼
┌─────────────────────────────────┐
│ If-Range                        │
│  Mismatch? → serve full         │
└─────────────────┬───────────────┘
                  │ Match
                  ▼
┌─────────────────────────────────┐
│ Range header                    │
│  Valid? → 206 Partial           │
│  Invalid? → 416                 │
└─────────────────┬───────────────┘
                  │ No range
                  ▼
           200 OK (full body)
```

## Conditional request behavior

### If-None-Match

- Weak comparison: `W/"abc"` matches `W/"abc"` (inner value comparison).
- Wildcard: `If-None-Match: *` matches any resource with an ETag.
- Multiple ETags: comma-separated list; any match triggers 304.
- Returns 304 with `ETag` and `Last-Modified` headers, empty body.

### If-Modified-Since

- Only evaluated when `If-None-Match` is absent or does not match.
- Parsed via `httpdate::parse_http_date`. Malformed dates are silently ignored (treated as absent).
- Returns 304 when file modification time ≤ the given date.
- Returns 200 when file is newer.

## Range request behavior

### Supported formats

| Syntax | Meaning |
|--------|---------|
| `bytes=0-99` | First 100 bytes |
| `bytes=0-` | From byte 0 to EOF |
| `bytes=-10` | Last 10 bytes |
| `bytes=50-99` | Bytes 50 through 99 |

### Range evaluation rules

- **Suffix range (`bytes=-N`)**: Returns last N bytes. If N exceeds file size, returns the whole file.
- **Open-ended range (`bytes=START-`)**: Returns from START to EOF. If START ≥ file size, returns 416.
- **Closed range (`bytes=START-END`)**: Returns START through END (clamped to file size). If START > END, returns 416. If START ≥ file size, returns 416.
- **Multiple ranges**: Falls through to full 200 OK response (single-range only).
- **Unsupported unit**: Falls through to full 200 OK response.
- **Empty file**: All range requests return 416.

### If-Range

- Entity-tags use strong comparison. Weak ETags (including eggserve's generated
  metadata ETags) never authorize a range; they remain valid for
  `If-None-Match`.
- A matching valid `Last-Modified` date authorizes the range (206); stale,
  malformed, empty, or nonmatching values produce a full 200 OK response.
- With no `If-Range`, a satisfiable range is served normally.

## HEAD/GET parity

HEAD responses produce the same `StaticResponsePlan` as GET but with `BodyPlan::Empty`:

- Same status code
- Same headers (including `Content-Length`)
- No body transfer

This is mechanically enforced by the planner: `ReadOnlyMethod::Head` produces `BodyPlan::Empty` while `ReadOnlyMethod::Get` produces `BodyPlan::FileFull` or `BodyPlan::FileRange`.

Directory-listing HEAD responses retain the nonzero `Content-Length` of the
equivalent GET representation while transmitting no body. Origin responses
receive exactly zero or one runtime-generated IMF-fixdate `Date` header at
finalization per `DatePolicy` (default one system-clock `Date`; EggServe is the
sole authority with Hyper automatic `Date` disabled). `DatePolicy::Custom(provider)`
uses a caller-supplied trusted time value; `Suppress` emits zero `Date` as an
explicit RFC 9110 tradeoff. `Server` is suppressed by default with optional fixed
value (never versions). See `docs/deployment.md` minimal-fingerprint profile.

## Error mapping

| Error | HTTP Status | Description |
|-------|-------------|-------------|
| `RequestValidationError::MethodNotAllowed` | 405 | Method not in {GET, HEAD} |
| `RequestValidationError::InvalidRequestTarget` | 400 | Target not origin-form |
| `RequestValidationError::InvalidContentLength` | 400 | Malformed Content-Length |
| `RequestValidationError::BodyTooLarge` | 413 | Content-Length exceeds limit |
| `RequestValidationError::UnsupportedTransferEncoding` | 400 | Non-empty Transfer-Encoding |
| `RequestValidationError::ConflictingBodyHeaders` | 400 | Both CL and TE present |
| Path traversal / dotfile denial | 403 | Path policy violation |
| Malformed percent encoding | 400 | Bad %xx sequence |
| File not found | 404 | No file at resolved path |

## Downstream use by app-server/adapter projects

eggserve's primitive layer is designed for embedding. Downstream projects may build ASGI/WSGI/CGI/FastCGI/app servers externally using these primitives, but eggserve does not implement those protocols in-tree (Plan 167 no-go for CGI/FastCGI). Plan 199 provides generic tunnel handoff (`TunnelRequest`/`TunnelCapability`/`TunnelIo`; H1 `Upgrade`/`CONNECT`, H2/H3 Extended `CONNECT`; H3 generic `:protocol` blocked by `h3` 0.0.8); WebSocket framing itself stays downstream (see `tunnel_upgrade.rs`).

### Rust embedding

```rust
use eggserve_core::primitives::planner::plan_file_response;
use eggserve_core::primitives::http::ReadOnlyMethod;

let plan = plan_file_response(
    ReadOnlyMethod::Get,
    &file_metadata,
    "text/plain; charset=utf-8",
    if_none_match_header,
    if_modified_since_header,
    range_header,
    if_range_header,
);

// plan.status, plan.headers, plan.body are Hyper-independent
// Translate to your framework of choice
```

### Python embedding

```python
from eggserve.lowlevel import SecureRoot, StaticPolicy

root = SecureRoot("public", policy=StaticPolicy())
resource = root.resolve_path("/assets/app.css")
if resource.is_file:
    plan = resource.file.plan_response("GET")
    print(plan.status, plan.body_kind)  # 200 file_full
```

## See also

- [http-response-planning.md](http-response-planning.md) — detailed planner behavior
- [python-api.md](python-api.md) — Python API reference
- [architecture/response-planning.md](../architecture/response-planning.md) — architecture deep dive
