# Release Process

eggserve releases are performed manually by a maintainer via a workflow
dispatch. The release cadence is a maintainer decision and is not triggered by
merges, pushes, tags, or CI state. No push, tag, or merge automatically
publishes to PyPI.

The release workflow builds, qualifies, and publishes wheels through a
controlled pipeline: preflight checks, a wide platform build matrix,
aggregate validation, and a single Trusted Publishing job. PyPI publication
uses OIDC via a protected `pypi` GitHub Environment — no long-lived tokens
are stored in repository secrets.

## Release workflow overview

The workflow is manually dispatched with a `publish_target` input
(`none` | `testpypi` | `pypi`) and follows this job graph:

All third-party GitHub Actions used by the workflow are pinned to immutable
commit SHAs; the pinned digests and the update procedure are maintained in
[the action pinning policy](action-pinning.md).

```
workflow_dispatch (publish_target: none|testpypi|pypi)  │
  ▼
preflight (version-sync check, wheel-matrix authority, source commit)
  │
  ▼
wide wheel build matrix (10 required targets, from wheel-matrix.toml)
  │
  ├──▶ ABI proof: same x86_64 wheel on CPython 3.11–3.15 (binary-only)
  ├──▶ native AArch64 + Windows ARM64 qualification lanes
  │
  ▼
aggregate + validate complete wheel set
  │
  ▼
aggregate + validate complete wheel set
  │
  ├──▶ optional TestPyPI publication
  │
  ▼
production approval (pypi environment)
  │
  ▼
publish-pypi (OIDC Trusted Publishing)
  │
  ▼
post-publication binary-only smoke checks
```

No matrix build job may independently publish its wheel. The publication job
receives only the already-built and already-qualified release artifact set.

## Maintainer procedure

1. Synchronize/bump release versions in workspace `Cargo.toml`, Python crate
   `Cargo.toml`, and `pyproject.toml` (all must agree). Keep the excluded
   Python crate `[profile.dist]` exactly equal to the workspace profile;
   `scripts/check-python-release-metadata.py` is the cheap preflight for both.
2. Verify the working tree is clean and routine CI is green.
3. Run `bash scripts/install-cargo-tools.sh` followed by
   `bash scripts/check-supply-chain.sh`; the workflow preflight repeats both
   checks for the root and excluded Python lockfiles.
4. Run the release preflight locally or rely on the workflow preflight job.
5. Manually dispatch the release workflow for the intended commit.
6. Inspect the build matrix results and aggregate manifest.
7. Approve the `pypi` environment only after all required wheels are present.
8. Confirm the publication job succeeds.
9. Review post-publication binary-only smoke checks.
10. Optionally create and push a repository tag.

### Stable Rust API version selection

Patch releases preserve source compatibility for the stable
`eggserve-core::primitives` surface. Before 1.0, an intentional breaking
stable Rust API change uses an explicit minor transition, with release notes
and migration guidance; experimental `server` APIs retain their separately
documented policy.

The current `main` tree contains the Plan 171 outbound response-conversion
transition and later stable-facade changes. That line is classified as
`0.2.0`, not as a compatible `0.1.x` patch release, and the development
metadata reads `0.2.0` accordingly.
The migration entry is the release note for this transition until the
maintainer prepares the final release announcement. The development metadata
reads `0.2.0`; that line must not be published as a `0.1.x` patch, and any
future version change stays on the `0.2.x` line (fix forward, never roll
back to `0.1.x`).

## Preflight version-sync check

The workflow runs a preflight job before any platform builds:

- Checks out the exact selected ref and records the commit SHA.
- Runs `scripts/check-python-release-metadata.py` to verify version
  agreement across workspace `Cargo.toml`, Python crate `Cargo.toml`,
  `pyproject.toml`, and `__init__.py` (which derives from
  `importlib.metadata.version("eggserve")`), plus exact `[profile.dist]`
  equivalence between the workspace and the excluded Maturin crate.
- Validates `abi3-py311`, `requires-python >=3.11` with advertised 3.11–3.15
  classifiers, and the `python3.11` wheel ABI baseline.
- Validates the wheel-target authority (`scripts/wheel-matrix.py validate`,
  `self-test`, and `scripts/check-release-wheel-set.py --self-test`) and
  emits the build matrix from `release/wheel-matrix.toml`, so the workflow
  carries no second platform list.
