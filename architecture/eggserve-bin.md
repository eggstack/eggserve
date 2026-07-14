# eggserve-bin — Deep Dive

The CLI binary crate. Owns the process lifecycle: argument parsing, startup logging, TCP binding, accept loop, connection management, signal handling, and graceful shutdown. Contains optional TLS support via rustls.

## Module Map

| Module | Purpose |
|--------|---------|
| `main.rs` | Thin `fn main()` → `eggserve_bin::run()` |
| `lib.rs` | `pub fn run()` entrypoint; accept loop; connection serving with timeouts |
| `args.rs` | Manual argument parsing (no clap dependency) |
| `shutdown.rs` | Signal handling (Ctrl+C, SIGTERM) with broadcast channel |
| `tls.rs` | TLS certificate loading and rustls config (behind `tls` feature) |

## Entry Point

```rust
// lib.rs
pub fn run() -> Result<(), Box<dyn std::error::Error>>
```

`run()` is the single entrypoint. The binary crate calls it from `main.rs`; the Python package calls it from `server.py` via subprocess. It:

1. Parses CLI arguments
2. Constructs `ServeConfig`
3. Prints startup summary (bind address, root, policy)
4. Binds TCP listener
5. Enters accept loop
6. Handles shutdown signal → graceful drain

## Accept Loop Architecture

The non-TLS path delegates to `ServerBuilder`/`ServerHandle` from `eggserve-core::server`, which manages the accept loop, connection semaphore, lifecycle state machine, and graceful shutdown internally. The TLS path retains its own accept loop with per-connection TLS handshake via rustls.

### Non-TLS path (uses core server)

```
┌─────────────────────────────────────────────┐
│ ServerBuilder::start() → ServerHandle       │
│  • TCP accept with connection semaphore     │
│  • Lifecycle state machine                  │
│  • Spawn Tokio task per connection          │
│  • On shutdown signal: drain and stop       │
└─────────────────┬───────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────┐
│ serve_connection()                          │
│  • Read headers with header_read_timeout    │
│  • Call handle_request() from eggserve-core │
│  • Write response with response_write_timeout│
│  • Drop semaphore permit on completion      │
└─────────────────────────────────────────────┘
```

### TLS path (own accept loop)

```
┌─────────────────────────────────────────────┐
│ accept_loop() with TLS                      │
│  • Accept TCP connections                   │
│  • Acquire semaphore permit (connection limit)│
│  • TLS handshake per connection             │
│  • Spawn Tokio task per connection          │
│  • On shutdown signal: break loop           │
└─────────────────┬───────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────┐
│ serve_connection_tls()                      │
│  • Read headers with header_read_timeout    │
│  • Call handle_request() from eggserve-core │
│  • Write response with response_write_timeout│
│  • Drop semaphore permit on completion      │
└─────────────────────────────────────────────┘
```

When the semaphore is exhausted, new connections are dropped immediately (connection limit enforcement).

## CLI Arguments (`args.rs`)

Manual parsing — no clap. Arguments:

| Flag | Default | Description |
|------|---------|-------------|
| `--directory` / `-d` | `.` | Root directory to serve |
| `--bind` / `-b` | `127.0.0.1` | Bind address |
| `--port` / `-p` | `8000` | Port number |
| `--public` | off | Bind to `0.0.0.0` (requires explicit opt-in) |
| `--directory-listing` | off | Enable directory listing |
| `--follow-symlinks` | off | Follow symbolic links |
| `--allow-dotfiles` | off | Serve dotfiles |
| `--log-format` | `text` | Log format (`text` or `json`) |
| `--quiet` | off | Suppress startup output |
| `--max-connections` | `64` | Connection limit |
| `--max-file-streams` | `32` | File stream limit |
| `--header-timeout` | `10s` | Header read timeout |
| `--write-timeout` | `60s` | Response write timeout |
| `--tls-cert` | — | TLS certificate PEM path |
| `--tls-key` | — | TLS private key PEM path |

Positional: `PORT` and `DIRECTORY` (in that order).

## Signal Handling (`shutdown.rs`)

Uses `tokio::sync::broadcast` channel. On Ctrl+C (all platforms) or SIGTERM (Unix):

1. Signal handler sends shutdown message
2. Accept loop receives message → breaks
3. In-flight connections get `graceful_shutdown_timeout` to complete
4. Server exits

## TLS Support (`tls.rs`)

Behind the `tls` feature flag. Uses `rustls` + `tokio-rustls`.

- Loads PEM certificate chain and private key
- Supports PKCS#1, PKCS#8, and SEC1 key formats
- Rejects encrypted keys and multiple private keys
- Handshake timeout enforced per connection

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `eggserve-core` | Request handling, config, policy |
| `tokio` | Async runtime |
| `hyper` | HTTP/1.1 server |
| `hyper-util` | Tokio integration |
| `http-body-util` | Body types |
| `bytes` | Buffer types |
| `rustls` (optional) | TLS implementation |
| `tokio-rustls` (optional) | Async TLS |
| `rustls-pemfile` (optional) | PEM parsing |

## See Also

- [eggserve-core.md](eggserve-core.md) — Core library (request handling)
- [architecture/overview.md](overview.md) — Data flow diagram
