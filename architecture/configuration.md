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
| `max_connections` | `RuntimeConfig` | 64 | > 0 | `--max-connections` | `max_connections` | Connection semaphore in accept loop |
| `max_file_streams` | `RuntimeConfig` | 32 | > 0 | `--max-file-streams` | `max_file_streams` | One file-stream semaphore per running server |
| `max_python_callbacks` | `PyServer` | 8 | > 0 | N/A | `max_python_callbacks` | Callback semaphore in `PythonCallbackService` |
| `max_listing_entries` | `Limits` | 4096 | > 0, <= 10485760 (entries) | N/A | N/A | Directory listing enumeration |
| `max_listing_response_bytes` | `Limits` | 1 MiB | > 0 | N/A | N/A | Directory listing response body cap |

### Timeouts

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `header_read_timeout` | `RuntimeConfig` | 10s | > 0 | `--header-timeout` | `header_timeout_secs` | Hyper header read timeout |
| `connection_total_timeout` | `RuntimeConfig` | 60s | > 0 | `--connection-total-timeout` | `connection_total_timeout_secs` | Maximum connection lifetime (wraps entire Hyper connection future) |
| `handler_timeout` | `RuntimeConfig` | 30s | > 0 | `--handler-timeout` | `handler_timeout_secs` | `tokio::time::timeout` around service call |
| `body_read_timeout` | `RuntimeConfig` | 30s | > 0 | `--body-read-timeout` | `body_timeout_secs` | Total body consumption deadline |
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
| `server_header` | `RuntimeConfig` | None | Option\<String\> | N/A | N/A | Server header on responses |

### Filesystem policy

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `root` | `ServeConfig` | "." | PathBuf | `--directory` | `root` | PinnedRoot at startup |
| `directory_listing` | `StaticPolicy` | Disabled | enum | `--directory-listing` | `directory_listing` (StaticPolicy) | Directory listing response |
| `symlinks` | `StaticPolicy` | Denied | enum | `--follow-symlinks` | `follow_symlinks` (StaticPolicy) | Path traversal resolution |
| `dotfiles` | `StaticPolicy` | Denied | enum | `--allow-dotfiles` | `allow_dotfiles` (StaticPolicy) | Dotfile path component check |
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
