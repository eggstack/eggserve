# Plans 264–266 — Wheel qualification record

Execution evidence for the Python wheel distribution expansion (Plans 264,
265, 266). Plan 267's independent feasibility gate is
`release/plan-267-wheel-feasibility.md`.

- Source SHA: `f483b267bfc456b9452215ca46ff36773ec053d3` (Plan 263 program
  commit; implementation commits follow on top — see `git log`).
- Workflow run: local verification below; remote release-workflow lanes
  (`abi-proof`, `qualify-aarch64-native`, `qualify-windows-arm64`,
  expanded post-publish matrix) execute on the next manual release dispatch,
  not on push/PR.
- Matrix authority: `release/wheel-matrix.toml`
  (`sha256 20cb514dbca4447b2e83ba7d3aade58ee6d14013d662fafec48881515e0ddf94`
  at record time; the aggregate MANIFEST records the release-time revision).

## Built wheel list and SHA-256 (local Plan 264 proof)

One `cp311-abi3` wheel built with the release baseline
(`--interpreter python3.11`, maturin 1.14.1, Rust 1.98.1, `dist` profile):

```text
eggserve-0.2.0-cp311-abi3-manylinux_2_34_x86_64.whl
sha256 78e363183963d2ce830b7cc4d6aca7f7624649b52fb4403bb03934eb0d6a3a30
```

(Local tags read `manylinux_2_34` from the bare-host build; release
containers emit the matrix `manylinux_2_17` policy tag. The ABI proof is
about interpreter reuse of one artifact, not the glibc floor.)

## Build-once/test-many matrix (same wheel bytes, binary-only install)

| Interpreter | Install | Import + `_native` | CLI / module help | `release_smoke.py` | `abi_smoke.py` |
|---|---|---|---|---|---|
| CPython 3.11.15 | `--only-binary=:all:` OK | OK | OK | 200 exact bytes | passed |
| CPython 3.12.3 | `--only-binary=:all:` OK | OK | OK | 200 exact bytes | passed |
| CPython 3.13.12 | `--only-binary=:all:` OK | OK | OK | 200 exact bytes | passed |
| CPython 3.14.6 | `--only-binary=:all:` OK | OK | OK | 200 exact bytes | passed |
| CPython 3.15.0rc2 | `--only-binary=:all:` OK | OK | OK | 200 exact bytes | passed |

3.12–3.14 lanes ran through `scripts/test-python-wheel.sh`
(`WHEEL_PATH` reuse + `MODE=abi-smoke`); 3.11/3.15 system interpreters lack
`ensurepip`, so those lanes used `uv venv` + `uv pip install
--only-binary=:all:` with the same assertions. No per-minor
`cp312-cp312`/`cp315-cp315` wheel was built. Local 3.15 is a release
candidate; the release `abi-proof` job targets final 3.15.

Full installed-wheel suite still passes on the primary CI interpreter via
routine CI (`scripts/test-python-wheel.sh` default mode).

## Native/QEMU runtime matrix

| Target | Strategy | Evidence at record time |
|---|---|---|
| Linux x86_64 glibc | native | Local proof above (this host) |
| Linux AArch64 glibc | native hosted (`ubuntu-24.04-arm`, `qualify-aarch64-glibc`, CPython 3.11 + 3.15) | Pending release dispatch (Plan 268 split: manylinux wheel only) |
| Linux AArch64 musl | native ARM64 Alpine container (`qualify-aarch64-musl`, musllinux wheel only) | Pending release dispatch (Plan 268 split; glibc-host musl install is not qualification) |
| Linux ARMv7 glibc/musl | QEMU matching userspace (`arm32v7/python:3.11-bookworm`, `arm32v7/python:3.11-alpine`, `sh -c`) | Release build `qemu` smoke steps; pending release dispatch |
| Linux x86_64/aarch64 musl | Alpine post-publish smoke | Release post-publish `alpine` lanes; pending release dispatch |
| macOS x86_64/arm64 | native hosted | Release build `native` smoke; pending release dispatch |
| Windows x86_64 | native hosted | Release build `native` smoke; pending release dispatch |
| Windows ARM64 | native hosted (`windows-11-arm`, required gate) | Release `qualify-windows-arm64` lane (blocks aggregation per Plan 268); pending release dispatch |

No AArch64 compat-mode execution is accepted as ARMv7 proof; the manifest
pins per-target `qemu_image` values for the matching userspaces.

## Real-SBC evidence

`scripts/qualify-python-wheel-target.sh` (rootless; local wheel or
published package) was executed on this host against the proof wheel:
device evidence (Python/machine/OS/libc), binary install, import,
CLI/module help, loopback smoke (200 exact bytes), and confinement smoke
(200 public / 403 dotfile) all passed. Physical Raspberry Pi / Le Potato
runs remain maintainer-optional and are recorded here when executed:

```text
(real-device runs: none yet — lane available, not required per-PR)
```

## Blocked lanes

- Windows ARM64 hosted runner (`windows-11-arm`) availability to this
  repository is assumed but unproven until the first release dispatch. Per
  Plan 268 Track E it is a required gate: if the lane fails to schedule or
  fails, aggregation blocks and the failure itself is the recorded blocker.
  `docs/toolchain-support.md` support wording stays at supported-functional
  without a hardened claim.
- Final CPython 3.15 on hosted runners: local evidence used 3.15.0rc2. Per
  Plan 268 Track F the release `abi-proof`, AArch64 glibc, and post-publish
  max lanes carry `allow-prereleases: true` until final 3.15 resolves
  without it; the exact interpreter version is recorded in each lane.

## Plan 268 corrective (implemented, awaiting closure run)

Corrective baseline: Plan 268 plan commit plus the implementation in this
tree (`release/wheel-matrix.toml` split baseline/policy + deferred smoke
routing, `scripts/wheel-matrix.py` + new
`scripts/check-release-workflow.py` guards, corrected release graph with
`qualify-aarch64-glibc` / `qualify-aarch64-musl` / `qualify-windows-arm64`
gating aggregation, QEMU `sh` + explicit binfmt setup, 3.15 prerelease
handling).

No configured-but-never-executed lane is claimed as runtime qualification
above: every release lane remains "pending release dispatch" until the
mandatory `publish_target=none` manual run on the exact corrective SHA
succeeds and its run ID/URL, 10 wheel filenames, SHA-256 manifest, ABI-proof
(3.11–3.15), native/QEMU results, and aggregate validator output are
recorded in a dedicated Plan 268 closure record. Until then Plans 263–267
remain implemented but not closed.

## Support claims retained

- Normal GIL-enabled CPython 3.11–3.15 via one `cp311-abi3` wheel per
  platform (Plan 264).
- 10 required release targets from `release/wheel-matrix.toml`, including
  `musllinux_1_2_armv7l` (Plan 265); i686 glibc/musl + `win32` stay
  candidates, never counted as supported (Plans 265/267).
- ARM/SBC claims stated as supported userspaces (AArch64 glibc/musl,
  ARMv7 glibc/musl), not board-specific hardware behavior (Plan 266).
- Free-threaded CPython unsupported; ARMv6/PPC64LE/s390x/RISC-V not
  shipped (Plan 267 gate above).
- No API, protocol-tier, TLS, filesystem-policy, or dependency change.
