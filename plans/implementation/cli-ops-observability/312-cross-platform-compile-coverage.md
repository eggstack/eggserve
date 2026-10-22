# CLI, Ops, and Observability Milestone 312 — Cross-platform compile coverage for the hardened filesystem path

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md#milestone-312--cross-platform-compile-coverage`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (safe defaults; no serving outside the configured root)
- `plans/001-terminology-and-domain-model.md` (`SecureRoot`, `PinnedRoot`)
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/adr-002-windows-handle-relative-filesystem.md` (accepted ADR; the Windows confinement design)
- `architecture/filesystem-confinement.md`
- `architecture/path-confinement.md`
- `docs/threat-model.md`
- `docs/security-policy.md`
- `docs/toolchain-support.md`
- `docs/unsafe-code-policy.md`

Primary class: invariant

## 1. Objective

Make routine CI compile the product on every platform the product claims
to support, so a platform-specific break is caught on the pull request
instead of at the next manual qualification run.

At the baseline, **every rust job in `.github/workflows/ci.yml` runs on
`ubuntu-latest`** (`:22`, `:101`, `:116`). The Windows handle-relative
filesystem implementation — `crates/eggserve-static/src/fs/windows.rs`,
roughly 2,800 lines with **42 `unsafe` blocks**, gated
`#[cfg(windows)]` at `crates/eggserve-static/src/fs/mod.rs:30-31` — is
therefore never compiled by routine CI.

Windows *is* compiled somewhere: the release workflow's wheel matrix
includes `x86_64-pc-windows-msvc` (`scripts/wheel-matrix.py:345`), and
`platform-qualification.yml` runs a `windows-latest` job. Both are
`workflow_dispatch`-only, so a Windows-only compile break can sit
undetected for months and is discovered only when someone publishes a
release.

This milestone adds compile-only coverage for the Windows and macOS
targets to routine CI. It does **not** claim runtime evidence — the manual
platform qualification remains the source of runtime evidence, and this
milestone must not be described as replacing it.

## 2. Why this milestone is ready

- The change is CI configuration plus, at most, a small portability fix.
- The Windows and macOS toolchains and runners are standard GitHub-hosted
  images; no new infrastructure is required.
- `docs/toolchain-support.md` already records the supported platforms, so
  the coverage set is defined.
- No hard dependency on other open milestones.

## 3. Current implementation evidence

### 3.1 The CI job graph

`.github/workflows/ci.yml`:

| Job | Runner | Notes |
|---|---|---|
| `rust` | `ubuntu-latest` (`:22`) | fmt, MSRV checks, clippy, workspace tests, feature lanes |
| `supply-chain` | `ubuntu-latest` (`:101`) | audits both lockfiles |
| `python` | `ubuntu-latest` (`:116`) | excluded Python crate check, wheel metadata, packaging tests |

The `rust` job runs `cargo +1.89 check --workspace --all-targets` and the
same with `--features http2,tls` and `--features http3,tls`, plus
per-feature checks for `http-interop` and `tower`. All of these are
host-target checks.

### 3.2 Where non-Linux compilation does happen

- `.github/workflows/release.yml` — `workflow_dispatch` only; its wheel
  matrix builds `x86_64-pc-windows-msvc` and `aarch64-unknown-linux-gnu`.
- `.github/workflows/platform-qualification.yml` — `workflow_dispatch`
  only; a `macos-14` job and a `windows-latest` job that run
  `scripts/test-python-wheel.sh` (an installed-wheel runtime qualification,
  not a workspace compile).

### 3.3 The platform-specific code at risk

`crates/eggserve-static/src/fs/windows.rs` is the largest platform-specific
production file in the repository and contains the security-critical
Windows confinement path: directory-handle retention, child resolution, and
directory enumeration via `NtQueryDirectoryFile`. Its 42 `unsafe` blocks
are the reviewed Windows FFI boundary named in `docs/unsafe-code-policy.md`.

The corresponding Unix path (`crates/eggserve-static/src/fs/unix.rs`, 342
lines) uses safe `rustix` wrappers, so the asymmetry is expected — but it
means a Windows-only compile error (a changed `windows-sys` API, a
cfg-gated item, a new `#[cfg(windows)]` call site) is invisible until a
Windows toolchain is invoked.

The Windows adversarial test suite is additionally gated behind both the
platform and a feature:
`crates/eggserve-core/tests/windows_plan086.rs:23`
(`#![cfg(all(windows, feature = "windows-adversarial-qualification"))]`),
so it is not even compiled on a routine Windows runner.

### 3.4 The documented support set

`docs/toolchain-support.md` and `docs/security-policy.md` record Linux,
macOS, and Windows support, with Windows explicitly "functional-only and
trusted/local-content only" (`docs/non-goals.md`). The compile coverage
added here must match the documented support set, not exceed it.

