# Plan 325 — Compatible dependency refresh and supply-chain tool pin maintenance

## 1. Status

- **State**: closed
- **Plan number**: 325
- **Subsystem**: CLI, ops, and observability
  (`plans/subsystems/cli-ops-observability-roadmap.md`, Milestone 325)
- **Source roadmap**: `plans/subsystems/cli-ops-observability-roadmap.md`
- **Long-term requirements**:
  - `plans/000-long-term-specification.md#2` (dependency and verification
    policy; security floors)
  - `plans/001-terminology-and-domain-model.md` (verification vocabulary)
  - `plans/003-planning-process.md` §6 and §10 (required verification and
    planning review)
- **Primary class**: infrastructure (lockfile maintenance) with invariant
  constraints (security floors, MSRV, graph neutrality)

## 2. Repository baseline

Repository baseline: `f137877` (`Plans 320-324: post-audit security
corrective`), branch `feat/auto-20261116-7cc8b1dc`, clean worktree.

A read-only dependency audit on **2026-11-17** found **no advisories** and a
sizeable set of compatible updates that have not been taken:

- Advisory database: `1,527` advisories, snapshot `233155b4` at
  `2026-11-17T11:14:35+01:00`.
- `cargo audit` exits 0 on all three closures (root 182 packages, Python 84,
  fuzz 57); no vulnerability, unsound, unmaintained, or yanked warnings.
- `cargo deny check` passes in both distributed closures
  (`advisories ok, bans ok, licenses ok, sources ok`); the only output is
  `multiple-versions = "warn"` notices.
- `scripts/check-advisory-floors.py` passes with the floors
  `quinn-proto 0.11.19`, `quinn-udp 0.5.16`, `rustix 1.1.6`,
  `tokio 1.53.3`.
- `cargo update --dry-run` offers **66** compatible updates in the root
  closure, **66** in the Python closure, and **28** in fuzz.

Fifteen direct dependencies are behind their newest semver-compatible
release:

| Crate | Resolved | Latest compatible |
|---|---|---|
| `hyper` | 1.11.1 | 1.12.0 |
| `hyper-util` | 0.1.20 | 0.1.21 |
| `http` | 1.4.2 | 1.5.0 |
| `http-body` | 1.0.1 | 1.1.0 |
| `http-body-util` | 0.1.3 | 0.1.5 |
| `tokio` | 1.53.3 | 1.54.1 |
| `tokio-rustls` | 0.26.4 | 0.26.6 |
| `rustls-pki-types` | 1.15.0 | 1.16.0 |
| `quinn` | 0.11.11 | 0.11.12 |
| `futures-util` | 0.3.32 | 0.3.34 |
| `serde` | 1.0.228 | 1.0.229 |
| `serde_json` | 1.0.150 | 1.0.154 |
| `thiserror` | 2.0.18 | 2.0.21 |
| `libc` | 0.2.186 | 0.2.191 |
| `proptest` (dev) | 1.11.0 | 1.12.0 |

Transitives move too (`rustls-webpki 0.103.17`, `h2 0.4.20`, `mio 1.2.5`,
`socket2 0.6.5`, `cc 1.11.2`, `aws-lc-rs 1.19.0`/`aws-lc-sys 0.47.0`, …).

The closures have drifted apart on shared packages, all within semver:

| Crate | Root | Python | Fuzz |
|---|---|---|---|
| `hyper` | 1.11.1 | 1.11.1 | **1.10.1** |
| `http` | 1.4.2 | **1.5.0** | 1.4.2 |
| `http-body` | 1.0.1 | **1.1.0** | 1.1.0 |
| `http-body-util` | 0.1.3 | **0.1.5** | 0.1.4 |
| `futures-util` | 0.3.32 | **0.3.34** | 0.3.32 |
| `thiserror` | 2.0.18 | **2.0.20** | 2.0.18 |
| `libc` | 0.2.186 | **0.2.189** | 0.2.186 |
| `socket2` | 0.6.4 | **0.6.5** | 0.6.5 |
| `rustls-pki-types` | 1.15.0 | **1.15.1** | absent |

