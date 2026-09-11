# Configuration Inventory and Ownership Model

Single source of truth for every operator-facing configuration field, its
owner, enforcement path, and cross-frontend mapping.

Plan 179 canonical authority: shared runtime/transport defaults and
scalar/cross-field validation live once in
`crates/eggserve-core/src/runtime_limits.rs` (`SharedRuntimeValues` +
`Violation`). `Limits::default()`, `RuntimeConfig::default()`, builders, and
the `ServeConfig` bridge consume those values; `Limits::validate()` delegates
shared checks to the kernel and appends static-only budgets;
`RuntimeConfigBuilder::build()` builds the candidate shared group once and
adapts kernel violations to `ServerError::Config`;
`try_from_serve_config()` validates `Limits` then projects through the single
`RuntimeConfig::from_shared_runtime` helper. Plan 179 shared fields remain
single-source; the later feature-gated `Http2Config` is intentionally a
protocol-owned namespace rather than a duplicate shared knob set.
The internal `RuntimeConfig::http1_config()` projection owns the HTTP/1 parser
view of the compatibility `max_buf_size` and `max_headers` fields without
duplicating defaults or validation. With the `http2` feature,
`RuntimeConfig::http2` owns the bounded H2 transport controls and the driver
projects them directly into Hyper's HTTP/2 builder. Plans 186 and 190 keep this path
experimental; the response-stall fallback is connection-scoped because the
public Hyper server API has no safe stream-reset hook at EggServe's response
body boundary. Future protocol-specific controls belong to their own
projections.

With the `http3` feature, `RuntimeConfig::http3` owns the optional QUIC/H3
transport envelope. `ServerBuilder::http3_identity` supplies PEM paths for a
separate TLS 1.3/`h3` configuration; callers do not provide Quinn or rustls
transport objects. H3 remains disabled by default and requires the native TCP
listener alongside its same-port UDP endpoint.

## Ownership split

**Runtime/transport** (canonical kernel in `runtime_limits.rs`):

- Connection/file-stream concurrency, request-body ceiling, HTTP/1
  parser buffer/header/target limits, optional HTTP/2 stream/flow-control
  limits, in-flight service admission,
  header/TLS/handshake/handler/body/total/shutdown/keep-alive/response-write
  timeouts, max requests per connection, file-stream chunk size
- `RuntimeConfig` fields — transport enforcement; `Limits` fields — validated
  subset fed into `RuntimeConfig` via the bridge
- CLI flags (`--max-connections`, `--handler-timeout`, etc.)
- Python `Server()` constructor params (`max_connections`, `handler_timeout_secs`, etc.)

**Static-service-only** (outside the generic runtime):

- `ServeConfig` fields — root directory, bind address, static policy
- `StaticPolicy` fields — symlink, dotfile, directory listing policies
- `Limits::max_listing_entries`, `max_listing_response_bytes`
- `Limits::max_extra_headers`, `max_extra_header_bytes` (enforced via
  `validate_static_metadata_with_limits`, not the transport kernel)

**Frontend-only** (owning surface only):

- Bind exposure acknowledgements, CLI logging format, Python callback
  concurrency, compatibility-facade response buffering

A setting may be shared by reference, but only one validated value owns enforcement.

## Validation timing

- `Limits::validate()` — shared kernel + static listing budgets, all violations.
- `RuntimeConfigBuilder::build()` — shared kernel + `ResponsePolicy`, joined
  `ServerError::Config`.
- `RuntimeConfig::validate()` — full hand-constructed config check (shared
  kernel + response policy). Required because `RuntimeConfig` fields are
  public; builder validation alone is not an invariant boundary.
- `ServerBuilder::build()` / `static_service()` — reject invalid
  hand-constructed `RuntimeConfig` before semaphore/Hyper construction.
- `Server::start_with_service()` — defense-in-depth re-validation.
- `RuntimeState::try_new()` — validated constructor (preferred);
  `RuntimeState::new()` validates and panics with context.
