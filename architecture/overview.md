# Architecture Overview

Plan 108 is the active corrective follow-up to Plan 107. The runtime
ownership contract is: one
`RuntimeState` per running server, one transport file-stream semaphore, and
canonical static file/range bodies preserved until Hyper conversion. Static
state owns confinement only; custom Rust/Python services are rootless.

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction. It competes with `python -m http.server` for local development use cases — not with nginx, Caddy, or Uvicorn.

## What eggserve Is

- **A hardened static file server** — serves files from a directory with security guarantees
- **A CLI tool** — `eggserve` binary with `--directory`, `--bind`, `--port`, TLS, and policy flags
- **A Python package** — `eggserve` wheel with `python -m eggserve` and `http.server`-compatible API
- **A reusable Rust library** — `eggserve-core` for embedding path confinement and policy enforcement

## What eggserve Is Not

- Not an ASGI/WSGI server, CGI executor, or web framework
- Not a reverse proxy, ACME client, or plugin host
- Not a file upload handler, auth system, or template engine

## Core Invariants

1. **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag.
2. **No serving outside the configured root.** Path traversal and symlink escape are denied at the library level. On Unix with safe defaults, symlink denial is *descriptor-relative* — `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`.
3. **No broad dependencies.** Every dependency has an explicit purpose. No framework dependencies beyond Hyper.
4. **Plan-driven development.** Every change traces to a plan in `plans/`. No ad-hoc feature additions.

---

## Workspace Layout

```
eggserve/
├── Cargo.toml                  # workspace root (resolver = "2", edition 2021)
├── crates/
│   ├── eggserve-core/          # library: security primitives, HTTP serving, response construction
│   ├── eggserve-bin/           # binary: CLI, accept loop, signal handling
│   └── eggserve-python/        # Python wheel (maturin + PyO3, excluded from workspace)
├── architecture/               # this directory — deep-dive docs per subsystem
├── docs/                       # reference docs (32 files)
├── plans/                      # design plans (000–101, all complete)
├── conformance/                # shared Rust/Python conformance corpora
├── fuzz/                       # fuzzing targets and seed corpora (12 targets)
├── benchmarks/                 # benchmark baselines (Plan 088)
├── tests/                      # repo-level integration tests (proxy interop, soak, qual)
├── scripts/                    # verify.sh, test-python-wheel.sh, install-cargo-tools.sh
├── release/                    # release artifacts and closure reports
└── examples/                   # Python usage examples
```

---

## Crate Architecture

Three crates, strict dependency hierarchy:

```
eggserve-core          ← eggserve-bin (path dep, workspace member)
eggserve-core          ← eggserve-python (path dep, excluded from workspace)
eggserve-bin           → standalone, owns process lifecycle
eggserve-python        → standalone, owns Python packaging
```

- **`eggserve-core`** has no workspace dependencies. All security-critical logic lives here.
- **`eggserve-bin`** depends on `eggserve-core` via path. Owns CLI parsing, signal handling, accept loop.
- **`eggserve-python`** depends on `eggserve-core` via path. Excluded from workspace; has its own `Cargo.lock`. Built via maturin. Bundles the platform-native CLI binary.

The Python subprocess layer communicates with the binary via CLI arguments — no shared memory, no FFI to the bin crate.

### Feature Flags

| Feature | Crate | Purpose |
|---------|-------|---------|
| `tls` | `eggserve-core`, `eggserve-bin` | Server TLS via rustls/tokio-rustls |
| `client` | `eggserve-core` | Hyper client support |
| `client-tls` | `eggserve-core` | Client TLS via rustls/webpki-roots |
| `python-bindings-internal` | `eggserve-core` | Internal flag for Python binding constructors |
| `windows-plan086` | `eggserve-core` | Windows adversarial qualification |

---

## Component Index

Each component links to a deep-dive document in this directory. Use this as your starting point for understanding any subsystem.

### Core Crates

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Core library | `eggserve-core` | [eggserve-core.md](eggserve-core.md) | The heart of the project — all security-critical logic, path confinement, policy enforcement, HTTP serving, response construction |
| CLI binary | `eggserve-bin` | [eggserve-bin.md](eggserve-bin.md) | Process entry point — CLI argument parsing, signal handling, current-thread tokio runtime, graceful shutdown |
| Python bindings | `eggserve-python` | [eggserve-python.md](eggserve-python.md) | PyO3 bindings — `eggserve.server` facade, `SimpleHTTPRequestHandler`, `RequestBody`, structured logging bridge |