Four upgrades are outside the current semver requirements and need their own
scoped plans (all are MSRV-compatible with 1.89, but are source-breaking):

| Crate | Current | Latest | Surface |
|---|---|---|---|
| `pyo3` | 0.29.2 | **0.29.3** patch (in scope) / 0.30.1 (out of scope) | Python wheel bindings |
| `phf` | 0.11.3 | 0.14.0 | static MIME-map authority |
| `rcgen` | 0.13.2 | 0.14.10 | dev/test certificate generation |
| `tokio-tungstenite` | 0.26.2 | 0.30.0 | dev tunnel fixture |

The pinned supply-chain tools are behind as well: `cargo-audit 0.22.2`
(latest `0.23.0`) and `cargo-deny 0.19.0` (latest `0.21.0`), declared in
`scripts/install-cargo-tools.sh` and restated in
`.opencode/skills/eggserve-dev/SKILL.md` and `docs/dependency-policy.md`.

### 2.1 Trial evidence (read-only, throwaway worktree at `f137877`)

The compatible refresh was validated without touching the branch: `cargo
update` in all three closures, then:

- `cargo +1.89 check --workspace --all-targets --locked` — **pass**.
- Workspace tests — **95 test binaries, 0 failures** (≈2,092 tests; the
  known macOS-only `application_server_qualification_297` case was skipped
  because it fails identically at the pristine baseline on macOS).
- Advisory guard, all three audits, and `cargo deny check` — **pass**.
- Key resulting versions: `tokio 1.54.1`, `hyper 1.12.0`, `http 1.5.0`,
  `http-body 1.1.0`, `quinn 0.11.12`, `quinn-proto 0.11.19`,
  `rustls 0.23.46`, `rustix 1.1.6`, `rustls-webpki 0.103.17`,
  `tokio-rustls 0.26.6`; Python `pyo3 0.29.3`.
- The resolution added optional lock-only entries (`borsh`, `borsh-derive`,
  `proc-macro-crate`, `toml_datetime`, `toml_edit`, `toml_parser`,
  `core_detect`, `winnow`). None is built in the production graph
  (`cargo tree -p eggserve-bin -e normal,no-dev` has zero matches).

This trial is guidance, not a contract: versions available at execution time
will be newer, and the executing agent must reproduce the evidence.

## 3. Problem statement

The repository is advisory-clean but not on its newest compatible dependency
set. Three separate lockfiles have drifted, so the published crates, the
Python wheel, and the fuzz workspace can resolve different patch versions of
the same HTTP/TLS stack. The supply-chain tools that enforce the advisory
gate are also two minor lines behind.

None of this is a security defect today. It is maintenance debt that makes
future advisory response slower (a fix must be reconciled against three
different resolutions) and leaves compatible bug fixes on the table. The
repository's own dependency policy is plan-driven, so this refresh needs one
bounded milestone rather than an ad-hoc `cargo update`.

## 4. Goal

Move all three distributed/development closures to the newest semver-
compatible set in one pass, align shared packages across them, keep every
security floor, the MSRV 1.89 promise, and the feature graph intact, and
bring the pinned advisory tools up to their current stable releases with the
scripts adjusted — while leaving breaking upgrades to separate plans.

## 5. Non-goals

- No breaking upgrades: `pyo3 0.30`, `phf 0.14`, `rcgen 0.14`, and
  `tokio-tungstenite 0.30` are out of scope and need their own plans.
- No publication, tag, release, or version bump.
- No manifest requirement changes for dependencies. This is a lockfile
  refresh; the root `[workspace.dependencies]` table, member manifests, and
  the Python manifest stay byte-identical except for the tool pins in
  `scripts/install-cargo-tools.sh`.
- No source, API, behavior, capability, or feature-gate change.
- No new dependency. Optional lock-only packages may appear through
  resolution; they must not be built by any production profile.
- No advisory ignore, floor rollback, `[patch]`, git source, or hand-edited
  lockfile.
- No H3 tier or evidence claim change.
- No CI workflow redesign; runner images and action pins are untouched.

## 6. Scope

### Work package A — Capture the baseline and the drift inventory

Record before any update:

- `cargo update --dry-run` for the root, Python, and fuzz closures.
- Resolved versions for every direct dependency in each lockfile, and the
  drift table for shared packages.
