# Plan 141 — PyPI Trusted Publishing and Release Aggregation

## Status

**READY FOR HANDOFF — 2026-08-18.**

Parent roadmap: Plan 138.

Dependencies:

```text
Plan 139 — Python Release Version and Wheel Contract
Plan 140 — Wide Platform Wheel Build and Qualification
```

Both must be closed before production publishing is enabled.

Reviewed repository baseline:

```text
main = 2b90abe0b118c03ea23cc37d63a4fe35b174bfdd
```

This phase turns the already-qualified wide wheel matrix into a controlled PyPI release path while preserving EggServe's manual release cadence.

---

# 1. Goal

Implement one production publication boundary with these properties:

```text
maintainer manually starts release
all platform wheels build and qualify first
complete artifact set is aggregated and validated
production environment requires explicit approval
one OIDC/Trusted Publishing job uploads all artifacts
post-publication checks prove pip resolves binary wheels
```

The workflow should automate release mechanics, not release policy.

A maintainer still decides **whether and when** a release occurs.

---

# 2. Non-goals and hard boundaries

Do not:

- publish on every push, merge, or tag;
- publish individual wheels from matrix jobs;
- store a long-lived PyPI API token in GitHub secrets;
- publish before every Tier 1 target is complete;
- couple PyPI publication to crates.io publication;
- create GitHub Releases automatically unless separately requested later;
- add an external release orchestration service;
- add custom artifact signing infrastructure;
- require a self-hosted runner to hold publication credentials;
- grant OIDC write permission to the whole workflow;
- let TestPyPI configuration share production environment approval or identity by accident;
- use an sdist/source build to conceal missing wheel coverage;
- expand routine CI.

---

# 3. Resolve the existing release-policy wording explicitly

The current documentation states that GitHub Actions never publishes to PyPI and that maintainers upload wheels manually with Twine.

This plan intentionally changes that **mechanism**, but not the manual cadence.

Replace the old policy with an explicit contract equivalent to:

```text
Release cadence is manual.
A maintainer starts the release workflow deliberately.
GitHub Actions may build, qualify, and publish the selected release artifacts.
Production PyPI upload requires the protected `pypi` environment.
No push/tag/merge automatically releases EggServe.
```

Do not leave both the old “Actions never publishes” statement and the new OIDC workflow in active documentation.

Historical plan text may remain historical; active release docs must describe current policy.

### Acceptance criteria

- documentation no longer contradicts the implemented workflow;
- manual cadence remains explicit;
- implementation cannot publish merely because `main` changed.

---

# 4. Workflow trigger and source selection

Keep production release initiation under:

```yaml
on:
  workflow_dispatch:
```

Do not add automatic tag publication in this phase.

The workflow must build one exact Git commit. Record the checked-out SHA during preflight and propagate it as release evidence.

If the workflow accepts a ref input, it must:

- resolve to one commit before builds start;
- use that same resolved commit in every build job;
- reject ambiguous/stale metadata through Plan 139 preflight;
- never let matrix jobs independently fetch changing branch heads.

The simplest acceptable implementation is to dispatch the workflow from the exact desired branch/tag/ref and use the workflow run's immutable commit SHA.

### Acceptance criteria

- every artifact can be traced to one commit SHA;
- concurrent commits to `main` cannot change the source midway through a run;
- release evidence records the source SHA.

---

# 5. Job graph

Use a release graph equivalent to:

```text
preflight
  |
  +------------------------------+
  |                              |
  v                              v
wide wheel build/qualification matrix (Plan 140)
  |
  v
aggregate
  |
  v
validate complete wheel set
  |
  +--------------------+
  |                    |
  v                    v
optional TestPyPI      production approval boundary
qualification              |
                           v
                      publish-pypi
                           |
                           v
                      post-publish smoke
```

The exact YAML job count may differ, but these authority boundaries must remain visible.

Do not compress build, aggregate, and publish into one job merely to shorten YAML.

---

# 6. Build-job permissions

Release build jobs do not need PyPI identity.

Keep their permissions at least privilege, typically:

```yaml
permissions:
  contents: read
```

Do not set workflow-global:

```yaml
permissions:
  id-token: write
```

because that unnecessarily grants OIDC-token capability to every job.

The production publication job alone should declare:

```yaml
permissions:
  id-token: write
```

and any additional permission required by the pinned publishing action only if current official documentation requires it.

### Acceptance criteria

- build/test jobs cannot request a production PyPI OIDC credential;
- `id-token: write` is scoped to the publication job;
- no PyPI secret is exposed to untrusted build scripts.

---

# 7. Aggregate before publication

Each Plan 140 matrix member uploads its wheel as a GitHub Actions artifact.

The aggregate job must:

1. download every Tier 1 artifact from the same workflow run;
2. place all intended publication files into one clean directory;
3. reject nested duplicate copies or unrelated files;
4. invoke the release wheel-set validator;
5. verify the expected version from Plan 139;
6. produce a human-readable manifest of exact filenames and hashes;
7. upload/retain the aggregate set as workflow evidence if useful;
8. expose only the validated directory to downstream publication jobs.

Recommended manifest contents:

```text
source commit SHA
EggServe version
wheel filename
SHA-256 digest
ABI tag
platform tag
build job / target identity
```

Use standard tools or Python `hashlib`; do not add a dependency solely for hashing.

### Acceptance criteria

- aggregate job fails when any Tier 1 artifact is absent;
- aggregate job fails on version/tag mismatch;
- publication cannot depend directly on unvalidated per-target artifacts;
- exact uploaded files are reviewable before production approval.

---

# 8. Atomic release policy

PyPI artifacts for a released version are effectively immutable from the project's release perspective. Therefore the workflow must avoid creating a partially populated version.

Mandatory policy:

```text
all Tier 1 wheels must be built and qualified before first production upload
```

No build matrix job may invoke:

```text
maturin publish
twine upload
pypa/gh-action-pypi-publish
```

The sole production uploader must receive the complete, already-validated directory.

If any Tier 1 target fails before publication:

```text
release stops
PyPI receives nothing
```

If the PyPI upload itself fails after some files have been accepted, treat that as an exceptional recovery case. Do not design normal operation around incremental upload retries with mutated artifacts under the same version.

Document the recovery rule:

- inspect which files PyPI accepted;
- never rebuild changed contents under the same filename/version;
- if a coherent immutable release cannot be completed safely, increment the package version and perform a fresh full release.

### Acceptance criteria

- normal workflow has exactly one production upload job;
- that job depends on the complete-set validator;
- no partial target release can occur because an ARM job finished later than x86_64.

---

# 9. Configure PyPI Trusted Publishing

Use PyPI's GitHub Actions Trusted Publisher mechanism.

Configure the PyPI project/pending publisher for:

```text
owner/organization: eggstack
repository:         eggserve
workflow:           the production release workflow filename
GitHub environment: pypi
```

Use the exact workflow filename that lands in the repository. Renaming it later requires corresponding publisher configuration review.

The publication job should use the official PyPA GitHub Action for PyPI publication, pinned according to the repository's supply-chain policy. Do not use a floating major tag if the project policy requires immutable action SHAs.

Do not configure:

```text
PYPI_API_TOKEN
TWINE_PASSWORD
PyPI account password
```

for the production path.

### GitHub environment

Create/configure a protected GitHub Environment named:

```text
pypi
```

Prefer required reviewer approval so the artifact manifest can be inspected before upload.

The release workflow's production job must reference this environment explicitly.

### Acceptance criteria

- PyPI accepts the GitHub OIDC identity for the configured repository/workflow/environment tuple;
- repository secrets contain no required long-lived PyPI upload credential;
- production job cannot run past the environment boundary without required approval;
- trusted publisher configuration is documented for maintainers.

---

# 10. TestPyPI staging path

A TestPyPI path is recommended for first implementation and future publication-mechanism changes, but it must remain proportionate.

Use a separate GitHub Environment:

```text
testpypi
```

and a separate Trusted Publisher registration for TestPyPI.

