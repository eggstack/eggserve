# eggserve-bin — Deep Dive

The CLI binary crate. Owns the process lifecycle: argument parsing, startup logging, TCP binding, signal handling, and graceful shutdown. Delegates accept loop, connection management, and TLS to `eggserve-core::server`. Uses a current-thread Tokio runtime.

## Module Map

| Module | Purpose |
|--------|---------|
| `main.rs` | Thin `fn main()` → `eggserve_bin::run()` |
| `lib.rs` | `run()` executable entrypoint and integration-only `run_cli(argv) -> i32`; delegates to core server |
| `args.rs` | Manual argument parsing (no clap dependency) |
| `shutdown.rs` | Signal handling (Ctrl+C, SIGTERM, SIGHUP) with broadcast channel |
| `tls.rs` | Re-exports `eggserve_core::tls` (feature-gated: `tls`); loading lives in core |

## Entry Points

```rust
// lib.rs
pub fn run()  // calls run_cli with std::env::args, then std::process::exit
pub fn run_cli(argv: Vec<String>) -> i32  // Python integration entrypoint
```

`run_cli()` parses the same arguments as the executable, constructs
`ServeConfig`, starts the server, and returns an exit code without calling
`std::process::exit()`. This narrowly supports the Python extension's
extension-backed CLI without terminating the host process. It is not a
general Rust embedding API; Rust applications should use `eggserve-core`.
`run()` is the executable wrapper that calls `run_cli` and exits with the
returned code.

The binary crate calls `run()` from `main.rs`. The Python package calls
`run_cli()` via the native `_run_cli` PyO3 binding, and `ServerProcess`
launches `python -m eggserve` as a subprocess.

1. Parses CLI arguments
2. Constructs `ServeConfig`
3. Prints startup summary (bind address, root, policy)
4. Binds TCP listener
5. Enters accept loop
6. Handles shutdown signal → graceful drain

## Accept Loop Architecture

The accept loop lives entirely in `eggserve-core::server::accept_loop_generic()`.
Both TLS and non-TLS paths use `Server::builder()` → `Server::start()`.
When `RuntimeConfig.tls_config` is set, the accept loop performs a per-connection
TLS handshake via `tokio_rustls::TlsAcceptor` before dispatching to the HTTP
connection handler.

### Unified accept loop (core server)

```
┌─────────────────────────────────────────────┐
│ accept_loop_generic()                       │
│  • TCP accept with connection semaphore     │
│  • Lifecycle state machine                  │
│  • Spawn Tokio task per connection          │
│  • (TLS): per-connection TLS handshake      │
│  • On shutdown signal: drain and stop       │
└─────────────────┬───────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────┐
│ per-connection handler                      │
│  • Read headers with header_read_timeout    │
│  • Call service with StaticService           │
│  • Write response with connection_total_timeout│
│  • Drop semaphore permit on completion      │
└─────────────────────────────────────────────┘
```

When the semaphore is exhausted, new connections are dropped immediately (connection limit enforcement).

## CLI Arguments (`args.rs`)

Manual parsing — no clap. Arguments:

| Flag | Default | Description |
|------|---------|-------------|
| `--directory` | `.` | Root directory to serve |
| `--bind` | `127.0.0.1` | Bind host, hostname, or host:port |
| `--port` | `8000` | Port number |
| `--addr` | — | Full socket address (HOST:PORT); cannot combine with `--bind` |
| `--public` | off | Bind to `0.0.0.0` or `::` (requires explicit opt-in) |
| `--directory-listing` | off | Enable directory listing |
| `--follow-symlinks` | off | Follow symbolic links |
| `--allow-dotfiles` | off | Serve dotfiles |
| `--log-format` | `text` | Log format (`text`, `json`, or `none`) |
| `--quiet` | off | Wrap log sink with warn/error filter |
| `--max-connections` | `64` | Connection limit |
| `--max-file-streams` | `32` | File stream limit |
| `--header-timeout` | `10s` | Header read timeout |
| `--connection-total-timeout` | `60s` | Total connection lifetime timeout |
| `--handler-timeout` | `30s` | Handler invocation timeout |
| `--body-read-timeout` | `30s` | Request body read timeout |
| `--tls-cert` | — | TLS certificate PEM path (feature-gated: `tls`) |
| `--tls-key` | — | TLS private key PEM path (feature-gated: `tls`) |
| `--content-type` | `application/octet-stream` | Fallback MIME type for unknown extensions |
| `-H` / `--header` | — | Repeatable safe header for final 200 static responses |

Positional parsing has two logical slots: `PORT` and `DIRECTORY` (in that
order). An explicit port in `--bind`, `--addr`, or `--port` occupies PORT;
host-only `--bind` leaves PORT available. The next positional token after an
occupied PORT is DIRECTORY verbatim, even when it is numeric. `--directory`
occupies DIRECTORY and leaves a positional numeric token available for PORT.
Once both slots are occupied, additional positionals are rejected. A single
valid numeric positional remains PORT for compatibility. `--bind` and `--addr`
may not be combined. Hostnames are resolved once before the native listener
starts. Static metadata headers are ordered and validated against runtime-owned
and hop-by-hop fields. With TLS enabled, omitting `--tls-key` makes
`--tls-cert` serve as both PEM paths for a combined file.

## Signal Handling (`shutdown.rs`)

Uses `tokio::sync::broadcast` channel. On Ctrl+C (all platforms), SIGTERM (Unix),
or SIGHUP (Unix):

1. Signal handler sends shutdown message
2. Accept loop receives message → breaks
3. In-flight connections get `graceful_shutdown_timeout` to complete
4. Server exits

SIGHUP is treated as a graceful stop rather than its default immediate-terminate
action, matching daemon-management expectations. Only the first Ctrl+C, SIGTERM,
or SIGHUP is acted on. Additional signals received during graceful shutdown are
consumed but do not escalate; use the platform's normal external termination
mechanism if a stuck process must be stopped.

## TLS Support

Behind the `tls` feature flag. Uses `rustls` + `tokio-rustls`.

`bin/src/tls.rs` is a one-line re-export (`pub use eggserve_core::tls::*`).
All loading logic lives in `eggserve-core::tls::load_tls_config()`:

- Loads PEM certificate chain and private key
- Supports PKCS#1, PKCS#8, and SEC1 key formats
- Validates exactly one private key is present
- Handshake timeout enforced per connection

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `eggserve-core` | Request handling, config, policy, HTTP serving, TLS loading |
| `tokio` | Async runtime |

`rustls`, `tokio-rustls`, and `rustls-pemfile` are transitive through
`eggserve-core` (optional, behind `tls` feature). `bin/Cargo.toml` lists
them only as dev-dependencies for integration tests.

## See Also

- [eggserve-core.md](eggserve-core.md) — Core library (request handling)
- [architecture/overview.md](overview.md) — Data flow diagram