### Security Subsystems

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Path confinement | `eggserve-core::path` | [path-confinement.md](path-confinement.md) | 6-stage path validation pipeline — parse, decode, normalize, validate, platform checks. 16 rejection variants |
| Filesystem confinement | `eggserve-core::fs` | [filesystem-confinement.md](filesystem-confinement.md) | `PinnedRoot`, `RootGuard`, descriptor-relative traversal (Unix), handle-relative (Windows). Prevents symlink escape and TOCTOU |
| Policy system | `eggserve-core::policy` | [policy-system.md](policy-system.md) | `StaticPolicy`, `SymlinkPolicy`, `DotfilePolicy`, `DirectoryListingPolicy`. Safe defaults enforced |
| Security model | cross-cutting | [security-model.md](security-model.md) | Central invariant, 7 defensive layers, attacker model, trust boundaries |

### HTTP Subsystems

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Public API boundary | `eggserve-core::primitives` | [primitives-api.md](primitives-api.md) | Canonical types for embedding — `SecureRoot`, `ResolvedResource`, HTTP validation, request/response types |
| Response planning | `eggserve-core::primitives::planner` | [response-planning.md](response-planning.md) | Conditional requests (ETag, If-Modified-Since), range requests, HEAD parity, `normalize_response()` |
| Runtime service boundary | `eggserve-core::server` | [runtime.md](runtime.md) | `Server`, `ServerBuilder`, `Service` trait, `StaticService`, lifecycle state machine, connection pipeline |
| HTTP client primitives | `eggserve-core::primitives::client` | [client.md](client.md) | Low-level HTTP client for Python (`http.client`-like), TLS support, timeout semantics |

### Operational Subsystems

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Structured logging | `eggserve-core::ops` | [structured-logging.md](structured-logging.md) | Event-based logging (schema v1), JSON Lines output, operational counters, sanitized fields |
| Configuration model | cross-cutting | [configuration.md](configuration.md) | `RuntimeConfig`, `ServeConfig`, `Limits` — field inventory, ownership model, CLI/Python/Rust convergence |
| Error taxonomy | cross-cutting | [error-taxonomy.md](error-taxonomy.md) | 7 error layers — `PathRejection`, `Error`, `RequestValidationError`, `ServerError`, `ServiceError`, `RequestBodyError`, `ClientError` |
| TLS support | `eggserve-core::tls` | [tls.md](tls.md) | rustls-based TLS — PEM loading, PKCS#1/8/SEC1 key formats, feature-gated |

### Testing and Quality

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Testing and conformance | `tests/`, `conformance/`, `fuzz/` | [testing-and-conformance.md](testing-and-conformance.md) | Multi-layer test strategy — Rust unit/integration, Python suites, 21 fuzz targets, conformance corpora |

### Decision Records

| ADR | Topic | Status |
|-----|-------|--------|
| [adr-002](adr-002-windows-handle-relative-filesystem.md) | Windows handle-relative filesystem confinement | Accepted (Plans 084–086) |
| [adr-003](adr-003-custom-service-ownership.md) | Custom-service ownership model | Accepted (Plan 078) |

---

## How It All Works Together

### Request Lifecycle

A request travels through these stages:

```
HTTP Request
    │
    ▼
┌─────────────────────────────────────────────────────┐
│ eggserve-bin: process entry point                   │
│  • CLI argument parsing (args.rs, no clap)          │
│  • Optional TLS cert loading (tls.rs)               │
│  • Tokio runtime creation                           │
│  • Signal handler registration (shutdown.rs)        │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ eggserve-core::server: accept loop + lifecycle      │
│  • TCP accept with connection semaphore (64 max)    │
│  • Optional TLS handshake (feature-gated)           │
│  • HTTP/1 connection via Hyper                      │
│  • Lifecycle: Created → Starting → Running          │
│  • Canonical RequestHead extraction                 │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Connection pipeline (server/connection.rs)          │
│  • TE+CL framing validation (smuggling prevention)  │
│  • Body policy selection (Reject/Buffer/Stream)     │
│  • Body ingestion (timeout, limit, accounting)      │
│  • Handler timeout enforcement                      │
│  • Request → canonical Request envelope             │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Service::call(Request)                              │
│  e.g. StaticService or Python callback handler     │
│                                                     │
│  StaticService pipeline:                            │
│  1. Validate method (GET/HEAD only)                 │
│  2. Parse target → ConfinedPath (path confinement)  │
│  3. Resolve via SecureRoot → ResolvedResource       │
│  4. Plan response (conditional, range, ETag)        │
│  5. Stream file / list directory / error            │
│                                                     │
│  Python callback pipeline:                          │
│  1. spawn_blocking → GIL acquire                    │
│  2. Call Python handler with PyRequest              │
│  3. Convert PyResponse → canonical Response         │
│  4. Validate handler response (hop-by-hop, status)  │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Response pipeline                                   │
│  1. Canonical response normalization                │
│     (HEAD suppression, body-forbidden enforcement,  │
│      hop-by-hop stripping, content-length)          │
│  2. Transport-body conversion (to_hyper_response)   │
│  3. Write timeout enforcement                       │
│  4. Permit release + connection termination         │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
         HTTP Response
```

### Security Layers

Defense in depth across seven layers:

| Layer | What it defends against | Deep Dive |
|-------|------------------------|-----------|
| Path confinement | Traversal, encoding abuse, NUL bytes | [path-confinement.md](path-confinement.md) |
| Policy enforcement | Symlinks, dotfiles, directory listing | [policy-system.md](policy-system.md) |
| Filesystem confinement | Symlink escape, root traversal, TOCTOU | [filesystem-confinement.md](filesystem-confinement.md) |
| Input validation | Double-encoding, method abuse, body framing | [security-model.md](security-model.md) |
| Resource limits | Slowloris, exhaustion, file stream contention | [configuration.md](configuration.md) |
| Response normalization | Hop-by-hop smuggling, content-length manipulation | [response-planning.md](response-planning.md) |
| Sanitized logging | Log injection, path/header leakage | [structured-logging.md](structured-logging.md) |

Safe defaults are not advisory — the code rejects non-conforming requests before any filesystem access.

### Configuration Flow

Configuration is split between runtime-owned (transport) and static-service-owned (filesystem) concerns:

```
CLI flags / Python params / Rust structs
         │
         ▼
┌─────────────────────────────────────────┐
│ Limits (validated subset)               │
│  • 14 fields: connections, streams,     │
│    timeouts, body sizes, chunk size     │
└────────┬───────────────┬────────────────┘
         │               │
         ▼               ▼
┌────────────────┐  ┌────────────────────┐
│ RuntimeConfig  │  │ ServeConfig        │
│ (transport)    │  │ (filesystem)       │
│ • bind addr    │  │ • root directory   │
│ • timeouts     │  │ • static policy    │
│ • TLS          │  │ • file streams     │
│ • keep-alive   │  │ • bind address     │
└────────────────┘  └────────────────────┘
```

CLI flags, Python constructor params, and Rust struct fields all converge on the same underlying configuration. See [configuration.md](configuration.md) for the full field inventory and ownership model.

---

## Core Library Module Map (`eggserve-core`)

| Module | Visibility | Purpose | Stability |
|--------|-----------|---------|-----------|
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` | Stable-ish |
| `limits.rs` | **pub** | `Limits` — connections, streams, timeouts | Stable-ish |
| `policy.rs` | **pub** | `StaticPolicy`, `SymlinkPolicy`, `DotfilePolicy`, `DirectoryListingPolicy` | Stable-ish |
| `service.rs` | **pub** | `handle_request()` — the HTTP handler | Experimental |
| `error.rs` | pub(crate) | `Error` enum taxonomy | Internal |
| `path/` | pub(crate) | Path confinement pipeline (7 submodules) | Internal |
| `fs/` | pub(crate) | Filesystem confinement, descriptor-relative traversal on Unix | Internal |
| `response.rs` | pub(crate) | Response helpers (file streaming, directory listing, error responses) | Internal |
| `mime.rs` | pub(crate) | MIME type detection via `phf` map (~60 extensions) | Internal |
| `ops/` | **pub** | Structured logging, operational events, counters | Stable-ish |
| `primitives/` | **pub** | Public facade — all canonical types for embedding consumers | Stable |
| `server/` | **pub** | Runtime service boundary: `Server`, `Service` trait, `StaticService`, lifecycle | Experimental |
| `tls.rs` | **pub** | TLS config loading (feature-gated: `tls`) | Experimental |

### `path/` submodules

| File | Purpose |
|------|---------|
| `mod.rs` | `ConfinedPath` type — the validated path |
| `request_target.rs` | HTTP origin-form parsing |
| `decode.rs` | Single-pass percent decoding |
| `components.rs` | Normalization, splitting, validation |
| `rejected.rs` | `PathRejection` enum (16 variants) |
| `policy.rs` | `PathPolicy`, `DotfilePolicy` (path-level) |
| `platform.rs` | Windows-specific checks |

### `fs/` submodules

| File | Purpose |
|------|---------|
| `mod.rs` | `PinnedRoot`, `RootGuard`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory` |
| `unix.rs` | Descriptor-relative traversal (statat + openat) |
| `windows.rs` | Handle-relative traversal (NtOpenFile, NtQueryDirectoryFile) |

