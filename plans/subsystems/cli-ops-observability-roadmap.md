# CLI, Ops, and Observability Roadmap

Status: proposed

Long-term references:

- `plans/000-long-term-specification.md#2` (safe defaults; operator-visible diagnostics)
- `plans/001-terminology-and-domain-model.md` (`OpsContext`, `RuntimeConfig`)
- `plans/002-long-term-roadmap.md` (phase 5)
- `plans/003-planning-process.md` (verification and guard requirements)

Related ADRs:

- none yet (no accepted ADR governs the tooling/verification boundary)

## 1. Purpose and ownership boundary

Owns the operator-facing and repository-facing surfaces: the `eggserve-bin`
CLI grammar and behavior, the structured ops/logging authority, the
verification harness (fuzz targets, conformance corpora, platform
qualification), the CI job graph, the static lint policy, the supply-chain
gates, and dependency-version specification hygiene.

Owns: `crates/eggserve-bin/`, `scripts/`, `.github/workflows/`,
`scripts/check-crate-topology.py`, `scripts/verify-conformance-matrix.py`,
`scripts/check-supply-chain.sh`, `deny.toml`, `audit.toml`, and the
workspace dependency-specification surface (`[workspace.dependencies]`).

Does not own: runtime behavior, protocol framing, static/path/filesystem
decisions, TLS identity, or the Python bridge semantics. Those stay in
their owning subsystems.

Consumes: every crate (as a consumer for verification), but must never
change another subsystem's behavior to satisfy a tooling need.

## 2. Work classification

### Invariants

- No `println!`/`eprintln!` in library code; runtime code emits through
  `OpsContext`.
- `Logger` stays a `OnceLock` initialized with `try_init()`; never
  `init()` twice.
- The CLI grammar stays `[OPTIONS] [PORT] [DIRECTORY]` with manual parsing
  (no clap); a directory after an occupied port slot is verbatim even if
  numeric.
- Routine CI must not silently stop compiling or linting a code path that
  the product can build.
- Dependency floors that respond to an advisory (currently the `rustls`
  `0.23.45` caret floor for RUSTSEC-2026-0285) stay present in **every**
  constraining manifest, including the excluded Python crate, which is not
  a workspace member and therefore cannot inherit a workspace table.
- `deny.toml` wildcard and `native-tls`/`openssl-sys` bans stay green for
  both lockfiles; `aws-lc-rs` remains dev-only and the production tree stays
  ring-only.
- Verification claims are never made from an unrun command.

### Capabilities

- `eggserve-bin` CLI: static-only server with the documented option set.
- `verify.sh fast|full|deep` as the ordered verification entry point.
- Topology gate, conformance-matrix gate, Python release-metadata gate, and
  supply-chain gates as executable static guards.
- CI job graph (rust / supply-chain / python) plus the manual
  platform-qualification workflow.

### Infrastructure

- Fuzz harness crate (`fuzz/`) and its committed corpus.
- Conformance inventories (`conformance/*.toml`, `corpus.json`,
  `body_corpus.json`).
- Workspace `[workspace.dependencies]` specification surface.
- Lint policy (`missing_docs`, `undocumented_unsafe_blocks`) and the
  workspace `[lints]` table.

### Polish

- Job-graph parallelism and evidence ergonomics.
- Diagnostic wording for gate failures.

## 3. Non-goals

- No CLI feature expansion beyond the documented grammar.
- No new verification framework that replaces `verify.sh` or the existing
  gate scripts.
- No dependency added, removed, or version-floored as part of tooling
  hygiene work without an explicit advisory or dependency-policy reason
  (`docs/dependency-policy.md`).
- No change to published crate behavior from this subsystem.

## 4. Current state

The tooling surface is strong on policy and weak on coverage breadth.

- `scripts/verify.sh` encodes the required ordering (cheap topology and
  metadata checks before builds) with `fast`, `full`, and `deep` scopes.
- `scripts/check-crate-topology.py` enforces crate ownership and includes a
  `--self-test` mutation suite (25 checks) that exercises the gate against
  synthetic fixture trees.
- `scripts/verify-conformance-matrix.py` validates the shape of the
  conformance inventories (ids, categories, transports, consumers, routine
  flags, evidence strings). All 55 `app_server_conformance.toml` scenarios
  currently reference evidence files that exist.

