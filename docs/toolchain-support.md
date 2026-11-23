# Toolchain and Language Support Policy

This document defines the supported language versions, Rust toolchain requirements, and platform targets for eggserve. It is the normative reference for toolchain compatibility; the capability matrix ([library-capability-matrix.md](library-capability-matrix.md)) and release contract ([release-contract.md](release-contract.md)) should be consulted for feature-level details.

## Rust

### Edition and Resolver

All workspace crates use Rust edition **2021** with workspace resolver **v2**.

### MSRV Policy

The minimum supported Rust version (MSRV) is **1.89** for the workspace's
current 0.2 line. Routine CI tests stable and also runs
`cargo +1.89 check --workspace --all-targets`; protocol feature additions must
retain this check or update the MSRV deliberately in a dedicated plan.
The MSRV includes the patched `time` release required by the
current RustSec advisory database. There is no backward-compatibility
guarantee for compilers older than the MSRV.

Release wheel builds use Rust **1.98.1** exactly. This is intentionally
separate from the floating `stable` compatibility lanes, so a stable compiler
patch update cannot silently change a release build.

### Supported Targets

| Target | Status | Notes |
|--------|--------|-------|
| `x86_64-unknown-linux-gnu` | Supported | Routine CI |
| `aarch64-unknown-linux-gnu` | Supported | Cross-compiled for release |
| `armv7-unknown-linux-gnueabihf` | Supported | Cross-compiled for release |
| `x86_64-unknown-linux-musl` | Supported | Cross-compiled for release |
| `aarch64-unknown-linux-musl` | Supported | Cross-compiled for release |
| `armv7-unknown-linux-musleabihf` | Supported | Cross-compiled for release (Plan 265) |
| `x86_64-apple-darwin` | Supported | Release matrix |
| `aarch64-apple-darwin` | Supported | Release matrix |
| `x86_64-pc-windows-msvc` | Supported | Release matrix |
| `aarch64-pc-windows-msvc` | Supported | Cross-compiled or native for release |

Other targets may compile but are not tested in CI and are not supported.

### Routine CI Compile Coverage

Plan 312 adds **compile-only** coverage for the Windows and macOS hosts to
routine CI, on every push and pull request. The CI job is `platform-compile`
in `.github/workflows/ci.yml` and runs on native hosted runners
(`windows-latest`, `macos-latest`).

| Platform | Runner | What is compiled | What is not |
|----------|--------|------------------|--------------|
| Windows | `windows-latest` | `cargo check --workspace --all-targets`; the same with `--features http2,tls` and `--features http3,tls`; the excluded Python crate with `--locked` | The Windows test suites are not run |
| macOS | `macos-latest` | The same four lanes | The macOS test suites are not run |

Native hosted runners are used deliberately. The Windows confinement
implementation (`crates/eggserve-static/src/fs/windows.rs`) is gated
`#[cfg(windows)]`, and a Linux-hosted cross-check either skips that module
outright or builds a different ABI flavor, which looks like coverage without
being coverage.

Three limits are deliberate and must not be read as more than they are:

- **This is compile coverage, not runtime qualification.** Runtime evidence
  for Windows and macOS still comes only from the manual
  `platform-qualification.yml` workflow and the release wheel gates.
- **It does not change the Windows security posture.** Windows remains
  `supported-functional` (trusted/local-content only) in the classification
  table below and in `docs/security-policy.md` and `docs/non-goals.md`.
- **It does not run tests.** Running the Windows or macOS test suites in
  routine CI is a separate cost decision and needs its own plan.

A local equivalent is available for developers who want it:

```sh
EGGSERVE_PLATFORM_CHECK=1 ./scripts/verify.sh fast
```

It compiles the confinement authority (`eggserve-static`) for
`x86_64-pc-windows-gnu` and `aarch64-apple-darwin`, skipping — visibly, never
silently — any target whose standard library is not installed. The scope is
deliberately narrow: a whole-workspace cross-check additionally needs a
target-native C toolchain for `ring` and `aws-lc-sys`.

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
| GIL-enabled CPython 3.11–3.15 (`>=3.11`) | Supported (one `cp311-abi3` wheel per platform; Plan 264) |
| CPython < 3.11 | Unsupported |
| PyPy | Unsupported |
| Free-threaded CPython 3.14t (`cp314-cp314t`) | Beta — Linux x86_64 (glibc), macOS arm64, Windows x86_64 only (Plan 326) |

