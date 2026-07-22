# Architecture Overview

eggserve is a security-oriented, Rust-backed static file server with safe-by-default behavior. It ships as a CLI binary and a Python-packaged tool, backed by a Rust library for path confinement, policy enforcement, and response construction.

## Workspace Structure

```
eggserve/
├── Cargo.toml                  # workspace root (resolver = "2", edition 2021)
├── crates/
│   ├── eggserve-core/          # library: security primitives, HTTP serving, response construction
│   ├── eggserve-bin/           # binary: CLI, accept loop, signal handling
│   └── eggserve-python/        # Python wheel (maturin + PyO3, independent build)
├── architecture/               # this directory — deep-dive docs per subsystem
├── docs/                       # reference docs (31 files)
├── plans/                      # design plans (000–084, all complete)
├── release/                    # machine-readable release criteria (criteria.toml)
├── conformance/                # shared Rust/Python conformance corpora
├── fuzz/                       # fuzzing targets and seed corpora
├── scripts/                    # release validation, contract consistency, CI evidence
└── examples/                   # Python usage examples
```

## Crate Dependency Graph

```
eggserve-core          ← eggserve-bin (path dep)
eggserve-core          ← eggserve-python (path dep, excluded from workspace)
eggserve-bin           → standalone, owns process lifecycle
eggserve-python        → standalone, owns Python packaging
```

`eggserve-core` has no workspace dependencies. `eggserve-bin` and `eggserve-python` each depend on `eggserve-core` via path. The Python subprocess layer communicates with the binary via CLI arguments (no shared memory, no FFI to the bin crate).

## Component Index

| Component | Crate / Location | Deep Dive |
|-----------|-----------------|-----------|
| Core library | `eggserve-core` | [eggserve-core.md](eggserve-core.md) |
| CLI binary | `eggserve-bin` | [eggserve-bin.md](eggserve-bin.md) |
| Python bindings | `eggserve-python` | [eggserve-python.md](eggserve-python.md) |
| Path confinement | `eggserve-core::path` | [path-confinement.md](path-confinement.md) |
| Filesystem confinement | `eggserve-core::fs` | [filesystem-confinement.md](filesystem-confinement.md) |
| Policy system | `eggserve-core::policy` | [policy-system.md](policy-system.md) |
| Public API boundary | `eggserve-core::primitives` | [primitives-api.md](primitives-api.md) |
| HTTP response planning | `eggserve-core::primitives::planner` | [response-planning.md](response-planning.md) |
| Runtime service boundary | `eggserve-core::server` | [runtime.md](runtime.md) |
| HTTP client primitives | `eggserve-core::primitives::client` | [client.md](client.md) |
| Security model | (cross-cutting) | [security-model.md](security-model.md) |
| Release infrastructure | `release/`, `scripts/` | [release-infrastructure.md](release-infrastructure.md) |
| Testing and conformance | `tests/`, `conformance/`, `fuzz/` | [testing-and-conformance.md](testing-and-conformance.md) |

## Data Flow

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
| `primitives/` | **pub** | Public facade — all canonical types for embedding consumers | Stable |
| `primitives/canonical.rs` | **pub** | `StatusCode`, `Response`, `normalize_response()`, `normalize_metadata()` | Stable |
| `primitives/body.rs` | **pub** | `BodySource`, `BodyKind`, `BodySourceError` | Stable |
| `primitives/client/` | **pub** (feature-gated) | HTTP client: `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse` | Experimental |
| `server/` | **pub** | Runtime service boundary: `Server`, `Service` trait, `StaticService`, lifecycle | Experimental |

## Key Architectural Decisions

1. **Safe by default** — Every security default (loopback bind, no symlinks, no dotfiles, no directory listing) is enforced unless the user explicitly passes a flag. Binding to `0.0.0.0` requires `--public` to acknowledge intent.

2. **No serving outside the configured root** — Path traversal and symlink escape are denied at the library level. On Unix with safe defaults, symlink denial is *descriptor-relative* — each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is refused rather than followed.

3. **No broad dependencies** — Every dependency has an explicit purpose. `phf` for compile-time MIME map, `rustix` for Unix syscalls, `httpdate` for Last-Modified. No framework dependencies beyond Hyper. Manual argument parsing (no clap).

4. **Plan-driven development** — Every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

5. **Framework-independent response planning** — `StaticResponsePlan`, `BodyPlan`, `HeaderMapPlan` are pure value objects with no Hyper dependency. The Python bindings consume these directly.

