# Toolchain and Language Support Policy

This document defines the supported language versions, Rust toolchain requirements, and platform targets for eggserve. It is the normative reference for toolchain compatibility; the capability matrix ([library-capability-matrix.md](library-capability-matrix.md)) and release contract ([release-contract.md](release-contract.md)) should be consulted for feature-level details.

## Rust

### Edition and Resolver

All workspace crates use Rust edition **2021** with workspace resolver **v2**.

### MSRV Policy

There is no formal minimum supported Rust version. eggserve tracks the current stable Rust toolchain. Newer stable releases may be required at any time without advance notice. There is no backward-compatibility guarantee for older compilers.

### Supported Targets

| Target | Status |
|--------|--------|
| `x86_64-unknown-linux-gnu` | Supported |
| `aarch64-unknown-linux-gnu` | Supported |
| `x86_64-apple-darwin` | Supported |
| `aarch64-apple-darwin` | Supported |
| `x86_64-pc-windows-msvc` | Supported |

Other targets may compile but are not tested in CI and are not supported.

### Feature Flags

| Feature | Crate | Default | Description |
|---------|-------|---------|-------------|
| (none) | `eggserve-core` | Yes | Core server and primitives |
| `python-bindings-internal` | `eggserve-core` | No | `ResolvedFile` extraction methods for Python bindings |
| `tls` | `eggserve-bin` | No | TLS server via rustls |

## Python

### Supported Versions

| Implementation | Status |
|----------------|--------|
| CPython 3.11+ (`>=3.11`) | Supported (abi3 stable ABI) |
| CPython < 3.11 | Unsupported |
| PyPy | Unsupported |
| Free-threaded CPython (3.13t, 3.14t) | Unsupported |

### ABI

eggserve uses the Python stable ABI (`abi3`). One abi3 wheel per OS/architecture
serves all supported CPython minor versions (3.11+). The minimum supported
version is CPython 3.11.

### Build System

Wheels are built with **maturin** (latest stable, `>=1.0, <2.0`). The build backend is declared in `pyproject.toml`.

### PyO3 Version

The Python bindings use PyO3 **0.24** with the `extension-module` and `abi3-py311` features.

### Wheel Matrix

| Platform | Wheel Target |
|----------|-------------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` |
| macOS arm64 | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

The wheel bundles the platform-native `eggserve` CLI binary in the `bin/` package directory. Users do not need a separate Rust installation to use the CLI from a wheel.

## Platform Security Classification

| Platform | Classification | Hardening |
|----------|---------------|-----------|
| Linux x86_64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile/reparse hardening. Pinned root identity. |
| Linux aarch64 | supported-hardened | Same as Linux x86_64. |
| macOS arm64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile hardening. Pinned root identity. |
| macOS x86_64 | supported-hardened | Same as macOS arm64. |
| Windows x86_64 | supported-functional | Handle-relative confinement implemented (Plans 084–086): directory-handle retention, child resolution, directory enumeration via `NtQueryDirectoryFile`. Adversarial qualification scaffold established (114 tests). Independent adversarial review is incomplete. Windows remains functional-only until that review is completed. |

### Classification Definitions

- **supported-hardened**: Full security hardening is active. Descriptor-relative traversal on Unix provides TOCTOU-resistant symlink denial. The serving root is pinned at startup (`PinnedRoot`), so renaming or replacing the configured pathname does not redirect the running server. These platforms are suitable for serving untrusted content with safe defaults.
- **supported-functional**: The server is functional and tested in CI, but the platform has not yet passed independent safety review for production hardened status. Windows has handle-relative confinement implemented but independent adversarial review is incomplete. These platforms are suitable only for trusted local content.

See [security-policy.md](security-policy.md) and [non-goals.md](non-goals.md) for the full Windows hardening statement and deferred scope.

## Toolchain Requirements for Development

| Tool | Required Version | Purpose |
|------|-----------------|---------|
| Rust stable | Current stable | Building all crates, running tests |
| Python | 3.11+ (abi3 wheel) | Wheel builds, Python tests |
| maturin | `>=1.0, <2.0` | Python wheel builds |
| bash | Any POSIX-compatible | CI and local scripts (`scripts/`) |