### ABI

eggserve uses the Python stable ABI (`abi3`). One `cp311-abi3` wheel per
OS/architecture serves all supported GIL-enabled CPython minor versions
(3.11, 3.12, 3.13, 3.14, 3.15). The minimum supported version is CPython
3.11; there is no artificial upper bound below 3.15. Wheels are built
against the CPython 3.11 interpreter baseline, and the release proves the
same Linux x86_64 wheel bytes on every supported minor (build-once,
test-many; see `docs/release-process.md`). No per-minor `cp312-cp312`
artifacts are produced.

Free-threaded CPython 3.14t adds an explicit second family:
`cp314-cp314t` wheels on Linux x86_64 (glibc), macOS arm64, and Windows
x86_64 only, with support tier **Beta** until the Plan 326 promotion gate.
The GIL `cp311-abi3` family is unchanged. CPython 3.13t/3.15t, `abi3t`
(PEP 803), PyPy, ARMv7, Windows arm64, and musl free-threaded builds are
not supported; this line does not use `abi3t`.

### Build System

Wheels are built with **maturin** (`>=1.14.1, <2.0`, matching the pinned
1.14.1 release pipeline and the `abi3t`-capable backend). The build backend
is declared in `pyproject.toml`.

### PyO3 Version

The Python bindings use PyO3 **0.29.3** (resolved; the manifest keeps a
`0.29.2` caret requirement) with the `extension-module` and `abi3-py311`
features. Since PyO3 0.28, `#[pymodule]` without `gil_used` declares
`Py_MOD_GIL_NOT_USED`; the crate now states `gil_used = false` explicitly
for the CPython 3.14t Beta family. `abi3-py311` is ignored on a free-threaded
interpreter, where PyO3 builds version-specifically. The excluded Python
crate has its own lockfile, which is audited and checked against the shared
dependency policy in CI.

### Wheel Matrix

The canonical wheel-target authority is `release/wheel-matrix.toml`. The
tables below derive from it; the release workflow build matrix and
`scripts/check-release-wheel-set.py` consume the same file, so no second
platform list may drift.

| Platform | Wheel Target |
|----------|-------------|
| Linux x86_64 (glibc) | `manylinux_2_17_x86_64` |
| Linux aarch64 (glibc) | `manylinux_2_17_aarch64` |
| Linux armv7 (glibc) | `manylinux_2_17_armv7l` |
| Linux x86_64 (musl) | `musllinux_1_2_x86_64` |
| Linux aarch64 (musl) | `musllinux_1_2_aarch64` |
| Linux armv7 (musl) | `musllinux_1_2_armv7l` |
| macOS arm64 | `macosx_11_0_arm64` |
| macOS x86_64 | `macosx_11_0_x86_64` |
| Windows x86_64 | `win_amd64` |
| Windows arm64 | `win_arm64` |

All GIL wheels are `cp311-abi3`, compatible with GIL-enabled CPython
3.11–3.15. One wheel per platform serves all supported CPython minor
versions, built against the minimum ABI baseline (CPython 3.11) rather than a
newer interpreter. The free-threaded family adds `cp314-cp314t` wheels on
the three targets below; the matrix has 13 required entries on 10 platform
tags, and the public free-threaded tier is Beta.

| Free-threaded target | Wheel Target | Binary interpreter |
|---|---|---|
| Linux x86_64 (glibc) | `manylinux_2_17_x86_64` | `/opt/python/cp314-cp314t/bin/python` |
| macOS arm64 | `macosx_11_0_arm64` | `python3.14t` |
| Windows x86_64 | `win_amd64` | `python3.14t` |

The GIL family keeps the unchanged build-once/test-many ABI proof; the
free-threaded family is built and smoked against its own 3.14t interpreter
and must leave the GIL disabled on import.

Extended candidates (not supported; promotion needs build, binary-install,
and smoke evidence with no dependency-policy exception): manylinux i686,
musllinux i686, Windows x86 (`win32`). See `release/wheel-matrix.toml` and
`release/plan-267-wheel-feasibility.md`.

