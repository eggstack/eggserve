# Plan 268 — Wheel release-pipeline corrective qualification and closure

## Purpose

Correct the release-only defects found after implementing Plans 264–267 and
close the Python wheel-distribution expansion only after the complete release
graph has executed successfully on the exact candidate SHA.

This is a narrow release-pipeline correctness and evidence plan. It does not
add another wheel target, change the Python or Rust API, change runtime
behavior, alter TLS/security dependencies, or reopen the Plan 267
free-threaded/long-tail decisions.

Planning baseline:

```text
857fb6a plans: implement Python wheel expansion program (264-267)
```

## Why this corrective is required

Routine CI is green on the baseline SHA, including the Rust, Python, and
supply-chain jobs. The implementation also established the intended
architecture:

- normal GIL-enabled CPython remains `cp311-abi3`;
- `release/wheel-matrix.toml` is the canonical target authority;
- 10 release targets are required, including `musllinux_1_2_armv7l`;
- i686/win32 remain candidates;
- free-threaded Python and long-tail architectures remain deferred/NO-GO as
  recorded by Plan 267.

However, the new release-only lanes have not yet been exercised by a manual
release dispatch, and review of the executable workflow found several defects
that routine CI cannot expose:

1. the manifest conflates the maturin manylinux container/baseline selection
   with `--compatibility pypi`;
2. cross-built Linux AArch64 and Windows ARM64 artifacts are marked for native
   smoke in their x86_64 build jobs;
3. the native AArch64 qualification job downloads both glibc and musl wheels
   and tries to install both directly on Ubuntu;
4. the ARMv7 Alpine/musl smoke invokes `bash` in a minimal Alpine Python
   image;
5. post-publication ARMv7 QEMU lanes do not install QEMU/binfmt explicitly;
6. before CPython 3.15 final is available, release lanes request `3.15`
   without the prerelease setup needed to resolve 3.15 RC builds;
7. aggregate/publication depends on `build` but not on the new ABI/native
   qualification jobs, so those evidence jobs may fail without preventing a
   release.

The campaign is therefore implemented but not closed.

## Constraints

- No Python public API change.
- No Rust public API change.
- No product capability or support-tier change.
- No new dependency and no TLS/crypto-provider change.
- Keep direct `rustls` constraints and the current supply-chain policy
  unchanged.
- Preserve `abi3-py311` and the 10 required normal wheel targets.
- Keep candidate/deferred targets non-release-blocking and non-supported.
- No push/tag/merge may publish automatically; publication remains manual and
  OIDC/environment protected.
- Do not weaken aggregate validation to make a failing target pass.
- Do not replace native/QEMU runtime proof with compile-only evidence.

## Track A — Separate manylinux baseline from PyPI compatibility policy

Files:

- `release/wheel-matrix.toml`
- `scripts/wheel-matrix.py`
- `.github/workflows/release.yml`
- validator/self-test fixtures as needed
- `docs/release-process.md` / `docs/toolchain-support.md` only if wording
  currently reflects the conflated setting.

The matrix must represent these as separate concepts:

1. **container/platform compatibility baseline**, e.g. manylinux `2_17`,
   musllinux `1_2`, or native/auto for non-Linux targets;
2. **maturin/PyPI compatibility policy**, supplied separately as
   `--compatibility pypi` where intended.

Do not pass the string `pypi` through the action's `manylinux:` input.

Recommended manifest shape is additive and explicit, for example:

```toml
manylinux = "2_17"
compatibility = "pypi"
```

or equivalent names that make the two controls unambiguous.

For musllinux targets, preserve the existing portable musl policy and verify
the action invocation maps it correctly. For macOS/Windows, keep the native
platform policy unchanged.

Self-tests must reject invalid combinations such as:

- manylinux target with no manylinux baseline;
- musllinux target assigned a manylinux baseline;
- `compatibility = "pypi"` accidentally reused as the container selector;
- Linux target whose expected wheel tag contradicts its baseline.

Acceptance for this track is an emitted release matrix whose manylinux
x86_64/aarch64/armv7 builds explicitly target the intended
`manylinux_2_17_*` family and still use the project PyPI compatibility
policy separately.

## Track B — Correct cross-build smoke routing

Files:

- `release/wheel-matrix.toml`
- `scripts/wheel-matrix.py`
- `.github/workflows/release.yml`

Cross-built artifacts must not be installed on an incompatible build host.

Specifically:

### Linux AArch64 glibc

The x86_64-hosted build job may:

- build the AArch64 wheel;
- verify wheel composition/tags;
- upload the artifact.

It must **not** install the AArch64 wheel into the x86_64 build-host venv.

Execution belongs to the native `ubuntu-24.04-arm` qualification job.

### Windows ARM64

The x86_64 Windows build job may:

- cross-build the `win_arm64` wheel;
- verify wheel composition/tags;
- upload the artifact.

It must **not** install/execute that wheel on the x86_64 Windows host.

Execution belongs to the native `windows-11-arm` qualification job.

Represent this distinction explicitly in the manifest rather than special-case
target IDs in YAML. Add or refine a smoke strategy such as
`qualified-native` / `deferred-to-qualifier`, with validator self-tests
ensuring that a cross-built target cannot claim ordinary build-host native
smoke.

## Track C — Split AArch64 glibc and musl qualification correctly

The current AArch64 native job downloads both `*-aarch64` artifacts and loops
over them on Ubuntu. That is not a valid musllinux runtime proof.

Refactor qualification so:

### AArch64 glibc

On `ubuntu-24.04-arm`:

- download only the `manylinux_2_17_aarch64` artifact;
- binary-only install;
- import `eggserve` + `eggserve._native`;
- CLI/module help;
- `release_smoke.py`;
- `abi_smoke.py`;
- record architecture, Python, OS, and glibc.

Run the minimum and maximum supported Python endpoint lanes required by Plan
266 (3.11 and 3.15/3.15 prerelease as Track F specifies).

### AArch64 musl

On the ARM64 hosted runner:

- run an AArch64 Alpine/Python container natively;
- install only the `musllinux_1_2_aarch64` wheel;
- use binary-only installation;
- run import, CLI/module, release smoke, and ABI smoke;
- record Alpine/musl identity.

Do not treat successful installation of a musllinux wheel on a glibc host as
qualification.

The post-publication AArch64 musl lane should follow the same native ARM64
Alpine pattern.

## Track D — Repair ARMv7 musl/QEMU execution

For both pre-publication and post-publication ARMv7 musl smoke:

- use the matching ARMv7 Alpine image;
- invoke `sh -c`, not `bash -c`, unless Bash is explicitly installed as
  part of the lane (prefer not installing it);
- preserve binary-only wheel installation;
- run import, CLI/module help, `release_smoke.py`, and `abi_smoke.py`.

For ARMv7 glibc, continue using the matching ARMv7 Debian/glibc userspace.

The post-publication job must explicitly run the pinned
`docker/setup-qemu-action` (or an equivalently deterministic binfmt setup)
before any `linux/arm/v7` Docker invocation. Do not rely on runner-global
binfmt state.

Add workflow/static checks where practical so a future ARMv7 QEMU lane cannot
be added without the setup step.

## Track E — Make qualification jobs release gates

This is the highest-priority corrective.

The release graph must prevent aggregate/publication if any required
qualification fails.

At minimum, `aggregate` must depend on successful completion of:

- `preflight`;
- `build`;
- `abi-proof`;
- required native AArch64 qualification;
- required Windows ARM64 qualification.

If AArch64 musl is split into a separate qualifier, include it as well.

Do not use `continue-on-error` on a required qualification lane.

If Windows ARM64 hosted-runner availability is still considered conditional,
make the support contract and graph internally consistent. Choose one:

1. **required target:** runner/job must succeed before aggregate; or
2. **not yet required:** demote the target/support claim until evidence exists.

Do not keep `win_arm64` as a mandatory published artifact while allowing its
only native execution proof to fail without blocking publication.

Post-publication smoke remains post-publication evidence and cannot protect the
already-completed upload. Pre-publication qualification must therefore carry
the mandatory release gate.

Add a lightweight workflow-structure checker or self-test if necessary to
guard the required `needs:` relationship against future drift.

## Track F — CPython 3.15 prerelease-to-final transition

As of this corrective's planning date, CPython 3.15 final has not yet reached
its scheduled final release and local evidence used 3.15.0rc2.

