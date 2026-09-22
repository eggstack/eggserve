# Plan 265 — Declarative wheel-matrix authority and ARM/SBC expansion

## Purpose

Remove the hard-coded nine-wheel release ceiling, establish one declarative
authority for wheel targets, and expand prebuilt Linux coverage for ARM/SBC
users without changing EggServe behavior.

Depends on Plans 263 and 264.

Planning baseline:

```text
09ada539 docs: simplify readme around python/rust quick starts
```

## Problem

The release workflow and `scripts/check-release-wheel-set.py` encode the
platform set independently. The validator currently treats any target outside
its hard-coded `REQUIRED_TARGETS` set as an error.

That structure makes additive wheel support unnecessarily risky: the build
matrix, aggregate validator, documentation, and support-tier text can drift.

The current primary ARM gap is `musllinux_1_2_armv7l`. The repository already
has glibc ARMv7, glibc/musl AArch64, and QEMU machinery.

## Track A — Canonical wheel matrix

Add a small machine-readable authority, preferably
`release/wheel-matrix.toml`.

Each entry should carry only release-relevant facts, for example:

- stable target ID;
- human-readable name;
- Rust target triple;
- expected wheel platform tag/family;
- GitHub runner/build host;
- maturin compatibility mode;
- build strategy: native / maturin cross-container;
- smoke strategy: native / QEMU / post-publish only;
- support tier: required / candidate / deferred;
- architecture family;
- libc family;
- notes needed by validation.

Do not put secrets, mutable versions, or complex workflow logic in this file.

Add a small parser/renderer script that:

1. validates the manifest;
2. emits the GitHub Actions JSON matrix used by the release build; and
3. exposes the required expected wheel platform tags to the aggregate
   validator.

The release workflow and `check-release-wheel-set.py` must consume the same
authority. No second manually maintained platform list remains.

## Track B — Validator behavior

Refactor `scripts/check-release-wheel-set.py` so it remains fail-closed while
allowing planned breadth:

- every enabled `required` target must be present;
- no required target may appear twice;
- wheel version must match;
- normal wheels must remain `cp311-abi3`;
- generic `linux_*` tags remain rejected for PyPI release artifacts;
- manylinux and musllinux families may not be conflated;
- unexpected targets not declared in the manifest fail;
- declared `candidate` targets may not silently count as supported;
- platform aliases such as manylinux2014 vs PEP 600 `manylinux_2_17` are
  normalized in one place;
- macOS expected deployment tags come from actual matrix policy rather than
  conflicting hard-coded documentation.

Add manifest parser self-tests for duplicate IDs, duplicate expected tags,
unknown support tiers, malformed triples, and impossible smoke strategies.

## Track C — Add ARMv7 musl

Add:

```text
Rust target:     armv7-unknown-linux-musleabihf (or the exact maturin-supported
                 target chosen after a preflight build)
Wheel platform:  musllinux_1_2_armv7l
ABI:             cp311-abi3
```

The implementation agent must confirm the exact Rust/maturin target spelling
against the pinned tool versions before editing the final manifest; do not
guess the triple from the wheel tag.

Qualification requirements:

- build with the pinned release Rust/maturin versions;
- verify wheel composition;
- install under an ARMv7 musl runtime via QEMU or native hardware;
- import native extension;
- run CLI/module help;
- run `scripts/release_smoke.py`;
- record `platform.machine()`, libc identity, Python version, and resolved
  wheel filename.

Once those pass, promote ARMv7 musl to `required`.

## Track D — Extended 32-bit x86 candidates

Investigate and, if clean, add:

- manylinux i686;
- musllinux i686;
- Windows x86 / `win32`.

These begin as `candidate`, not required.

Promotion criteria for each:

- current dependency closure compiles without policy exceptions;
- no architecture-specific product behavior or public API fork;
- binary-only install succeeds;
- release smoke passes;
- wheel tag is accepted by current packaging tooling;
- CI runtime cost is reasonable;
- documentation can state the target precisely.

If a candidate fails because the existing dependency closure does not support
it, record the blocker and leave it deferred. Do not change TLS/security
dependencies merely to increase wheel count in this plan.

## Track E — Release workflow

Refactor `.github/workflows/release.yml` to receive its build matrix from the
preflight-generated manifest output.

Preserve:

- exact Rust release compiler pin;
- pinned GitHub Action SHAs;
- manual workflow dispatch;
- OIDC Trusted Publishing only in final publish jobs;
- one aggregate validation step before publication;
- no per-matrix-job publication.

QEMU setup should be selected from manifest strategy/architecture rather than
a growing collection of string-contains conditions when practical.

The aggregate manifest should additionally record the matrix authority revision
or source commit so a published artifact set can be reconstructed.

## Track F — Documentation

Synchronize:

- `docs/toolchain-support.md`;
- `docs/release-process.md`;
- `docs/release-contract.md`;
- `SECURITY.md` platform statements where security classification changes;
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` wheel-count notes.

Do not call candidate targets supported before their promotion evidence exists.

## Verification

At minimum:

```sh
python3 scripts/check-python-release-metadata.py
python3 scripts/check-release-wheel-set.py <fixture-dir> --version <version>
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo fmt --all -- --check
```

Add fixture-level/self-tests that prove:

- old nine-target manifest passes when configured as the baseline;
- adding ARMv7 musl is accepted only when the wheel exists;
- a missing required target fails;
- an undeclared extra target fails;
- duplicate platform tags fail;
- wrong ABI tag fails;
- generic Linux tags fail.

## Acceptance

- one canonical wheel-target manifest drives build and aggregate validation;
- ARMv7 musllinux is a required, runtime-smoked `cp311-abi3` artifact;
- existing nine targets remain intact;
- feasible i686/win32 candidates are promoted only after successful
  binary-install evidence;
- documentation and validation derive from or agree with the same target set;
- no API/capability/security-policy regression.