The wheel packages the platform-native extension, which includes the CLI entry
point. Users do not need a separate Rust installation to use the CLI from a
wheel, and no standalone executable is placed in the package's `bin/`
directory.

## Platform Security Classification

| Platform | Classification | Hardening |
|----------|---------------|-----------|
| Linux x86_64 (glibc) | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile/reparse hardening. Pinned root identity. |
| Linux aarch64 (glibc) | supported-hardened | Same as Linux x86_64. Pre-publish proof is native ARM64 Ubuntu (`qualify-aarch64-glibc`, manylinux wheel only; never a musl install on glibc). |
| Linux armv7 (glibc) | supported-hardened | Same as Linux x86_64. Matching ARMv7 glibc userspace via QEMU (`sh -c`). |
| Linux x86_64 (musl) | supported-hardened | Same as Linux x86_64. musl libc uses the same descriptor-relative path. Pre-publish composition gate only; runtime proof is Alpine post-publication smoke. |
| Linux aarch64 (musl) | supported-hardened | Same as Linux x86_64 (musl). Pre-publish proof is a native ARM64 Alpine container (`qualify-aarch64-musl`, musllinux wheel only). |
| Linux armv7 (musl) | supported-hardened | Same as Linux x86_64 (musl). Matching ARMv7 musl userspace via QEMU (`sh -c`; minimal Alpine has no bash). |
| macOS arm64 | supported-hardened | Descriptor-relative traversal via `statat` + `openat`. Full symlink/dotfile hardening. Pinned root identity. |
| macOS x86_64 | supported-hardened | Same as macOS arm64. |
| Windows x86_64 | supported-functional | Handle-relative confinement and manual qualification are complete for the executed classes. Two open-descendant root-rename cases remain explicitly skipped because NTFS rejects that external path operation; Windows remains trusted/local-content only. |
| Windows arm64 | supported-functional | Same as Windows x86_64. The `win_arm64` artifact is cross-built and executed natively on the Windows ARM64 hosted runner as a required pre-publication gate (`qualify-windows-arm64` blocks aggregation; plus post-publication smoke); support remains functional (trusted/local-content only), not hardened. |

### Qualification evidence tiers

Wheel claims distinguish four evidence levels; a target is called supported
only after a binary-only install and runtime smoke succeed on a compatible
runtime (never from `cargo check --target` alone):

- **build support** — the dependency closure compiles for the target with no
  policy exception.
- **emulated runtime qualification** — the wheel executes under a matching
  userspace via QEMU (ARMv7 glibc/musl release gates).
- **native hosted qualification** — the wheel executes on a hosted native
  runner (x86_64, AArch64 Linux, macOS, Windows x86_64/ARM64).
- **real-device qualification** — optional maintainer-run proof on physical
  SBC hardware via `scripts/qualify-python-wheel-target.sh` (rootless;
  Raspberry Pi / Le Potato-class userspaces, never board-specific kernel
  claims). Volunteer hardware is never a mandatory per-PR security boundary.

### Classification Definitions

- **supported-hardened**: Full security hardening is active. Descriptor-relative traversal on Unix provides TOCTOU-resistant symlink denial. The serving root is pinned at startup (`PinnedRoot`), so renaming or replacing the configured pathname does not redirect the running server. These platforms are suitable for serving untrusted content with safe defaults.
- **supported-functional**: The server is functional and manually qualified for the executed platform classes, but the platform is not promoted to hardened public-content status. Windows has handle-relative confinement, with two skipped open-descendant root-rename cases caused by NTFS path-rename semantics. These platforms are suitable only for trusted local content.

See [security-policy.md](security-policy.md) and [non-goals.md](non-goals.md) for the full Windows hardening statement and deferred scope.

## Toolchain Requirements for Development

| Tool | Required Version | Purpose |
|------|-----------------|---------|
| Rust stable | Current stable | Building all crates, running tests |
| Python | 3.11–3.15 (GIL-enabled, `cp311-abi3` wheel); 3.14t (Beta, `cp314-cp314t` on three targets) | Wheel builds, Python tests |
| maturin | `>=1.14.1, <2.0` | Python wheel builds |
| bash | Any POSIX-compatible | CI and local scripts (`scripts/`) |
