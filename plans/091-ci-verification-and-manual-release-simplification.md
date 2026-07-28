# Plan 091 — CI, Verification, and Manual Release Simplification

## Goal

Reduce eggserve's CI, verification, and release apparatus to a proportionate system that preserves substantive correctness coverage while restoring fast iteration.

This plan intentionally removes the repository's bespoke release-certification framework. Eggserve is a small, narrowly scoped Rust static-file server and library with a Python package. It does not require a machine-readable ontology of every assertion, exact-SHA evidence aggregation, generated release checklists, profile-promotion automation, or GitHub-hosted registry publication.

Completion of this plan means:

- routine pull-request and `main` CI is small, fast, readable, and limited to high-signal correctness checks;
- expensive, platform-specific, adversarial, packaging, fuzz, proxy, soak, and qualification suites remain available but run locally or through explicit manual workflows;
- ordinary test output is the evidence of ordinary CI execution; CI no longer emits a second structured evidence system;
- no generated checklist, closure report, embedded commit SHA, evidence manifest, or profile-promotion record is required for a green build;
- no GitHub Actions workflow publishes to crates.io, PyPI, or GitHub Releases;
- crates.io publication is performed manually by a maintainer from a local trusted environment;
- release cadence is a maintainer decision and is not triggered by merges, pushes, tags, schedules, or CI state;
- documentation accurately distinguishes routine CI, optional deep verification, and manual release procedures;
- substantive Rust, Python, HTTP, filesystem, security, and interoperability tests are preserved unless independently shown to be redundant or invalid.

This is a reductive infrastructure plan. It is not permission to weaken product behavior, remove core correctness tests, broaden scope, or lower documented security defaults.

## Policy decision and supersession

This plan establishes a new repository policy and supersedes conflicting CI, evidence, qualification-orchestration, and automated-publication requirements in earlier plans, including the relevant portions of Plans 039, 044–046, 086, 089, and 090.

The supersession is intentionally narrow:

- preserve product implementation and real behavioral test coverage created by those plans;
- preserve accurate platform limitations and security caveats;
- preserve useful manual qualification harnesses;
- remove the requirement that every test concern be represented as a release gate;
- remove the requirement for exact-SHA evidence bundles and generated release checklists;
- remove profile promotion as a machine-enforced release prerequisite;
- remove automated publication and GitHub release assembly;
- remove routine execution of expensive qualification work from PR and `main` CI.

Older plan documents remain historical records. Their infrastructure requirements must not continue to control current CI or release policy after this plan lands.

## Why this plan is required

The current repository has crossed from thorough verification into a self-maintaining certification subsystem.

At the start of this plan, the repository includes approximately:

- a 577-line primary CI workflow;
- a separate release workflow of roughly 380 lines;
- a 2,600-plus-line release-gate registry with 135 nominal gates;
- an approximately 850-line local release validator;
- a CI evidence wrapper for every command;
- generated JSON evidence artifacts uploaded by most jobs;
- a final aggregation job depending on the entire job graph;
- generated release checklists and exact-SHA closure reports;
- tooling and tests dedicated to validating the gate/evidence system itself;
- repeated artifact builds and repeated execution of overlapping tests;
- tag-triggered crates.io, PyPI, and GitHub Release publication.

Recent commits have repeatedly repaired cleanup traps, evidence output, generated checklists, workflow expressions, artifact staging, and CI retriggers. These failures are primarily failures of the verification apparatus rather than failures of eggserve behavior.

The resulting costs are unacceptable for this repository:

1. Small implementation changes trigger a large, slow job graph.
2. The same workspace and package are built repeatedly in different jobs.
3. Many nominal gates map to tests already run by `cargo test --workspace`.
4. CI success depends on evidence wrappers, artifact upload/download, generated-file synchronization, and aggregation logic.
5. Documentation-only or planning changes can invalidate generated release state.
6. Release infrastructure requires ongoing maintenance despite the intended manual release cadence.
7. Agents spend iterations fixing CI mechanics instead of product correctness.
8. The complexity obscures which checks actually catch defects.

The correct response is deletion and consolidation, not another abstraction layer over the existing system.

## Scope firewall

Do not use this plan to:

- remove or weaken safe defaults;
- remove path-confinement, HTTP framing, request-body, lifecycle, timeout, logging, TLS, or resource-bound tests merely because they are numerous;
- change supported public APIs;
- add ASGI, WSGI, framework, routing, proxy, ACME, HTTP/2, HTTP/3, WebSocket, virtual-hosting, or edge-platform functionality;
- add a new task-runner framework, build system, CI generator, workflow generator, or release-management service;
- replace the current evidence framework with a different evidence framework;
- introduce release bots, release PR automation, semantic-release, release-plz, cargo-release automation, or tag-driven publication;
- require Docker, Nix, Bazel, just, Make, tox, nox, or another orchestration dependency solely for verification;
- make deep local verification a merge prerequisite;
- claim Windows or production-profile guarantees beyond what the retained tests and documentation support;
- delete historical plan files solely to make the repository look smaller.

The target system should be understandable from the workflow and one small verification script without first reading a release architecture document.

## Governing principles

