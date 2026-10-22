# CLI, Ops, and Observability Milestone 312 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/312-cross-platform-compile-coverage.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `a7e2f22` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Commit `41a813a` — the `platform-compile` CI job, the fuzz build/lint/format
  steps, the local `verify.sh` equivalent, and the two documentation updates.
  This commit was first pushed to the validation branch
  `plan/312-platform-compile-validation` so the new job could be exercised
  before it landed (CI run `39816677832`); it was then fast-forwarded onto
  `main` unchanged, together with commits `9a24d48` (Milestones 309–311) and
  `88d78be` (the closure records). The validation branch was deleted after
  the run.
- Commit `3d284b3` on the throwaway branch
  `plan/312-platform-break-probe` — the deliberate platform-break
  demonstration (CI run `39820541031`). That branch was **never** merged; it
  existed only to prove that the gate fails when Windows-only code does not
  compile.

## 1. Executive finding

Routine CI now compiles the workspace for the Windows and macOS hosts on
every push and pull request. Before this milestone the only compile coverage
was Linux; the `#[cfg(windows)]` confinement implementation and the
`#[cfg(unix)]` traversal implementation were never compiled by CI on the
platform they exist for.

The coverage is **compile-only** and three boundaries are stated explicitly
rather than blurred:

- It is **not** runtime qualification. No Windows or macOS test suite runs in
  routine CI; that evidence still comes only from the manual
  `platform-qualification.yml` workflow and the release wheel gates.
- It does **not** change the Windows security posture. Windows remains
  `supported-functional` (trusted/local-content only).
- It does **not** run the platform test suites. That is a separate cost
  decision and needs its own plan.

Native hosted runners were chosen deliberately over Linux-hosted cross-checks,
and the plan's warning about the wrong flavor was confirmed empirically
rather than assumed (§4.2).

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| The CI job covers the Windows and macOS targets | met | New `platform-compile` job, matrix `[windows-latest, macos-latest]`, in `.github/workflows/ci.yml` |
| The job compiles the workspace with `--all-targets` | met | Step *Workspace compile (all targets)* |
| The job compiles the excluded Python crate | met | Step *Excluded Python crate compile*, `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` |
| The job covers the feature lanes that carry platform-specific code | met | Steps *HTTP/2 and TLS feature lane compile* (`--features http2,tls`) and *HTTP/3 and TLS feature lane compile* (`--features http3,tls`) |
| The job fails the overall check when a platform breaks | met | §4.3 — the deliberate-break run `39820541031` took `platform-compile (windows-latest)` to `failure` |
| The job is not a Linux-hosted cross-check | met | §4.2 — `x86_64-pc-windows-msvc` cross-checking from Linux fails at `ring`'s C build (`failed to find tool "lib.exe"`), so a cross-check could not have produced this coverage |
| A local, opt-in equivalent exists | met | `EGGSERVE_PLATFORM_CHECK=1 ./scripts/verify.sh fast` → `cmd_platform_check` in `scripts/verify.sh` |
| The local check skips cleanly and visibly when a target is absent | met | The skip prints `Platform compile coverage SKIPPED for <target> (target std not installed: rustup target add <target>)`; it is never silent |
| The Windows security posture is unchanged | met | No change to `docs/security-policy.md` or `docs/non-goals.md`; §5 |
| Documentation updated | met | `docs/toolchain-support.md` (coverage matrix + boundaries), `architecture/testing-and-conformance.md` (verification strategy) |

## 3. Production implementation evidence

### 3.1 The CI job