- `cargo audit` on all three lockfiles, `cargo deny check` in both
  distributed closures, the offline floor guard, and the advisory database
  snapshot (commit + timestamp).
- Current tool versions (`cargo-audit`, `cargo-deny`).

Acceptance: the closure record can show an exact before/after for every
changed package and every gate.

### Work package B — Root compatible refresh

Run `cargo update` (no `--precise`) in the root workspace. This is the
intentional drift: every changed package must be attributable to the
resolver, not to manual edits.

Rules:

- Do not edit any manifest.
- If a specific bump is judged too risky for this milestone, pin that crate
  back with `--precise` and record the exception; do not widen a manifest.
- Optional lock-only additions are acceptable and must be checked against the
  production graph (Work package D). Do not delete them by hand.
- `quinn 0.11.11 → 0.11.12` is in scope; H3 stays experimental and its
  protocol crates stay at their released line unless resolution says
  otherwise (record the resulting `h3`/`h3-quinn`/`quinn-proto`/`quinn-udp`
  versions).

Expected direct results from the 2026-11-17 trial (verify at execution):
`hyper 1.12.0`, `hyper-util 0.1.21`, `http 1.5.0`, `http-body 1.1.0`,
`http-body-util 0.1.5`, `tokio 1.54.1`, `tokio-rustls 0.26.6`,
`rustls-pki-types 1.16.0`, `quinn 0.11.12`, `futures-util 0.3.34`,
`serde 1.0.229`, `serde_json 1.0.154`, `thiserror 2.0.21`,
`libc 0.2.191`, `proptest 1.12.0`.

### Work package C — Python and fuzz refresh, then closure alignment

- Python: run `cargo update` in `crates/eggserve-python` from a **standalone
  checkout** (this worktree's nesting makes `cargo metadata` resolve the
  parent repository; a detached worktree outside the parent tree or a normal
  clone avoids it). Expect `pyo3 0.29.3`, `tokio 1.54.1`,
  `hyper 1.12.0`, `http 1.5.0`, `libc 0.2.191`. PyO3 stays on the 0.29 line
  and the wheel stays `cp311-abi3`.
- Fuzz: run `cargo update --manifest-path fuzz/Cargo.toml`; expect
  `tokio 1.54.1`, `hyper 1.12.0`, `http 1.5.0`.
- Alignment: after all three updates, every package present in more than one
  closure must resolve to the same version. Exceptions are legitimate only
  where a closure does not carry the package at all (H3/QUIC crates are
  absent from Python and fuzz; the wheel omits H3). Record the final
  comparison. If a shared package still differs, resolve it with a precise
  update in the lagging closure or record the blocking constraint.
- Rebuild the wheel and run the full Python suite; run the fuzz check and
  clippy.

### Work package D — Prove floors, MSRV, graph neutrality, and tests

- Floor guard passes; the manifest `rustls 0.23.46` floor is untouched.
- `cargo audit` on all three lockfiles exits 0; `cargo deny check` passes in
  both distributed closures; no ignore entry was added.
- `cargo +1.89 check --workspace --all-targets --locked`, the Python crate
  under 1.89, and the fuzz workspace under 1.89 all pass.
- Normalized `cargo tree -e features` deltas contain version strings only;
  no new default feature, no 0-RTT change, no new production package.
- Default `eggserve-bin`/`eggserve-core` graphs still contain no `quinn*`;
  the production TLS graph is still ring-only (`aws-lc` dev-only).
- Full workspace tests; H1 parity and H3 integration targets; H2/TLS and
  H3/TLS feature lanes; Python wheel suite; fuzz targets. The macOS-only
  `application_server_qualification_297::app_bounded_body_accepts_and_over_limit_rejects`
  failure is pre-existing (fails at the pristine baseline on macOS) and is
  not a gate for this milestone; run the suite with that single test skipped
  on macOS and let Linux CI cover it normally.

### Work package E — Supply-chain tool pin bump (independently closable)

- Raise `cargo-audit` and `cargo-deny` pins in
  `scripts/install-cargo-tools.sh` to the newest stable releases at
  execution (audit baseline: `cargo-audit 0.23.0`, `cargo-deny 0.21.0`).