- `serve_http1_connection(_with_id)` — caller-owned boundary: invalid configs
  log and return `ConnectionOutcome::Internal` instead of panicking a task.

Services may lower request-body ceilings but cannot raise the runtime
`max_request_body_bytes` hard ceiling.

The compatibility `ServeConfig`/`StaticService` path remains the owner of
static-service policy. Its bridge projects validated runtime values into the
same `RuntimeConfig` authority; the broader static-service builder migration
is intentionally deferred until the compatibility adapter can change without
breaking its current API.

## Field inventory

### Concurrency limits

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_connections` | `RuntimeConfig` | 64 | > 0 | `--max-connections` | `max_connections` (`Server` + `lowlevel.RuntimeConfig`) | Connection semaphore in accept loop |
| `max_in_flight_requests` | `RuntimeConfig` | 64 | > 0 | `--max-in-flight-requests` | `max_in_flight_requests` (`lowlevel`; compat facade default) | Service semaphore held across `Service::call`; 503 on exhaustion |
| `max_file_streams` | `RuntimeConfig` | 32 | > 0 | `--max-file-streams` | `max_file_streams` | One file-stream semaphore per running server |
| `max_active_tunnels` | `RuntimeConfig` | 64 | > 0 | — (static never tunnels; Rust `RuntimeConfigBuilder::max_active_tunnels`) | — (Python sync facade unchanged; async projection owns Plan 204) | Server-wide tunnel semaphore held until tunnel close; 503 on exhaustion; H1 keeps owning connection alive, H2/H3 stream-scoped |
| `max_python_callbacks` | `PyServer` | 8 | > 0 | N/A | `max_python_callbacks` | Callback semaphore in `PythonCallbackService` |
| `max_listing_entries` | `Limits` | 4096 | > 0, <= 10485760 (entries) | N/A | N/A | Directory listing enumeration |
| `max_listing_response_bytes` | `Limits` | 1 MiB | > 0 | N/A | N/A | Directory listing response body cap |

### Parser ceilings

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_buf_size` | `RuntimeConfig` → internal `Http1Config` | 65536 | 8192–4194304 | `--max-buf-size` | `max_buf_size` (`lowlevel`; compat default) | Hyper `http1::Builder::max_buf_size`, set explicitly per connection |
| `max_headers` | `RuntimeConfig` → internal `Http1Config` | 100 | 1–10000 | `--max-headers` | `max_headers` (`lowlevel`; compat default) | Hyper `http1::Builder::max_headers` (Hyper answers 431 itself) |
| `max_header_bytes` | `RuntimeConfig` | 32768 | 1024–1048576 | `--max-header-bytes` | `max_header_bytes` (`lowlevel`; compat default) | Post-parse aggregate check in `convert_request_head`; 431 pre-service |
| `max_request_target_bytes` | `RuntimeConfig` | 8192 | 128–65536 | `--max-request-target-bytes` | `max_request_target_bytes` (`lowlevel`; compat default) | Post-parse target check in `convert_request_head`; 414 pre-service |

Hyper exposes no aggregate header-byte, request-target, or request-line knob: the request line is bounded jointly by the parser buffer and the target ceiling.

### HTTP/2 transport limits (`http2` feature)

`Http2Config` is opt-in and transport-owned. Its defaults are explicit and
validated independently of the server-wide `max_in_flight_requests` admission
semaphore: 100 concurrent streams, 32 KiB decoded header list, 16 KiB maximum
frame, 256 KiB stream receive window, 1 MiB connection receive window, and a
256 KiB send buffer. Reset-flood ceilings and optional keepalive settings are
also validated by the config owner. The native Rust runtime uses cleartext
prior knowledge and TLS ALPN; the Python compatibility facade remains H1-only.

### HTTP/3 and QUIC transport limits (`http3` feature)

