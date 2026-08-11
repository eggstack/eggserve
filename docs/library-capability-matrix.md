# Library Capability Matrix

This document maps every eggserve capability across all surfaces and indicates
its status using a constrained vocabulary.

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
| **Runtime experimental** | `eggserve-core::server` — transport-owning runtime: `Server`, `Service` trait, `StaticService`. |
| **Python stable** | `eggserve.server` compatibility classes and `serve_directory`; advanced wrappers are under `eggserve.lowlevel`, subprocess helpers under `eggserve.subprocess`. |
| **Python experimental** | No default Python client surface. The internal callback engine and native bridge types are not supported imports. |
| **Built-in static service** | The static service used by CLI and Python Server (GET/HEAD only, body rejection, path confinement, conditional/range responses). |
| **Generic callback server** | Python `Server` with a user-provided handler callback; bounded concurrency via `max_python_callbacks`. |

## Capability Matrix

| Capability | CLI | Rust stable | Runtime experimental | Python stable | Python experimental | Built-in static service | Generic callback server |
|---|---|---|---|---|---|---|---|
| Bind/listen lifecycle | stable | — | experimental | stable | — | stable | stable |
| Plaintext HTTP/1.x | stable | — | experimental | — | — | stable | stable |
| TLS server | stable | — | — | — | — | — | — |
| GET/HEAD static serving | stable | stable | experimental | stable | — | stable | — |
| Request-target validation | stable | stable | experimental | stable | — | stable | — |
| Request-body policy | stable | stable | experimental | stable | — | stable | stable |
| Canonical request types | — | stable | experimental | stable | stable | — | — |
| Canonical response types | — | stable | experimental | stable | stable | — | — |
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
| Generic byte responses | — | stable | — | stable | — | — | stable |
| Duplicate headers | — | stable | experimental | stable | — | stable | stable |
| Callback handlers | — | — | — | stable | — | — | stable |
| Existing-listener support | — | implemented | experimental | Rust-only | — | — | — |
| Lifecycle methods (wait_ready, shutdown, force_shutdown, wait, state) | — | — | experimental | stable | — | — | stable |
| Graceful shutdown | stable | — | experimental | stable | — | stable | stable |
| Observability hooks | minimal | minimal | minimal | minimal | — | minimal | minimal |
| Redirects | — | — | — | — | — | — | — |
| Retries | — | — | — | — | — | — | — |
| Cookies | — | — | — | — | — | — | — |
| Proxies | — | — | — | — | — | — | — |
| Decompression | — | — | — | — | — | — | — |
| ASGI/WSGI adapters | — | — | — | — | — | — | — |
| Windows reparse-point hardening | — | — | — | — | — | — | — |

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
| Windows x86_64 | supported-functional | Handle-relative confinement implemented (Plans 084–086): directory-handle retention, child resolution, directory enumeration via `NtQueryDirectoryFile`. Adversarial qualification scaffold established (114 tests). Independent adversarial review is incomplete. Windows remains functional-only until that review is completed. |

## Notes

- **Follow-symlinks mode** is weaker than descriptor-relative traversal. On
  Unix with safe defaults, symlink denial is descriptor-relative — each path
  component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with
  `openat(O_NOFOLLOW)`, so a symlink swapped into place between the two is
  refused rather than followed. Follow-symlinks mode falls back to
  component-wise `symlink_metadata` checks and is explicitly outside the
  descriptor-relative hardening guarantee.

- **Windows handle-relative confinement** is implemented (Plans 084–086) but
  awaiting independent safety review. Windows is functional-only. Do not use
  with untrusted public content on Windows until independent adversarial review is completed.

- **Python wheels** are CPython 3.11+ with abi3 stable ABI (`>=3.11`) on the Linux,
  macOS, and Windows wheel matrix. The wheel bundles the platform-native CLI
  binary.

- **Runtime service boundary is experimental.** `eggserve-core::server` provides
  a transport-owning runtime (`Server`, `Service` trait, `StaticService`) for
  embedding. Its API is subject to change without notice. It is not covered by
  the stable API contract.
