# Library Capability Matrix

This is the technical inventory for maintainers and embedders. The concise
user-facing comparison of `python -m http.server`, the CLI, Python, and Rust
is maintained in [python-http-server-compatibility.md](python-http-server-compatibility.md).

This document maps every eggserve capability across all surfaces and indicates
its status using a constrained vocabulary.

## Vocabulary

| Term | Meaning |
|---|---|
| **stable** | Public API, semver-considered; patch releases preserve source compatibility, while pre-1.0 breaking changes require an explicit minor transition with release notes and migration guidance. |
| **experimental** | Public but unstable; breaking changes may occur in minor releases. |
| **internal** | Not part of the public API; external consumers should not depend on it. |
| **CLI-only** | Available only through the CLI binary; not exposed as a library API. |
| **planned** | Not yet implemented; tracked by an existing plan. |
| **intentionally unsupported** | Explicit non-goal; see `docs/non-goals.md`. |
| **platform-limited** | Implemented on some platforms but unavailable or weakened on others. |

## Surfaces

| Column | Description |
|---|---|
| **CLI** | `eggserve-bin` command-line interface (all flags from `args.rs`). |
| **Rust stable** | `eggserve-core::primitives` module — the intended public Rust boundary. |
| **Runtime experimental** | `eggserve-core::server` — transport-owning runtime: `Server`, `Service` trait, `StaticService`. |
| **Python stable** | `eggserve.server` compatibility classes and `serve_directory`; advanced wrappers are under `eggserve.lowlevel`, subprocess helpers under `eggserve.subprocess`. |
| **Python experimental** | No default Python client surface. The internal callback engine and native bridge types are not supported imports. |
| **Built-in static service** | The static service used by CLI and Python Server (GET/HEAD only, body rejection, path confinement, conditional/range responses). |
| **Generic callback server** | Python `Server` with a user-provided handler callback; bounded concurrency via `max_python_callbacks`. |

## Capability Matrix