1. **Tests express behavioral claims.** CI does not need a separate gate for every assertion already represented by a test case.
2. **Routine CI optimizes iteration.** It should catch common regressions quickly, not emulate a release certification campaign.
3. **Deep verification is explicit.** Expensive or environment-sensitive checks run when their signal is useful.
4. **One execution produces one result.** A command's normal exit status and log are authoritative; no JSON translation layer is required.
5. **No evidence about evidence.** Do not upload per-command evidence, aggregate manifests, or validate generated verification state.
6. **No publication from CI.** Registry credentials and release authority remain outside GitHub Actions.
7. **Manual release means manual cadence.** A maintainer decides whether and when to publish after reviewing the current repository state.
8. **Platform checks follow platform changes.** Windows-specific qualification belongs with Windows filesystem work and release preparation, not every unrelated commit.
9. **Failure labels come from test names and job steps.** Separate gate registries are not needed for diagnosability.
10. **Deletion is preferred to compatibility shims.** Remove obsolete infrastructure rather than preserving adapters that keep it alive.
11. **Documentation is descriptive, not executable release state.** Human-readable support and release policy must not become generated CI inputs.
12. **The smallest correct mechanism wins.** Every retained script or workflow must justify its maintenance cost through direct defect-detection value.

## Required end state

### Routine CI

There must be one routine workflow, `.github/workflows/ci.yml`, triggered by:

- pull requests targeting `main`;
- pushes to `main`.

It should contain no more than two blocking jobs:

1. `rust`
2. `python`

The `python` job may be omitted temporarily only if the Python package is explicitly removed from supported deliverables in a separate decision. This plan assumes Python remains supported.

Both jobs should run on Ubuntu. The workflow should retain concurrency cancellation for superseded runs and minimal permissions.

Routine CI must not:

- use an OS matrix;
- run scheduled jobs;
- publish packages;
- upload release evidence;
- download evidence from other jobs;
- produce generated checklists;
- run benchmarks;
- install Caddy or nginx;
- run soak tests;
- run adversarial Windows qualification;
- build cross-target release archives;
- generate SBOM/provenance bundles;
- inspect or assemble GitHub release assets;
- require a clean Git tree beyond the naturally clean checkout;
- read `release/criteria.toml` or any replacement gate registry.

### Manual/platform verification

Platform-specific checks may be provided through one small manual-only workflow, such as `.github/workflows/platforms.yml`, with `workflow_dispatch` as its only trigger.

That workflow may run a simple macOS/Windows matrix for:

- workspace compile/tests;
- platform-specific tests naturally selected by `cfg`;
- optional installed-wheel smoke tests when explicitly requested.

It must not be required by branch protection. It must not upload evidence manifests. It should not reproduce the entire Linux deep-verification suite.

A manual workflow is optional. If local macOS and Windows systems are available and documented, deleting all non-routine workflows is acceptable and simpler.

### Local verification

There must be one small human-readable script, preferably `scripts/verify.sh`, with these modes:

- `fast`
- `full`
- `deep`

The script should stream command output, stop on the first failure, and return the original nonzero status.

It must not:

- generate JSON evidence;
- calculate evidence freshness;
- inspect candidate SHA validity;
- implement waivers;
- maintain dependency graphs between gates;
- generate Markdown;
- create release manifests;
- refuse routine development checks merely because the working tree is dirty;
- dynamically evaluate arbitrary commands from TOML;
- contain a second test inventory that must be synchronized with CI.

The script should remain approximately 100–150 lines or less unless a concrete portability requirement justifies more.

### Release

There must be no automated release workflow.

Crates.io release is performed manually from a maintainer-controlled environment. The repository may provide a concise runbook and simple read-only metadata checks, but must not publish from GitHub Actions.

A Git tag or GitHub Release is optional historical metadata created manually after successful publication. A tag must not trigger CI publication.

Python/PyPI publication, if retained, is also manual and outside GitHub Actions. It must not be coupled to crates.io publication through a shared workflow.

## Track A — Establish the new verification and release policy

### Objective

Make the simplification an explicit repository policy before deleting infrastructure, so future agents do not reconstruct the removed system from old plan requirements.

### Required changes

Update at least:

- `AGENTS.md`;
- the repository skill/instructions file, if present;
- `README.md` where CI/release status is described;
- `docs/release-process.md` or its replacement;
- any documentation claiming that release gates, evidence bundles, profile promotion, or the GitHub release workflow are authoritative.

The policy text must state:

- routine CI is a small regression screen, not release certification;
- deep verification is local/manual and selected by change risk;
- crates.io publishing is manual;
- GitHub Actions never publishes;
- old evidence/qualification plan requirements are historical and superseded by Plan 091;
- no generated release checklist is required;
- platform support claims are human-maintained and must remain conservative.

### Required audit

Search the repository for active references to:

```text
release/criteria.toml
release_criteria.py
release-validate.sh
ci-gate-evidence.sh
release-checklist.md
evidence-aggregate
gate-evidence
release-bundle
CARGO_REGISTRY_TOKEN
PYPI_TOKEN
TWINE_PASSWORD
action-gh-release
workflow_dispatch dry_run
profile promotion
candidate SHA
exact-SHA evidence
```

Classify each reference as:

- delete with obsolete infrastructure;
- rewrite as historical context;
- retain only if it describes a real product behavior rather than release machinery.

