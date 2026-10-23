# Plan 318 — Raise the manifest `rustls` floor and stage the TestPyPI publication path

## 1. Status

- **State**: active — implementation in progress
- **Plan number**: 318
- **Subsystem**: CLI, ops, and observability (`plans/subsystems/cli-ops-observability-roadmap.md`)
- **Source roadmap**: `plans/subsystems/cli-ops-observability-roadmap.md`
- **Primary class**: dependency-policy (security floor) + release-operations (staging)

## 2. Repository baseline

At the start of this plan the repository is at the Plan 317 commit line, with
Plan 317 (Windows runtime qualification and corpus seed hygiene) closed.

Two residual items were recorded in the Plan 305 closure record
(`plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`) §10 and
never actioned:

1. The manifest `rustls` floor stayed at `0.23.45` while the resolved version
   is `0.23.46`. Four RUSTSEC advisories (RUSTSEC-2026-0346, -0347, -0348,
   -0349) are fixed in `0.23.46`. A consumer who pins `rustls = "=0.23.45"`
   explicitly can still unify to the vulnerable release alongside these
   crates, because these crates accept it.
2. The TestPyPI trusted publisher is unconfigured. The `0.4.0` release run
   failed on a TestPyPI trusted-publishing exchange failure; the corrective
   `0.5.0` run used `publish_target=pypi`, which skips the TestPyPI job
   entirely, so no staging-package verification has ever preceded a
   production PyPI upload.

## 3. Problem statement

The `rustls` floor is the one dependency constraint in this repository that is
declared as security-sensitive and simultaneously under-enforced. Every other
floor in the workspace table exists to prevent a *build* from resolving an
unintended version; this one exists to prevent a *consumer* from resolving an
unintended version, and it does not do so while it sits below the patched
release.

The TestPyPI staging path is the release process's only defence against a
malformed or mis-built wheel reaching production PyPI, and it has never run
successfully.

## 4. Goal

Make the declared `rustls` floor match the patched release, and make the
TestPyPI staging lane executable and evidenced.

## 5. Non-goals

- Not a change to any other dependency floor, feature set, or lockfile
  resolution.
- Not a change to the TLS identity, trust, client-auth, or reload behaviour
  in `eggnet-tls`.
- Not a publication. Publication remains a manual maintainer action
  (`AGENTS.md`: "no push/tag/merge ever publishes").
- Not a change to the release workflow's structure. The TestPyPI job already
  exists and is correctly wired; what is missing is the account-side
  registration.

## 6. Scope

### Work package A — Raise the manifest floor

Raise the `rustls` requirement from `0.23.45` to `0.23.46` in:

- the root `[workspace.dependencies]` table, which every workspace member that
  constrains `rustls` inherits;
- `crates/eggserve-python/Cargo.toml`, which is excluded from the workspace
  and therefore carries its own explicit specification.

Update every narrative reference to the floor: `AGENTS.md`,
`.opencode/skills/eggserve-dev/SKILL.md`, `docs/dependency-policy.md`,
`docs/migration-guide.md`, `architecture/eggserve-h3.md`,
`architecture/eggnet-tls.md`.

### Work package B — Prove the change is resolution-neutral

The resolved version is already `0.23.46` in all three lockfiles, so raising
the floor must not change any resolution. Prove it:

- all three lockfiles byte-identical before and after;
- the resolved package set unchanged (`cargo metadata`);
- the resolved *feature* set unchanged — the lockfile records versions, not
  features, so the feature tree is the real guard.

### Work package C — Stage the TestPyPI publication path

- Establish the current state of the GitHub-side configuration: the `testpypi`
  environment and the release workflow's TestPyPI job.
- Record the exact remaining maintainer step (registering the trusted
  publisher on the TestPyPI account) and the exact command that exercises it
  afterwards.
- If the account-side registration cannot be made from the build environment,
  say so plainly and record the step as an open operational item rather than
  claiming a staging run that did not happen.

### Work package D — Record the republication that the floor raise creates

`eggnet-tls` 0.2.0 is published with the `0.23.45` floor. Raising the floor
in the workspace table changes `eggnet-tls`'s manifest, so its content no
longer matches the published artifact and it needs a republication for the
raised floor to reach consumers of that crate.

## 7. Invariants that must not regress

1. **The resolved dependency set does not change.** No crate gains, loses, or
   upgrades a dependency as a result of this plan.
2. **The resolved feature set does not change.** The `rustls` provider
   selection (`ring`), the `std` feature, and the `tokio-rustls` feature set
   are unchanged.
3. **The floor is declared once** in the root `[workspace.dependencies]` table
   and inherited by every member that constrains `rustls`; the excluded
   Python crate keeps its own explicit specification and its own lockfile.
4. **No publication happens as a result of this plan.**
5. **The floor never moves backwards.** Raising it is the point; a later
   regression below `0.23.46` re-exposes the four advisories.

## 8. Recorded material deviation

