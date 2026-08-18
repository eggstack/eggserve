# Plan 138 — PyPI Wide Binary Distribution Roadmap

## Status

**READY FOR HANDOFF — 2026-08-18.**

Reviewed repository baseline:

```text
main = 2b90abe0b118c03ea23cc37d63a4fe35b174bfdd
```

This roadmap is a deliberately narrow release-distribution track. It is authorized to supersede the older release-scope restrictions in Plans 117 and 126 only where necessary to provide fast `pip install eggserve` installation from prebuilt wheels across mainstream x86 and ARM systems.

It does **not** reopen EggServe's product scope, routine CI strategy, crates.io release process, HTTP feature set, Python API design, or deployment model.

---

# 1. Why this roadmap exists

EggServe already has the difficult pieces needed for a compact Python binary distribution strategy:

- the Python package is built with Maturin;
- the native module uses PyO3 `abi3-py311`;
- one platform wheel can therefore serve the supported CPython 3.11+ range instead of requiring one wheel per Python minor version;
- the wheel contains the Python facade plus the `_native` extension rather than a second standalone server binary;
- installed-wheel smoke tests already prove imports, console-script invocation, `python -m eggserve`, and real fixture serving;
- the current release workflow is manually dispatched, builds platform artifacts, and does not affect routine pull-request CI.

The current release workflow is nevertheless incomplete for a public PyPI binary distribution:

1. it builds only Linux x86_64, macOS arm64, and Windows x86_64;
2. the Linux wheel is built on a normal Ubuntu runner and repaired afterward instead of being intentionally built against a portable manylinux baseline;
3. there is no aarch64 Linux wheel for Raspberry Pi 3/4/5-class 64-bit SBCs or ARM servers;
4. there is no armv7 Linux wheel for 32-bit ARM SBC installations;
5. there are no musllinux wheels for Alpine/musl environments;
6. there is no macOS x86_64 or Windows arm64 release artifact;
7. release artifacts are uploaded to GitHub Actions but never aggregated and published to PyPI;
8. Python-facing release versions are currently not synchronized with the Rust workspace version.

The objective of this roadmap is to close those distribution gaps without recreating the large release/verification apparatus that earlier consolidation work intentionally removed.

---

# 2. Product and release contract to preserve

The distribution contract after this track should be simple:

```text
pip install eggserve
    -> downloads a prebuilt wheel on supported mainstream platforms
    -> installs the Python package and PyO3 native extension
    -> provides the `eggserve` console script
    -> provides `python -m eggserve`
    -> requires no Rust toolchain on the destination machine
```

The Python wheel continues to contain the extension-linked CLI implementation. Do not reintroduce a second copied executable under `eggserve/bin/`.

The release decision remains manual:

```text
maintainer manually dispatches release workflow
    -> workflow builds and qualifies complete wheel set
    -> publication remains gated by an explicit production environment approval
    -> one publication job uploads the complete set
```

A merge, push, or tag must not automatically publish to PyPI.

Routine CI remains small. Wide binary construction is a **release-time workflow**, not a pull-request matrix.

---

# 3. Required platform support matrix

## Tier 1 — required release wheels

The first complete PyPI binary release must target the following platform families.

| Platform family | Required wheel target | Primary users |
|---|---|---|
| glibc Linux x86_64 | `manylinux_2_17_x86_64` | normal Linux desktops, servers, containers |
| glibc Linux aarch64 | `manylinux_2_17_aarch64` | Raspberry Pi 3/4/5 64-bit, ARM SBCs, ARM servers |
| glibc Linux armv7 | `manylinux_2_17_armv7l` | 32-bit Raspberry Pi OS and ARMv7 SBCs |
| musl Linux x86_64 | `musllinux_1_2_x86_64` | Alpine/musl x86_64 |
| musl Linux aarch64 | `musllinux_1_2_aarch64` | Alpine/musl ARM64 SBCs and containers |
| macOS x86_64 | normal Maturin macOS x86_64 tag | Intel Macs |
| macOS arm64 | normal Maturin macOS arm64 tag | Apple Silicon Macs |
| Windows x86_64 | `win_amd64` | standard 64-bit Windows |
| Windows arm64 | `win_arm64` | Windows on ARM |

