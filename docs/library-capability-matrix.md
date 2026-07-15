# Library Capability Matrix

This document is for Plan 043 — contract/scope reconciliation. It maps every
eggserve capability across all surfaces and indicates its status using a
constrained vocabulary.

## Vocabulary

| Term | Meaning |
|---|---|
| **stable** | Public API, semver-considered; breaking changes require a major version bump. |
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
| **Rust experimental** | `eggserve-core::service` (HTTP handler) and `client` module (feature-gated). |
| **Runtime experimental** | `eggserve-core::server` — transport-owning runtime: `Server`, `Service` trait, `StaticService`. |
| **Python stable** | Python types wrapping core primitives: `ServeConfig`, `ServerProcess`, `serve_directory`, `StaticPolicy`, `PathPolicy`, `SecureRoot`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory`, `Request`, `Response`, `StaticResponder`, `Server`, `ServerSecureRoot`, `ServerBodySource`. |
| **Python experimental** | Python client types: `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse`, `ClientError`, `Method`. |
| **Built-in static service** | The `handle_request` function used by CLI and Python Server (GET/HEAD only, no request bodies, path confinement, conditional/range responses). |
| **Generic callback server** | Python `Server` with a user-provided handler callback; bounded concurrency via `max_python_callbacks`. |
| **Experimental client** | `HttpClient` — synchronous, buffered, no connection pooling, no redirect following, no streaming. |

## Capability Matrix

| Capability | CLI | Rust stable | Rust experimental | Runtime experimental | Python stable | Python experimental | Built-in static service | Generic callback server | Experimental client |
|---|---|---|---|---|---|---|---|---|---|
| Bind/listen lifecycle | stable | — | — | experimental | stable | — | stable | stable | — |
| Plaintext HTTP/1.x | stable | — | stable | experimental | — | — | stable | stable | stable |
| TLS server | stable | — | — | — | — | — | — | — | — |
| TLS client | — | — | stable | — | — | stable | — | — | stable |
| GET/HEAD static serving | stable | stable | stable | experimental | stable | — | stable | — | — |
| Request-target validation | stable | stable | stable | experimental | stable | — | stable | — | — |
| Request-body rejection | stable | stable | stable | experimental | stable | — | stable | stable | — |
| Canonical request types | — | stable | stable | experimental | stable | stable | — | — | — |
| Canonical response types | — | stable | stable | experimental | stable | stable | — | — | — |
| Duplicate-preserving headers | — | stable | stable | experimental | — | experimental | stable | stable | — |
| Connection metadata | — | stable | stable | experimental | stable | stable | — | — | — |
| Service trait | — | — | — | experimental | — | — | — | — | — |
| ServerBuilder | — | — | — | experimental | — | — | — | — | — |
| ServerHandle | — | — | — | experimental | — | — | — | — | — |
| StaticService | — | — | — | experimental | — | — | — | — | — |
| service_fn | — | — | — | experimental | — | — | — | — | — |
| RuntimeConfig | — | — | — | experimental | — | — | — | — | — |
| Conformance corpus and parity testing | — | — | experimental | — | — | experimental | — | — | — |
| Bounded request-body support | planned | experimental | experimental | experimental | planned | — | planned | planned | — |
| `normalize_metadata()` | — | stable | stable | experimental | — | — | stable | stable | — |
| StatusCode range (100–999) | — | stable | stable | experimental | stable | — | stable | stable | — |
| Secure root resolution | stable | stable | stable | experimental | stable | — | stable | stable | — |
| Symlink policy | stable | stable | stable | experimental | stable | — | stable | stable | — |
| Dotfile policy | stable | stable | stable | experimental | stable | — | stable | stable | — |
| Directory listing | stable | stable | stable | experimental | stable | — | stable | — | — |
| Index files | stable | stable | stable | experimental | stable | — | stable | — | — |
| Conditional requests | stable | stable | stable | experimental | stable | — | stable | — | — |
| Range requests | stable | stable | stable | experimental | stable | — | stable | — | — |
| File streaming | stable | stable | stable | experimental | stable | — | stable | stable | — |
| Generic byte responses | — | stable | stable | — | stable | — | — | stable | — |
| Duplicate headers | — | stable | stable | experimental | stable | — | stable | stable | — |
| Callback handlers | — | — | — | — | stable | — | — | stable | — |
| Existing-listener support | planned | planned | implemented | experimental | Rust-only | — | planned | planned | — |
| Lifecycle methods (wait_ready, shutdown, force_shutdown, wait, state) | — | — | — | experimental | stable | — | — | stable | — |
| Graceful shutdown | stable | — | — | experimental | stable | — | stable | stable | — |
| Observability hooks | minimal | minimal | planned | minimal | minimal | — | minimal | minimal | — |
| Redirects | — | — | — | — | — | — | — | — | — |
| Retries | — | — | — | — | — | — | — | — | — |
| Cookies | — | — | — | — | — | — | — | — | — |
| Proxies | — | — | — | — | — | — | — | — | — |
| Decompression | — | — | — | — | — | — | — | — | — |
| ASGI/WSGI adapters | — | — | — | — | — | — | — | — | — |
| Windows reparse-point hardening | — | — | — | — | — | — | — | — | — |

Rows with no annotation in any column are **intentionally unsupported** (empty
cell = not applicable to that surface). The explicitly labeled rows at the
bottom — redirects, retries, cookies, proxies, decompression, ASGI/WSGI,
Windows reparse-point hardening — are intentionally unsupported or
platform-limited as noted.

## Platform support

| Platform | Status | Notes |
|---|---|---|
| Linux x86_64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile/reparse hardening. |
| Linux aarch64 | supported-hardened | Same as Linux x86_64. |
| macOS arm64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile hardening. |
| macOS x86_64 | supported-hardened | Same as macOS arm64. |
| Windows x86_64 | supported-functional | Parser-level checks only (reserved names, ADS, drive prefixes, backslash). Filesystem-level hardening against reparse points and NTFS junctions is **deferred**. Windows is explicitly a trusted/local-use platform, not hardened for untrusted mutable public roots. |

## Notes

- **Follow-symlinks mode** is weaker than descriptor-relative traversal. On
  Unix with safe defaults, symlink denial is descriptor-relative — each path
  component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with
  `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is
  refused rather than followed. Follow-symlinks mode falls back to
  component-wise `symlink_metadata` checks and is explicitly outside the
  descriptor-relative hardening guarantee.

- **Windows reparse-point hardening** is deferred. Windows is functional with
  parser-level checks only. Do not use with untrusted public content on Windows.

- **Python wheels** are CPython 3.14 only (`>=3.14,<3.15`) on the Linux,
  macOS, and Windows wheel matrix. The wheel bundles the platform-native CLI
  binary.

- **Client is experimental/buffered only.** `HttpClient` buffers the complete
  response body in memory. No connection pooling, no redirect following, no
  streaming. TLS verification uses `webpki-roots`. The client is feature-gated
  behind `client` (and optionally `client-tls`) in Rust, and exposed as
  experimental Python bindings.

- **Runtime service boundary is experimental.** `eggserve-core::server` provides
  a transport-owning runtime (`Server`, `Service` trait, `StaticService`) for
  embedding. Its API is subject to change without notice. It is not covered by
  the stable API contract.
