# Plan 140 — Wide Platform Wheel Build and Qualification

## Status

**READY FOR HANDOFF — 2026-08-18.**

Parent roadmap: Plan 138.

Dependency: Plan 139 must be complete first so every platform artifact shares a validated version and `cp311-abi3` contract.

Reviewed repository baseline:

```text
main = 2b90abe0b118c03ea23cc37d63a4fe35b174bfdd
```

This phase widens the existing manually dispatched release workflow from three platform wheels to the explicit x86/ARM, glibc/musl binary distribution contract required for practical PyPI installation, including Raspberry Pi/SBC-class systems.

---

# 1. Goal

Produce, inspect, install, and smoke-test prebuilt EggServe wheels for all required Tier 1 platform targets before any publication is possible.

Required Tier 1 set:

```text
manylinux_2_17_x86_64
manylinux_2_17_aarch64
manylinux_2_17_armv7l
musllinux_1_2_x86_64
musllinux_1_2_aarch64
macOS x86_64
macOS arm64
Windows x86_64
Windows arm64
```

Desired Tier 2 evaluation:

```text
musllinux_1_2_armv7l
```

The outcome is a release artifact set suitable for aggregation by Plan 141. This phase must not itself upload to PyPI.

---

# 2. Non-goals and hard boundaries

Do not:

- expand routine `ci.yml` into this matrix;
- build one wheel per Python minor version;
- add cibuildwheel on top of Maturin merely to express the matrix;
- introduce self-hosted SBC runners;
- build ARMv6, i686, PowerPC, s390x, RISC-V, Android, or iOS artifacts;
- create TLS/non-TLS Python wheel variants;
- create a second standalone binary inside the wheel;
- change application behavior or platform security policy to accommodate packaging;
- treat successful Windows wheel execution as proof of Windows adversarial filesystem hardening;
- add custom Docker images when maintained Maturin manylinux/musllinux images already satisfy the target;
- permit an sdist fallback to count as wheel qualification;
- publish any artifact from a matrix member.

---

# 3. Replace the current Linux portability model

## Current problem

The current Linux release job builds on `ubuntu-latest` and uses an auditwheel repair flag. That proves the extension can be compiled on the hosted runner, but it does not define the oldest glibc interface against which the binary was linked.

A repaired wheel cannot erase references to newer libc symbols that entered during compilation.

## Required model

Build glibc Linux wheels **inside an explicit manylinux 2.17 environment** or through Maturin's current maintained equivalent.

Required targets:

```text
x86_64-unknown-linux-gnu     -> manylinux_2_17_x86_64
aarch64-unknown-linux-gnu    -> manylinux_2_17_aarch64
armv7-unknown-linux-gnueabihf -> manylinux_2_17_armv7l
```

Use the current Maturin GitHub Action/cross-container mechanism rather than hand-maintaining linker/sysroot setup.

Release invocation should include the project's existing distribution profile and locked dependencies, conceptually:

```text
maturin build
  --profile dist
  --locked
  --compatibility pypi
  --out <release-dist>
```

plus target/manylinux arguments appropriate to each matrix entry.

Do not hard-code a command from this plan if the installed Maturin action's current schema provides a more direct target field. Preserve the semantics, not incidental syntax.

### Acceptance criteria

- no published Linux glibc wheel originates as a generic host-linked `linux_*` artifact;
- each glibc wheel advertises the intended manylinux 2.17 compatibility family;
- the x86_64, aarch64, and armv7 wheel tags are checked mechanically;
- `--compatibility pypi` or the maintained equivalent rejects unsupported external dependencies/tags.

---

# 4. Add musllinux as a first-class release family

## Required targets

Build:

```text
x86_64-unknown-linux-musl  -> musllinux_1_2_x86_64
aarch64-unknown-linux-musl -> musllinux_1_2_aarch64
```

Evaluate:

```text
armv7-unknown-linux-musleabihf or Maturin's documented ARMv7 musl target
    -> musllinux_1_2_armv7l
```

Use the target triple actually supported by the pinned Rust/Maturin toolchain. Do not guess or preserve a stale triple merely because it appears in this plan; verify the exact maintained target during implementation.

## Why musllinux is distinct