- Adapt `scripts/check-supply-chain.sh` to the pinned tool's CLI. Since
  cargo-deny 0.20 the `--config` option is global, so the Python-closure
  invocation becomes `cargo deny --config ../../deny.toml check`; verify the
  exact form against the installed binary's `--help`, not against memory.
- Check `deny.toml` schema compatibility. Mechanical adjustments are in
  scope; a semantic policy change (new/removed deny categories, changed
  default behavior) is a stop condition.
- Confirm the script still audits both lockfiles and applies the same policy,
  then run it end-to-end.
- This package is independently closable: if 0.21 cannot be made to work
  without semantic changes, keep the 0.19.0/0.22.2 pins, record the blocked
  upgrade with the exact failure, and close the dependency refresh without
  it.

### Work package F — Documentation, changelog, and planning records

- `docs/dependency-policy.md`: update the "Release validation tool versions"
  section and record the refreshed resolved set (or the policy-relevant
  summary) so the document does not name superseded versions.
- `.opencode/skills/eggserve-dev/SKILL.md`: update the tool pins if they are
  restated there.
- `CHANGELOG.md`: an Unreleased maintenance entry stating that no public API,
  capability, behavior, or tier changed.
- `plans/registry.md` and the CLI/ops roadmap status.

## 7. Invariants that must not regress

1. **Security floors hold.** `rustls >= 0.23.46` in every constraining
   manifest; `quinn-proto >= 0.11.19`, `quinn-udp >= 0.5.16`,
   `rustix >= 1.1.6`, `tokio >= 1.53.3` in every lockfile that carries them.
2. **No advisory silence.** All audits and denies pass; both ignore lists
   stay empty; no yanked release is accepted.
3. **MSRV 1.89.** Workspace, Python crate, and fuzz all check under 1.89.
4. **Lockfile-only dependency delta.** No manifest requirement changes; no
   hand-edited lock content; no `[patch]`, git dependency, or alternate
   registry.
5. **No feature-set or capability drift.** The normalized feature trees
   differ only by version strings; no new default feature; 0-RTT stays
   disabled.
6. **Production graph stays lean.** No `quinn*` in default graphs; no
   `native-tls`/`openssl-sys`; `aws-lc` remains dev-only; optional
   lock-only entries are not built.
7. **H3 stays experimental.** No tier, evidence, or protocol-line claim
   changes.
8. **Tool bump preserves gate semantics.** The same lockfiles are audited
   and the same deny policy is applied after the bump.
9. **No publication.** No tag, release, or version bump.

## 8. Failure, cancellation, restart, and contention semantics

- **MSRV conflict**: pin the offending crate back to the newest release that
  checks under 1.89 with `cargo update -p <crate> --precise <version>`, record
  it, and continue. Do not raise the workspace MSRV in this plan.
- **API or behavior break** (a "compatible" bump requires source changes or
  changes observable behavior): stop and split the crate into a separate
  plan. The refresh must not smuggle a source change.
- **New advisory during the refresh**: remediate to a patched release or
  stop; never add an ignore.
- **Yanked release** in the new resolution: pin back or stop.
- **Registry or advisory database unavailable**: record the exact failing
  command; do not hand-edit a lockfile and do not claim a green gate.
- **Python nesting**: run the Python closure's update/check in a standalone
  checkout (see Work package C). This is an environment limitation, not a
  product defect.
- **Tool-pin conflict** (cargo-deny 0.21 schema/CLI incompatibility): keep the
  current pins and record the blocked upgrade; the dependency refresh closes
  without it.
- **Partial update**: leave the tree uncommitted, record which closure or
  crate failed, and do not claim alignment.
- **Optional lock-only additions**: verify they are unbuilt; do not remove
  them manually. If one appears in a production tree, stop and investigate.
- **Concurrent lockfile edits**: no two agents may run `cargo update` in the
  same closure concurrently; the worktree must be clean and serial.

## 9. Compatibility and migration

- Lockfile-only changes do not alter any public Rust or Python API, and no
  currently published artifact changes until the next release.