### Acceptance criteria

- Current repository policy clearly says GitHub CI does not publish releases.
- Current repository policy clearly says crates.io cadence is manual.
- No active contributor instruction tells agents to extend the evidence registry or generated checklist.
- No active documentation treats Plans 039/044–046/086/089/090 infrastructure requirements as current release blockers.
- Product security limitations remain accurately documented.

## Track B — Replace routine CI with a minimal workflow

### Objective

Replace the current multi-job evidence-producing workflow with no more than two high-signal Ubuntu jobs.

### Target Rust job

The Rust job should perform the minimum reliable set below, adjusted only where repository feature relationships require an equivalent command:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features client-tls
cargo test -p eggserve-bin --features tls
```

Rationale:

- formatting and Clippy catch inexpensive structural defects;
- `cargo test --workspace` is the canonical default correctness suite and naturally runs unit, integration, and doc tests that Cargo includes;
- explicit feature runs retain coverage not exercised by default workspace features;
- test binary and test case names provide failure localization without separate gate IDs.

Before finalizing commands, inspect Cargo feature definitions and verify that the chosen commands compile and execute all supported default, client, client-TLS, and server-TLS configurations without redundant duplicate runs.

Do not add separate steps for every integration test already discovered by `cargo test --workspace`.

Do not run Criterion benchmarks in routine CI. `cargo bench -- --test` is not a meaningful cross-run performance regression gate on shared GitHub runners.

### Target Python job

The Python job should:

1. check out the repository;
2. install the Rust toolchain and supported Python version;
3. install the pinned or documented Maturin version;
4. build the `eggserve-bin` release binary once;
5. stage that binary into the Python package once;
6. build one wheel once;
7. install the wheel into the runner environment or a temporary virtual environment;
8. run Python test discovery once;
9. run one installed CLI/server smoke test if it is not already included in discovery.

Prefer a command shaped like:

```sh
python -m unittest discover -s <installed-test-location> -p 'test_*.py' -v
```

or a single package-provided test entry point. Do not invoke every test module as a separate workflow step unless discovery cannot represent the suite correctly.

The job must run against the installed package, with source-tree path leakage prevented. Preserve `PYTHONPATH=""` or an equivalent clean-environment guard.

### Workflow structure

Retain:

- `concurrency` with `cancel-in-progress: true`;
- `permissions: contents: read` unless another read permission is genuinely needed;
- existing action pinning if maintainers want immutable action references;
- Rust caching if it demonstrably reduces runtime without introducing custom cache scripts;
- clear per-command step names.

Remove:

- all `scripts/ci-gate-evidence.sh` wrappers;
- all `actions/upload-artifact` evidence steps;
- all `actions/download-artifact` evidence steps;
- the final evidence aggregation job;
- the Rust OS matrix;
- duplicate standalone jobs for wire, production path, corpus replay, stateful fuzz, fault injection, filesystem race, proxy interop, supply chain, packaging, package dry-run, and wheel matrices;
- all `if: github.event_name == 'push'` branches that create a second larger main-push pipeline;
- release-oriented comments describing evidence classes and trigger policy.

### Branch-protection compatibility

Before merging the final workflow change, identify currently required GitHub check names.

Use one of these rollout strategies:

1. Preserve an existing required check name for the new minimal job until branch protection is updated; or
2. Coordinate the branch-protection update in the same maintenance window.

Do not leave `main` permanently blocked waiting for deleted job names.

The intended final required checks are only the minimal Rust and Python jobs.

### Acceptance criteria

- `.github/workflows/ci.yml` is short enough to understand in one reading and should normally remain under roughly 150 lines.
- A PR executes no more than two blocking jobs.
- A push to `main` executes the same routine jobs rather than a larger release-like matrix.
- No routine CI step writes to `target/release-evidence/`.
- No routine CI artifact is uploaded solely to prove that CI ran.
- Default, client/client-TLS, server-TLS, and Python installed-package coverage remain represented.
- A failing test produces a normal failing step with visible streamed output.
- A documentation-only change cannot fail because generated release state is stale.
- Branch protection references only checks that still exist.

## Track C — Remove or reduce auxiliary GitHub workflows

### Objective

Leave no scheduled or tag-triggered automation that recreates a second verification or release pipeline.

### Required workflow disposition

#### `.github/workflows/release.yml`

Delete it completely.

Do not retain a disabled copy, commented publication steps, a dry-run-only release workflow, or a tag-triggered artifact builder. The presence of the workflow continues to impose maintenance and encourages accidental reactivation.

#### `.github/workflows/fuzz-replay.yml`

Preferred disposition: delete it and document corpus replay under `scripts/verify.sh deep`.

Acceptable alternative: retain a manual-only `workflow_dispatch` workflow with one corpus replay job. Remove the weekly schedule. It must be non-blocking and contain no evidence upload.

#### Other workflows

Inspect every file under `.github/workflows/`.

For each workflow, choose exactly one:

- merge its high-signal command into minimal routine CI;
- convert it to a small manual-only platform/deep workflow;
- delete it.

No schedule, tag trigger, release trigger, or publication action should remain.

### Optional manual platform workflow

If retained, `.github/workflows/platforms.yml` should have:

```yaml
on:
  workflow_dispatch:
```

and a simple matrix over `macos-latest` and `windows-latest`.

It should run the workspace tests and only the minimal platform-specific package smoke needed for a deliberate qualification run. It must not be required for merge and must not produce a profile-promotion decision.

### Acceptance criteria

- No workflow triggers on `v*` tags.
- No workflow contains `cargo publish`, `twine upload`, or GitHub Release creation.
- No workflow has `contents: write` or `id-token: write` for publication.
- No workflow runs on a cron schedule unless a later, separate decision reintroduces one for a narrowly justified non-blocking check.
- Repository workflow count is one routine workflow plus at most one manual platform/deep workflow.

## Track D — Replace release validation with a small local verification script

### Objective

Provide a useful local developer interface without recreating release-gate machinery.

### New script

Create or rewrite `scripts/verify.sh` with this interface:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
./scripts/verify.sh deep
```

Unknown modes must print usage and exit nonzero.

Each command must be displayed before execution. Output must stream directly. The script should use normal shell control flow and stop at the first failing command.

### `fast` mode

`fast` is the routine local edit loop:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The implementation may offer a separate formatting mutation command only if already established, but verification mode itself must not rewrite files.

`fast` must work on a dirty development tree.

### `full` mode

`full` includes `fast`, then adds supported feature/package checks:

```sh
cargo test -p eggserve-core --features client-tls
cargo test -p eggserve-bin --features tls
```

It should also run the installed Python wheel suite when the supported Python and Maturin commands are available.

Retain a Rust package dry-run appropriate to the interdependent `eggserve-core` and `eggserve-bin` crates. The existing `scripts/verify-cargo-packages.sh` may remain if it directly solves the local-registry dependency-order problem and is not coupled to evidence generation.

Supply-chain commands may be included in `full` only when their required tools are already installed or the script gives a direct, non-misleading prerequisite message. Do not install cargo-audit and cargo-deny on every routine invocation.

A preferred pattern is:

```text
fast       always available with Rust toolchain
full       Rust features + Python wheel + cargo package
security   documented direct commands: cargo audit; cargo deny check
```

Do not add a fourth mode solely to match old gate categories unless it clearly improves usability.

### `deep` mode

`deep` includes `full`, then executes applicable expensive suites explicitly.

Candidate retained commands include:

- canonical/raw-wire suites not already included by the workspace run, after verifying whether they are actually redundant;
- corpus replay;
- stateful fuzz replay;
- fault injection;
- Unix filesystem race qualification;
- installed-binary qualification;
- Caddy/nginx interoperability;
- proxy desynchronization corpus;
- native TLS abuse tests;
- platform-specific Windows qualification when running in the required Windows environment.

Do not blindly duplicate tests already run by `cargo test --workspace`. Build a command inventory first and include only test executables excluded by default features, marked ignored, environment-gated, or otherwise not naturally discovered.

Long-duration soak tests and benchmarks should remain direct documented commands rather than automatically running every time `deep` is selected, unless `deep` has explicit sub-options. A developer should not accidentally launch a 24-hour test from an ordinary verification command.

### Environment-sensitive behavior

For tests requiring Caddy, nginx, NTFS privileges, Developer Mode, or other external capabilities:

- check prerequisites plainly;
- print a clear skip/not-run message when the environment is inapplicable;
- do not fabricate evidence or claim that the test passed;
- do not make unrelated local verification fail because an optional external tool is absent;
- retain a direct command so a maintainer can run the suite in the correct environment.

The distinction is simple: local `deep` is a convenience runner, not a release-certification result.

### Disposition of current scripts

Delete:

- `scripts/ci-gate-evidence.sh`;
- the evidence/manifest portions of `scripts/release-validate.sh`;
- arbitrary gate lookup and execution through `release/criteria.toml`;
- candidate/profile/evidence commands that exist only for release qualification.

Replace `scripts/release-validate.sh` with `scripts/verify.sh`, or reduce and rename it if a clean rename would break fewer references. Do not retain both as competing entry points.

Retain only focused helper scripts that directly execute real checks, such as package verification, proxy harnesses, installed-binary smoke, SBOM generation when manually requested, or soak harnesses.

### Acceptance criteria

- A developer can understand all verification modes by reading one small script.
- `fast` provides a practical edit loop.
- `full` provides a practical pre-merge or pre-release local check.
- `deep` exposes expensive suites without making them routine merge gates.
- The script writes no evidence JSON and no manifest.
- The script does not parse a gate registry.
- The script preserves the failing command's exit status.
- There is one canonical local verification entry point.

## Track E — Delete the release evidence and generated-checklist subsystem

### Objective

Remove infrastructure whose only purpose is representing, validating, aggregating, or displaying release-gate evidence.

### Required deletions

Delete, subject to confirming exact current filenames:

- `release/criteria.toml`;
- `scripts/release_criteria.py`;
- `scripts/ci-gate-evidence.sh`;
- `docs/release-checklist.md`;
- tests dedicated to `release_criteria.py`, evidence aggregation, waiver records, candidate/profile promotion, generated checklist synchronization, or trigger-policy cross-validation;
- CI inventory documentation that exists only to mirror the workflow/gate registry;
- evidence schemas, sample evidence bundles, waiver records, aggregate manifests, candidate freeze records, and generated closure reports that are active release inputs;
- `target/release-evidence` assumptions in scripts and documentation.