The exact macOS deployment tag must be intentionally selected and documented during implementation rather than inferred from whichever hosted runner happens to build the wheel.

## Tier 2 — desired extended SBC wheel

Evaluate and add:

```text
musllinux_1_2_armv7l
```

if Maturin's current cross/container path plus runtime emulation can produce and execute the wheel reliably without introducing a self-hosted runner or substantial bespoke infrastructure.

This is valuable for unusually small Alpine/musl ARMv7 deployments, but it must not block the first complete Tier 1 release if its qualification cost becomes disproportionate.

## Explicitly out of scope for this track

Do not add release targets merely for theoretical completeness:

```text
Linux i686 / x86
Windows x86 / i686
ARMv6 / Raspberry Pi 1 / original Pi Zero
PowerPC
s390x
RISC-V
Android
iOS
PyPy
CPython free-threaded builds
```

A future target should require demonstrated user demand and a maintainable build/qualification path.

---

# 4. Python ABI strategy

The existing `abi3-py311` configuration is the mechanism that keeps this release matrix tractable.

Preserve:

```text
minimum CPython ABI = 3.11
wheel ABI           = abi3
requires-python     = >=3.11
```

Do **not** create separate wheels for CPython 3.11, 3.12, 3.13, and 3.14 on every platform.

The desired matrix scales approximately with platform/architecture count, not:

```text
platforms × architectures × Python minor versions
```

Release qualification should prove the stable-ABI claim using at least:

- the minimum supported CPython version; and
- the newest supported/current CPython version used by the project.

That compatibility proof does not require building separate artifacts for each interpreter.

---

# 5. Linux portability policy

A Linux wheel is not considered release-ready merely because it was built successfully on `ubuntu-latest` and subsequently passed an audit/repair command.

The implementation must intentionally build against the declared portability baseline.

For glibc Linux, prefer:

```text
manylinux 2.17
```

for x86_64, aarch64, and armv7. This gives a conservative compatibility floor appropriate for broad Linux and SBC deployment while remaining compatible with the project's current Rust toolchain expectations.

For musl Linux, prefer:

```text
musllinux 1.2
```

for x86_64 and aarch64, with armv7 evaluated as Tier 2.

Use Maturin's maintained manylinux/musllinux build mechanisms rather than maintaining project-specific sysroots or custom Docker images unless a concrete dependency makes that unavoidable.

`--compatibility pypi` or the current equivalent must be part of release artifact validation so an accidentally non-PyPI-compatible platform tag fails before publication.

---

# 6. Native versus cross-built qualification

Prefer native execution where GitHub-hosted infrastructure exists and is stable enough for release use:

```text
Linux aarch64
macOS arm64
macOS x86_64
Windows x86_64
Windows arm64 when hosted ARM64 runner availability is acceptable
```

Cross-building is acceptable where it materially reduces infrastructure, especially:

```text
Linux armv7
musllinux targets
```

However, cross-building alone is not qualification. Every published Tier 1 wheel must have an execution proof appropriate to the target:

- native install/smoke on the target architecture where practical; or
- QEMU/container execution for targets without hosted native runners.

Do not add permanent self-hosted Raspberry Pi or SBC runners as part of this track. If armv7 cannot be qualified reliably without one, document the blocker and reassess the target rather than introducing a maintenance service.

---

# 7. Native dependency qualification

The Python wheel has no Python runtime dependencies, which is favorable for binary portability. The native extension compiles EggServe's Rust dependency graph into the wheel.

The release implementation must specifically exercise targets affected by native/assembly dependencies rather than assuming successful x86_64 builds imply ARM support.

At minimum prove:

```text
rustls/ring-backed TLS compiles and loads on:
  Linux aarch64
  Linux armv7
  musllinux aarch64
  Windows arm64
```