Possible release workflow behavior:

```text
workflow_dispatch input:
  publish_target = none | testpypi | pypi
```

or separate manual jobs/workflows if that is clearer.

Avoid an input design that accidentally allows both TestPyPI and production uploads in parallel.

For TestPyPI qualification:

1. publish the complete validated artifact set;
2. create a fresh environment on representative platforms;
3. install the exact version from TestPyPI using binary-only selection;
4. run import/CLI/server smoke;
5. confirm pip selected the expected wheel rather than building locally.

Do not require TestPyPI for every routine release once the production path is proven unless maintainers find the extra gate useful.

### Acceptance criteria

- first end-to-end publication implementation is proven without risking production PyPI;
- TestPyPI identity cannot publish to production;
- production publication remains a distinct explicit choice/approval.

---

# 11. Production publication job

The production job should conceptually be:

```yaml
publish-pypi:
  needs: [aggregate]
  environment: pypi
  permissions:
    id-token: write
  steps:
    - download validated publication artifact
    - optionally verify manifest/digests again
    - invoke pinned pypa/gh-action-pypi-publish
```

Do not check out and rebuild source in the publication job. The job's purpose is to publish the artifacts already produced and validated by upstream jobs.

If the official PyPA action currently produces package attestations/provenance by default or through a simple supported option, retain that behavior. Do not duplicate it with a custom signing scheme.

### Acceptance criteria

- publication job performs no compilation;
- uploaded bytes are the same bytes validated by aggregate;
- OIDC is used only at publication time;
- all intended wheels are uploaded in one job.

---

# 12. Source distribution decision

This track is specifically about wide binary support. An sdist is optional.

## Preferred first-release policy

Do not make sdist publication a blocker for the binary pipeline.

If an sdist already has a clear, correct build path and is desired, include it in the aggregate manifest as a separate expected artifact. Validate it independently.

If there is any risk that pip could fall back to compiling from sdist and hide missing binary support in smoke tests, post-publication tests must explicitly use:

```text
--only-binary=:all:
```

An sdist does not satisfy any Tier 1 wheel requirement.

### Acceptance criteria

- every required platform is proven to receive a wheel;
- source fallback cannot make post-publication qualification falsely pass;
- sdist inclusion, if any, is an explicit documented choice rather than an incidental Maturin side effect.

---

# 13. Post-publication binary-only qualification

After a successful PyPI upload, run a small set of representative binary-only installation checks.

At minimum:

```text
Linux x86_64 manylinux
Linux aarch64 manylinux
one musllinux target
macOS arm64
Windows x86_64
```

Add Windows arm64 and ARMv7 when runner/emulation cost is reasonable, particularly for the first production release.

Install with:

```text
python -m pip install --only-binary=:all: eggserve==<exact-version>
```

Prefer a clean environment that does not have source/build dependencies preinstalled.

Verify:

```text
pip resolves without local compilation
eggserve.__version__ == expected version
import eggserve._native succeeds
eggserve --help succeeds
python -m eggserve --help succeeds
real fixture serving succeeds
```

Capture pip's selected wheel filename or installation verbose output when practical so the release evidence identifies the artifact actually consumed.

Post-publication smoke failure must be surfaced prominently, but remember the package version is already immutable. Recovery uses a new version when artifact contents must change.

---

# 14. Release concurrency

Prevent two production release runs from racing against one another.

Add an explicit release concurrency group, for example:

```text
release-pypi
```

with semantics that do **not** cancel a publication already in progress merely because another maintainer dispatched a second run.

Prefer queueing/rejecting concurrent production runs over `cancel-in-progress: true` once a release has reached the publication boundary.

Test/build-only dispatches may use a different concurrency policy if useful.

### Acceptance criteria

- two maintainers cannot simultaneously publish two release sets through the same production environment unintentionally;
- an in-progress upload is not killed by an unrelated new workflow dispatch.

---

# 15. Failure and retry model

Define failure semantics in `docs/release-process.md`.

## Before production upload

Safe behavior:

```text
fix source/workflow
rerun release
nothing has reached PyPI
```