Likely files requiring review include:

- `scripts/test_release_criteria.py`;
- `scripts/test_corrective_tooling.py` where tests are evidence-only;
- `scripts/check-contract-consistency.py`;
- `docs/ci-gate-inventory.md`;
- `release/corrective-findings.toml`;
- `release/corrective-status.md`;
- `release/support-profiles.toml`;
- `release/plan-*-closure-report.md`;
- release candidate or evidence directories.

Do not delete these files mechanically. Classify their contents first.

### Support-profile disposition

The support-profile registry currently mixes useful platform/support information with release-promotion machinery.

Preferred outcome:

- remove `release/support-profiles.toml` as an executable release authority;
- preserve truthful support statuses and limitations in a concise human-maintained document or existing README/deployment/security documentation;
- remove required-gate lists, candidate promotion dependencies, approval records, and exact-SHA evidence semantics.

If maintainers retain `support-profiles.toml` for documentation generation, it must not be read by CI or release tooling and must not contain gate dependency graphs. A plain Markdown support table is simpler and preferred.

### Corrective registry disposition

Historical corrective findings may remain as static historical documentation, but they must not control routine CI or release publication.

Preferred outcome:

- archive completed corrective reports under a clearly historical documentation location, or leave existing plan files as the historical source;
- delete active state fields such as evidence freshness, evidence SHA, required gate dependencies, and profile-promotion status;
- stop embedding changing HEAD SHAs into committed closure documents;
- stop requiring closure-report regeneration after implementation changes.

### Contract checker disposition

`scripts/check-contract-consistency.py` currently validates many release and documentation claims and has grown into another large synchronization surface.

Replace it with a narrowly scoped metadata checker only if direct value remains. A justified replacement may check:

- package versions agree across Cargo and Python metadata;
- local README documentation links resolve;
- declared supported Python version agrees with package metadata.

The replacement should not:

- parse release criteria;
- enforce plan status prose;
- validate profile-promotion state;
- require generated API inventories unless those inventories are actual public compatibility contracts;
- encode broad natural-language documentation policy through fragile regexes.

Prefer ordinary tests for public API behavior and compiler checks for Rust API compatibility.

### Removal safety

Before deletion, produce a temporary inventory mapping every removed file to one of:

- obsolete release/evidence machinery;
- real behavioral test retained elsewhere;
- useful documentation migrated to a simpler location;
- helper script retained independently.

This inventory may live in the implementation commit message or temporary working notes; it does not need to become another permanent registry.

### Acceptance criteria

- No `release/criteria.toml` or replacement gate DSL exists.
- No generated release checklist exists.
- No evidence JSON schema or aggregator exists.
- No CI job validates generated release state.
- No active documentation requires exact-SHA evidence bundles.
- Real test suites and conformance corpora remain present.
- Useful platform limitations survive in normal documentation.
- Deleting evidence tooling removes substantially more code than the simplification adds.

## Track F — Establish a strictly manual release procedure

### Objective

Replace GitHub-hosted publication with a short, explicit local crates.io process.

### Release authority

A release occurs only when a maintainer deliberately runs `cargo publish` from a trusted local environment.

No repository event is publication authority:

- not a merge;
- not a push to `main`;
- not a tag;
- not a GitHub environment approval;
- not a workflow dispatch;
- not a green CI badge;
- not a generated checklist.

### Minimal manual crates.io sequence

Document a sequence equivalent to:

```sh
git status --short
./scripts/verify.sh full

cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked

# Confirm the new eggserve-core version is visible to the crates.io index.

cargo publish -p eggserve-bin --locked --dry-run
cargo publish -p eggserve-bin --locked
```

The runbook must explain that crates.io versions are immutable. If a version has been successfully published, any correction requires a new version number. Do not instruct maintainers to retry publication of changed contents under an existing version.

The binary crate must not be published until its exact core dependency version is available through the registry index.

### Version checks

Retain or implement a small read-only version consistency command if needed. It may compare:

- `crates/eggserve-core/Cargo.toml`;
- `crates/eggserve-bin/Cargo.toml`;
- Python package metadata if those versions are intentionally synchronized.

This check must remain simple and must not grow into a release state machine.

### Python publication

If the Python package continues to be published:

- document a separate manual wheel build/test/upload procedure;
- do not publish it from GitHub Actions;
- do not require Python publication to occur in the same transaction as crates.io;
- clearly document supported Python and platform wheel coverage;
- use a trusted local or explicitly chosen external build environment as a maintainer operation.

This plan does not require redesigning Python distribution. It only removes automated publication.

### Tags and GitHub Releases

After successful registry publication, maintainers may manually create a tag:

```sh
git tag "vX.Y.Z"
git push origin "vX.Y.Z"
```

The tag is a historical marker only.

A GitHub Release may be created manually if desired. It is not required for crates.io release and must not be automatically assembled by CI.

### Credentials and repository settings

After deleting automated publication:

- remove the GitHub Actions `CARGO_REGISTRY_TOKEN` secret if present;
- remove PyPI publication secrets if no workflow uses them;
- remove the protected `release` environment if it exists only for automated publication;
- verify no workflow has registry credentials or write permissions;
- update branch protection required checks to the new minimal CI names.

These are repository-configuration actions and may require a maintainer outside the code commit. The plan implementation report must list them explicitly rather than pretending the code change completed them.

### Acceptance criteria

- `.github/workflows/release.yml` does not exist.
- Repository search finds no `cargo publish` in GitHub workflows.
- Repository search finds no `twine upload` in GitHub workflows.
- A pushed version tag does not publish anything.
- Manual release instructions fit on one short document and are executable in order.
- Core-before-bin registry ordering is documented.
- Immutable-version handling is documented.
- Registry secrets are no longer required by GitHub Actions.

## Track G — Reclassify expensive and specialized verification

### Objective

Preserve meaningful deep coverage while removing it from the routine merge path.

### Classification model

Every current verification command must be assigned to one of three categories.

#### Category 1: routine CI

Fast, deterministic, high-signal checks appropriate for every PR:

- formatting;
- Clippy;
- workspace tests;
- supported feature compilation/tests;
- one installed Python wheel test pass.

#### Category 2: manual deep verification

Expensive or dependency-sensitive checks that remain convenient to run:

- corpus replay;
- stateful fuzz replay;
- fault injection;
- filesystem race qualification;
- installed-binary qualification;
- proxy interoperability and desynchronization corpus;
- native TLS abuse tests;
- supply-chain audit and deny;
- package dry-runs;
- cross-platform installed-wheel smoke.

#### Category 3: campaign/qualification work

Long-running or specialized-environment checks run only for a deliberate hardening or release campaign:

- 24-hour soak tests;
- benchmarks and allocation profiling;
- privileged Windows reparse/race qualification;
- independent security review;
- SBOM/provenance generation when specifically desired;
- broad multi-target release artifact assembly.

Category 3 must not be hidden inside `full` or routine `deep` without an explicit subcommand or direct operator action.

### Preserve tests, remove orchestration

Do not delete a test merely because its old gate is deleted.

For each specialized suite:

1. verify the suite still tests a real invariant;
2. verify whether `cargo test --workspace` already executes it;
3. retain it if it provides unique coverage;
4. provide one direct command in documentation or `verify.sh`;
5. remove CI evidence wrappers and gate metadata;
6. remove duplicated commands that execute the same test binary without adding coverage.

### Performance verification

Remove performance checks from routine CI.

Shared GitHub runners do not provide a reliable stable baseline for small latency or allocation changes. Keep benchmark source and documented local invocation. Treat performance work as comparative measurement on a controlled machine, not a pass/fail release gate.

### Fuzz verification

Keep regression corpus replay because it is deterministic and inexpensive when the corpus is bounded. Place it in `deep` or a manual workflow.

Do not treat the absence of a corpus directory as a generated `not-applicable` evidence record. The script can plainly report that no corpus exists.

Long fuzz campaigns remain manual and should record findings through ordinary issues/commits, not permanent release evidence schemas.

### Windows verification

Keep ordinary Windows compile/unit testing available manually.

Keep the Plan 086 adversarial suite and its environment requirements if it provides real hardening value. Remove the requirement that every ordinary `main` push attempt privileged qualification.

Windows support documentation must remain conservative until maintainers have actually run the specialized suite in an appropriate NTFS environment. That judgment is documented, not machine-promoted.

### Acceptance criteria

- Every removed CI job's substantive test has an explicit retained command or a documented reason for deletion as redundant.
- Expensive suites do not run on every PR or `main` push.
- Benchmarks are not blocking checks on shared runners.
- Windows specialized tests remain available without blocking unrelated iteration.
- No support claim is strengthened merely because a workflow was simplified.

## Track H — Simplify documentation and remove release-state churn

### Objective

Stop documentation from functioning as mutable release evidence while retaining useful operator and security guidance.

### Required documentation structure

Prefer a small set of clear documents:

- `README.md`: project scope, installation, basic support status, links;
- `docs/development.md` or `CONTRIBUTING.md`: `fast`, `full`, and `deep` commands;
- `docs/release-process.md`: manual crates.io procedure;
- existing security/deployment documents: behavioral guarantees and limitations.

Delete or archive documents whose primary purpose is:

- listing every CI gate;
- presenting generated pending/passed status cells;
- embedding the current candidate SHA;
- describing evidence classes and freshness rules;
- requiring run IDs and artifact IDs for every release;
- tracking profile promotion through gate dependencies;
- recording closure reports that must be refreshed whenever HEAD changes.

### README cleanup

The README may retain platform and deployment guidance, but remove operational claims such as:

- a profile is awaiting a generated gate bundle;
- a specific plan's evidence scaffold determines release readiness;
- a GitHub workflow is the release authority;
- publication requires a GitHub environment approval.

Replace with direct support language, for example:

- Linux/macOS: supported with documented hardened defaults;
- Windows: functional or qualified to the level maintainers can substantiate;
- public deployment: reverse proxy recommended;
- specialized adversarial tests: available and run manually when relevant.

Do not overclaim hardened production support during the cleanup.

### Plan status language

