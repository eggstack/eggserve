# Plan 001: Rust core skeleton and HTTP substrate

## Goal

Create the initial Rust workspace and Hyper-backed HTTP substrate for eggserve. This milestone should produce a minimal server that accepts connections, applies typed configuration, routes only the intended method surface, and returns deterministic placeholder responses. It should not yet claim complete static-file serving or security hardening; those belong to later path and filesystem milestones.

The main purpose is architectural: establish the crate boundaries, runtime model, request lifecycle, error taxonomy, and configuration types that later security-critical modules can plug into.

## Scope

In scope:

```text
Cargo workspace
core crate with typed config/policy/limits/error modules
binary crate with CLI entry point and server startup
Hyper/Tokio HTTP/1.1 accept loop
GET/HEAD placeholder handling
405 for unsupported methods
basic graceful shutdown hook
initial connection limit scaffolding
initial logging scaffolding
unit tests for config defaults and method policy
```

Out of scope:

```text
real filesystem path resolution
static file reads
index files
directory listings
symlink policy implementation
TLS
HTTP/2
Range requests
Python packaging
public Python API
full CLI parity
```

## Workspace layout

Create or refine:

```text
Cargo.toml
crates/
  eggserve-core/
    Cargo.toml
    src/
      lib.rs
      config.rs
      limits.rs
      policy.rs
      error.rs
      service.rs
      response.rs
      telemetry.rs
  eggserve-bin/
    Cargo.toml
    src/
      main.rs
      args.rs
      shutdown.rs
```

`eggserve-core` should contain all reusable serving logic. `eggserve-bin` should contain process concerns only: argument parsing, startup logging, signal handling, and invoking the core server.

## Initial dependencies

Keep dependencies minimal:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "signal", "time"] }
hyper = { version = "1", features = ["http1", "server"] }
hyper-util = { version = "0.1", features = ["tokio", "server", "http1"] }
http-body-util = "0.1"
bytes = "1"
```

Add `tracing` only if the implementation immediately uses it. Do not add `reqwest`, Axum, Tower, Clap, MIME crates, TLS crates, compression crates, or directory-rendering helpers in this milestone.

For CLI parsing, either use manual parsing or a very small parser. If `clap` is chosen, document why the UX benefit outweighs dependency size. The preferred initial approach is a tiny hand-written parser supporting only bind/port/directory placeholders.

## Core types

Add stable, boring config types. They do not need all fields implemented yet, but defaults should express the intended security posture.

Example target shape:

```rust
pub struct ServeConfig {
    pub bind: std::net::SocketAddr,
    pub root: std::path::PathBuf,
    pub limits: Limits,
    pub static_policy: StaticPolicy,
    pub log_policy: LogPolicy,
}

pub struct Limits {
    pub max_connections: usize,
    pub max_header_bytes: usize,
    pub max_request_target_bytes: usize,
    pub read_timeout: std::time::Duration,
    pub write_timeout: std::time::Duration,
    pub idle_timeout: std::time::Duration,
}

pub struct StaticPolicy {
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
}
```

Default values should be safe even if not all are enforced yet:

```text
bind: 127.0.0.1:8000
max_connections: bounded, not unlimited
max_header_bytes: small default such as 32 KiB
max_request_target_bytes: small default such as 8 KiB
directory listing: disabled
symlinks: denied
dotfiles: denied
```

If a limit is not yet enforced, add an issue marker or TODO in the implementation and tests for the later milestone. Do not silently expose fields that are ignored forever.

## Server lifecycle

Implement a simple lifecycle:

```text
parse config
bind TCP listener
print effective startup policy
accept loop
apply connection permit
serve HTTP/1.1 connection with hyper
release permit on connection end
shutdown on signal
```

The accept loop should live in core if it is reusable, or in bin if it is process-specific. Prefer `eggserve-core` owning the reusable server and `eggserve-bin` calling `Server::serve(config, shutdown_signal)`.

The service should initially respond:

```text
GET /      -> 200 placeholder body
HEAD /     -> 200 same headers without body
POST /     -> 405 Method Not Allowed
PUT /      -> 405 Method Not Allowed
DELETE /   -> 405 Method Not Allowed
malformed target handling can remain basic until path milestone
```

Add `Allow: GET, HEAD` to 405 responses.

## Error taxonomy

Create an error enum that separates operator/config errors from request errors and internal failures.

Example categories:

```text
ConfigError
BindError
RuntimeError
RequestRejected
PathRejected, placeholder for next milestone
IoError
```

Do not expose raw internal errors directly in HTTP response bodies. HTTP responses should map to stable status codes and short generic messages.

## Response helpers

Add small response helpers in core:

```rust
fn text_response(status: StatusCode, body: &'static str) -> Response<BoxBody<Bytes, Infallible>>
fn empty_response(status: StatusCode) -> Response<BoxBody<Bytes, Infallible>>
fn method_not_allowed() -> Response<...>
```

HEAD handling should be explicit. Avoid accidentally sending a body for HEAD.

## Startup policy display

The binary should print an effective policy summary, even if some policies are placeholders:

```text
Serving root: /current/working/directory
Listening: http://127.0.0.1:8000
Methods: GET, HEAD
Directory listing: disabled
Symlinks: denied
Dotfiles: denied
```

This establishes the user experience early and makes unsafe changes visible in later milestones.

## Tests

Add tests for:

```text
ServeConfig::default binds loopback
StaticPolicy::safe_default disables directory listing
StaticPolicy::safe_default denies symlinks
StaticPolicy::safe_default denies dotfiles
GET placeholder returns 200
HEAD placeholder returns 200 without body
POST returns 405 with Allow: GET, HEAD
```

If full integration tests are awkward before the server shape settles, unit-test the service function directly using Hyper request/response types.

## Acceptance criteria

This milestone is complete when:

```text
cargo fmt --all -- --check passes
cargo test --workspace passes
cargo check --workspace passes
A minimal eggserve binary starts and serves placeholder GET/HEAD responses
Unsupported methods receive deterministic 405 responses
The default config communicates safe default posture
No broad framework/client dependencies were added
The next path-policy milestone has clear integration points
```

## Review checklist

Before merging, verify:

```text
No reqwest dependency
No Axum/Tower dependency unless explicitly justified
No real filesystem serving without path policy
No public bind default
No dynamic request handler abstraction
No Python API added prematurely
No app-server wording introduced in README/docs
```

## Handoff notes

The next plan should replace the placeholder request-target handling with the path confinement engine. Keep this milestone intentionally incomplete. It should establish the skeleton, not rush into serving files with an unsafe path translation shortcut.