Do not relabel a glibc manylinux wheel as musllinux. These are separate libc ABIs and require separate compiled artifacts.

The release matrix should make the distinction visible in artifact names, for example:

```text
wheel-linux-manylinux-x86_64
wheel-linux-manylinux-aarch64
wheel-linux-manylinux-armv7
wheel-linux-musllinux-x86_64
wheel-linux-musllinux-aarch64
```

If Tier 2 armv7 musl is retained:

```text
wheel-linux-musllinux-armv7
```

### Runtime qualification

For musllinux wheels, execute the installed artifact inside a matching Alpine/musl environment or equivalent target runtime.

Required smoke:

```text
python -m pip install --no-index <wheel>
python -c 'import eggserve, eggserve._native'
eggserve --help
python -m eggserve --help
scripts/release_smoke.py
```

The environment must not contain a source checkout on `PYTHONPATH` and must not have a Cargo-built EggServe executable shadowing the wheel entry point.

### Acceptance criteria

- musllinux x86_64 and aarch64 artifacts are built separately from manylinux artifacts;
- both install and execute under a musl runtime;
- extension loading proves no accidental glibc dependency;
- Tier 2 armv7 musl is either qualified with the same standard or explicitly deferred with recorded evidence.

---

# 5. Linux x86_64 execution strategy

For `manylinux_2_17_x86_64`:

1. build through the explicit manylinux path;
2. validate wheel metadata and composition;
3. install the resulting wheel on a normal hosted x86_64 Linux runner;
4. run the existing installed release smoke;
5. additionally perform the minimum/newest CPython ABI proof defined by Plan 139 on this representative target.

This target remains the easiest place to run the complete installed-wheel Python suite.

For `musllinux_1_2_x86_64`:

1. build using Maturin musllinux support;
2. execute inside Alpine/musl;
3. run the common release smoke.

Do not use the success of the manylinux wheel as evidence for the musllinux wheel or vice versa.

---

# 6. Linux aarch64 / SBC execution strategy

The aarch64 wheel is the primary SBC artifact for modern Raspberry Pi and comparable ARM boards.

Prefer a GitHub-hosted Linux ARM64 runner when available and stable for release work. The implementation should use a current stable ARM64 Ubuntu label rather than an unbounded `latest` alias if that improves reproducibility.

Required proof for `manylinux_2_17_aarch64`:

```text
wheel built for aarch64
wheel tag validated
wheel installed under native aarch64 CPython
_native imported
console script executed
real HTTP fixture served
TLS/native dependency load exercised at least enough to prove extension initialization
```

Required proof for `musllinux_1_2_aarch64`:

- build the musl-targeted wheel;
- execute it in an aarch64 Alpine/musl environment on the ARM64 runner where practical;
- otherwise use a maintained emulation/container path with explicit architecture selection.

Do not assert Raspberry Pi hardware-specific behavior from generic ARM64 CI. The claim is architecture/ABI wheel compatibility, not board-peripheral qualification.

### SBC mapping to document

After qualification, documentation may describe:

```text
64-bit Raspberry Pi OS / Debian / Ubuntu on Pi 3/4/5 -> manylinux aarch64 wheel
64-bit Alpine on ARM SBC                         -> musllinux aarch64 wheel
```

Do not claim support for an OS release whose Python/pip wheel-tag implementation cannot actually select the produced wheel.

---

# 7. Linux armv7 / 32-bit SBC strategy

GitHub-hosted native ARM32 runners are not part of the normal release path, so use Maturin's maintained ARMv7 cross-build route.

Required target family:

```text
armv7-unknown-linux-gnueabihf
```

for the glibc/manylinux wheel.

## Qualification requirement

A successful cross-link is not enough. Use QEMU/binfmt or another maintained emulator path to execute a CPython environment matching ARMv7 and install the produced wheel.

The smoke must prove:

```text
architecture reported by runtime is ARMv7-compatible
pip selects/installs the target wheel directly
eggserve._native loads
console script starts
real fixture serving succeeds
```

If QEMU cannot reliably execute the installed extension or a native dependency such as `ring` on the selected baseline, first determine whether the problem is:

```text
actual artifact defect
emulator/tooling defect
unsupported target in current dependency graph
```

Do not create a self-hosted Raspberry Pi runner as the default response.