Update agent documentation so plan status does not require a continually growing paragraph listing every completed plan and its release gate effects.

A concise statement is sufficient:

- historical plans are in `plans/`;
- Plan 091 defines current CI and release policy;
- implementation decisions remain in code and architecture documents.

### Acceptance criteria

- No committed document must be regenerated because HEAD changed.
- No generated checklist appears in documentation navigation.
- Development and release instructions are concise and accurate.
- README platform claims remain conservative.
- Historical plan records do not control active CI.

## Track I — Verification of the simplification itself

### Objective

Demonstrate that deletion did not remove essential test coverage or leave dangling automation references.

### Required repository checks

Run searches equivalent to:

```sh
rg -n "ci-gate-evidence|release_criteria|release/criteria.toml|release-evidence|evidence-aggregate|gate-evidence" .
rg -n "cargo publish|twine upload|action-gh-release|CARGO_REGISTRY_TOKEN|PYPI_TOKEN|TWINE_PASSWORD" .github scripts docs README.md
rg -n "release-checklist|exact-SHA evidence|profile promotion|candidate SHA" AGENTS.md README.md docs release scripts .github
```

Expected outcome:

- no active workflow or script references removed machinery;
- historical references may remain only in plan files or clearly historical reports that are intentionally retained;
- no publication command exists in `.github/workflows/`.

### Required command verification

On Linux, run:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Run applicable retained deep commands or:

```sh
./scripts/verify.sh deep
```

where the environment supports them.

On macOS and Windows, run the documented manual workspace/platform command or dispatch the optional manual platform workflow.

### Coverage reconciliation

Before deleting old CI jobs, create a temporary command map with columns:

```text
old job/step
old command
covered by new routine CI?
retained manual command
redundant and removed?
reason
```

The map is an implementation aid, not a permanent gate registry. It may be included in the plan completion commit message or a short one-time closure note.

No old command may disappear silently. It must be:

- covered by broader retained test discovery;
- retained as a manual/deep command;
- or deliberately removed with a concrete redundancy/invalidity explanation.

### Workflow validation

Validate workflow syntax and behavior by:

- reviewing the final YAML directly;
- pushing the implementation commit and observing the minimal jobs;
- confirming job count and names;
- confirming output streams normally;
- confirming failure propagation with an implementation-time controlled failure if practical, then reverting it;
- confirming no artifact upload occurs;
- confirming a tag does not match any workflow publication trigger.

### Iteration budget

Measure the post-change workflow shape, not a brittle exact runtime SLA.

Target characteristics:

- no more than two routine jobs;
- no nested dependency fan-out;
- no artifact aggregation tail;
- no package or wheel rebuilt multiple times in the same workflow;
- no repeated OS matrix on ordinary PRs;
- routine Linux checks complete in a time appropriate for iterative work.

If one remaining test dominates runtime, profile that test separately and decide whether it belongs in `deep`; do not rebuild a gate framework around it.

### Acceptance criteria

- `fast` and `full` pass on the implementation commit.
- New routine CI passes with no more than two jobs.
- The implementation commit does not require a follow-up checklist regeneration commit.
- No deleted check name remains required in branch protection.
- No real behavioral test suite was accidentally deleted with evidence tooling.
- Repository code and documentation contain no active automated-publication path.

## Ordered implementation sequence

Implement this plan in deletion-first, reviewable commits.

### Commit 1 — Policy and command inventory

- record the old workflow command map in working notes;
- update `AGENTS.md` and current policy documentation to establish Plan 091 supersession;
- identify required branch-protection check names and repository secrets/environment actions;
- make no broad test deletion.

Acceptance for this commit:

- future commits have an authoritative target policy;
- all current verification commands are accounted for.

### Commit 2 — Minimal routine CI

- replace `.github/workflows/ci.yml` with the Rust and Python jobs;
- remove evidence wrappers and artifact uploads from routine CI;
- preserve or coordinate required check names;
- validate that the new jobs execute.

Acceptance for this commit:

- routine CI is already materially smaller;
- normal code/test failures propagate directly.

### Commit 3 — Local verification simplification

- create `scripts/verify.sh`;
- implement `fast`, `full`, and `deep`;
- retain focused helper scripts;
- remove or deprecate `scripts/release-validate.sh` without leaving two canonical interfaces.

Acceptance for this commit:

- local development no longer depends on release criteria or evidence generation.

### Commit 4 — Remove release and auxiliary workflows

- delete `.github/workflows/release.yml`;
- delete or manualize fuzz/platform workflows according to this plan;
- verify there are no tag, schedule, or publication triggers.

Acceptance for this commit:

- GitHub Actions cannot publish.

### Commit 5 — Delete evidence/gate tooling

- remove `release/criteria.toml`;
- remove `scripts/release_criteria.py` and dedicated tests;
- remove `scripts/ci-gate-evidence.sh`;
- remove generated checklist and evidence-only documentation/state;
- migrate any useful support limitations before deleting mixed-purpose files.

Acceptance for this commit:

- the repository has no second verification data model.

### Commit 6 — Documentation and manual release runbook

- rewrite release documentation around local `cargo publish`;
- document version immutability and core-before-bin order;
- document manual Python publication separately if retained;
- simplify README support/profile language;
- remove stale references.

Acceptance for this commit:

- a maintainer can release without GitHub Actions or the deleted evidence system.

### Commit 7 — Closure verification

- run `fast`, `full`, and applicable `deep` checks;
- run cross-platform manual checks where available;
- perform repository-wide stale-reference searches;
- confirm branch-protection and secrets/environment follow-up actions;
- document any environment-limited deep checks without blocking completion of the simplification.

Acceptance for this commit:

- the implementation satisfies the Definition of Done below.

Avoid combining the entire reduction into one opaque commit. The sequence should make it possible to bisect product-test regressions independently from removal of administrative tooling.

## Explicit non-goals

This plan does not require:

- reducing the number of product test cases to an arbitrary target;
- proving production readiness for every platform;
- running a new independent security review;
- completing 24-hour soaks;
- generating release binaries for every platform;
- publishing a release;
- changing crate versions;
- changing public APIs;
- replacing Maturin or PyO3;
- changing HTTP behavior;
- changing filesystem confinement design;
- changing TLS implementation;
- changing supported Python versions;
- adding a new dependency update service;
- preserving compatibility with deleted internal release-tool CLIs.

## Rejection criteria

Reject an implementation that:

- keeps `release/criteria.toml` and merely reduces its gate count;
- keeps the evidence aggregator but makes fewer jobs upload evidence;
- replaces the evidence framework with another YAML/TOML/JSON registry;
- keeps automated publication behind more approvals;
- leaves the release workflow disabled but present;
- leaves old generated checklists as required documentation;
- deletes security tests instead of moving them to manual/deep execution;
- makes `deep` the default CI path;
- adds path filters or change-detection complexity comparable to the removed matrix;
- uses a meta-workflow to dynamically decide which of dozens of gates to run;
- leaves branch protection requiring deleted checks;
- claims the work is complete while GitHub Actions still holds active registry publication credentials;
- introduces a large Makefile/task framework to replace the large shell/Python framework;
- retains exact-SHA closure-report churn.

## Definition of Done

Plan 091 is complete only when all of the following are true.

### Routine CI

- [ ] One routine CI workflow exists.
- [ ] It runs on pull requests and pushes to `main`.
- [ ] It contains no more than two blocking Ubuntu jobs.
- [ ] It has no OS matrix.
- [ ] It has no evidence upload or aggregation.
- [ ] It has no benchmark, soak, proxy installation, SBOM, provenance, or release assembly.
- [ ] It runs the default Rust workspace, supported feature configurations, and installed Python package tests.
- [ ] Its required check names match branch protection.

### Local verification

- [ ] One canonical `scripts/verify.sh` entry point exists.
- [ ] `fast`, `full`, and `deep` are documented and functional.
- [ ] Output streams directly and failures propagate.
- [ ] The script generates no evidence records or manifests.
- [ ] Expensive real suites remain reachable manually.

### Evidence/tooling removal

- [ ] `release/criteria.toml` is removed.
- [ ] `scripts/release_criteria.py` is removed.
- [ ] `scripts/ci-gate-evidence.sh` is removed.
- [ ] Generated release checklist enforcement is removed.
- [ ] Evidence-only tests and documentation are removed or archived as historical material.
- [ ] No active script or workflow depends on profile-promotion or candidate-evidence state.

### Release

- [ ] `.github/workflows/release.yml` is removed.
- [ ] No workflow triggers publication on a tag.
- [ ] No workflow contains crates.io, PyPI, or GitHub Release publication commands.
- [ ] Manual crates.io instructions document verification, dry-run, core publication, index propagation, binary publication, and immutable versions.
- [ ] Python publication is manual if retained.
- [ ] GitHub registry secrets and release environment removal are recorded as maintainer actions.

### Correctness preservation

- [ ] Product test source is preserved except for documented redundant or invalid tests.
- [ ] Every removed job command is mapped to routine coverage, manual coverage, or an explicit removal rationale.
- [ ] Platform/security limitations remain conservative and accurate.
- [ ] `./scripts/verify.sh fast` passes.
- [ ] `./scripts/verify.sh full` passes.
- [ ] New routine CI passes.
- [ ] Repository-wide searches find no active stale references to deleted machinery.

### Iterative capability

- [ ] A normal implementation PR no longer triggers release-like qualification.
- [ ] A documentation-only PR cannot fail because release state is stale.
- [ ] A test failure can be diagnosed from the direct job log without downloading artifacts.
- [ ] A maintainer can merge ordinary changes without regenerating checklists, evidence, manifests, or embedded SHAs.
- [ ] Release timing remains entirely manual and independent of GitHub CI.

## Handoff note

The implementing agent should resist the repository's existing tendency to preserve every old abstraction for compatibility. Internal release/evidence tooling has no downstream compatibility contract. Delete it cleanly.

The main risk is not insufficient ceremony; it is accidentally deleting real test coverage while deleting the machinery around it. Use the temporary command map, preserve substantive suites, and keep the final system mechanically simple.

When there is ambiguity, choose the outcome that has:

1. fewer workflow jobs;
2. fewer generated files;
3. fewer synchronized sources of truth;
4. direct command execution;
5. no publication authority in CI;
6. conservative documentation;
7. retained behavioral tests;
8. lower maintenance cost.