`Http3Config` is experimental and disabled by default. It pins 100 incoming
bidirectional streams, 16 unidirectional streams, 256 KiB per-stream and
4 MiB per-connection receive windows, a 4 MiB send window, 60 seconds idle
timeout, 64 pending handshakes, 32 KiB decoded field sections, and a 256 KiB
outgoing H3 send bound. `stateless_retry` and `advertise_alt_svc` default to
false. The runtime applies the shared header/target/body/admission/file and
timeout limits in addition to these protocol-owned values. See
[`http3.md`](http3.md) for listener, lifecycle, and qualification ownership.

### Timeouts

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `header_read_timeout` | `RuntimeConfig` | 10s | > 0 | `--header-timeout` | `header_timeout_secs` | Hyper header read timeout (also bounds idle keep-alive gaps when shorter than the idle timeout) |
| `connection_total_timeout` | `RuntimeConfig` | 60s | > 0 | `--connection-total-timeout` | `connection_total_timeout_secs` | Hard maximum connection lifetime (driver deadline loop) |
| `handler_timeout` | `RuntimeConfig` | 30s | > 0 | `--handler-timeout` | `handler_timeout_secs` | `tokio::time::timeout` around service call |
| `body_read_timeout` | `RuntimeConfig` | 30s | > 0 | `--body-read-timeout` | `body_timeout_secs` | Total body consumption deadline |
| `keep_alive_idle_timeout` | `RuntimeConfig` | 60s | > 0, independent of total | `--keep-alive-idle-timeout` | `keep_alive_idle_timeout_secs` (`lowlevel`; compat default) | Driver deadline loop; resets on request/transport activity |
| `response_write_timeout` | `RuntimeConfig` | 30s | > 0, independent of total | `--response-write-timeout` | `response_write_timeout_secs` (`lowlevel`; compat default) | Driver + `ProgressIo` no-progress tracking; steady progress never trips |
| `max_requests_per_connection` | `RuntimeConfig` | None (unlimited) | None or >= 1 | `--max-requests-per-connection` (`0` = unlimited) | `max_requests_per_connection` (`lowlevel` `None`; compat default) | H1 `Connection: close`, H2 graceful drain/GOAWAY after the limit response; every response counts |
| `graceful_shutdown_timeout` | `RuntimeConfig` | 10s | > 0 | N/A | `graceful_shutdown_timeout_secs` | Drain deadline after SIGTERM |