- Exposes the expected package version as a job output for downstream matrix
  jobs.

A metadata mismatch prevents all platform builds.

## Wide platform wheel matrix

Release wheels are built for all 10 required targets declared in
`release/wheel-matrix.toml` (the single authority; the workflow matrix is
generated from it in preflight):

| Platform family | Wheel target | Build method |
|---|---|---|
| Linux x86_64 (glibc) | `manylinux_2_17_x86_64` | manylinux container |
| Linux aarch64 (glibc) | `manylinux_2_17_aarch64` | manylinux container (cross-build, native hosted execution) |
| Linux armv7 (glibc) | `manylinux_2_17_armv7l` | cross-build + QEMU smoke under matching ARMv7 glibc userspace |
| Linux x86_64 (musl) | `musllinux_1_2_x86_64` | musllinux container |
| Linux aarch64 (musl) | `musllinux_1_2_aarch64` | musllinux container (cross-build, native hosted execution where available) |
| Linux armv7 (musl) | `musllinux_1_2_armv7l` | cross-build + QEMU smoke under matching ARMv7 musl userspace |
| macOS x86_64 | `macosx_11_0_x86_64` | native hosted runner |
| macOS arm64 | `macosx_11_0_arm64` | native hosted runner |
| Windows x86_64 | `win_amd64` | native hosted runner |
| Windows arm64 | `win_arm64` | cross-build + native hosted execution |

Each wheel is built with exact Rust **1.98.1** and
`--profile dist --locked --interpreter python3.11` and
validated for platform/ABI/version correctness, wheel composition (no second
standalone binary), and runtime smoke (import, CLI help, real fixture serving).

### Stable-ABI proof (build-once, test-many)

One `cp311-abi3` artifact per platform serves GIL-enabled CPython 3.11–3.15;
no per-minor wheels are produced. The release proves reuse with the exact
built bytes: the `abi-proof` job downloads the Linux x86_64 wheel artifact
and installs it with `--only-binary=:all:` on CPython 3.11, 3.12, 3.13,
3.14, and 3.15, running import, CLI/module help, `scripts/release_smoke.py`,
and the compact native fixture (`scripts/abi_smoke.py`) on each, recording
the wheel filename and interpreter version.

### Architecture-aware qualification

- Linux AArch64 wheels execute natively on the ARM64 hosted runner on both
  CPython 3.11 (minimum) and CPython 3.15 (maximum); this lane is the
  representative evidence for 64-bit Raspberry Pi and Le Potato-class
  userspaces (userspace/architecture claims only, never board-specific
  kernel/device claims).
- The Windows ARM64 wheel executes natively on the Windows ARM64 hosted
  runner. If that runner is unavailable, the blocker is recorded and support
  wording stays conservative.
- ARMv7 glibc and musl wheels execute under matching ARMv7 runtime
  environments via QEMU (never AArch64 compat mode alone).
- `scripts/qualify-python-wheel-target.sh` provides a repeatable rootless
  real-device path (local wheel or published package) for maintainer-run SBC
  proof; volunteer hardware is never a mandatory per-PR gate.

## Aggregate and validate

After all matrix jobs succeed:

1. Download every required artifact from the workflow run.
2. Place all wheels in one clean directory.
3. Run the release wheel-set validator (`scripts/check-release-wheel-set.py`,
   which loads its required set from the same `release/wheel-matrix.toml`).
4. Verify all wheels share the expected version and `cp311-abi3` tag.
5. Produce a human-readable manifest with SHA-256 hashes plus the matrix
   authority revision, so a published artifact set can be reconstructed.
6. Upload the aggregate set as workflow evidence.

The aggregate step rejects the release if any required target is missing,
duplicated, mis-tagged, undeclared, or a generic `linux_*` wheel, or if
manylinux/musllinux families are conflated. Declared `candidate` targets
never silently count as supported.

## PyPI Trusted Publishing (OIDC)

Production publication uses PyPI Trusted Publishing through GitHub Actions
OIDC. No long-lived `PYPI_API_TOKEN` or PyPI password is stored in
repository secrets.

