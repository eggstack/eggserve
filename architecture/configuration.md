# Configuration Inventory and Ownership Model

Single source of truth for every operator-facing configuration field, its
owner, enforcement path, and cross-frontend mapping.

## Ownership split

**Runtime-owned** (transport, concurrency, timeouts):

- `RuntimeConfig` fields — connection limits, timeouts, body ceiling
- `Limits` fields — validated subset fed into `RuntimeConfig`
- `Limits::stream_chunk_size` — translated once into `RuntimeConfig`
- CLI flags (`--max-connections`, `--handler-timeout`, etc.)
- Python `Server()` constructor params (`max_connections`, `handler_timeout_secs`, etc.)

**Static-service-owned** (filesystem, policy):

- `ServeConfig` fields — root directory, bind address, static policy
- `StaticPolicy` fields — symlink, dotfile, directory listing policies
- `Limits::max_file_streams` — translated once into `RuntimeConfig` and the
  one runtime-owned file-stream semaphore
- `Limits::max_listing_entries`, `max_listing_response_bytes`

A setting may be shared by reference, but only one validated value owns enforcement.

## Field inventory

### Concurrency limits

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_connections` | `RuntimeConfig` | 64 | > 0 | `--max-connections` | `max_connections` (`Server` + `lowlevel.RuntimeConfig`) | Connection semaphore in accept loop |
| `max_in_flight_requests` | `RuntimeConfig` | 64 | > 0 | `--max-in-flight-requests` | `max_in_flight_requests` (`lowlevel`; compat facade default) | Service semaphore held across `Service::call`; 503 on exhaustion |
| `max_file_streams` | `RuntimeConfig` | 32 | > 0 | `--max-file-streams` | `max_file_streams` | One file-stream semaphore per running server |
| `max_python_callbacks` | `PyServer` | 8 | > 0 | N/A | `max_python_callbacks` | Callback semaphore in `PythonCallbackService` |
| `max_listing_entries` | `Limits` | 4096 | > 0, <= 10485760 (entries) | N/A | N/A | Directory listing enumeration |
| `max_listing_response_bytes` | `Limits` | 1 MiB | > 0 | N/A | N/A | Directory listing response body cap |

### Parser ceilings

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_buf_size` | `RuntimeConfig` | 65536 | 8192–4194304 | `--max-buf-size` | `max_buf_size` (`lowlevel`; compat default) | Hyper `http1::Builder::max_buf_size`, set explicitly per connection |
| `max_headers` | `RuntimeConfig` | 100 | 1–10000 | `--max-headers` | `max_headers` (`lowlevel`; compat default) | Hyper `http1::Builder::max_headers` (Hyper answers 431 itself) |
| `max_header_bytes` | `RuntimeConfig` | 32768 | 1024–1048576 | `--max-header-bytes` | `max_header_bytes` (`lowlevel`; compat default) | Post-parse aggregate check in `convert_request_head`; 431 pre-service |
| `max_request_target_bytes` | `RuntimeConfig` | 8192 | 128–65536 | `--max-request-target-bytes` | `max_request_target_bytes` (`lowlevel`; compat default) | Post-parse target check in `convert_request_head`; 414 pre-service |

Hyper exposes no aggregate header-byte, request-target, or request-line knob: the request line is bounded jointly by the parser buffer and the target ceiling.

### Timeouts

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `header_read_timeout` | `RuntimeConfig` | 10s | > 0 | `--header-timeout` | `header_timeout_secs` | Hyper header read timeout (also bounds idle keep-alive gaps when shorter than the idle timeout) |
| `connection_total_timeout` | `RuntimeConfig` | 60s | > 0 | `--connection-total-timeout` | `connection_total_timeout_secs` | Hard maximum connection lifetime (driver deadline loop) |
| `handler_timeout` | `RuntimeConfig` | 30s | > 0 | `--handler-timeout` | `handler_timeout_secs` | `tokio::time::timeout` around service call |
| `body_read_timeout` | `RuntimeConfig` | 30s | > 0 | `--body-read-timeout` | `body_timeout_secs` | Total body consumption deadline |
| `keep_alive_idle_timeout` | `RuntimeConfig` | 60s | > 0, independent of total | `--keep-alive-idle-timeout` | `keep_alive_idle_timeout_secs` (`lowlevel`; compat default) | Driver deadline loop; resets on request/transport activity |
| `response_write_timeout` | `RuntimeConfig` | 30s | > 0, independent of total | `--response-write-timeout` | `response_write_timeout_secs` (`lowlevel`; compat default) | Driver + `ProgressIo` no-progress tracking; steady progress never trips |
| `max_requests_per_connection` | `RuntimeConfig` | None (unlimited) | None or >= 1 | `--max-requests-per-connection` (`0` = unlimited) | `max_requests_per_connection` (`lowlevel` `None`; compat default) | `Connection: close` on the limit response; every response counts |
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

### Filesystem policy

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `root` | `ServeConfig` | "." | PathBuf | `--directory` | `root` | PinnedRoot at startup |
| `directory_listing` | `StaticPolicy` | Disabled | enum | `--directory-listing` | `directory_listing` (StaticPolicy) | Directory listing response |
| `symlinks` | `StaticPolicy` | Denied | enum | `--follow-symlinks` | `follow_symlinks` (StaticPolicy) | Path traversal resolution |
| `dotfiles` | `StaticPolicy` | Denied | enum | `--allow-dotfiles` | `allow_dotfiles` (StaticPolicy) | Dotfile path component check |
| `static_metadata.emit_etag` / `emit_last_modified` | `StaticPolicy` | true / true | bool | N/A (Rust-only) | N/A | Static `ETag`/`Last-Modified`; `minimal_fingerprint()` suppresses both |
| `stream_chunk_size` | `Limits` / `RuntimeConfig` | 8192 | >= 64, <= 1 MiB | N/A | N/A | File streaming read chunk size |

### TLS (feature-gated)

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `tls_config` | `RuntimeConfig` | None | Option\<Arc\<ServerConfig\>\> | `--tls-cert` + `--tls-key` | N/A | TLS handshake via rustls |

## Naming drift (cross-boundary)

These are intentional API-surface differences, not duplicates:

| Rust field | Python param | CLI flag | Notes |
|---|---|---|---|
| `header_read_timeout` | `header_timeout_secs` | `--header-timeout` | Python/CLI drop "read" |
| `body_read_timeout` | `body_timeout_secs` | `--body-read-timeout` | Python drops "read" |