- The Python wheel is rebuilt and tested; `pyo3 0.29.3` is a patch on the
  same 0.29 line and the abi3 contract is unchanged.
- `quinn 0.11.12` is a patch on the same QUIC line; H3 remains experimental.
- The tool bump affects maintainer tooling only. If cargo-deny's CLI changes,
  the script is adapted; no product behavior changes.
- No migration-guide entry is expected. If any bump produces an observable
  behavior change, stop: that is a separate plan with its own migration note.
- Publication remains a separate maintainer decision; this plan does not
  select a release line.

## 10. Required tests

### Focused

- Floor guard self-test and live check.
- Tool version assertions (`cargo audit --version`, `cargo deny --version`).
- Shared-crate alignment check across the three lockfiles.
- Python abi3 import smoke after `pyo3 0.29.3` (the wheel lane covers it).

### Integration

- `cargo test --workspace` (with the documented macOS-only skip).
- H1 direct/compat parity targets.
- H2/TLS and H3/TLS feature lanes for `eggserve-core` and `eggserve-bin`.
- `bash scripts/test-python-wheel.sh`.
- Fuzz `cargo check --bins --locked`, clippy, and fmt.

### Security and negative

- `cargo audit` on root, Python, and fuzz lockfiles.
- `cargo deny check` in both distributed closures.
- `bash scripts/check-supply-chain.sh` after the tool bump.
- Production graph checks: no `quinn*` in default trees, no
  `native-tls`/`openssl-sys`, no `aws-lc` in `-e no-dev`.

### Compatibility

- `cargo metadata` package delta and normalized `cargo tree -e features`
  delta: versions only, no new production package, no feature change.
- MSRV checks under `+1.89` for workspace, Python crate, and fuzz.
- Python wheel rebuild and installed-wheel suite.

## 11. Required verification commands

```bash
# baseline and after
cargo update --dry-run > /tmp/update-root-before.txt
cargo update --dry-run --manifest-path fuzz/Cargo.toml > /tmp/update-fuzz-before.txt
# Python: run from a standalone checkout
(cd <standalone-checkout>/crates/eggserve-python && cargo update --dry-run)

# refresh
cargo update
cargo update --manifest-path fuzz/Cargo.toml
(cd <standalone-checkout>/crates/eggserve-python && cargo update)

# neutrality
cargo metadata --format-version 1 --locked > /tmp/metadata-after.json
cargo tree -e features --locked > /tmp/features-after.txt
cargo tree -p eggserve-bin -e normal,no-dev | grep -cE 'quinn|borsh|toml_edit|aws-lc|native-tls|openssl'
git diff -- Cargo.lock crates/eggserve-python/Cargo.lock fuzz/Cargo.lock

# security
python3 scripts/check-advisory-floors.py --self-test
python3 scripts/check-advisory-floors.py
cargo audit --file Cargo.lock
cargo audit --file crates/eggserve-python/Cargo.lock
cargo audit --file fuzz/Cargo.lock
cargo deny check
(cd <standalone-checkout>/crates/eggserve-python && cargo deny check)

# MSRV and tests
cargo +1.89 check --workspace --all-targets --locked
(cd <standalone-checkout> && cargo +1.89 check --manifest-path crates/eggserve-python/Cargo.toml --locked)
cargo +1.89 check --manifest-path fuzz/Cargo.toml --bins --locked
cargo test --workspace -- --skip app_bounded_body_accepts_and_over_limit_rejects
cargo test -p eggserve-h3 --locked
cargo test -p eggserve-core --test http3_runtime --features http3,tls --locked
cargo test -p eggserve-core --features http2,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo fmt --all -- --check
bash scripts/test-python-wheel.sh

# tool pins
bash scripts/install-cargo-tools.sh
cargo audit --version
cargo deny --version
bash scripts/check-supply-chain.sh

# repository gates
python3 scripts/check-crate-topology.py --self-test
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
./scripts/verify.sh fast
```

Network access to crates.io and the RustSec advisory database is required.
The Python wheel lane needs Python 3.14 + maturin.

## 12. Documentation updates

- `docs/dependency-policy.md` — refresh the resolved-state note and the tool
  version pins.