```yaml
  platform-compile:
    name: platform-compile (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, macos-latest]
    env:
      PYO3_USE_ABI3_FORWARD_COMPATIBILITY: "1"
      PYTHONNOUSERSITE: "1"
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd780845e06377c3d76bf3379d6b058f73c593e726a3c48d2e6f834001d9 # v4.3.1
      - uses: dtolnay/rust-toolchain@fa04a1451ff1842e2626ccb99004d0195b455a88 # master
      - uses: Swatinem/rust-cache@23869a5bd66c73db3c0ac40331f3206eb23791dc # v2.9.1
      - uses: actions/setup-python@a26af69be951a213d495a4c3e4e4022e16d87065 # v5.6.0
        with:
          python-version: "3.14"
      - name: Workspace compile (all targets)
        run: cargo check --workspace --all-targets
      - name: HTTP/2 and TLS feature lane compile
        run: cargo check --workspace --all-targets --features http2,tls
      - name: HTTP/3 and TLS feature lane compile
        run: cargo check --workspace --all-targets --features http3,tls
      - name: Excluded Python crate compile
        run: cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

`fail-fast: false` is deliberate: one platform failing must not cancel the
other platform's diagnostics. Both legs must still pass for the check to
pass.

### 3.2 The local equivalent

`scripts/verify.sh` gains `cmd_platform_check`, invoked from `cmd_fast`:

- Opt-in via `EGGSERVE_PLATFORM_CHECK=1`. Without it the step prints
  `Platform compile coverage SKIPPED (opt-in: set EGGSERVE_PLATFORM_CHECK=1)`.
- Targets default to `x86_64-pc-windows-gnu aarch64-apple-darwin`, overridable
  with `EGGSERVE_PLATFORM_TARGETS`.
- A target whose standard library is not installed is skipped **visibly**:
  `Platform compile coverage SKIPPED for <target> (target std not installed:
  rustup target add <target>)`.
- The scope is `cargo check -p eggserve-static --lib --target <target>` — the
  confinement authority, the only crate with platform-specific production
  code. A whole-workspace cross-check would additionally need a
  target-native C toolchain for `ring` and `aws-lc-sys`, which a developer
  rarely has; the hosted CI job covers the full workspace natively instead.

## 4. Verification executed

### 4.1 Local compile checks

| Command | Outcome |
|---|---|
| `cargo check -p eggserve-static --lib --target x86_64-pc-windows-gnu` | PASS — the `#[cfg(windows)]` confinement module (35 `unsafe` blocks) compiles |
| `cargo check --workspace --lib --bins --target x86_64-pc-windows-gnu` | PASS — all eight crates compile for Windows |
| `cargo check -p eggserve-static --lib --target aarch64-apple-darwin` | PASS — the `#[cfg(unix)]` traversal path compiles for macOS |
| `cargo check --workspace --lib --bins --target aarch64-apple-darwin` | FAIL — `ring`'s C build script requires the macOS SDK and an `aarch64-apple-darwin` clang; not reachable from Linux. This is why the CI job uses native runners. |
| `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` | FAIL — `ring`'s build script needs the MSVC toolchain (`failed to find tool "lib.exe"`). Confirms the plan's warning that a Linux-hosted cross-check of the wrong flavor is not viable evidence. |
| `bash -n scripts/verify.sh` | PASS |
| `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` | PASS — jobs `rust`, `platform-compile`, `supply-chain`, `python` |

### 4.2 Hosted CI — the coverage works (run `39816677832`)

Branch `plan/312-platform-compile-validation`, commit `41a813a` — the same
commit that later landed on `main`.

| Job | Conclusion |
|---|---|
| `platform-compile (windows-latest)` | **success** |
| `platform-compile (macos-latest)` | **success** |
| `python` | success |
| `supply-chain` | success |
| `rust` | **failure** — at *Build fuzz harness*, §4.4 |

The macOS runner is `aarch64-apple-darwin` (confirmed from the job log), so
the coverage includes an arm64 macOS target from the documented support set.

### 4.3 Hosted CI — the gate catches a platform break (run `39820541031`)

Branch `plan/312-platform-break-probe`, commit `3d284b3`. The branch adds one
temporary file, `crates/eggserve-core/tests/windows_platform_break_probe.rs`:

```rust
#![cfg(windows)]

#[test]
fn deliberate_platform_break_probe() {
    let _: u32 = "this is deliberately not a u32";
}
```

| Job | Conclusion |
|---|---|
| `platform-compile (windows-latest)` | **failure** — at *Workspace compile (all targets)*; the three later lanes were skipped |
| `platform-compile (macos-latest)` | **success** |

Two properties are proven at once: the Windows lane **does** compile
Windows-only code (so the coverage is real and not a no-op), and the coverage
is genuinely platform-specific (a Windows-only break does not fail the macOS
lane). The probe file was never intended for `main`; both validation branches
are closed without merging.

### 4.4 The fuzz gate is real (same run, `rust` job)

The `rust` job failed at the new *Build fuzz harness* step, with the
downstream steps skipped:

```
success   Check excluded Python crate
failure   Build fuzz harness
skipped   Server TLS lint and tests
```

This is the Plan 310 gate working as intended, and it is also the
demonstration that the gate would have caught the pre-Plan-310 state: on that
branch's base, four of the eleven fuzz targets did not compile, and the
repository was otherwise green. Milestone 310 repairs them; the gate now
keeps that from regressing silently.

### 4.5 Commands not run

- The Windows and macOS **test suites** were not run. That is outside this
  milestone's scope (§1, §5).
- The manual `platform-qualification.yml` workflow was not run — it requires
  maintainer dispatch and a Windows/macOS host with the qualification
  fixtures.

## 5. Invariant review