Known gaps at the baseline:

- **Routine CI compiles only `ubuntu-latest`.** All rust jobs in
  `.github/workflows/ci.yml` run on `ubuntu-latest`. The Windows
  handle-relative filesystem implementation
  (`crates/eggserve-static/src/fs/windows.rs`, roughly 2,800 lines with
  roughly 42 `unsafe` blocks) is therefore never compiled by routine CI;
  Windows and macOS evidence comes only from the manual
  `workflow_dispatch`-only `platform-qualification.yml`. The Windows
  adversarial suite is additionally gated behind
  `cfg(all(windows, feature = "windows-adversarial-qualification"))`.
- **The fuzz harness does not build.** `cargo check --bins` in `fuzz/`
  fails for four of eleven targets (`range_header`,
  `fuzz_header_block`, `fuzz_normalize_response`, `fuzz_request_body`),
  and `fuzz_directory_buffer` is `#![cfg(windows)]`-gated, so it is a no-op
  on Linux and references a module (`eggserve_core::fs::windows`) that was
  deleted when filesystem ownership moved to `eggserve-static`. Nothing in
  routine CI builds this crate, so the breakage is invisible. The
  `static-confinement` roadmap's verification strategy claims "11 fuzz
  targets", which is not currently true.
- **The lint policy is unenforced.** `missing_docs` and
  `undocumented_unsafe_blocks` appear zero times in the workspace, although
  `docs/unsafe-code-policy.md` names specific approved unsafe boundaries and
  the crates are published API surfaces.
- **The `python-bindings-internal` feature is never linted.** It appears
  zero times in `.github/` and `scripts/`, so the Python-only internal code
  path escapes clippy entirely.
- **Dependency versions are repeated per manifest.** There is no
  `[workspace.dependencies]` table; the `rustls` `0.23.45` floor is
  restated in every constraining manifest, which is the drift risk the
  agent guide warns about.
- `docs/fuzzing.md` cites `crates/eggserve-core/src/path/decode.rs` and
  `crates/eggserve-core/src/path/platform.rs`, both of which were deleted
  when path authority moved to `eggserve-static`.

## 5. Target architecture

Every code path the product can build is compiled and linted by routine CI
on the platforms the product claims to support, at minimum as a
cross-target `cargo check`. The verification harness is built by CI, not
only by hand. The lint policy that the documented unsafe and API policies
imply is enforced mechanically. Dependency specifications are declared
once per workspace and restated only where a manifest is not a workspace
member.

## 6. Dependency graph

```text
platform coverage + lint policy + dependency specs (independent)
    |
    `--> routine CI evidence completeness (future)