**The account-side TestPyPI registration cannot be completed from the build
environment**, and it is recorded here as a scoped deferral with the exact
remaining step — not as a completed staging run. The GitHub-side
configuration was established and verified: the `testpypi` environment exists
in `eggstack/eggserve`, and the release workflow's `publish-testpypi` job
targets `https://test.pypi.org/legacy/` with `id-token: write` and
`environment: testpypi`. What remains is registering the trusted publisher
on the TestPyPI account for `eggstack/eggserve`, workflow `release.yml`,
environment `testpypi` — the same three fields the production `pypi`
publisher uses, pointed at TestPyPI. Registering it requires a human with
TestPyPI account access; this repository does not control that service.

**Work package A added a work package that the plan did not name.**
`docs/dependency-policy.md` states that "a static guard must protect the
floor", and no such guard existed in any script — the policy described a
check the repository did not have. The guard was added
(`check_plan318_rustls_floor`) because the plan's own goal, making the floor
mean something, is not met by a floor nothing checks.

## 9. Verification strategy

- `git diff --stat -- '*Cargo.lock'` is empty across all three lockfiles.
- `cargo metadata --format-version 1` package set is unchanged before/after.
- `cargo tree -e features -p <crate>` is unchanged before/after for every
  crate that constrains `rustls`.
- `cargo check --workspace --all-targets`,
  `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`,
  `cargo check --manifest-path fuzz/Cargo.toml --bins --locked`.
- `cargo audit --file Cargo.lock`, `--file crates/eggserve-python/Cargo.lock`,
  `--file fuzz/Cargo.lock` — clean.
- `python3 scripts/check-crate-topology.py` and `--self-test`.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`.

## 10. Stop conditions

Stop and escalate rather than continue if any of the following holds.

- Raising the floor changes a resolved version or a feature set. That would
  mean the lockfiles were not already on the patched release, and the
  dependency picture needs a separate decision.
- The floor raise breaks a downstream compatibility proof in the release
  workflow's registry-only consumer checks.
- The TestPyPI staging run reaches the upload step and fails for a reason
  other than the unconfigured publisher.

## 11. Risks and decision points

- **Risk**: a consumer that pins `rustls = "=0.23.45"` can no longer use these
  crates. That is the intended effect — the pinned version carries four
  advisories — but it is a compatibility event and belongs in the migration
  guide.
- **Decision point**: whether the floor raise ships in the same release as
  the `eggserve-static` patch that is already owed, or as its own release.
  The static patch is a patch-level change to one crate; the floor raise
  touches every TLS-capable crate and `eggnet-tls`. They are separate
  releases unless the repository is already batching.
- **Decision point**: the TestPyPI registration is an account-side action on
  a service this repository does not control. It cannot be automated from the
  build environment, and pretending otherwise would produce a closure record
  that claims evidence it does not have.

## 12. Dependencies

- Plan 305 published the corrective `0.5.0` line and recorded both residual
  items in its §10.
- Plan 315 consolidated the dependency specifications into the root
  `[workspace.dependencies]` table, which is where the floor now lives.

## 13. Closure requirements

A closure record under `plans/closure/cli-ops-observability/` recording: the
floor values before and after in every manifest and every narrative
reference; the lockfile, package-set, and feature-set evidence; the exact
state of the GitHub-side TestPyPI configuration; the exact remaining
maintainer step if the account-side registration was not completed; and the
`eggnet-tls` republication that the change creates.

## 14. Implementation notes

The floor raise itself is two version literals. Everything else is the
narrative references and the missing guard.

The resolution-neutrality proof is worth stating precisely, because it is
the point of the change: the lockfiles were **already** on `0.23.46`, so the
advisory scan had been clean and the gap had gone unnoticed. A clean
`cargo audit` says the *resolution* is safe; it says nothing about what the
crates *accept*. `docs/dependency-policy.md` now states that distinction
explicitly.

## 15. Verification evidence

- Lockfile neutrality: all three lockfiles byte-identical before and after
  (`sha256`).
- Feature-tree neutrality: `cargo tree -e features` unchanged for the
  workspace and for the excluded crate.
- The new rule fails closed: a deliberately regressed floor in the excluded
  crate and in the workspace table were each caught and the tree restored.
- Topology self-test: 89 checks passed (83 → 89 with this milestone's five
  mutations).
- `cargo audit` over all three lockfiles: clean.
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`
  and `cargo check --manifest-path fuzz/Cargo.toml --bins --locked`: clean.
- `cargo test --workspace`: exit 0. `cargo clippy --workspace --lib --bins
  --tests -- -D warnings`: clean. `cargo fmt --all -- --check`: clean.
- Hosted CI: run `39927249552`, all five jobs green.

## 16. Rollback

The floor raise is a single-line change in each of two manifests plus the
narrative references; reverting both manifests restores the previous floor
and the previous lockfile-compatible resolution. The documentation changes
revert independently.