Until final 3.15 is actually available through the pinned setup action:

- lanes requesting `python-version: "3.15"` must explicitly enable the
  action's prerelease resolution mechanism;
- record the exact interpreter version in evidence.

Once final CPython 3.15 is available through the pinned setup action:

- remove the prerelease exception;
- run the closure release qualification against final 3.15;
- update the evidence record from RC to final.

Do not permanently carry `allow-prereleases` after final is available.

The implementation must check availability at execution time instead of
assuming October 1 has passed.

## Track G — Release evidence correction

Update `release/plan-264-266-wheel-qualification.md` or add a dedicated
Plan 268 closure record after execution.

The final evidence must distinguish:

- local preflight/unit/self-test evidence;
- routine CI evidence;
- manual release-workflow evidence;
- pre-publication native/QEMU qualification;
- post-publication evidence (only if an actual TestPyPI/PyPI publication is
  intentionally performed later).

Do not describe a configured-but-never-executed workflow lane as runtime
qualification.

The closure record must include:

- exact source SHA;
- manual workflow run ID/URL;
- `publish_target=none`;
- all 10 built wheel filenames;
- SHA-256 manifest;
- ABI-proof results for 3.11, 3.12, 3.13, 3.14, 3.15;
- AArch64 glibc native result;
- AArch64 musl native-container result;
- ARMv7 glibc and musl QEMU results;
- Windows x86_64 native build/smoke result;
- Windows ARM64 native qualification result;
- macOS x86_64/arm64 build-smoke results;
- aggregate validator result.

If any required lane is unavailable or fails, the plan remains open and the
release claim must be narrowed rather than waived.

## Track H — Documentation and planning-state reconciliation

After the full no-publish release run succeeds:

- update `plans/ROADMAP.md` to mark Plan 268 complete and Plans 263–267
  closed through the corrective;
- update `docs/release-process.md` to reflect the actual qualification graph;
- update `docs/toolchain-support.md` if native-vs-QEMU wording changed;
- update `release/plan-264-266-wheel-qualification.md` so no "pending first
  manual dispatch" text remains;
- keep Plan 267 GO/NO-GO/DEFERRED decisions unchanged unless new evidence was
  deliberately gathered under a separate plan.

## Verification before remote release dispatch

Required cheap/static checks:

```sh
python3 scripts/check-python-release-metadata.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Routine CI on the exact corrective SHA must pass all normal jobs.

## Mandatory closure run

After the corrective commit is on `main`, manually dispatch:

```text
Release workflow
publish_target = none
ref = <exact corrective SHA/main at that SHA>
```

This no-publish run is the closure gate.

Expected graph:

```text
preflight
   |
 build (10 required wheels)
   |
   +--> abi-proof (3.11–3.15)
   +--> AArch64 glibc/musl qualification
   +--> Windows ARM64 qualification
            |
            v
        aggregate
            |
        validation only
   (no TestPyPI/PyPI upload)
```

The aggregate job must not start successfully if any mandatory qualification
job fails.

## Acceptance

Plan 268 closes only when all of the following are true:

- manylinux baseline and PyPI compatibility are separate controls;
- release manylinux artifacts are actually tagged at the declared baseline;
- cross-built AArch64/Windows ARM64 wheels are not executed on incompatible
  x86_64 build hosts;
- AArch64 glibc is qualified natively on Ubuntu ARM64;
- AArch64 musl is qualified in native ARM64 Alpine/musl;
- ARMv7 glibc and musl run in matching QEMU userspaces;
- ARMv7 Alpine uses a valid shell and post-publish QEMU setup is explicit;
- Python 3.15 RC/final handling matches current availability;
- ABI proof and required native qualification jobs gate aggregation;
- routine CI passes on the exact corrective SHA;
- one complete manual `publish_target=none` release run succeeds for the
  exact corrective SHA;
- the aggregate contains exactly the 10 required `cp311-abi3` wheels and
  passes the shared validator;
- closure evidence records the successful run and exact artifact set;
- no API, dependency, protocol, TLS, filesystem-policy, or support-tier
  regression was introduced.

Until that no-publish run passes, Plans 263–267 are **implemented but not
closed**.