| Capability | CLI | Rust stable | Runtime experimental | Python stable | Python experimental | Built-in static service | Generic callback server |
|---|---|---|---|---|---|---|---|
| Bind/listen lifecycle | stable | — | experimental | stable | — | stable | stable |
| Plaintext HTTP/1.1 | stable | — | experimental | stable | — | stable | stable |
| Native HTTP/2 (opt-in `http2`; prior knowledge and TLS ALPN) | — | — | experimental (Plan 190 DATA/body-policy corrections pass; broader client/platform evidence and safe stream-local reset/wire-progress remain open) | — | — | experimental | — |
| Native HTTP/3/QUIC (opt-in `http3`; same-port UDP, TLS 1.3/`h3` ALPN) | — | — | experimental (Plan 190 in-process body/lifecycle corrections pass; Plan 192 `BLOCKED` on latest released stack with upstream `h3#338` unfixed and `#262` remainder open; Plan 193 retains experimental on the unchanged candidate with two-family, browser, adversarial, impairment, and platform evidence still missing; Plan 194 adds per-stream producer no-progress timeout with stream reset; Plan 195 correctively qualifies that bound with no tier change) | — | — | experimental | — |
| TLS server (rustls; H1 by default, experimental H2 ALPN with `http2,tls`) | stable | — | experimental (feature-gated `tls`/`http2`: `RuntimeConfig::tls_config`, ALPN-selected accept loop) | stable (H1-only) | — | stable | stable |
| GET/HEAD static serving | stable | stable | experimental | stable | — | stable | — |
| Request-target validation | stable | stable | experimental | stable | — | stable | — |
| Request-body policy | stable | stable | experimental | stable | — | stable | stable |
| Canonical request types | — | stable | experimental | stable | stable | — | — |
| Canonical response types | — | stable | experimental | stable | stable | — | — |
| Explicit Hyper transport conversion (`to_hyper_response`) | — | stable (explicit adapter) | — | — | — | — | — |
| Duplicate-preserving headers | — | stable | experimental | — | experimental | stable | stable |
| Connection metadata | — | stable | experimental | stable | stable | — | — |
| Service trait | — | — | experimental | — | — | — | — |
| ServerBuilder | — | — | experimental | — | — | — | — |
| ServerHandle | — | — | experimental | — | — | — | — |
| StaticService | — | — | experimental | — | — | — | — |
| service_fn | — | — | experimental | — | — | — | — |
| RuntimeConfig | — | — | experimental | — | — | — | — |
| Conformance corpus and parity testing | — | experimental | — | — | experimental | — | — |
| Bounded request-body support | — | experimental | experimental | experimental | — | — | experimental |
| Request-body framing strictness | — | experimental | experimental | experimental | — | — | experimental |
| `normalize_metadata()` | — | stable | experimental | — | — | stable | stable |
| StatusCode range (100–599) | — | stable | experimental | stable | — | stable | stable |
| Secure root resolution | stable | stable | experimental | stable | — | stable | stable |
| Symlink policy | stable | stable | experimental | stable | — | stable | stable |
| Dotfile policy | stable | stable | experimental | stable | — | stable | stable |
| Directory listing | stable | stable | experimental | stable | — | stable | — |
| Index files | stable | stable | experimental | stable | — | stable | — |
| Conditional requests | stable | stable | experimental | stable | — | stable | — |
| Range requests | stable | stable | experimental | stable | — | stable | — |
| File streaming | stable | stable | experimental | stable | — | stable | stable |
| Streaming response bodies (known/unknown; `Send`, not `Sync`, producer) | — | stable | experimental | — | — | — | — |
| Low-level handler-only service substrate (`eggserve.lowlevel`) | — | — | experimental (server seam) | stable | — | — | stable |
| Caller-owned connection driver (`serve_http1_connection`) | — | — | experimental | — | — | — | — |
| Caller-owned H1/H2 prior-knowledge driver (`serve_http_connection`, `http2`) | — | — | experimental | — | — | — | — |
| Downstream HTTP-only app-server bridge (bounded full-duplex, deferred body, `RequestLifecycle`, admission split; qualified by `app_server_consumer`) | — | stable (byte metadata, `ResponseStream`) | experimental (`Service`, lifecycle, driver) | — | — | — | — |
| Typed request context (`RequestContext`: connection + lifecycle; single capability attachment point, no type map, no raw handles; Plan 197) | — | — | experimental | — | — | — | — |
| Application-service contract fixture + example (`application_service_contract`, `application_service`; buffered/streamed/lifecycle, no static FS; Plan 197) | — | — | experimental | — | — | — | — |
| Response privacy policy (Server/Date/denylist/errors, static validators) | — | stable (static validators) | experimental | stable (safe subset) | — | stable | stable |
| Generic byte responses | — | stable | — | stable | — | — | stable |
| Duplicate headers | — | stable | experimental | stable | — | stable | stable |
| Callback handlers | — | — | — | stable | — | — | stable |
| Existing-listener support | — | implemented | experimental | Rust-only | — | — | — |
| Lifecycle methods (wait_ready, shutdown, force_shutdown, wait, state) | — | — | experimental | stable | — | — | stable |
| Graceful shutdown | stable | — | experimental | stable | — | stable | stable |
| Observability (events/sinks/counters/snapshots) | stable (stderr text/json/none, `--quiet` filter) | process-global default only (standalone canonical conversions) | experimental (per-runtime `OpsContext`: sink, counters, correlation IDs; snapshots via `RuntimeState`/`ServerHandle`) | minimal (process-global default stderr sink; no per-server sink selection) | — | experimental (service-owned events via the server context) | — |
| Static directory canonicalization | CLI | — | experimental | stable | — | stable | — |
| General application redirects | — | — | — | — | — | — | — |
| Retries | — | — | — | — | — | — | — |
| Cookies | — | — | — | — | — | — | — |
| Proxies | — | — | — | — | — | — | — |
| Decompression | — | — | — | — | — | — | — |
| ASGI/WSGI adapters | — | — | — | — | — | — | — |
| CGI / FastCGI adapters (Plan 167 no-go) | — | — | — | — | — | — | — |
| Generic tunnel handoff (Plan 199; H1 `Upgrade`/`CONNECT`, H2/H3 Extended `CONNECT`; H3 generic `:protocol` blocked) | — | — | experimental | experimental | experimental | — | — |
| Cross-protocol application conformance (Plan 207; `app_server_conformance.toml` inventory + `cross_protocol_conformance.rs` routine subset; H1/TLS/prebound/Unix, H2, H3, caller-owned; native/`http`/Tower/async-Python/ASGI) | — | — | experimental (routine H1/prebound/Unix/caller-owned + H2/Tower-gated; H1 TLS/H2 TLS/H3/`http-interop`/Python owned by existing suites; manual interop/soak fail-closed) | — (Python H1-only by contract) | — | — | — |
| Windows reparse-point hardening | — | — | — | — | — | — | — |

