# Architecture Overview

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade. The
CLI is static-only; the Python facade adds bounded synchronous custom handlers;
`eggserve.lowlevel` exposes a handler-only runtime/service substrate for
downstream bounded application servers; the Rust crate exposes a low-level,
embeddable service boundary. EggServe is not an application framework, ASGI/WSGI
runtime, proxy, or general-purpose `socketserver` replacement.

**This document is the entry point for understanding the codebase.** Use the [Deep Dive Index](#deep-dive-index) to jump to any subsystem.

## What eggserve Is

- **A hardened static file server** — serves files from a directory with security guarantees
- **A CLI tool** — `eggserve` binary with `--directory`, `--bind`, `--port`, TLS, and policy flags
- **A Python package** — `eggserve` wheel with `python -m eggserve` and `http.server`-compatible API
- **A reusable Rust library** — `eggserve-core::primitives` is the public
  security/HTTP facade and `eggserve-core::server` is the experimental
  transport-owning runtime and `Service` boundary

## What eggserve Is Not

- Not an ASGI/WSGI server, CGI executor, or web framework
- Not a reverse proxy, ACME client, or plugin host
- Not a file upload handler, auth system, or template engine

The user-facing Python compatibility matrix is maintained in
[`docs/python-http-server-compatibility.md`](../docs/python-http-server-compatibility.md).

## Core Invariants

1. **Safe defaults are not defaults if they can be overridden silently.** Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag.
2. **No serving outside the configured root.** Path traversal and symlink escape denied at library level.
3. **No broad dependencies.** Every dependency has an explicit purpose.
4. **Plan-driven development.** Every change traces to a plan in `plans/`.

---

## Deep Dive Index

Every subsystem has a dedicated deep-dive document. Use this index to navigate directly to what you need.

### Crates

| Document | Covers |
|----------|--------|
| [eggserve-core.md](eggserve-core.md) | Core library — module map, key types, server module, error types, dependencies |
| [eggserve-bin.md](eggserve-bin.md) | CLI binary — `run()` entrypoint, accept loop, argument inventory, signal handling, TLS loading |
| [eggserve-python.md](eggserve-python.md) | Python wheel — `eggserve.server` facade, `eggserve.lowlevel`, `eggserve.subprocess`, security boundary |

### Security

| Document | Covers |
|----------|--------|
| [path-confinement.md](path-confinement.md) | 6-stage path validation pipeline — parsing, decoding, normalization, component validation, 17 rejection variants |
| [filesystem-confinement.md](filesystem-confinement.md) | `PinnedRoot`, `RootGuard`, descriptor-relative traversal (Unix), handle-relative (Windows), TOCTOU prevention |
| [policy-system.md](policy-system.md) | `StaticPolicy` (+`StaticMetadataPolicy`, `ErrorRepresentationPolicy`), `ResponsePolicy`/`DatePolicy`/denylist, safe defaults, CLI/Python mapping |
| [security-model.md](security-model.md) | Central invariant, 7 defensive layers, attacker model, trust boundaries, platform security |

### HTTP and Runtime

| Document | Covers |
|----------|--------|
| [primitives-api.md](primitives-api.md) | Public facade for embedding — `SecureRoot`, `ResolvedResource`, canonical types, HTTP validation, body primitives |
| [response-planning.md](response-planning.md) | Conditional/range/ETag planning, static validator privacy, HEAD parity, `normalize_response()`, streaming buffer |
| [runtime.md](runtime.md) | `Server`, `ServerBuilder`, `Service` trait, `StaticService`, lifecycle state machine, connection pipeline (incl. final privacy boundary), body ingestion |
| [tls.md](tls.md) | rustls-based TLS — PEM loading, PKCS key formats, ALPN, deployment profiles, limitations |

### Operations

| Document | Covers |
|----------|--------|
| [structured-logging.md](structured-logging.md) | Event-based logging (schema v1), JSON Lines/text output, operational counters, sanitized fields, log sink types |
| [configuration.md](configuration.md) | `RuntimeConfig`, `ServeConfig`, `Limits` — full field inventory, ownership model, CLI/Python/Rust convergence |
| [error-taxonomy.md](error-taxonomy.md) | 5 error layers — `PathRejection`, `RequestValidationError`, `ServerError`, `ServiceError`, `RequestBodyError` |

### Quality and Process

| Document | Covers |
|----------|--------|
| [testing-and-conformance.md](testing-and-conformance.md) | Rust unit/integration tests, Python suites, 11 fuzz targets, conformance corpora, packaging smoke tests |

### Decision Records

| Document | Topic | Status |
|----------|-------|--------|
| [adr-002](adr-002-windows-handle-relative-filesystem.md) | Windows handle-relative filesystem confinement | Accepted |
| [adr-003](adr-003-custom-service-ownership.md) | Custom-service ownership model | Accepted |

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
├── docs/                       # reference docs
├── plans/                      # historical design and implementation plans
├── conformance/                # shared Rust/Python conformance corpora
├── fuzz/                       # fuzzing targets and seed corpora (11 targets)
├── benchmarks/                 # benchmark baselines
├── tests/                      # repo-level integration tests (proxy interop, soak, qual)
├── scripts/                    # small verification hierarchy plus package/release checks
├── release/                    # release artifacts and closure reports
└── examples/                   # canonical CLI/Python examples and fixtures
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
- **`eggserve-bin`** depends on `eggserve-core` via path. Owns CLI parsing and signal handling; drives the core server runtime (accept loop, connection management, and TLS live in `eggserve-core::server` / `eggserve-core::tls`).
- **`eggserve-python`** depends on `eggserve-core` and `eggserve-bin` via path. Excluded from workspace; has its own `Cargo.lock`. Built via maturin. Includes an `eggserve` console script backed by the native extension.

### Feature Flags

| Feature | Crate | Purpose |
|---------|-------|---------|
| `tls` | `eggserve-core`, `eggserve-bin`, `eggserve-python` | Server TLS via rustls/tokio-rustls |
| `python-bindings-internal` | `eggserve-core` | Internal flag for Python binding constructors |
| `windows-adversarial-qualification` | `eggserve-core` | Windows adversarial qualification |

---

## Component Map

Each component links to a deep-dive document. Use this as your starting point for understanding any subsystem.

### Core Crates

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Core library | `eggserve-core` | [eggserve-core.md](eggserve-core.md) | All security-critical logic — path confinement, policy enforcement, HTTP serving, response construction |
| CLI binary | `eggserve-bin` | [eggserve-bin.md](eggserve-bin.md) | Process entry point — CLI argument parsing, integration-only `run_cli()`, signal handling, current-thread tokio runtime, graceful shutdown |
| Python bindings | `eggserve-python` | [eggserve-python.md](eggserve-python.md) | PyO3 bindings — `eggserve.server` facade, `SimpleHTTPRequestHandler`, `RequestBody`, structured logging bridge |

### Security Subsystems

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Path confinement | `eggserve-core::path` | [path-confinement.md](path-confinement.md) | 6-stage path validation pipeline — parse, decode, normalize, validate, platform checks. 17 rejection variants |
| Filesystem confinement | `eggserve-core::fs` | [filesystem-confinement.md](filesystem-confinement.md) | `PinnedRoot`, `RootGuard`, descriptor-relative traversal (Unix), handle-relative (Windows). Prevents symlink escape and TOCTOU |
| Policy system | `eggserve-core::policy` | [policy-system.md](policy-system.md) | `StaticPolicy`, `SymlinkPolicy`, `DotfilePolicy`, `DirectoryListingPolicy`. Safe defaults enforced |
| Security model | cross-cutting | [security-model.md](security-model.md) | Central invariant, 7 defensive layers, attacker model, trust boundaries |

### HTTP Subsystems

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Public API boundary | `eggserve-core::primitives` | [primitives-api.md](primitives-api.md) | Canonical types for embedding — `SecureRoot`, `ResolvedResource`, HTTP validation, request/response types |
| Response planning | `eggserve-core::primitives::planner` | [response-planning.md](response-planning.md) | Conditional requests (ETag, If-Modified-Since), range requests, HEAD parity, `normalize_response()` |
| Runtime service boundary | `eggserve-core::server` | [runtime.md](runtime.md) | `Server`, `ServerBuilder`, `Service` trait, `StaticService`, lifecycle state machine, connection pipeline |

### Operational Subsystems

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Structured logging | `eggserve-core::ops` | [structured-logging.md](structured-logging.md) | Event-based logging (schema v1), JSON Lines output, operational counters, sanitized fields |
| Configuration model | cross-cutting | [configuration.md](configuration.md) | `RuntimeConfig`, `ServeConfig`, `Limits` — field inventory, ownership model, CLI/Python/Rust convergence |
| Error taxonomy | cross-cutting | [error-taxonomy.md](error-taxonomy.md) | 5 error layers — `PathRejection`, `RequestValidationError`, `ServerError`, `ServiceError`, `RequestBodyError` |
| TLS support | `eggserve-core::tls` | [tls.md](tls.md) | rustls-based TLS — PEM loading, PKCS#1/8/SEC1 key formats, feature-gated |

### Testing and Quality

| Component | Location | Deep Dive | What It Does |
|-----------|----------|-----------|--------------|
| Testing and conformance | `tests/`, `conformance/`, `fuzz/` | [testing-and-conformance.md](testing-and-conformance.md) | Multi-layer test strategy — Rust unit/integration, Python suites, 11 fuzz targets, conformance corpora |

### Decision Records

| ADR | Topic | Status |
|-----|-------|--------|
| [adr-002](adr-002-windows-handle-relative-filesystem.md) | Windows handle-relative filesystem confinement | Accepted (Plans 084–086) |
| [adr-003](adr-003-custom-service-ownership.md) | Custom-service ownership model | Accepted |

---

## How It All Works Together

### Request Lifecycle

```
HTTP Request
    │
    ▼
┌─────────────────────────────────────────────────────┐
│ eggserve-bin: process entry point                   │
│  • CLI argument parsing (args.rs, no clap)          │
│  • Optional TLS config load via eggserve_core::tls  │
│  • Tokio runtime creation                           │
│  • Signal handler registration (shutdown.rs)        │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ eggserve-core::server: accept loop + lifecycle      │
│  • Shared RuntimeState admission pool               │
│    (connection semaphore, default 64; server-wide   │
│     file-stream semaphore cloned per connection)    │
│  • Optional TLS handshake (feature-gated)           │
│  • HTTP/1 connection via Hyper                      │
│  • Caller-owned stream entry (no socket required)   │
│  • Lifecycle: Created → Starting → Running →        │
│    Draining → Stopped/Failed                        │
│  • Canonical RequestHead extraction                 │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Canonical driver (server/connection.rs)             │
│  • serve_http1_connection: transport-neutral        │
│  • ConnectionContext (TCP, TLS, or caller-owned)    │
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
│  3. Permit release + connection termination         │
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

### Configuration Flow

Configuration is split between runtime-owned (transport) and static-service-owned (filesystem) concerns:

```
CLI flags / Python params / Rust structs
         │
         ▼
┌─────────────────────────────────────────┐
│ Limits (validated subset)               │
│  • 11 fields: connections, streams,     │
│    timeouts, body sizes, listing,       │
│    chunk size                           │
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

---

## Core Library Module Map (`eggserve-core`)

| Module | Visibility | Purpose | Stability |
|--------|-----------|---------|-----------|
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` | Stable-ish |
| `limits.rs` | **pub** | `Limits` — connections, streams, timeouts | Stable-ish |
| `policy.rs` | **pub** | `StaticPolicy`, `SymlinkPolicy`, `DotfilePolicy`, `DirectoryListingPolicy` | Stable-ish |
| `path/` | pub(crate) | Path confinement pipeline (7 submodules) | Internal |
| `fs/` | pub(crate) | Filesystem confinement, descriptor-relative traversal on Unix | Internal |
| `response.rs` | pub(crate) | Response helpers (file streaming, directory listing, error responses) | Internal |
| `mime.rs` | pub(crate) | MIME type detection via `phf` map (~60 extensions) | Internal |
| `ops.rs` | **pub** | Structured logging, operational events, counters | Stable-ish |
| `primitives/` | **pub** | Public facade — all canonical types for embedding consumers | Stable |
| `server/` | **pub** | Runtime service boundary: `Server`, `Service` trait, `StaticService`, lifecycle | Experimental |
| `tls.rs` | **pub** | TLS config loading (feature-gated: `tls`) | Experimental |

---

## Error Taxonomy

Five distinct error layers, each scoped to a specific subsystem:

| Error Type | Scope | Variants |
|-----------|-------|----------|
| `PathRejection` | Path parsing | 17 variants: `Empty`, `TooLong`, `MalformedPercentEncoding`, `ControlCharacter`, `ParentComponent`, `DotfileDenied`, `SymlinkDenied`, `RootEscapeDenied`, ... |
| `RequestValidationError` | HTTP-level | 6 variants: `MethodNotAllowed`, `InvalidContentLength`, `BodyTooLarge`, `UnsupportedTransferEncoding`, `ConflictingBodyHeaders`, `InvalidRequestTarget` |
| `ServerError` | Server lifecycle | 10 variants: `Bind`, `Config`, `AlreadyStarted`, `NotStarted`, `Accept`, `TlsSetup`, `Transport`, `ShutdownTimeout`, `Startup`, `Terminal` |
| `ServiceError` | Per-request | `Internal`, `Rejected(u16)`, `Panic`, `Timeout` |
| `RequestBodyError` | Body consumption | 12 variants: `RejectedByPolicy`, `LimitExceeded`, `ReadTimeout`, `PrematureEof`, `AlreadyConsumed`, ... |

---

## Module Visibility Model

| Tier | Modules | Stability |
|------|---------|-----------|
| **Stable** | `primitives` (facade), all `primitives::*` submodules | Intended public boundary for embedding consumers |
| **Stable-ish** | `config`, `limits`, `policy`, `ops` | Field shapes may evolve before 1.0 |
| **Experimental** | `server` (all types) | API may change without notice |
| **Internal** | `fs`, `path`, `response`, `mime` | `pub(crate)` — not part of public API |

---

## Platform Support

| Platform | Status | Security Model |
|----------|--------|----------------|
| **Linux x86_64** (glibc, manylinux_2_17) | Supported-hardened | Descriptor-relative traversal via `statat`+`openat` |
| **Linux aarch64** (glibc, manylinux_2_17) | Supported-hardened | Same descriptor-relative guarantees as Linux x86_64 |
| **Linux armv7** (glibc, manylinux_2_17) | Supported-hardened | Same descriptor-relative guarantees as Linux x86_64 |
| **Linux x86_64** (musl, musllinux_1_2) | Supported-hardened | Same descriptor-relative guarantees; musl libc uses the same path |
| **Linux aarch64** (musl, musllinux_1_2) | Supported-hardened | Same descriptor-relative guarantees as Linux x86_64 (musl) |
| **macOS** (x86_64, arm64) | Supported-hardened | Same descriptor-relative guarantees as Linux |
| **Windows x86_64** | Supported-functional | Handle-relative child resolution, reparse-point denial, and directory enumeration are qualified for the executed classes. Two open-descendant root-rename cases remain skipped because NTFS rejects that external path operation; keep Windows for trusted/local content. |
| **Windows arm64** | Supported-functional | Same as Windows x86_64. Requires native ARM64 execution before Tier 1. |

---

## Testing Strategy

Multi-layered testing spans the Python and Rust suites, 11 fuzz targets, and 2 conformance corpora:

| Layer | Location | Scope |
|-------|----------|-------|
| Rust unit tests | `crates/*/src/**/*.rs` (inline `#[cfg(test)]`) | Module-level logic |
| Rust integration tests | `crates/*/tests/*.rs` | Cross-module, live TCP, TLS (30 files in core, 4 in bin) |
| Python test suites | `crates/eggserve-python/tests/test_*.py` | Compatibility facade, TLS, low-level primitives, conformance, body, boundary hardening |
| Packaging smoke tests | `crates/eggserve-python/packaging-tests/` | Installed-wheel validation |
| Conformance corpora | `conformance/*.json` | Shared Rust/Python test data |
| Fuzz targets | `fuzz/fuzz_targets/*.rs` | Property-based input fuzzing (11 targets) |
| Repo-level tests | `tests/` | Proxy interop, soak, installed-binary qual |

See [testing-and-conformance.md](testing-and-conformance.md) for the full test matrix.

The executable product demonstrations are indexed in
[`../examples/README.md`](../examples/README.md). The full verification script
compiles the Cargo examples and smoke-tests the canonical Python and Rust
examples with loopback port `0`; this is deliberately a small addition to the
existing Rust/Python checks rather than a separate CI job.

---

## Fuzz Targets

11 fuzz targets under `fuzz/fuzz_targets/` provide property-based input fuzzing:

| Target | What It Fuzzes |
|--------|---------------|
| `request_target` | HTTP origin-form parsing, path confinement, request target validation |
| `percent_decode` | Single-pass percent decoding |
| `path_components` | Path normalization and component validation |
| `validate_method` | HTTP method construction and validation, body rejection |
| `range_header` | Range header parsing |
| `if_none_match` | If-None-Match ETag comparison |
| `platform_component` | Windows platform-specific checks |
| `fuzz_header_block` | HeaderName, HeaderValue, and HeaderBlock operations |
| `fuzz_normalize_response` | StatusCode validation, response building, normalization |
| `fuzz_request_body` | RequestBody state machine |
| `fuzz_directory_buffer` | Directory listing buffer behavior |

Each target has a seed corpus under `fuzz/corpus/`. Fuzzing invariants: no panics on arbitrary input, no `..`/`.` in accepted path components, no NUL bytes in decoded paths, no double-decoding, satisfiable ranges within file size. Corpus regression replay runs via `cargo test -p eggserve-core --test corpus_replay`.

---

## Scripts and Verification

The `scripts/` directory provides a small, layered verification hierarchy:

| Script | Purpose |
|--------|---------|
| `verify.sh` | Main entry point: `fast` (Rust-only dev check), `full` (examples + Rust + Python wheel), `deep` (expensive suites) |
| `test-python-wheel.sh` | Build wheel, install in venv, run smoke + tests — the authoritative Python test entry point |
| `test-examples.sh` | Compile Cargo examples and smoke-test canonical Python/Rust demos on loopback port 0 |
| `verify-cargo-packages.sh` | Package dry-run gates (`--mode all`) for release validation |
| `verify-conformance-matrix.py` | Validate conformance corpus consistency (CI runs this first) |
| `check-wheel-composition.py` | Inspect wheel contents for correctness |
| `check-release-wheel-set.py` | Validate that a release directory contains the expected 9-platform wheel set |
| `check-python-release-metadata.py` | Validate release metadata (versions, tags, artifact naming) |
| `release_smoke.py` | Release artifact smoke tests |
| `install-cargo-tools.sh` | Deterministic installation of `cargo-audit` and `cargo-deny` for manual security checks |

Verification levels:
- **`fast`** — `cargo fmt --check`, `cargo clippy`, `cargo test --workspace` (routine dev)
- **`full`** — fast + Cargo examples compiled and smoke-tested, Python wheel built and tested (pre-release)
- **`deep`** — full + expensive suites, manual execution only

---

## Benchmarks

The `benchmarks/` directory holds benchmark baselines (Criterion-based, historical). Current representative results on macOS arm64 (APFS, warm cache):

| Workload | Median | Notes |
|----------|--------|-------|
| GET 1 KiB | 12.6 us | Handler latency (no TCP/TLS) |
| GET 128 KiB | 12.9 us | Streaming body is lazy |
| HEAD 128 KiB | 12.1 us | Body suppressed by normalize_metadata |
| Range 16 KiB | 26.3 us | Seek + range parsing overhead |
| 304 Not Modified | 11.3 us | No file open or streaming |
| 404 Not Found | 1.9 us | Path parse + resolve only |
| Dir listing 1000 entries | 2.25 ms | Linear scaling |

Full results: `benchmarks/088-baseline/results.json`. The old Criterion harness is historical; current changes use a deliberately selected measurement session rather than treating numbers as a CI gate.

---

## Examples

### Python examples (`examples/`)

| File | Purpose |
|------|---------|
| `python_http_server_static.py` | Stock `SimpleHTTPRequestHandler` — fast-path, no Python dispatch |
| `python_custom_handler.py` | Custom `BaseHTTPRequestHandler` — callback path |
| `python_custom_headers.py` | Static metadata: `default_content_type`, ordered `extra_response_headers` |
| `python_https_server.py` | Rust-runtime TLS server via the facade (`HTTPSServer`) |
| `python_subprocess.py` | `eggserve.subprocess` lifecycle helpers |
| `python_safe_download.py` | Safe download with bounded response |
| `README.md` | Mechanically checked example index |

### Rust examples (`crates/eggserve-core/examples/`)

| File | Purpose |
|------|---------|
| `static_server.rs` | Built-in confined static service via `Server::builder()` |
| `custom_service.rs` | Custom `Service` via `service_fn` |
| `custom_headers.rs` | Static metadata: `default_content_type`, ordered `extra_response_headers` (final 200 responses only) |
| `https_server.rs` | TLS serving via `load_tls_config` + `tls_config()` (`--features tls`) |
| `primitives.rs` | Response planning without opening a socket |

All examples bind loopback, support port `0` for smoke tests, wait for readiness, and cleanly shut down on Ctrl+C. They are compiled and smoke-tested by `scripts/verify.sh full`.

---

## Crate Source Structure

### eggserve-core (44 source files)

```
src/
├── lib.rs                    # module declarations, 3-tier stability model
├── config.rs                 # ServeConfig, ServeState, StartupSummary
├── limits.rs                 # Limits — connections, streams, timeouts
├── policy.rs                 # StaticPolicy, SymlinkPolicy, DotfilePolicy, DirectoryListingPolicy
├── ops.rs                    # structured logging event model, OpsCounters
├── tls.rs                    # TLS config loading (feature-gated)
├── response.rs               # Hyper response helpers, file streaming, error responses
├── mime.rs                   # MIME type detection via phf map (~60 extensions)
├── path/
│   ├── mod.rs                # ConfinedPath type
│   ├── request_target.rs     # origin-form parsing
│   ├── decode.rs             # single-pass percent decoding
│   ├── components.rs         # normalization, splitting, validation
│   ├── rejected.rs           # PathRejection (17 variants)
│   ├── policy.rs             # PathPolicy, DotfilePolicy (path-level)
│   └── platform.rs           # Windows reserved names, ADS, drive prefixes
├── fs/
│   ├── mod.rs                # PinnedRoot, RootGuard, ResolvedResource, ResolvedFile, ResolvedDirectory
│   ├── unix.rs               # descriptor-relative traversal (statat + openat)
│   └── windows.rs            # handle-relative traversal (NtOpenFile, NtQueryDirectoryFile)
├── primitives/
│   ├── mod.rs                # re-exports all public types
│   ├── secure_root.rs        # SecureRoot, ResolvedFile, ResolvedDirectory, ResolvedResource
│   ├── body.rs               # BodySource, BodyKind, BodySourceError
│   ├── canonical.rs          # StatusCode, Response, normalize_response, normalize_metadata
│   ├── method.rs             # Method (canonical HTTP method)
│   ├── version.rs            # HttpVersion
│   ├── header_block.rs       # HeaderBlock, HeaderName, HeaderValue
│   ├── request_target.rs     # RequestTarget
│   ├── request_head.rs       # RequestHead
│   ├── connection_info.rs    # ConnectionInfo, Scheme, TlsInfo
│   ├── request.rs            # Request (head + body + connection)
│   ├── request_body.rs       # RequestBody, BodyState
│   ├── request_body_error.rs # RequestBodyError (12 variants)
│   ├── request_body_policy.rs# RequestBodyPolicy (Reject/Buffer/Stream)
│   ├── incomplete_body_policy.rs # IncompleteBodyPolicy
│   ├── planner.rs            # plan_file_response, conditional/range/ETag evaluation
│   ├── response.rs           # StaticResponsePlan, BodyPlan, FileRange, ResponseStatus
│   └── http.rs               # ReadOnlyMethod, validate_method/body/target
└── server/
    ├── mod.rs                # Server, ServerBuilder, RuntimeState, accept_loop_generic
    ├── config.rs             # RuntimeConfig, RuntimeConfigBuilder
    ├── connection.rs         # Transport-neutral driver: serve_http1_connection, ConnectionContext, ConnectionShutdown, ConnectionOutcome
    ├── errors.rs             # ServerError, ShutdownResult
    ├── handle.rs             # ServerHandle (lifecycle control)
    ├── lifecycle.rs          # LifecycleState (Created→Running→Draining→Stopped/Failed)
    ├── service.rs            # Service trait, service_fn, ServiceError
    └── static_service.rs     # StaticService (hardened static file serving)
```

### eggserve-bin (5 source files)

```
src/
├── main.rs    # thin fn main() → eggserve_bin::run()
├── lib.rs     # run(), run_cli(argv); delegates accept loop to core server
├── args.rs    # manual argument parsing (no clap)
├── shutdown.rs# signal handling (Ctrl+C, SIGTERM, SIGHUP) with broadcast channel
└── tls.rs     # re-export shim for eggserve_core::tls (loading lives in core)
```

### eggserve-python (2 Rust source files + Python facade)

```
src/
├── lib.rs     # PyO3 module registration: 20 exceptions, 24 classes, 7 functions
└── server.rs  # PyRequestBody, PyRequest, PyResponse, PythonCallbackService, PyServer

python/eggserve/
├── __init__.py     # top-level namespace (version, serve_directory, facade classes)
├── __init__.pyi    # type stub for the facade namespace
├── _bin.py         # CLI entry point via native _run_cli
├── __main__.py     # python -m eggserve support
├── _native.pyi     # type stub over the native extension surface
├── server.py       # six-class Rust-runtime compatibility facade
├── server.pyi      # type stub for the facade classes
├── lowlevel.py     # advanced native exports (SecureRoot, StaticPolicy, canonical types)
└── subprocess.py   # subprocess lifecycle exports (ServeConfig, ServerProcess)
```

---

## Release Process

Release is a manual workflow dispatch. CI is a regression screen, not release certification:

1. Run `./scripts/verify.sh full` (examples, Rust + Python wheel)
2. Run `bash scripts/install-cargo-tools.sh` then `cargo audit` + `cargo deny check`
3. Manually dispatch the release workflow (builds, validates, and publishes via OIDC Trusted Publishing)
4. Production PyPI upload requires the protected `pypi` GitHub Environment
5. No push/tag/merge automatically publishes

See [docs/release-process.md](../docs/release-process.md) for the full procedure.

Historical design records remain in [`plans/`](../plans/); they are not
required to understand the current runtime or security contract.