## Environment approval rejected/cancelled

Safe behavior:

```text
no upload
workflow ends/can be rerun
```

## OIDC negotiation fails before upload

Safe behavior:

```text
no package mutation
fix publisher/environment configuration
rerun same validated release commit if version remains unpublished
```

## Partial PyPI acceptance

Exceptional behavior:

```text
inspect immutable accepted files
never overwrite/rebuild them under same version
complete only if remaining exact validated files can be safely uploaded unchanged
otherwise bump version and release a new complete set
```

Do not use `skip-existing` as the primary normal-release correctness mechanism. It can hide artifact drift.

---

# 16. Documentation updates

Update:

```text
docs/release-process.md
docs/python-packaging.md
README.md where installation/platform claims need correction
```

The release guide should contain a concise maintainer procedure:

1. synchronize/bump release versions under Plan 139's contract;
2. verify working tree/commit as appropriate before pushing;
3. ensure routine CI is green;
4. manually dispatch the release workflow for the intended commit;
5. inspect build matrix and aggregate manifest;
6. approve `pypi` environment only after all Tier 1 wheels are present;
7. confirm publication job succeeds;
8. review post-publication binary-only smoke;
9. optionally create/push the repository tag according to the project's manual tag policy.

Do not reintroduce a generated release checklist framework.

---

# 17. First end-to-end rollout sequence

Use the following rollout so production credentials are not introduced before the build system is trustworthy.

### Stage 1 — build only

```text
Plan 139 complete
Plan 140 complete
manual workflow produces entire validated set
no index publishing
```

### Stage 2 — TestPyPI

```text
configure testpypi Trusted Publisher
publish complete set
binary-only installation smoke
resolve any packaging/index metadata defects
```

### Stage 3 — production publisher configuration

```text
configure PyPI Trusted Publisher
configure protected `pypi` GitHub Environment
verify workflow/environment tuple
```

### Stage 4 — first PyPI publication

```text
use a new, intentionally selected package version
build complete set again from exact release commit
approve production environment
publish once
run binary-only post-publication smoke
```

Do not use an already partially/manual-published version for the first production workflow test if doing so would make completeness ambiguous.

---

# 18. Verification and evidence

Before declaring this plan complete, retain evidence for:

```text
source commit SHA
workflow run ID
expected version
validated wheel manifest and SHA-256 hashes
TestPyPI publication run, if used
production environment approval event/status
production PyPI publication success
representative post-publication wheel-resolution logs
```

This evidence can live in the workflow run and a concise plan closure note. Do not build another evidence registry or generated artifact-management subsystem.

---

# 19. Acceptance criteria

Plan 141 is complete when:

- the release workflow remains manually dispatched;
- no tag/push/merge automatically publishes;
- Plan 140's complete Tier 1 wheel set is required before production publication;
- all wheel artifacts are downloaded into one clean aggregate directory;
- the aggregate validator checks exact target completeness, ABI, version, and composition;
- a human-readable release manifest with hashes is produced before publication;
- matrix jobs contain no publication command or PyPI credential capability;
- only the final publication job has `id-token: write`;
- production uses a protected GitHub Environment named `pypi` or an equivalently explicit environment;
- PyPI Trusted Publishing/OIDC is configured for the exact EggServe repository/workflow/environment identity;
- no long-lived PyPI API token is required;
- TestPyPI uses separate identity/environment configuration if enabled;
- the publication job performs no build and uploads the exact validated bytes;
- production upload occurs once per release set rather than once per target;
- concurrent production release runs cannot race;
- post-publication installation uses `--only-binary=:all:` and proves wheel resolution;
- active documentation describes the new automated publication mechanism while preserving manual release cadence;
- crates.io release remains independent;
- no release orchestration framework, self-hosted credential runner, or unrelated CI expansion is introduced.

When this phase closes successfully, EggServe has a bounded production PyPI pipeline that provides prebuilt x86/ARM wheels across mainstream glibc, musl, macOS, and Windows environments without compromising the repository's intentionally simple routine CI model.