## 4. Invariants that must not regress

- Routine CI must not become so slow that it stops being routine. Compile
  coverage on added platforms is `cargo check`, not `cargo test`.
- The Windows confinement implementation is **not** modified by this
  milestone. This milestone changes CI configuration; it does not touch
  `fs/windows.rs`.
- The `unsafe` boundary in `docs/unsafe-code-policy.md` is unchanged: no
  new `unsafe`, no new approved boundary, no relaxation of the policy.
- The manual `platform-qualification.yml` workflow remains the source of
  runtime evidence for macOS and Windows, and remains `workflow_dispatch`
  only.
- `docs/security-policy.md` and `docs/non-goals.md` keep the Windows
  functional-only limitation. Compile coverage does not upgrade the Windows
  qualification tier.
- The Linux CI job's coverage is not reduced. Nothing is moved out of the
  existing job.
- The `verify.sh fast|full|deep` contract is unchanged; any new local
  check is additive.
- No published artifact changes.

## 5. Scope

### In scope

- Add CI coverage that compiles the workspace (and the excluded Python
  crate) for the Windows and macOS targets on every push and pull request.
- Choose and record the mechanism: hosted runners running a native
  `cargo check`, or Linux-hosted cross-target `cargo check`. The choice
  must be recorded with its trade-offs.
- Include `--all-targets` so the platform-specific **test** code also
  compiles, not only the library.
- Cover the feature lanes that carry platform-specific code (at least
  `tls`, and `http3,tls` if the H3 stack is platform-sensitive), at
  `--all-targets` compile level.
- Add a local, cheap equivalent to `scripts/verify.sh fast` where it is
  practical (for example a `cargo check --target` invocation that the
  developer can opt into), and document the platform requirement.
- Update `docs/toolchain-support.md` and
  `architecture/testing-and-conformance.md` to state what routine CI now
  compiles and what remains manual-only.

### Explicitly out of scope

- Running the Windows or macOS **test** suites in routine CI. That is a
  separate cost decision and is not required for compile coverage.
- Changing the `windows-adversarial-qualification` feature gate or the
  manual qualification workflow.
- Any change to `crates/eggserve-static/src/fs/windows.rs`, to the
  confinement logic, or to the unsafe-code policy.
- Upgrading the Windows security posture from "functional-only" to
  "hardened". That needs the runtime qualification evidence that only the
  manual workflow produces.
- Fixing a platform-specific defect if one is discovered — that becomes a
  separate corrective plan. This milestone reports it.
- The fuzz-harness repair (Milestone 310), the lint-policy milestone
  (313), and the dependency-specification milestone (315).
- Publication.

## 6. Required production changes

### Crates and ownership

No production code changes are expected.

- `.github/workflows/ci.yml`: new job(s) or additional steps for the
  platform compile coverage.
- `scripts/verify.sh`: an optional local equivalent.
- `docs/toolchain-support.md`, `architecture/testing-and-conformance.md`:
  the coverage statement.

If the compile coverage requires a platform-specific target to be
installed in the `rust` job, that is a toolchain change, not a crate
change.

### Config and policy

None.

### Protocol and compatibility

None.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and confinement

- The point of this milestone is to protect the Windows confinement
  implementation from silent bit-rot. It must be described as *compile*
  coverage, not as a security qualification.
- The `docs/security-policy.md` statement that Windows is
  functional-only remains true and must not be softened.

### Documentation and static guards

- `.github/workflows/ci.yml` — the new coverage.
- `docs/toolchain-support.md` — what is compiled where.
- `architecture/testing-and-conformance.md` — the verification strategy
  update.
- `plans/subsystems/cli-ops-observability-roadmap.md` — Milestone 312
  status.
- `plans/registry.md` — the milestone row.

## 7. Ordered work packages

### Work package A — Choose the mechanism and record it

Evaluate and choose between:

- **Hosted runners.** A `windows-latest` job and a `macos-latest` job
  running a native `cargo check --workspace --all-targets`. Highest
  fidelity (the real toolchain, the real target), highest runner cost.
- **Cross-target checks on Linux.** `cargo check --target
  x86_64-pc-windows-msvc` from a Linux runner. Cheap, but MSVC targets
  need an MSVC toolchain or the `gnu` variant, and `cargo check` on a
  foreign target does not always exercise the same code paths as a native
  build.

Record the choice, the reason, and the known limitations. A native
Windows runner is preferred for the `windows` target because the
confinement code is `#[cfg(windows)]` and a cross-check of the wrong
flavor can silently skip it.

Acceptance evidence: the decision is recorded in the closure record with
its trade-offs.

### Work package B — Add the CI coverage

Implement the chosen mechanism as a new job or steps in
`.github/workflows/ci.yml`:

- the workspace compiles for the target with `--all-targets`;
- the excluded Python crate compiles for the target;
- at least the `tls` feature lane compiles for the target;
- the job fails on any error.

Keep the existing Linux job's coverage intact.

Acceptance evidence: the new job appears in the CI run list and its log
shows the target-specific compile.

### Work package C — Prove the gate catches a platform break

Demonstrate that the new coverage fails on a deliberate Windows-only
compile break (for example a `#[cfg(windows)]` item that references a
removed API), then revert the deliberate break.

Acceptance evidence: the failed run is referenced in the closure record.

### Work package D — Local equivalent

Add an opt-in local check to `scripts/verify.sh` (for example
`verify.sh fast` gaining a platform-check step when the toolchain is
available, or a documented standalone command) so a developer can
reproduce the gate without CI.

The check must skip cleanly when the target toolchain is absent, and the
skip must be visible in the output rather than silent.

Acceptance evidence: the script's output states whether the platform
check ran or was skipped and why.

### Work package E — Documentation

Update `docs/toolchain-support.md` and
`architecture/testing-and-conformance.md` to state:

- what routine CI now compiles on which platforms;
- that the Windows and macOS **runtime** qualification remains manual;
- that compile coverage does not change the Windows security posture.

Acceptance evidence: the documents are consistent with
`docs/security-policy.md` and `docs/non-goals.md`.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable; no runtime change.

CI job failures must fail the overall check so the coverage is real, and
the job must not be marked `continue-on-error`.

## 9. Compatibility and migration

- No consumer-visible change and no semver event.
- No published artifact changes.
- The only change operators will notice is that PRs now wait on two more
  compile jobs.

## 10. Required tests

### Focused unit tests

None; no production code changes.

### Integration tests

None beyond the CI compile itself.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- The demonstration in Work package C proves the gate catches a
  Windows-only compile break.

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
# The new coverage, as CI will run it
cargo check --workspace --all-targets
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked

# Existing gates that must stay green
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
./scripts/verify.sh fast
```

The platform-specific commands (for example
`cargo check --workspace --all-targets --target x86_64-pc-windows-msvc`)
require the corresponding runner or toolchain and are recorded as run in
the CI job log, not emulated locally.

## 12. Documentation updates

- `docs/toolchain-support.md` — the compile coverage matrix.
- `architecture/testing-and-conformance.md` — the verification strategy.
- `plans/subsystems/cli-ops-observability-roadmap.md` — Milestone 312
  status.
- `plans/registry.md` — the milestone row.
- `plans/closure/cli-ops-observability/312-cross-platform-compile-coverage.md`
  (new).

## 13. Acceptance criteria

- Routine CI compiles the workspace and the excluded Python crate with
  `--all-targets` for the Windows and macOS targets on every push and pull
  request.
- The CI job fails on any compile error, with no `continue-on-error`.
- The Linux job's coverage is unchanged.
- A deliberate platform-only compile break is demonstrably caught.
- The manual platform-qualification workflow is unchanged and remains the
  source of runtime evidence.
- `docs/toolchain-support.md` and `architecture/testing-and-conformance.md`
  state the coverage accurately and do not imply a Windows security-tier
  change.
- No production code change and no published artifact change.

## 14. Stop conditions

Stop and report rather than improvise if:

- adding the platform jobs makes routine CI materially slow enough to be
  skipped by contributors — narrow the scope (compile only, one feature
  lane) and record the decision rather than dropping the coverage;
- the chosen mechanism cannot actually compile the `#[cfg(windows)]`
  confinement module — verify this explicitly before accepting the
  mechanism, because a check that silently skips the module is worse than
  no check;
- the coverage requires modifying `crates/eggserve-static/src/fs/windows.rs`
  to compile — that is a portability defect and belongs in a separate
  corrective plan; report it with the evidence;
- any Windows-specific defect is discovered — report it; do not fix
  production code in this milestone;
- the maintainer wants the Windows test suites in routine CI — that is a
  larger cost decision and needs its own plan.

## 15. Closure evidence required

Create `plans/closure/cli-ops-observability/312-cross-platform-compile-coverage.md`
containing:

- the mechanism decision and its trade-offs;
- the CI workflow diff;
- the CI run links and the compile output for each added target;
- the demonstration that a deliberate platform break is caught;
- the local-check behavior, including the skip path;
- the documentation diffs;
- any platform-specific defect discovered, with its severity and a
  corrective-plan reference;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

Two failure modes to avoid. First, a cross-target `cargo check` that
silently skips the `#[cfg(windows)]` module is worse than no coverage,
because it looks like coverage — verify that the module is actually
compiled. Second, do not let compile coverage drift into a claim about the
Windows security posture: `docs/security-policy.md` and
`docs/non-goals.md` still say functional-only, and that stays true.