### Body policy

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_request_body_bytes` | `RuntimeConfig` | 0 | 0 (reject bodies) or <= 1073741824 (1 GiB) | N/A | `max_request_body_bytes` | Hard ceiling, no service can exceed |

Body policy is service-declared via `Service::request_body_policy(&RequestHead)` (method-aware). The runtime only enforces the `max_request_body_bytes` ceiling. Incomplete body handling always closes the connection (hardcoded, not configurable).

### Network / binding

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `bind` | `ServeConfig` / `RuntimeConfig` | 127.0.0.1:8000 | SocketAddr | `--bind`, `--port`, `--addr` | `bind`, `port` | TCP listener bind |
| `default_content_type` | `ServeConfig` | `application/octet-stream` | non-empty header-safe string | `--content-type` | `SimpleHTTPRequestHandler.default_content_type` | Unknown-suffix static responses |
| `extra_response_headers` | `ServeConfig` | none | ordered safe name/value pairs | `-H`, `--header` | `SimpleHTTPRequestHandler.extra_response_headers` | Final static status-200 responses only |
| `error_policy` | `ServeConfig` / `RuntimeConfig.response_policy` | Minimal | `Minimal` \| `Empty` | N/A (Rust-only) | `error_policy` (`lowlevel` `minimal`/`empty`; compat default `minimal`) | Runtime-generated error bodies; application `Ok` never rewritten |
| `response_policy.server_identification` | `RuntimeConfig` | None (suppressed) | None \| fixed string | N/A (Rust `server_header(..)`) | `server_header` (`lowlevel`; compat default suppressed) | `Server` on responses; never versions |
| `response_policy.date_policy` | `RuntimeConfig` | SystemClock | `SystemClock` \| `Custom` \| `Suppress` | N/A (Rust-only) | `date_policy` (`lowlevel` `system`/`suppress`; `Custom` Rust-only) | Sole `Date` authority; Hyper auto-`Date` disabled |
| `response_policy.stripped_response_headers` | `RuntimeConfig` | none | validated denylist (no framing/`date`/`content-range`) | N/A (Rust-only) | `stripped_response_headers` (`lowlevel`; compat default none) | Post-service removal; `minimal_fingerprint()` strips `x-powered-by` |
| `trusted_proxy.peers` | `RuntimeConfig` | none (nothing trusted, loopback included) | exact IP/CIDR list, no DNS | N/A (Rust `trusted_proxy_peer(..)`; CLI safe-default) | `trusted_proxies` (`lowlevel` list; compat default none) | Immediate-peer trust for PROXY/forwarded; untrusted stays untrusted |
| `trusted_proxy.trust_unix` | `RuntimeConfig` | false (never implicit) | bool | N/A (Rust `trust_unix_local(..)`) | `trust_unix_local` (`lowlevel`) | Unix/peer-less header trust only when explicitly set; PROXY never read from Unix |
| `trusted_proxy.proxy_protocol` | `RuntimeConfig` | disabled, 5s timeout (max 60s) | `enabled` + `timeout` | N/A (Rust `proxy_protocol_enabled(..)`/`proxy_protocol_timeout(..)`) | `proxy_protocol` (`lowlevel` bool) | Bounded preamble before TLS/HTTP only from trusted peers; malformed/untrusted closes before service; disabled interprets bytes normally |
| `trusted_proxy.forwarded` | `RuntimeConfig` | disabled, 4 KiB / 16 elements (256–16384 B, 1–64 elements) | `standard_enabled` + `legacy_enabled` + budgets | N/A (Rust `forwarded_standard(..)`/`forwarded_legacy(..)`) | `forwarded_standard`/`forwarded_legacy` (`lowlevel`) | Single-hop rightmost-wins header policy with conflict fail-closed; canonical Host/target never rewritten; H3 ignores |

### Filesystem policy

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `root` | `ServeConfig` | "." | PathBuf | `--directory` | `root` | PinnedRoot at startup |
| `directory_listing` | `StaticPolicy` | Disabled | enum | `--directory-listing` | `directory_listing` (StaticPolicy) | Directory listing response |
| `symlinks` | `StaticPolicy` | Denied | enum | `--follow-symlinks` | `follow_symlinks` (StaticPolicy) | Path traversal resolution |
| `dotfiles` | `StaticPolicy` | Denied | enum | `--allow-dotfiles` | `allow_dotfiles` (StaticPolicy) | Dotfile path component check |
| `static_metadata.emit_etag` / `emit_last_modified` | `StaticPolicy` | true / true | bool | N/A (Rust-only) | N/A | Static `ETag`/`Last-Modified`; `minimal_fingerprint()` suppresses both |
| `stream_chunk_size` | `Limits` / `RuntimeConfig` | 8192 | >= 64, <= 1 MiB | N/A | N/A | File streaming read chunk size |

### TLS (feature-gated, Plan 203)

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `tls_config` | `RuntimeConfig` | None | Option\<Arc\<ServerConfig\>\> | `--tls-cert` + `--tls-key` | N/A | TLS handshake via rustls (fallback when no reload handle) |
| `tls_reload_handle` | `RuntimeConfig` | None | Option\<TlsReloadHandle\> | N/A (Rust-only) | N/A | Atomic snapshot for new handshakes; wins over `tls_config` |
| `tls_expose_peer_chain` | `RuntimeConfig` | false | bool | N/A (Rust-only) | N/A | Opt-in bounded DER chain in `TlsInfo` (8 × 64 KiB) |

## Naming drift (cross-boundary)

These are intentional API-surface differences, not duplicates:

| Rust field | Python param | CLI flag | Notes |
|---|---|---|---|
| `header_read_timeout` | `header_timeout_secs` | `--header-timeout` | Python/CLI drop "read" |
| `body_read_timeout` | `body_timeout_secs` | `--body-read-timeout` | Python drops "read" |