```

These milestones have no hard dependency on other open milestones; they
depend only on closed code state.

## 7. Milestones

### Milestone 312 — Cross-platform compile coverage

Class: invariant

Objective: ensure the hardened filesystem path compiles on every platform
the product claims to support, on every push, without depending on a
manual qualification dispatch.

Dependencies: none beyond the closed code state.

Deliverable boundary: CI compile coverage for the Windows and macOS
targets; no runtime behavior change; the manual platform-qualification
workflow remains the source of *runtime* evidence.

User or operator value: a Windows-only compile break is caught at PR time
instead of at the next manual qualification run, months later.

Exit conditions: routine CI compiles the workspace for the Windows and
macOS targets (including the Windows filesystem module) on every push; the
manual qualification workflow is unchanged; no published artifact changes.

### Milestone 313 — Lint policy enforcement

Class: invariant

Objective: enforce the documentation and unsafe-comment policies that the
crate contracts already promise, and bring the Python-only feature path
under clippy.

Dependencies: none beyond the closed code state.

Deliverable boundary: workspace lint configuration; doc comments added
where the lints require them; a clippy lane for
`python-bindings-internal`; no behavior change.

User or operator value: published API items carry doc comments, every
`unsafe` block carries a `SAFETY:` justification, and the Python-internal
path cannot rot unnoticed.

Exit conditions: the lints are enabled and the workspace is clean under
them; the Python-internal feature lane runs in CI; no behavior change is
observable.

### Milestone 315 — Dependency specification consolidation

Class: polish

Objective: declare shared dependency versions once per workspace so that
advisory floors and shared versions cannot drift between manifests.

Dependencies: none beyond the closed code state.

Deliverable boundary: `[workspace.dependencies]` table plus manifest
references; the excluded Python crate keeps explicit specifications
because it is not a workspace member; resolved versions unchanged.

User or operator value: one place to raise a floor when an advisory lands,
instead of N manifests.

Exit conditions: both lockfiles are byte-identical before and after the
change; `cargo deny`/`cargo audit` stay green; no resolved dependency
version changes.

## 8. Cross-cutting requirements

### Storage and migration

None.

### Protocol and compatibility

None; tooling changes must not alter wire behavior.

### Security and authorization

Lint and dependency work must not weaken `docs/security-policy.md` or
`docs/threat-model.md`. Cross-platform compile coverage raises assurance
for the Windows confinement path; it must not be described as a substitute
for runtime qualification.

### Concurrency, cancellation, and recovery

Not applicable to the tooling milestones themselves; the platforms brought
under compile coverage must not be given behavior that differs from the
Linux path.

### Observability and audit

Gate failures must name the failing check and the evidence required. CI
job names remain stable so release evidence can cite them.

### Performance and resource use

Added CI jobs must not make routine PR latency unacceptable; prefer
cross-target `cargo check` over full test execution on added platforms.

### Documentation and operations

`docs/fuzzing.md`, `docs/dependency-policy.md`, `docs/toolchain-support.md`,
and `architecture/testing-and-conformance.md` must describe the actual
harness and platform coverage.

## 9. Verification strategy

CI job-graph inspection, cross-target `cargo check` runs, the fuzz crate
build, `python3 scripts/check-crate-topology.py` (plus `--self-test`),
`python3 scripts/verify-conformance-matrix.py`,
`bash scripts/check-supply-chain.sh`, `cargo fmt --all -- --check`, and
`./scripts/verify.sh fast`.

## 10. Risks and decision points

- Adding Windows and macOS runners to routine CI increases PR latency and
  consumes runner minutes. The mitigation is compile-only coverage on those
  platforms, with runtime evidence remaining a manual, gated activity.
- Enabling `missing_docs` workspace-wide may surface a large number of
  undocumented public items. Remediation must be additive documentation
  only; items that should not be public should be made private rather than
  documented as public API.
- Introducing `[workspace.dependencies]` must not change any resolved
  version. Any lockfile delta is a stop condition, not a cleanup item.

## 11. Completion definition

Every platform the product claims to support compiles in routine CI, every
buildable code path is linted, the documented lint policy is enforced
mechanically, and dependency specifications are declared once without
changing any resolved version.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 312 | closed | `plans/implementation/cli-ops-observability/312-cross-platform-compile-coverage.md` | `plans/closure/cli-ops-observability/312-cross-platform-compile-coverage.md` | Routine CI now compiles the workspace (base, `http2,tls`, `http3,tls`) and the excluded Python crate on native `windows-latest` and `macos-latest` runners. Compile-only: runtime qualification stays manual and the Windows posture stays functional-only. The gate was proven to catch a deliberate `#[cfg(windows)]` break (run `39820541031`: the Windows leg failed, the macOS leg stayed green). |
| 313 | closed | `plans/implementation/cli-ops-observability/313-lint-policy-enforcement.md` | `plans/closure/cli-ops-observability/313-lint-policy-enforcement.md` | `missing_docs` and `clippy::undocumented_unsafe_blocks` are enabled in the root `[workspace.lints]` table; 539 undocumented public items and 23 missing `SAFETY:` comments were remediated; the `python-bindings-internal`, excluded-Python-crate, and Windows-confinement clippy lanes are in CI. The plan's lint name was wrong — the lint is `clippy::undocumented_unsafe_blocks`, not a rustc or rustdoc lint — and the deviation is recorded in the plan and the closure record. |
| 315 | closed | `plans/implementation/cli-ops-observability/315-dependency-specification-consolidation.md` | `plans/closure/cli-ops-observability/315-dependency-specification-consolidation.md` | A root `[workspace.dependencies]` table owns every shared dependency version and the `rustls` `0.23.45` floor; the excluded Python crate keeps explicit specs. All three lockfiles are byte-identical and the resolved package and feature sets are unchanged. The plan's `default-features` guidance was factually backwards and is recorded as a deviation. |
