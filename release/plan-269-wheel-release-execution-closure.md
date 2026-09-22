# Plan 269 — Wheel release execution, CI guardrail, and evidence closure

Closure record for the Plans 263–268 Python wheel-distribution campaign.
Plan 268's corrective implementation is complete; this record captures the
mandatory `publish_target=none` no-publish release execution on one exact
candidate SHA plus the routine-CI guardrail integration (Track A).

## Candidate and runs

- Candidate source SHA: `5960380fca36b577a91b8db5c8ef13cd37697b90`
  (`plans: pin MACOSX_DEPLOYMENT_TARGET 11.0 in release builds (269 Track F corrective)`).
- Routine CI: run `35766045827` — superseded; `35775345075` — superseded;
  final candidate run **`35779724999`** — success (rust / supply-chain /
  python, 2026-09-22).
  `https://github.com/eggstack/eggserve/actions/runs/35779724999`
- Its `Wheel-target authority` step executes all five structure checks
  (`wheel-matrix.py validate`, `wheel-matrix.py self-test`,
  `check-release-wheel-set.py --self-test`, `check-release-workflow.py`,
  `check-release-workflow.py --self-test`), including the new
  `MACOSX_DEPLOYMENT_TARGET` pin assertion and mutation. Log excerpts:
  `Wheel matrix valid: 10 required targets`,
  `Release workflow structure OK:`, `OK unpinned deployment target fails`.
- Manual Release workflow run: **`35782377840`**
  `https://github.com/eggstack/eggserve/actions/runs/35782377840`
- Dispatch input: `publish_target=none`, ref `main` while `main` pointed at
  the candidate. Preflight checkout + `Record git SHA` confirm the run
  executed `5960380fca36b577a91b8db5c8ef13cd37697b90`.
- TestPyPI/PyPI publication and post-publish checks were intentionally
  skipped (`publish_target=none`); those skips are expected and are not
  qualification failures.

## Toolchain versions

- Rust: **1.98.1** (exact release pin, `rustc 1.98.1 (48a229cea 2026-09-01)`).
- maturin: **1.14.1** (pinned in the release workflow).
- PyO3: **0.29.2** (locked in `crates/eggserve-python/Cargo.lock`).
- ABI-proof interpreters (same Linux x86_64 wheel bytes, binary-only
  install, all lanes success):
  - CPython **3.11.16** (final)
  - CPython **3.12.14** (final)
  - CPython **3.13.15** (final)
  - CPython **3.14.7** (final)
  - CPython **3.15.0rc2** (**RC evidence** — final 3.15 was not yet
    available through the pinned setup-python action; the lane resolved via
    the temporary `allow-prereleases` mechanism per the Plan 268/269
    RC-to-final policy. Before the first production release claiming
    final-3.15 evidence, remove `allow-prereleases` and re-run the relevant
    ABI/release qualification.)

## Wheel artifacts (10 required, `cp311-abi3`)

From the workflow-generated `MANIFEST` (source SHA verified equal to the
candidate; matrix authority digest verified equal to the local
`release/wheel-matrix.toml` sha256):

```text
Source: 5960380fca36b577a91b8db5c8ef13cd37697b90
Version: 0.2.0
Matrix authority: release/wheel-matrix.toml@ba53bc5afd0ec9e8068461c6d6d218f58b9db426bbb237344b0aac971df33ce6

0184931b41a8a4d1bb72083ee0c410dab5b1bdc770695bf77c0607c8dd0f1a32  dist/eggserve-0.2.0-cp311-abi3-macosx_11_0_arm64.whl
45cd5c2e3206c0303c8bd90cf597c01fb5d7d74ab8bccf0ee3adb1a8584b819a  dist/eggserve-0.2.0-cp311-abi3-macosx_11_0_x86_64.whl
76919dac3df330d303556030e4951469f1a17bb0cb5dff6b24202d28454966f5  dist/eggserve-0.2.0-cp311-abi3-manylinux_2_17_aarch64.manylinux2014_aarch64.whl
e90e457d5d6eedc329e63cc4e985cc1f3857786f2226af8439c8a4ede7108478  dist/eggserve-0.2.0-cp311-abi3-manylinux_2_17_armv7l.manylinux2014_armv7l.whl
c02bb7f2bdece5c65f87543cc85c5ad867a927c1f1684ab7b493f22865ba3dc9  dist/eggserve-0.2.0-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl
96a0efe47ac3e154284e534d334cf680952ab8392905d2b27f74af77e24d648f  dist/eggserve-0.2.0-cp311-abi3-musllinux_1_2_aarch64.whl
c0d1cf9384d368741a393f15013e93aee226b7799338f87c3902717d29cd490f  dist/eggserve-0.2.0-cp311-abi3-musllinux_1_2_armv7l.whl
52ef50430644dd95e45bf2a186d63a7fb4209b6c5e9d51edeb5e98fc70033043  dist/eggserve-0.2.0-cp311-abi3-musllinux_1_2_x86_64.whl
09c68d8f403817b89a39ae240a8f112bfd4a2850f2174a615ea16e5d750ec48a  dist/eggserve-0.2.0-cp311-abi3-win_amd64.whl
3a2b68089705926ba115d0bff2117507d4fd9ea2eafae986aa55e140b94a6e53  dist/eggserve-0.2.0-cp311-abi3-win_arm64.whl
```