## 32-bit Raspberry Pi claim

Only after execution qualification may documentation map this wheel to supported ARMv7 32-bit Raspberry Pi OS/SBC installations.

ARMv6 remains explicitly unsupported.

### Acceptance criteria

- manylinux armv7 wheel builds through a maintained cross path;
- wheel tag is correct;
- extension executes in an ARMv7 runtime/emulation proof;
- real server smoke passes;
- no self-hosted runner is required.

---

# 8. macOS x86_64 and arm64 strategy

The current release workflow already exercises macOS arm64. Expand the matrix to both architectures.

Required artifacts:

```text
macOS x86_64
macOS arm64
```

Use explicit hosted runner architecture labels.

Select and document a minimum macOS deployment target that is:

- supported by the Rust toolchain and native dependencies;
- consistent with EggServe's intended user base;
- low enough to make the wheel broadly useful;
- not merely inherited from the build runner's current OS version.

Do not create a universal2 wheel unless measurement demonstrates that it materially simplifies installation without complicating release validation. Two architecture-specific wheels are acceptable and easier to reason about.

For both macOS targets run:

```text
wheel metadata/composition checks
fresh venv install
import eggserve._native
eggserve --help
python -m eggserve --help
release_smoke.py
```

The existing deeper macOS arm64 product qualification workflow remains separate from release-wheel execution unless consolidation clearly reduces duplication without broadening routine CI.

### Acceptance criteria

- Intel and Apple Silicon wheels are both present;
- deployment target is explicit;
- both execute on matching native hosted runners;
- no universal2 requirement is introduced without evidence.

---

# 9. Windows x86_64 and arm64 strategy

Retain native x86_64 build/smoke and add Windows arm64.

Required artifacts:

```text
win_amd64
win_arm64
```

For Windows arm64, first verify that the current GitHub-hosted ARM64 runner is available and sufficiently stable for release-time use. Because hosted ARM64 Windows availability may still be preview/beta at implementation time, the workflow must not silently depend on an unstable label without documenting that operational dependency.

Preferred path:

```text
native Windows ARM64 hosted runner
    -> build
    -> install wheel
    -> import _native
    -> console script smoke
    -> release_smoke.py
```

If native build tooling is unavailable but native execution is available, cross-build on x86_64 and qualify on ARM64. If neither path is dependable, record Windows ARM64 as a concrete release blocker rather than publishing an unexecuted wheel as Tier 1.

A successful Windows ARM64 wheel smoke proves package/runtime compatibility only. It does not change the documented Windows security-hardening qualification status.

### Acceptance criteria

- Windows x86_64 remains green;
- Windows arm64 artifact is built and executed on matching architecture before Tier 1 publication;
- `_native.pyd` loads successfully;
- both console entry forms work;
- no misleading security-support claim is added.

---

# 10. Maturin/action pinning and supply-chain discipline

Use the official Maturin GitHub Action or the project's existing direct Maturin CLI where appropriate, but do not float release tooling.

Required:

```text
Maturin CLI version pinned (currently repository uses 1.14.1)
Maturin action pinned to an immutable commit SHA
GitHub first-party actions pinned consistently with repository policy
Rust toolchain selection explicit enough to reproduce the release
Cargo dependencies built with --locked
```

Do not introduce Renovate/Dependabot or another bot solely for this track. Tool updates remain normal maintainer work.

If container images are referenced directly, prefer immutable/versioned maintained images where practical and document which component controls the manylinux/musllinux baseline.

---

# 11. Common per-wheel qualification harness

Avoid nine copies of platform-specific shell logic.

Refactor only as much as necessary to expose one common installed-wheel smoke contract that can be invoked from Bash, PowerShell, container, or emulated environments.

Reuse:

```text
scripts/check-wheel-composition.py
scripts/release_smoke.py
crates/eggserve-python/packaging-tests/
scripts/test-python-wheel.sh
```

where practical.

If `test-python-wheel.sh` is too Unix-specific for Windows/musl/emulation reuse, do not turn it into a large cross-platform framework. Add a small Python entry-point smoke driver that performs the portable assertions, and leave platform orchestration in the workflow.

Common minimum runtime assertions:

```text
wheel installs into clean environment
import location is site-packages, not checkout
eggserve imports
eggserve._native imports
version equals expected release version
console script --help succeeds
python -m eggserve --help succeeds
real loopback server serves exact fixture bytes
server terminates cleanly
```

### Acceptance criteria

- all Tier 1 targets share the same behavioral smoke contract;
- platform-specific workflow code is limited to environment setup and path syntax;
- no new general-purpose test framework is added.

---

# 12. Complete wheel-set validator

Add one release-set validator, for example:

```text
scripts/check-release-wheel-set.py
```

It must be standard-library-only unless Plan 139 consolidated this logic into an existing checker.

Input:

```text
directory containing every downloaded release wheel
expected release version
```

Required validations:

1. every filename parses into the expected EggServe project/version/ABI/platform shape;
2. every wheel's internal `METADATA` reports the same project/version;
3. every wheel's internal `WHEEL` tags agree with the filename;
4. every wheel is ABI3 and anchored at the expected Python minimum;
5. required Tier 1 target set is exact;
6. no target appears twice under semantically equivalent tags;
7. no generic `linux_*` wheel substitutes for manylinux/musllinux;
8. manylinux and musllinux families are not conflated;
9. every wheel passes the existing no-second-binary composition invariant;
10. unexpected files in the publication directory cause failure unless explicitly allowed.

The exact target manifest should live in one obvious location. It may be encoded in the validation script or generated from the workflow matrix, but do not maintain two independent lists that can drift silently.

Preferred approach: define the target contract once in a small data structure/file and let both workflow and validator consume it if this can be done without introducing templating complexity. If not, keep the workflow explicit and have the validator fail whenever the two disagree.

### Acceptance criteria

- deleting any Tier 1 wheel makes the validation fail;
- adding an unexpected wrong-tag wheel makes validation fail;
- changing one artifact's version makes validation fail;
- the validator is run after artifacts are collected, not only within matrix jobs.

---

# 13. Artifact naming and retention

Each matrix job should upload a uniquely named GitHub Actions artifact containing only its final wheel(s).

Artifact names should encode libc/OS and architecture clearly, for example:

```text
wheel-manylinux-x86_64
wheel-manylinux-aarch64
wheel-manylinux-armv7
wheel-musllinux-x86_64
wheel-musllinux-aarch64
wheel-macos-x86_64
wheel-macos-arm64
wheel-windows-x86_64
wheel-windows-arm64
```

Do not put source trees, Cargo target directories, venvs, logs, or unrelated build products in the publication artifact.

A later aggregate job should download all wheel artifacts into one clean directory and run the complete-set validator.

### Acceptance criteria

- artifact names make omissions diagnosable from the Actions UI;
- publication directory contains only intended distribution files;
- an aggregate wheel set can be reproduced from one workflow run.

---

# 14. Test-depth allocation

## Every Tier 1 wheel

Must pass:

```text
build
platform/ABI/version validation
wheel composition validation
clean install
native import
CLI help
python -m help
real fixture serving
```

## Representative native platforms

Run the full existing installed-wheel Python suite on:

```text
Linux x86_64
Linux aarch64, if runtime cost is reasonable
macOS arm64 or Windows x86_64 as one additional non-Linux representative when useful
```

Do not turn every target into a complete product regression run.

## Cross/emulated targets

Focus on artifact viability and core runtime behavior. Expensive emulated full suites are unnecessary unless the smoke exposes a target-specific defect.

---

# 15. Failure policy

The matrix must use `fail-fast: false` so one architecture does not hide failures on the others. However, the aggregate validation/publication dependency must require all Tier 1 jobs to succeed.

A Tier 1 failure means:

```text
no production publication for that version
```

Do not mark a required target `continue-on-error` merely to finish the release.

Tier 2 musllinux armv7 may be non-blocking only if the roadmap/doc contract clearly marks it as optional and the production validator does not list it as Tier 1.

Never upload a partial wheel set to PyPI and plan to fill in missing files later as the normal release path.

---

# 16. Required verification before handoff to Plan 141

Run the manually dispatched release build workflow from the implementation commit without publication credentials enabled.

Required evidence:

```text
all Tier 1 build jobs green
all Tier 1 runtime smoke jobs green
aggregate artifact download succeeds
complete wheel-set validator succeeds
artifact directory contains expected release version only
```