- `.opencode/skills/eggserve-dev/SKILL.md` — tool pins, if restated.
- `CHANGELOG.md` — Unreleased maintenance entry (no API/behavior change).
- `plans/subsystems/cli-ops-observability-roadmap.md` — milestone status.
- `plans/registry.md` — status and closure pointer.
- `AGENTS.md` only if it restates a changed version (it currently does not).

## 13. Acceptance criteria

- Root, Python, and fuzz closures resolve the newest semver-compatible set
  available at execution time; every changed package is recorded.
- Shared packages resolve identically across the closures that carry them.
- All three audits are clean; both denies pass; the floor guard passes; no
  advisory ignore, yanked release, or floor rollback exists.
- MSRV 1.89 checks pass for the workspace, the Python crate, and fuzz.
- The normalized feature trees differ only by version strings; the default
  graphs contain no `quinn*`; production TLS stays ring-only; no optional
  lock-only package is built.
- Workspace tests, H1/H2/H3 lanes, the installed-wheel suite, and fuzz
  checks pass.
- The pinned advisory tools are on the newest stable releases and
  `check-supply-chain.sh` passes end-to-end, or the tool package is recorded
  blocked with the exact failure while the dependency refresh closes.
- No breaking upgrade, manifest requirement change, source change,
  publication, or version bump is included.

## 14. Stop conditions

Stop and report rather than improvise when:

- a compatible update cannot compile or pass tests under MSRV 1.89 and no
  `--precise` pin-back within the same requirement preserves the refresh;
- a "compatible" bump requires a source/API change or changes observable
  behavior;
- a new advisory or yanked release appears and cannot be remediated within
  the compatible set;
- the production graph gains a new package, a new default feature, or a
  non-ring TLS provider;
- cargo-deny's new pin requires a semantic `deny.toml` change, not a
  mechanical one;
- the registry or advisory database is unavailable;
- the Python wheel cannot be built or its abi3 contract changes;
- shared-package alignment requires changing a manifest requirement.

## 15. Closure evidence required

- `cargo update --dry-run` before/after and the full list of changed packages
  per closure.
- Lockfile diffs summarized by version/checksum/edge change; no manifest
  changes.
- `cargo metadata` and normalized feature-tree deltas (versions only).
- Default/production graph assertions (no QUIC, no non-ring TLS, no
  lock-only optional packages built).
- All three `cargo audit` outputs, both `cargo deny` outputs, the guard
  self-test and live output, and the advisory database snapshot.
- MSRV 1.89 outputs for workspace, Python crate, and fuzz.
- Workspace/H1/H2/H3/Python-wheel/fuzz test outputs, including the recorded
  macOS-only skip.
- Tool pin diff, `--help` verification of the deny invocation, and a full
  `check-supply-chain.sh` run.
- Shared-crate alignment table before/after.
- Residual risk and any pinned-back exceptions.

## 16. Handoff notes

- The 2026-11-17 trial at `f137877` established that the then-current
  compatible set passes MSRV 1.89 and the full workspace tests (95 binaries,
  0 failures). Re-run everything at execution time; the registry will offer
  newer releases.
- Use `--precise` only to pin something **back** for MSRV or risk. The bulk
  refresh is a plain `cargo update`.
- The Python crate's `cargo metadata` resolves the parent repository from a
  nested worktree. Run the Python closure's update/check in a checkout
  outside the parent tree; do not "fix" this by adding a workspace table to
  the excluded crate.
- cargo-deny 0.20+ moved `--config` to the global option. Verify against the
  pinned binary's `--help`; do not trust either the old or new script form
  from memory.
- `cargo update` may add optional lock-only packages (`borsh`, `toml_edit`)
  even though no profile builds them. That is resolution, not a dependency
  addition; prove them absent from production trees instead of deleting lock
  entries.
- The 2026-11-17 audit found no advisories. A green audit at the start does
  not authorize skipping the post-update audits — newer resolutions have
  exposed new advisories before.
- Do not run `cargo update` in the same closure concurrently with another
  agent; the lockfile is the artifact under test.
- Publication is not part of this milestone. If a consumer-visible fix is
  desired after the refresh, that is a separate release plan.