- Only the final publication job receives `id-token: write`.
- Production publication uses the protected `pypi` GitHub Environment with
  required reviewer approval.
- The publication job performs no compilation — it uploads only the
  validated aggregate artifacts.
- The official `pypa/gh-action-pypi-publish` action is used, pinned to a
  specific version.

### TestPyPI staging path

TestPyPI uses a separate `testpypi` GitHub Environment and Trusted Publisher
registration. TestPyPI publication is optional and intended for first
implementation validation or mechanism changes.

For TestPyPI qualification:

1. Publish the complete validated artifact set.
2. Install from TestPyPI with `--only-binary=:all:` on representative
   platforms.
3. Run import, CLI, and server smoke checks.
4. Confirm pip selected the expected wheel rather than building locally.

## Post-publication smoke checks

After a successful PyPI upload, run binary-only installation and smoke checks
across the published matrix in parallel:

- Linux x86_64 (glibc, manylinux) on CPython 3.11 (minimum) and 3.15 (maximum)
- Linux AArch64 (glibc, manylinux, native hosted runner)
- Linux ARMv7 (glibc, QEMU ARMv7 userspace)
- Linux x86_64 (musl, Alpine container)
- Linux AArch64 (musl, Alpine container on the ARM64 runner)
- Linux ARMv7 (musl, QEMU ARMv7 Alpine userspace)
- macOS arm64
- Windows x86_64
- Windows ARM64 (native hosted runner)

Each target installs from the published index with `--only-binary=:all:`
(where supported by the platform), which fails rather than falling back to
an sdist/local build. Each verifies `eggserve.__version__` matches
the expected release version, confirms `eggserve._native` imports, and runs
the release smoke test (real loopback server serving exact fixture bytes)
plus the compact native fixture.

Install in a clean environment without source/build dependencies. Verify pip
resolves a wheel without local compilation. Capture the resolved wheel
filename as release evidence.

Post-publication smoke failure must be surfaced prominently. Recovery uses a
new version when artifact contents must change.

## Release concurrency

The release workflow uses a concurrency group to prevent two production
release runs from racing. Concurrent dispatches queue rather than cancelling
an in-progress publication.

## Known limitations

- **Windows**: functionally qualified on the manual platform workflow, but not
  hardened for untrusted content. Two open-descendant root-rename cases are
  explicitly skipped because NTFS rejects that external path operation.
- **Follow-symlinks**: weaker than default symlink-denied mode. Uses
  canonicalize-based resolution outside the descriptor-relative hardening
  guarantee.
- **HTTP/2, redirects, retries, cookies, proxy, and multi-range responses**:
  outside scope. HTTP/1.1 with single byte ranges only.
- **Python wheels**: GIL-enabled CPython 3.11–3.15 with abi3 stable ABI
  (`>=3.11`, one `cp311-abi3` wheel per platform; free-threaded CPython
  unsupported, see `release/plan-267-wheel-feasibility.md`).

## crates.io publication

Core crate must be published before the binary crate, because the binary
depends on it by path (registry resolves the latest published version).

```sh
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked

# Wait for the new version to appear on the crates.io index.

cargo publish -p eggserve-bin --locked --dry-run
cargo publish -p eggserve-bin --locked
```

Versions are immutable on crates.io. If a version has been successfully
published and needs correction, a new version number is required. Do not retry
publication of changed contents under an existing version.

crates.io publication is independent of PyPI publication and is not required
to happen in the same transaction.

## Distribution builds

The `dist` profile produces stripped, size-optimized release artifacts for
distribution. Use it for manual release builds only — not for routine CI or
development:

```sh
cargo build --profile dist --locked -p eggserve-bin              # default CLI (no TLS)
cargo build --profile dist --locked -p eggserve-bin --features tls  # TLS CLI
```

The dist profile uses `opt-level = "z"`, fat LTO, single codegen unit,
and symbol stripping. Release builds pin Rust 1.98.1; floating `stable` remains
available in compatibility CI and platform qualification. See `Cargo.toml` and
`.github/workflows/release.yml` for the exact configuration.

## Post-publication tag

After publication, optionally create a tag:

```sh
git tag "vX.Y.Z"
git push origin "vX.Y.Z"
```

The tag is a historical marker only. A GitHub Release may be created manually
if desired.