Record in this plan or an implementation closure note:

- exact commit SHA;
- workflow run ID;
- final wheel filenames;
- per-target runner/build method;
- whether execution was native or emulated;
- Tier 2 musllinux armv7 result;
- any target-specific known caveat.

Do not proceed to production publishing merely because individual matrix jobs compile.

---

# 17. Acceptance criteria

Plan 140 is complete when:

- manylinux 2.17 wheels exist and execute for x86_64, aarch64, and armv7;
- musllinux 1.2 wheels exist and execute for x86_64 and aarch64;
- musllinux armv7 has either a qualified artifact or an explicit evidence-backed Tier 2 deferral;
- macOS x86_64 and arm64 wheels both execute natively;
- Windows x86_64 and arm64 wheels both execute on matching architecture before being marked Tier 1;
- modern 64-bit SBC installations are covered by aarch64 wheels;
- 32-bit ARMv7 SBC installations are covered by the manylinux armv7 wheel;
- ARMv6 remains out of scope;
- all wheels are `cp311-abi3` according to the implemented contract;
- all Linux tags are intentional manylinux/musllinux tags rather than generic host tags;
- native dependencies load successfully on ARM targets;
- every Tier 1 wheel passes the common installed-wheel server smoke;
- the complete artifact set is validated after aggregation;
- no target publishes directly to PyPI;
- no self-hosted runners or bespoke EggServe cross toolchains are introduced;
- routine CI remains unchanged except for any tiny shared validation hook justified by Plan 139;
- the resulting artifact set is ready for the publication boundary in Plan 141.

---

# 18. Implementation closure evidence

```text
source commit SHA:     (filled after first release workflow run)
workflow run ID:       (filled after first release workflow run)
expected version:      0.1.2
```

## Per-target build and execution evidence

| Target | Runner | Build method | Execution | Evidence |
|--------|--------|-------------|-----------|----------|
| `manylinux_2_17_x86_64` | ubuntu-latest | maturin-action manylinux container | native x86_64 | wheel composition check + release_smoke.py |
| `manylinux_2_17_aarch64` | ubuntu-latest | maturin-action + QEMU cross-build | native x86_64 smoke (cross-compiled wheel) | wheel composition check + release_smoke.py |
| `manylinux_2_17_armv7l` | ubuntu-latest | maturin-action + QEMU cross-build | QEMU ARMv7 Docker (arm32v7/python:3.11-bookworm) | wheel composition check + release_smoke.py in emulated ARMv7 |
| `musllinux_1_2_x86_64` | ubuntu-latest | maturin-action musllinux container | native x86_64 | wheel composition check + release_smoke.py |
| `musllinux_1_2_aarch64` | ubuntu-latest | maturin-action + QEMU cross-build | native x86_64 smoke (cross-compiled wheel) | wheel composition check + release_smoke.py |
| `macosx_11_0_arm64` | macos-14 | native maturin build | native ARM64 | wheel composition check + release_smoke.py |
| `macosx_11_0_x86_64` | macos-13 | native maturin build | native x86_64 | wheel composition check + release_smoke.py |
| `win_amd64` | windows-latest | native maturin build | native x86_64 | wheel composition check + release_smoke.py |
| `win_arm64` | windows-latest | native maturin build or cross-build | native ARM64 or cross-qualified | wheel composition check + release_smoke.py |

## Tier 2 musllinux armv7l

Deferred. No GitHub-hosted native ARM32 runner available. Maturin cross-build
for `armv7-unknown-linux-musleabihf` is possible but runtime qualification in
a musl ARMv7 environment is not reliably achievable without self-hosted
infrastructure. The manylinux armv7 wheel covers 32-bit ARM SBC deployments;
musllinux armv7 is an optional extension for Alpine/musl ARMv7 only.

## Known caveats

- aarch64 manylinux and musllinux smoke tests run on x86_64 host (cross-compiled wheel, not emulated ARM64 execution). Full ARM64 runtime qualification is covered by the platform qualification workflow.
- Windows arm64 may use cross-build from x86_64 if native ARM64 runner is unavailable for release builds. Functional qualification is covered by the platform qualification workflow.