The downloaded `release-wheel-set` artifact was re-verified locally:
every SHA-256 digest recomputes identically, and
`scripts/check-release-wheel-set.py ... --version 0.2.0` reports
`Wheels found: 10`, `OK: required target set is exact (10 targets)`,
`All checks passed.` No generic `linux_*`, per-minor
`cp312-cp312`-style, or candidate i686/win32 artifacts are present.

## Required job/result table (run 35782377840)

| Job | Result | Evidence |
|---|---|---|
| preflight | success | source SHA == candidate; supply-chain audit green; matrix emits 10 required targets |
| build × 10 required wheel targets | success | `cp311-abi3` artifact per target; macOS x86_64 built with `MACOSX_DEPLOYMENT_TARGET=11.0` → `macosx_11_0_x86_64` |
| abi-proof 3.11 / 3.12 / 3.13 / 3.14 / 3.15 | success | same wheel bytes install + import + CLI help + `release_smoke.py` + `abi_smoke.py` (3.15 lane: `3.15.0rc2`, RC evidence) |
| qualify-aarch64-glibc (3.11 + 3.15) | success | native ARM64 Ubuntu (`machine=aarch64`); 3.11.16 and 3.15.0rc2 endpoint lanes |
| qualify-aarch64-musl | success | native ARM64 host + ARM64 Alpine container (Python 3.11.16, `aarch64`) |
| qualify-windows-arm64 | success | native Windows ARM64 runner, ref == candidate |
| ARMv7 glibc QEMU smoke (build lane) | success | matching `arm32v7/python:3.11-bookworm` userspace, `sh -c`, import `version=0.2.0` + smoke scripts |
| ARMv7 musl QEMU smoke (build lane) | success | matching `arm32v7/python:3.11-alpine` userspace, `sh -c` |
| macOS x86_64/arm64 + Windows x86_64 + Linux x86_64 native build-host smoke (build lanes) | success | import + CLI help + smoke scripts on each build host |
| aggregate | success | `Wheels found: 10`, `OK: required target set is exact (10 targets)`, `All checks passed.` |
| publish-testpypi / publish-pypi / post-publish | skipped | expected under `publish_target=none`; not qualification failures |

Required qualification failures cannot be bypassed by aggregation: the
`aggregate` job gates `abi-proof`, `qualify-aarch64-glibc`,
`qualify-aarch64-musl`, and `qualify-windows-arm64` (structure-guarded),
and no required lane carries `continue-on-error`.

## Retry attempts and reasons

Two earlier closure attempts on prior candidates failed for repo-side
causes (both classified before repair; neither was external/transient,
so neither counts as closure evidence):

1. Run `35768407636` (candidate `7e52ece`, included the Track A guardrail):
   both macOS build lanes failed to compile —
   `rustix::net::sockopt::socket_acceptconn` is `#[cfg(not(apple))]`
   (Apple declares `SO_ACCEPTCONN` but does not implement it), while the
   systemd fd-adoption validator in
   `crates/eggserve-core/src/server/listener.rs` called it unconditionally.
   Routine CI is Linux-only and never compiled that path. Fixed by
   gating the probe: non-Apple keeps `SO_ACCEPTCONN`; Apple rejects
   connected sockets via `getpeername` (a listening socket has no peer).
   Fix verified by `cargo check -p eggserve-core --target
   x86_64-apple-darwin` plus the passing macOS lanes in the closure run.
2. Run `35776988095` (candidate `bef2a31`, included the Apple fix): all 10
   builds and all qualification lanes succeeded, but `aggregate` rejected
   the macOS x86_64 wheel — maturin defaults
   `MACOSX_DEPLOYMENT_TARGET=10.12` for x86_64 (aarch64 already floors at
   11.0) while the matrix declares `macosx_11_0_*`. Fixed by pinning
   `MACOSX_DEPLOYMENT_TARGET: "11.0"` in the release `build` job env and
   asserting the pin in `scripts/check-release-workflow.py` (plus a
   self-test mutation), so a future unpin fails on push/PR rather than at
   release time.

Both correctives intentionally invalidated their candidate and started a
new closure attempt per Track B; the successful run above executes the
final candidate with the final workflow content.

## Retained Plan 267 decisions

- Normal GIL-enabled CPython on `abi3-py311`; one wheel per platform.
- Free-threaded CPython unsupported; ARMv6/PPC64LE/s390x/RISC-V not
  shipped; i686 glibc/musl + `win32` stay candidates, never counted as
  supported.
- Optional physical Raspberry Pi/Le Potato runs remain
  maintainer-optional; the supported claim stays userspace/
  architecture-based (native hosted + matching QEMU userspaces).

## Campaign state after this record

Plans 263–267 are CLOSED through the 268/269 qualification; Plan 268 and
Plan 269 are COMPLETE. No API, runtime, dependency, security-policy, or
support-tier change was introduced by this plan: the Track F correctives
are a platform-gated compile fix and a release-workflow environment pin
with no behavior change on any supported path.