### `primitives/` submodules

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports all public types |
| `secure_root.rs` | `SecureRoot`, `ResolvedFile`, `ResolvedDirectory`, `ResolvedResource` |
| `http.rs` | `ReadOnlyMethod`, request validation functions (legacy) |
| `method.rs` | `Method`: validated HTTP method (standard + extension) |
| `version.rs` | `HttpVersion`: HTTP/1.0, HTTP/1.1 |
| `header_block.rs` | `HeaderBlock`: duplicate-preserving ordered headers |
| `request_target.rs` | `RequestTarget`: validated origin-form target |
| `request_head.rs` | `RequestHead`: canonical request head with Hyper conversion |
| `connection_info.rs` | `ConnectionInfo`: transport metadata |
| `request.rs` | `Request` envelope (head + body + connection info) |
| `request_body.rs` | `RequestBody` — one-shot transport-independent body |
| `request_body_error.rs` | `RequestBodyError` — 12-variant body error taxonomy |
| `request_body_policy.rs` | `RequestBodyPolicy` — Reject/Buffer/Stream |
| `incomplete_body_policy.rs` | `IncompleteBodyPolicy` — Close/Discard |
| `body.rs` | `BodySource`, `BodyKind`, `BodySourceError` — safe body streaming |
| `planner.rs` | Response planning (conditional, range, ETag) |
| `response.rs` | Planning types (`StaticResponsePlan`, `BodyPlan`, etc.) |
| `canonical.rs` | `StatusCode`, `Response`, `normalize_response()`, `normalize_metadata()` |
| `client/` | HTTP client primitives (feature-gated: `client`) |

### `server/` submodules

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports, `Server`, `ServerBuilder` |
| `config.rs` | `RuntimeConfig` — transport-level configuration |
| `connection.rs` | Body ingestion pipeline, Hyper incoming-body adapter |
| `errors.rs` | `ServerError`, `ServiceError`, `ShutdownResult` |
| `handle.rs` | `ServerHandle` — control handle for lifecycle management |
| `lifecycle.rs` | `LifecycleState` — lifecycle state machine |
| `service.rs` | `Service` trait, `service_fn` adapter |
| `static_service.rs` | `StaticService` — hardened static file service |

---

## Error Taxonomy

eggserve uses seven distinct error layers:

| Error Type | Scope | Variants |
|-----------|-------|----------|
| `PathRejection` | Path parsing | 16 variants: `Empty`, `TooLong`, `MalformedPercentEncoding`, `ParentComponent`, `DotfileDenied`, `SymlinkDenied`, `RootEscapeDenied`, ... |
| `Error` | Top-level crate | `PathEscape`, `PathNotAccessible`, `Config`, `Bind`, `Runtime`, `RequestRejected`, `Io`, `Client` |
| `RequestValidationError` | HTTP-level | `MethodNotAllowed`, `InvalidContentLength`, `BodyTooLarge`, `UnsupportedTransferEncoding` |
| `ServerError` | Server lifecycle | `Bind`, `Config`, `AlreadyStarted`, `Accept`, `TlsSetup`, `ShutdownTimeout`, `Startup`, `Terminal` |
| `ServiceError` | Per-request | `Internal`, `Rejected(u16)`, `Panic`, `Timeout` |
| `RequestBodyError` | Body consumption | 12 variants: `RejectedByPolicy`, `LimitExceeded`, `ReadTimeout`, `PrematureEof`, `AlreadyConsumed`, ... |
| `ClientError` | HTTP client | 12 variants: `InvalidUrl`, `UnsupportedScheme`, `Timeout`, `TlsError`, `ResponseBodyTooLarge`, ... |