6. **Canonical response normalization** — All response producers converge on a single normalization path. In-memory bodies use `primitives::canonical::normalize_response()`. File-backed bodies use `primitives::canonical::normalize_metadata()` to apply the same framing rules without consuming a `Response` value.

7. **Two DotfilePolicy types** — `path::DotfilePolicy` (parsing level) and `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served.

8. **File-stream semaphore** — A bounded semaphore limits concurrent file streams (default 32). When exhausted, the handler returns 503 Service Unavailable.

9. **Python immutability** — All PyO3 classes are `#[pyclass(frozen)]` and Python dataclasses use `frozen=True`. Immutability enforced at both layers.

10. **Evidence-driven release process** — Release gates are defined in `release/criteria.toml` as a machine-readable source of truth. A Python validator checks criteria integrity, generates checklists, and produces structured evidence.

11. **Contract-driven documentation** — All public-facing documents are reconciled against a single capability matrix (`docs/library-capability-matrix.md`). Cross-document claims are validated for consistency.

12. **Fail-closed evidence aggregation** — Evidence aggregation uses a severity-ordered precedence (MALFORMED > CONFLICTING > INVALIDATED > STALE > FAILED > MISSING) and never silently ignores malformed or conflicting records.

## Module Visibility Model

| Tier | Modules | Stability |
|------|---------|-----------|
| Stable | `primitives` (facade), `primitives::canonical`, `primitives::http`, `primitives::planner`, `primitives::response`, `primitives::method`, `primitives::version`, `primitives::header_block`, `primitives::request_target`, `primitives::request_head`, `primitives::connection_info`, `primitives::request`, `primitives::request_body`, `primitives::request_body_error`, `primitives::request_body_policy`, `primitives::incomplete_body_policy`, `primitives::body` | Intended public boundary for embedding consumers |
| Stable-ish | `config`, `limits`, `policy` | Field shapes may evolve before 1.0 |
| Experimental | `service` (`handle_request`), `server` (`Server`, `Service` trait, `StaticService`, `LifecycleState`, lifecycle, connection tracking), `primitives::client` | Body type, async surface, server API, and client API may change |
| Internal | `fs`, `path`, `response`, `mime`, `error` | `pub(crate)` — not part of public API |

## Error Taxonomy

eggserve uses three distinct error layers:

| Error Type | Scope | Variants |
|-----------|-------|----------|
| `PathRejection` | Path parsing (16 variants) | `Empty`, `TooLong`, `MalformedPercentEncoding`, `ParentComponent`, `DotfileDenied`, `SymlinkDenied`, `RootEscapeDenied`, ... |
| `Error` | Top-level crate | `PathEscape`, `PathNotAccessible`, `Config`, `Bind`, `Runtime`, `RequestRejected`, `Io`, `Client` |
| `RequestValidationError` | HTTP-level | `MethodNotAllowed`, `InvalidContentLength`, `BodyTooLarge`, `UnsupportedTransferEncoding` |
| `ServerError` | Server lifecycle | `Bind`, `Config`, `AlreadyStarted`, `Accept`, `TlsSetup`, `ShutdownTimeout`, `Startup`, `Terminal` |
| `ServiceError` | Per-request | `Internal`, `Rejected(u16)`, `Panic`, `Timeout` |
| `RequestBodyError` | Body consumption (12 variants) | `RejectedByPolicy`, `LimitExceeded`, `ReadTimeout`, `PrematureEof`, `AlreadyConsumed`, ... |
| `ClientError` | HTTP client (12 variants) | `InvalidUrl`, `UnsupportedScheme`, `Timeout`, `TlsError`, `ResponseBodyTooLarge`, ... |

## Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| Linux (x86_64, aarch64) | **Supported-hardened** | Descriptor-relative traversal via `statat`+`openat` |
| macOS (x86_64, aarch64) | **Supported-hardened** | Same descriptor-relative guarantees as Linux |
| Windows (x86_64) | **Supported-functional** | Plan 084 implements handle-relative child resolution via retained directory handles. Plan 085 implements handle-relative directory enumeration via `NtQueryDirectoryFile`. Plan 086 adversarial qualification test scaffold established (113 tests). Independent safety review and profile promotion decision awaited. Not for untrusted content until those human gates complete. |

## Non-Goals

eggserve explicitly does not aim to be: an ASGI/WSGI server, a CGI executor, a file upload handler, a reverse proxy, an ACME client, a plugin host, a template engine, or an auth system. It competes with `python -m http.server` for local development use cases, not with nginx, Caddy, or Uvicorn.