A compile-only proof is insufficient for Tier 1 if the resulting extension can be executed in a native or emulated environment.

Do not replace `ring`, rustls, or another dependency solely to make the matrix symmetrical unless the dependency actually blocks a required supported target.

---

# 8. Release workflow architecture

The release workflow should have a strict build-before-publish shape:

```text
workflow_dispatch
      |
      v
release preflight
      |
      v
wide wheel build matrix
      |
      v
per-wheel qualification
      |
      v
upload workflow artifacts
      |
      v
collect complete release set
      |
      v
validate names / versions / ABI / platform tags / completeness
      |
      +----> optional TestPyPI qualification
      |
      v
GitHub `pypi` environment approval
      |
      v
single Trusted Publishing job
      |
      v
post-publication `--only-binary` install smoke
```

No matrix build job may independently publish its wheel.

The publication job receives only the already-built and already-qualified release artifact set.

This ordering is mandatory because package-index artifacts are immutable. A failed late architecture must not leave a version partially published with only some target wheels.

---

# 9. Security and credential model

Use PyPI Trusted Publishing through GitHub Actions OIDC.

Required properties:

- no long-lived `PYPI_API_TOKEN` repository secret;
- no PyPI password stored in GitHub;
- build jobs use only read permissions required to fetch source;
- only the final PyPI publication job receives `id-token: write`;
- production publication uses a GitHub Environment named `pypi` or an equivalently explicit protected environment;
- configure required reviewers/approval in the environment so a manual workflow dispatch is not by itself sufficient to upload;
- TestPyPI, if enabled, uses an independent `testpypi` environment/publisher mapping.

Keep publication privilege structurally separated from arbitrary build steps.

Do not introduce a custom signing service. Use the provenance/attestation support already provided by the official Python packaging publication path where available.

---

# 10. Release version correctness

Wide publication must not proceed while Python and Rust release metadata disagree.

The currently reviewed repository has a concrete mismatch:

```text
root Rust workspace version        = 0.1.2
Python crate version               = 0.1.0
Python pyproject distribution      = 0.1.0
Python package __version__         = 0.1.0
```

Plan 139 must close this before the publication workflow is enabled.

The solution should remove unnecessary duplicate version ownership rather than adding another release-management framework.

The release preflight must prove that the version encoded in every wheel is the intended EggServe release version before the artifact is eligible for aggregation.

---

# 11. Wheel-set validation contract

Before a PyPI job can run, a standard-library-only validation step must inspect the collected artifacts and reject the release unless all required invariants hold.

At minimum validate:

```text
project name == eggserve
all wheels have exactly one common version
all wheels use cp311-abi3 or the equivalent expected abi3 tag
exactly one wheel exists for every required Tier 1 target
no duplicate platform target exists
no required target is missing
no unexpected generic linux_* wheel is accepted in place of manylinux/musllinux
Linux glibc wheels carry the declared manylinux baseline
a musl wheel carries the declared musllinux baseline
no wheel contains eggserve/bin/eggserve[.exe]
Python package/native extension members are present
```

The validator should consume filenames and wheel metadata directly. Avoid a new third-party Python dependency for this narrow check.

---

# 12. Test depth policy

Do not run EggServe's entire exhaustive test surface on every release architecture.

Use three levels:

### Level A — build and structural validation on every wheel

Required for every target:

```text
Maturin build succeeds
wheel platform/ABI tag is correct
wheel composition is correct
version is correct
artifact is retained
```

### Level B — installed-wheel runtime smoke on every Tier 1 target

Required on native or emulated target execution:

```text
pip install wheel succeeds
import eggserve succeeds
import eggserve._native succeeds
eggserve --help succeeds
python -m eggserve --help succeeds
release_smoke.py serves exact fixture bytes and terminates cleanly
```

### Level C — full Python compatibility suite on representative native platforms

Retain the existing full installed-wheel suite on representative native x86_64 Linux and at least one ARM64 platform. Routine CI remains the primary regression gate for the product; release jobs prove artifact portability rather than duplicating every test on every architecture.