Rows with no annotation in any column are **intentionally unsupported** (empty
cell = not applicable to that surface). General application redirects,
retries, cookies, proxies, decompression, ASGI/WSGI/CGI/FastCGI, WebSocket
framing (tunnel handoff itself experimental per above), and Windows
reparse-point hardening remain intentionally unsupported or platform-limited as
noted. Static directory canonicalization is the implemented narrow redirect
behavior.

## Platform support

| Platform | Status | Notes |
|---|---|---|
| Linux x86_64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile/reparse hardening. |
| Linux aarch64 | supported-hardened | Same as Linux x86_64. |
| macOS arm64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile hardening. |
| macOS x86_64 | supported-hardened | Same as macOS arm64. |
| Windows x86_64 | supported-functional | Handle-relative confinement and manual qualification cover directory-handle retention, child resolution, reparse denial, and directory enumeration. Two open-descendant root-rename cases remain explicitly skipped because NTFS rejects that external path operation. |

## Notes

- **Follow-symlinks mode** is weaker than descriptor-relative traversal. On
  Unix with safe defaults, symlink denial is descriptor-relative — each path
  component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with
  `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is
  refused rather than followed. Follow-symlinks mode falls back to
  component-wise `symlink_metadata` checks and is explicitly outside the
  descriptor-relative hardening guarantee.

- **Windows handle-relative confinement** is implemented and manually
  qualified for the executed classes. Two open-descendant root-rename cases are
  skipped due to NTFS path-rename semantics. Windows remains functional-only;
  do not use it with untrusted public content.

- **Python wheels** are CPython 3.11+ with abi3 stable ABI (`>=3.11`) on the Linux,
  macOS, and Windows wheel matrix. The wheel contains the native extension and
  extension-backed CLI entry point; it does not bundle a second standalone CLI
  binary.

- **Runtime service boundary is experimental.** `eggserve-core::server` provides
  a transport-owning runtime (`Server`, `Service` trait, `StaticService`) for
  embedding. Its API is subject to change without notice. It is not covered by
  the stable API contract.
- **Observability ownership (Plan 181).** Each runtime owns an `OpsContext`
  (sink, counters, correlation-ID source). Default construction clones the
  process-global default, so CLI/single-server behavior is unchanged;
  embedders running several servers in one process attach one context per
  server (`ServerBuilder::ops_context` / `RuntimeState::with_ops`) and read
  bounded snapshots (`RuntimeState::ops_snapshot`,
  `ServerHandle::ops_snapshot`). Connection IDs start at 1 per context.
  Standalone canonical conversions (`primitives::to_hyper_response`) and the
  Python facade keep the process-global default. No tracing/OpenTelemetry/
  Prometheus/exporter integration is provided.
- The Plan 175 consumer qualification establishes that a separate HTTP-only
  application server can use the public Rust substrate; it does not change the
  experimental tier of the runtime/server APIs or add upgrade support.
