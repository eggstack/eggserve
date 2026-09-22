# Plan 263 — Python wheel distribution expansion program

## Purpose

Expand EggServe's PyPI distribution from the current nine-target release design
into a broader, evidence-backed wheel program optimized for two goals:

1. one stable-ABI artifact per platform that works across normal GIL-enabled
   CPython 3.11 through 3.15; and
2. substantially broader platform coverage, especially Raspberry Pi,
   Libre Computer / Le Potato, ARMv7, AArch64, Alpine/musl, macOS, and Windows.

This is a packaging, CI, qualification, and release-contract program. It must
not change the Python API, Rust API, HTTP behavior, filesystem policy, protocol
support tiers, or dependency ownership merely to obtain more wheel tags.

Planning baseline:

```text
09ada539 docs: simplify readme around python/rust quick starts
```

## Current state and findings

The implementation already has the right normal-CPython ABI strategy:

- `crates/eggserve-python/Cargo.toml` uses PyO3 0.29.2 with
  `extension-module`, `abi3-py311`, and `generate-import-lib`.
- `crates/eggserve-python/pyproject.toml` declares Python `>=3.11`.
- release wheels are intended to be `cp311-abi3`, so one wheel per
  OS/architecture serves normal CPython 3.11+ rather than rebuilding the
  extension for every Python minor.
- `.github/workflows/release.yml` already describes nine targets:
  manylinux x86_64/aarch64/armv7, musllinux x86_64/aarch64,
  macOS x86_64/arm64, and Windows x86_64/arm64.
- `scripts/check-release-wheel-set.py` hard-codes that exact nine-target set,
  so the validator currently rejects any additive platform wheel as
  "unexpected" until its authority model is changed.
- routine Python CI and release ABI proof stop at CPython 3.14.
- ARMv7 has a glibc wheel but no musllinux ARMv7 wheel.
- AArch64 and Windows ARM64 are distributed targets but do not receive the
  same level of native installed-wheel qualification as the primary hosts.

The program therefore expands breadth without abandoning `abi3-py311`.

## Product decisions

### Normal CPython ABI

Keep `abi3-py311` as the supported normal-CPython ABI baseline.

Do **not** generate separate `cp311-cp311`, `cp312-cp312`,
`cp313-cp313`, `cp314-cp314`, and `cp315-cp315` wheels for the same
platform. That would multiply artifacts and release work without adding useful
coverage while the extension remains compatible with the stable ABI.

CPython 3.15 support is a qualification/metadata task under Plan 264, not a new
normal-ABI wheel family.

### Core ARM/SBC coverage

The core release contract should cover both common ARM userspace families:

- 64-bit glibc AArch64: Raspberry Pi 3/4/5/Zero 2 W class devices, Le Potato,
  and similar ARM64 Debian/Ubuntu/Armbian installations;
- 32-bit glibc ARMv7: older Pi-class and 32-bit ARM userspaces;
- 64-bit musl AArch64: Alpine ARM64;
- 32-bit musl ARMv7: Alpine/embedded ARMv7.

The missing core artifact is `musllinux_1_2_armv7l`.

ARMv6 (original Pi / Pi Zero-class 32-bit ARMv6 userspace) is **not** promoted
by this program without a separate successful portability qualification. It is
handled as a gated feasibility item in Plan 267.

### Legacy x86 breadth

Linux i686 glibc, Linux i686 musl, and Windows x86 are reasonable extended
compatibility candidates. Plan 265 may add them to the required release set
only after they build, install, and pass the release smoke suite without a
dependency-policy exception or product-code fork.

### Long-tail architectures and free-threaded Python

PPC64LE, s390x, RISC-V, ARMv6, and free-threaded Python are not silently
promoted. Plan 267 records explicit feasibility gates. A target that cannot
build the existing dependency closure, especially the rustls/ring TLS closure,
does not justify weakening security, changing TLS providers, or adding
architecture-specific product behavior inside this campaign.

## Plan sequence

```text
263  Python wheel distribution expansion program
 |
 +--264  CPython 3.11–3.15 stable-ABI qualification
 |
 +--265  Declarative wheel-matrix authority + ARM/SBC/legacy-x86 expansion
 |
 +--266  Native ARM64, Windows ARM64, ARMv7, and real-SBC qualification
 |
 `--267  Free-threaded + long-tail architecture feasibility gate
```

Plans 264–266 are the primary implementation path. Plan 267 is investigative
and may close with NO-GO/DEFERRED decisions for individual targets.

## Constraints

- No Python public API change.
- No Rust public API change.
- No HTTP behavior, path-confinement, safe-default, or protocol-tier change.
- Preserve the direct `rustls` `0.23.45` caret floor and existing
  supply-chain policy.
- Preserve `abi3-py311` for normal CPython unless a later plan explicitly
  proves a different ABI strategy is required.
- No source-build fallback may be presented as prebuilt-wheel support.
- No target is called supported until a binary-only install and runtime smoke
  have succeeded on a compatible runtime environment.
- Keep release publishing manual/OIDC-controlled; target expansion does not
  authorize auto-publishing from pushes or tags.
- Do not add cibuildwheel merely for matrix breadth if the existing
  maturin-action pipeline can express and verify the target cleanly.
- Keep `eggserve-python` workspace-excluded with its independent lockfile.

## Target end state

Primary normal-ABI wheel set after Plans 264–266:

```text
cp311-abi3-manylinux_2_17_x86_64
cp311-abi3-manylinux_2_17_aarch64
cp311-abi3-manylinux_2_17_armv7l
cp311-abi3-musllinux_1_2_x86_64
cp311-abi3-musllinux_1_2_aarch64
cp311-abi3-musllinux_1_2_armv7l
cp311-abi3-macosx_*_x86_64
cp311-abi3-macosx_*_arm64
cp311-abi3-win_amd64
cp311-abi3-win_arm64
```

The exact macOS deployment tag remains whatever the release build actually
emits and the declarative matrix records; documentation and validators must not
disagree about `10_12` vs `11_0`.

Extended candidates, promoted only with evidence:

```text
manylinux i686
musllinux i686
win32
```

Every normal-ABI target must remain usable on CPython 3.11–3.15 through the
same `cp311-abi3` wheel.

## Program acceptance

- Plan 264 proves one built stable-ABI wheel across CPython 3.11–3.15.
- Plan 265 replaces the hard-coded nine-wheel validator authority with one
  declarative matrix and adds ARMv7 musl; feasible legacy-x86 targets are
  promoted only with evidence.
- Plan 266 supplies native/representative runtime evidence for the ARM and
  Windows ARM64 claims and a repeatable real-SBC qualification path.
- release docs, toolchain docs, README installation claims, security support
  wording, and AGENTS/skill wheel notes match executable configuration.
- aggregate release validation remains fail-closed: required targets missing,
  duplicated, mis-tagged, or generic `linux_*` wheels fail the release.
- no change to EggServe behavior/API is required for this campaign.
- Plan 267 records explicit GO/NO-GO/DEFER outcomes for free-threaded and
  long-tail targets rather than leaving ambiguous support claims.
