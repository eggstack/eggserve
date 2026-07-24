# Configuration Inventory and Ownership Model

Plan 080, Tracks A and B. Single source of truth for every operator-facing
configuration field, its owner, enforcement path, and cross-frontend mapping.

## Ownership split

**Runtime-owned** (transport, concurrency, timeouts):

- `RuntimeConfig` fields — connection limits, timeouts, keep-alive, body policy
- `Limits` fields — validated subset fed into `RuntimeConfig`
- CLI flags (`--max-connections`, `--handler-timeout`, etc.)
- Python `Server()` constructor params (`max_connections`, `handler_timeout_secs`, etc.)

**Static-service-owned** (filesystem, policy):

- `ServeConfig` fields — root directory, bind address, static policy
- `StaticPolicy` fields — symlink, dotfile, directory listing policies
- `Limits::max_file_streams` — feeds into `ServeState` file-stream semaphore
- `Limits::max_listing_entries`, `max_listing_response_bytes`, `max_listing_filename_bytes`
- `Limits::listing_enumeration_timeout`, `stream_chunk_size`

A setting may be shared by reference, but only one validated value owns enforcement.

## Field inventory

### Concurrency limits

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_connections` | `RuntimeConfig` | 64 | > 0 | `--max-connections` | `max_connections` | Connection semaphore in accept loop |
| `max_file_streams` | `Limits` → `ServeState` | 32 | > 0 | `--max-file-streams` | `max_file_streams` | File-stream semaphore in `ServeState` |
| `max_python_callbacks` | `PyServer` | 8 | > 0 | N/A | `max_python_callbacks` | Callback semaphore in `PythonCallbackService` |
| `max_in_flight_requests` | `RuntimeConfig` | None | Option\<usize\> | N/A | N/A | HTTP/1.1 pipelining limit (hyper) |
| `max_listing_entries` | `Limits` | 4096 | > 0 | N/A | N/A | Directory listing enumeration |
| `max_listing_response_bytes` | `Limits` | 1 MiB | > 0 | N/A | N/A | Directory listing response body cap |
| `max_listing_filename_bytes` | `Limits` | 255 | > 0 | N/A | N/A | Single filename cap in listing |

### Timeouts

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `header_read_timeout` | `RuntimeConfig` | 10s | > 0 | `--header-timeout` | `header_timeout_secs` | Hyper header read timeout |
| `connection_total_timeout` | `RuntimeConfig` | 60s | > 0 | `--connection-total-timeout` | `connection_total_timeout_secs` | Hyper connection future timeout |
| `handler_timeout` | `RuntimeConfig` | 30s | > 0 | `--handler-timeout` | `handler_timeout_secs` | `tokio::time::timeout` around service call |
| `body_read_timeout` | `RuntimeConfig` | 30s | > 0 | `--body-read-timeout` | `body_timeout_secs` | Total body consumption deadline |
| `graceful_shutdown_timeout` | `RuntimeConfig` | 10s | > 0 | N/A | `graceful_shutdown_timeout_secs` | Drain deadline after SIGTERM |
| `listing_enumeration_timeout` | `Limits` | 30s | > 0 | N/A | N/A | Directory enumeration timeout |

### Body policy

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `max_request_body_bytes` | `RuntimeConfig` | 0 | u64 | N/A | `max_request_body_bytes` | Hard ceiling, no service can exceed |
| `request_body_policy` | `RuntimeConfig` | Reject | enum | N/A | `request_body_mode` | Service-level declaration, runtime ceiling |
| `incomplete_body_policy` | `RuntimeConfig` | Close | enum | N/A | `incomplete_body_policy` | Post-handler body drain behavior |

### Network / binding

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `bind` | `ServeConfig` / `RuntimeConfig` | 127.0.0.1:8000 | SocketAddr | `--bind`, `--port`, `--addr` | `bind`, `port` | TCP listener bind |
| `keep_alive` | `RuntimeConfig` | true | bool | N/A | N/A | Hyper keep-alive |
| `server_header` | `RuntimeConfig` | None | Option\<String\> | N/A | N/A | Server header on responses |

### Filesystem policy

| Canonical name | Owner | Default | Valid range | CLI flag | Python param | Enforcing path |
|---|---|---|---|---|---|---|
| `root` | `ServeConfig` | "." | PathBuf | `--directory` | `root` | PinnedRoot at startup |
| `directory_listing` | `StaticPolicy` | Disabled | enum | `--directory-listing` | `directory_listing` (StaticPolicy) | Directory listing response |
| `symlinks` | `StaticPolicy` | Denied | enum | `--follow-symlinks` | `follow_symlinks` (StaticPolicy) | Path traversal resolution |
| `dotfiles` | `StaticPolicy` | Denied | enum | `--allow-dotfiles` | `allow_dotfiles` (StaticPolicy) | Dotfile path component check |
| `stream_chunk_size` | `Limits` | 8192 | > 0 | N/A | N/A | File streaming read chunk |

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
| `request_body_policy` | `request_body_mode` | N/A | Python takes string, Rust takes enum |