| Invariant | Result |
|---|---|
| The CI job is opt-in for contributors and does not gate the local `verify.sh fast` run by default | Held. The local check is opt-in via `EGGSERVE_PLATFORM_CHECK=1` and skips visibly by default. |
| A skip is never silent | Held. Every skip prints a reason and, where relevant, the command that would enable the coverage. |
| The Windows security posture is unchanged | Held. No change to `docs/security-policy.md` or `docs/non-goals.md`; Windows remains `supported-functional`. |
| The job does not execute the test suites | Held. `cargo check` only. |
| The job does not publish, tag, or merge | Held. It is a compile step in the `pull_request` / `push` workflow. |
| No dependency or manifest change | Held. No `Cargo.toml` or `Cargo.lock` was modified by this milestone. |
| The job does not duplicate the Linux job's test coverage | Held. The Linux job still runs the tests; the platform job only compiles. |
| The job does not depend on a non-standard toolchain | Held. `dtolnay/rust-toolchain@stable` plus `actions/setup-python` for the excluded Python crate. |

## 6. Failure and recovery review

- A platform break fails the `platform-compile` job, which fails the pull
  request. Proven in §4.3.
- `fail-fast: false` means a Windows break does not hide a macOS break: both
  legs report independently.
- A contributor without the target toolchain is not blocked: the local check
  is opt-in and skips visibly.
- The job has a 30-minute timeout so a hung native build cannot occupy a
  runner indefinitely.

## 7. Migration and compatibility review

- No source, API, or behavior change. This milestone adds CI coverage only.
- No migration is required.
- The one visible consequence for contributors is CI latency: two additional
  runner legs on every push and pull request. The plan's stop condition
  ("adding the platform jobs makes routine CI materially slow enough to be
  skipped by contributors") was answered by narrowing the scope to
  **compile-only**, three lanes per platform, rather than dropping the
  coverage. The observed cost in run `39816677832` was acceptable: the
  platform legs finished alongside the Linux job rather than extending it
  materially.

## 8. Security review

- The change adds CI coverage only. No production code path is touched, and
  no security boundary moves.
- The Windows confinement implementation is unchanged, and the Windows
  security posture (`supported-functional`, trusted/local-content only) is
  unchanged. **Compile coverage is not a claim that the Windows path is
  hardened** — that claim would require the runtime qualification evidence
  that this milestone deliberately does not produce.
- The new CI job runs third-party actions pinned by commit SHA, matching the
  repository's existing pinning convention.
- The job installs a Python interpreter for the excluded Python crate's
  compile check. `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` and
  `PYTHONNOUSERSITE` are set so the build script does not pick up an
  ambient interpreter or ABI assumption.

## 9. Documentation and operations

| Surface | Change |
|---|---|
| `docs/toolchain-support.md` | New *Routine CI Compile Coverage* section: the per-platform matrix, the three explicit limits (compile-only, posture unchanged, no tests), and the opt-in local equivalent. |
| `architecture/testing-and-conformance.md` | The *Platform and external-consumer qualification* section now states that routine CI is Linux-focused **for execution** and cross-platform **for compilation**, and repeats the three boundaries. |
| `.github/workflows/ci.yml` | The new `platform-compile` job, plus the Plan 310 fuzz build/lint/format steps in the `rust` job. |
| `scripts/verify.sh` | `cmd_platform_check` and its `cmd_fast` invocation. |

## 10. Unresolved findings

1. **The macOS lane covers one macOS architecture.** The `macos-latest`
   runner is `aarch64-apple-darwin`. The `x86_64-apple-darwin` variant is not
   compiled by a dedicated lane; it compiles the same source and the release
   matrix cross-builds both, but there is no CI compile lane for it. Adding
   one is a cost decision, not a correctness gap.
2. **The platform test suites are still not run in routine CI.** This is the
   deliberate scope boundary in §1, and it needs its own plan if it is to
   change.
3. **The `x86_64-pc-windows-gnu` local check is not the same ABI flavor as
   the CI job's `x86_64-pc-windows-msvc`.** The local check is a developer
   convenience that exercises the same `#[cfg(windows)]` source; the CI job is
   the authoritative MSVC coverage.

## 11. Roadmap disposition

The CLI/ops/observability roadmap's Milestone 312 ("Cross-platform compile
coverage") is **closed**. Milestones 313 (lint policy enforcement) and 315
(dependency specification consolidation) are independent of it and remain
`ready`.

## 12. Registry updates

- `plans/registry.md` — Milestone 312 moved to `closed` with a pointer to this
  closure record.
- `plans/subsystems/cli-ops-observability-roadmap.md` — Milestone 312 status
  and closure-record column updated.