---

# 13. Source distribution policy

An sdist is not required to satisfy the wide-binary objective.

For this track, prefer making the binary contract unambiguous:

```text
all promised mainstream platforms have wheels
unsupported platforms may build from source only if explicitly documented
```

If an sdist is published, it must **not** be used to conceal a missing required wheel during release verification. Post-publication qualification must use:

```text
pip install --only-binary=:all: eggserve==<version>
```

on representative targets so a local compiler fallback cannot make an incomplete wheel release appear healthy.

---

# 14. Documentation contract

Implementation must update at least:

```text
docs/release-process.md
docs/python-packaging.md
README installation/platform wording where applicable
```

The documentation must distinguish:

```text
routine CI
manual platform/security qualification
release-time binary wheel construction
PyPI publication approval
```

Do not claim a platform is security-hardened merely because its wheel builds and runs. For example, Windows wheel qualification remains distinct from the repository's Windows adversarial filesystem support statement.

---

# 15. Execution sequence

Implement this roadmap through the following detailed plans in order:

1. **Plan 139 — Python Release Version and Wheel Contract**
   - remove release-version drift;
   - make the abi3 build contract explicit;
   - add preflight metadata checks;
   - build against the minimum supported ABI rather than relying on CPython 3.14 forward-compatibility mode for release artifact production.

2. **Plan 140 — Wide Platform Wheel Build and Qualification**
   - implement the manylinux, musllinux, macOS, Windows, x86_64, aarch64/arm64, and armv7 matrix;
   - choose native versus cross/emulated execution per target;
   - add complete wheel-set validation.

3. **Plan 141 — PyPI Trusted Publishing and Release Aggregation**
   - collect and validate the complete wheel set;
   - configure TestPyPI/PyPI OIDC publication boundaries;
   - publish once after all builds succeed and production approval is granted;
   - perform binary-only post-publication smoke checks.

Do not merge these phases into a single opaque workflow edit. Each phase has different failure modes and should be reviewable independently.

---

# 16. Global acceptance criteria

This roadmap is complete only when all of the following are true:

- `pip install eggserve` can resolve a prebuilt wheel on every Tier 1 target;
- Linux x86_64/aarch64/armv7 wheels use an intentional manylinux 2.17 baseline;
- musllinux 1.2 wheels exist for x86_64 and aarch64;
- macOS has both x86_64 and arm64 wheels;
- Windows has both x86_64 and arm64 wheels;
- one abi3 wheel per platform target supports the declared CPython 3.11+ range without a per-Python-version matrix;
- every Tier 1 wheel has a runtime smoke proof, native where practical and emulated where necessary;
- no self-hosted SBC runner is required;
- Python/Rust distribution versions are synchronized before artifact creation;
- the complete wheel set is assembled and validated before any production upload begins;
- production PyPI publication uses Trusted Publishing/OIDC, not a long-lived token;
- only the publication job has OIDC write permission;
- release initiation and final production upload remain explicit maintainer actions;
- routine CI remains the current small product-verification workflow rather than inheriting this release matrix;
- the existing no-second-binary wheel architecture remains intact;
- documentation accurately separates artifact compatibility from platform security qualification;
- no unrelated product feature, protocol feature, or framework dependency is introduced.

---

# 17. Stop conditions

Stop and document evidence instead of expanding scope if any required target would force one of the following:

- a permanently maintained self-hosted runner fleet;
- a bespoke cross-compilation SDK owned by EggServe;
- replacement of major runtime dependencies without a demonstrated target blocker;
- a separate wheel feature matrix for TLS/non-TLS variants;
- loss of the existing Python API or security semantics;
- per-Python-minor wheel builds despite the retained abi3 contract;
- automatic publishing on every merge/tag;
- a generalized release orchestration system unrelated to the binary-wheel requirement.

If one target proves disproportionate, record the exact blocker and keep the remainder of the matrix releasable rather than turning release engineering into a new subsystem.