---

## Module Visibility Model

| Tier | Modules | Stability |
|------|---------|-----------|
| **Stable** | `primitives` (facade), all `primitives::*` submodules | Intended public boundary for embedding consumers |
| **Stable-ish** | `config`, `limits`, `policy`, `ops` | Field shapes may evolve before 1.0 |
| **Experimental** | `service` (`handle_request`), `server` (all types), `primitives::client` | API may change without notice |
| **Internal** | `fs`, `path`, `response`, `mime`, `error` | `pub(crate)` — not part of public API |

---

## Platform Support

| Platform | Status | Security Model |
|----------|--------|----------------|
| **Linux** (x86_64, aarch64) | Supported-hardened | Descriptor-relative traversal via `statat`+`openat` |
| **macOS** (x86_64, aarch64) | Supported-hardened | Same descriptor-relative guarantees as Linux |
| **Windows** (x86_64) | Supported-functional | Handle-relative child resolution (Plan 084) + directory enumeration (Plan 085) + adversarial qualification scaffold (Plan 086, 114 tests). Independent safety review awaited. Not for untrusted public content until human gates complete. |

---

## Testing Strategy

Multi-layered testing with ~824 Python tests, ~200+ Rust tests, 21 fuzz targets, and 2 conformance corpora:

| Layer | Location | Scope |
|-------|----------|-------|
| Rust unit tests | `crates/*/src/**/*.rs` (inline `#[cfg(test)]`) | Module-level logic |
| Rust integration tests | `crates/eggserve-core/tests/*.rs` | Cross-module, live TCP, TLS (34 files) |
| Python test suites | `crates/eggserve-python/tests/test_*.py` | Compatibility facade, TLS, low-level primitives, conformance, body, boundary hardening |
| Packaging smoke tests | `crates/eggserve-python/packaging-tests/` | Installed-wheel validation |
| Conformance corpora | `conformance/*.json` | Shared Rust/Python test data |
| Fuzz targets | `fuzz/fuzz_targets/*.rs` | Property-based input fuzzing (12 targets) |
| Repo-level tests | `tests/` | Proxy interop, soak, installed-binary qual |

See [testing-and-conformance.md](testing-and-conformance.md) for the full test matrix.

---

## Release Process

Release is a manual crates.io procedure. CI is a regression screen, not release certification:

1. Run `./scripts/verify.sh full` (Rust + Python wheel)
2. Run `bash scripts/install-cargo-tools.sh` then `cargo audit` + `cargo deny check`
3. Manual crates.io publish from maintainer-controlled environment
4. GitHub Actions never publishes

See [docs/release-process.md](../docs/release-process.md) for the full procedure (Plan 091).

---

## Plan History

Plans 000–101 are all implementation-complete. Major feature tracks:

| Plans | Theme | Key Outcomes |
|-------|-------|--------------|
| 000–009 | Foundation & core | Security contract, path confinement, static serving MVP, TLS, Python packaging |
| 010–022 | Python API & primitives | PyO3 bindings, `SecureRoot`, HTTP validation, response planning, invariant tests |
| 023–030 | HTTP primitives production | Correctness primitives, body streaming, client substrate, server primitives |
| 031–046 | Release infrastructure | Evidence framework, CI gates, machine-readable criteria (simplified in 091) |
| 047–059 | Canonical HTTP types & runtime | `Method`, `StatusCode`, `HeaderBlock`, `Response`, runtime/service boundary, request body |
| 060–073 | Production internet & Windows | Windows handle-relative, internet deployment, reverse-proxy qualification, soak testing |
| 074–090 | Corrective passes | Runtime timeouts, Windows unicode, configuration authority, structured logging, performance |
| 091–093 | CI simplification | Reduced to proportionate two-job CI, manual release |
| 094–101 | `http.server` compatibility | RFC 9110 corrections, `SimpleHTTPRequestHandler`, `HTTPSServer`, final verification |
