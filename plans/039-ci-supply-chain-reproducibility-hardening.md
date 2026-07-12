# Phase 39 — CI, Supply-Chain, and Reproducibility Hardening

## Goal

Make EggServe’s release validation repeatable, reviewable, and resistant to dependency, workflow, artifact, and permission drift.

## Track A — Required CI matrix

Define required jobs for:
- formatting;
- clippy with warnings denied;
- Rust tests with default features;
- server TLS feature;
- client feature;
- client TLS feature;
- documentation tests;
- Python source-tree tests;
- Python live server integration tests;
- clean installed-wheel smoke tests;
- raw-wire correctness tests;
- fuzz corpus replay;
- audit and license/dependency policy;
- `cargo package` verification.

Cover supported operating systems and architectures according to the release contract. At minimum ensure Linux, macOS, and Windows claims are represented, while distinguishing hardened Unix guarantees from functional Windows support.

## Track B — Workflow permissions and pinning

Audit every GitHub Actions workflow:
- set minimal top-level and job-level permissions;
- avoid write permissions on ordinary CI;
- pin third-party actions to immutable commit SHAs or an explicitly documented update policy;
- use trusted official actions where possible;
- prevent untrusted pull requests from accessing release secrets;
- separate release workflows from pull-request validation.

Document the action-update procedure.

## Track C — Toolchain and dependency policy

Define:
- Rust channel/toolchain policy;
- whether an MSRV exists;
- Python version support;
- maturin/PyO3 version policy;
- lockfile policy for binaries/workspace;
- dependency update cadence;
- allowed licenses and sources;
- handling of yanked or vulnerable dependencies.

Enforce through `cargo deny`, `cargo audit`, lockfile checks, and automated dependency update tooling if already within project practice.

## Track D — Artifact provenance and integrity

For release artifacts:
- generate cryptographic checksums;
- record source commit/tag;
- ensure artifacts are produced only by the release workflow;
- retain build logs;
- use GitHub artifact attestations or Sigstore provenance if practical;
- generate an SBOM if it can be maintained without disproportionate complexity;
- verify wheel and binary contents before publication.

Do not make unverifiable reproducible-build claims. If byte-for-byte reproducibility is not achieved, document build provenance rather than overstating it.

## Track E — Release workflow isolation

Create or harden a release workflow that:
- triggers only from approved tags or manual protected dispatch;
- validates version/tag consistency;
- runs all release gates before publish steps;
- builds crates, wheels, and binaries from the same commit;
- uploads staging artifacts before publication;
- requires an explicit approval environment for registry publication;
- prevents partial publication where possible;
- records published artifact identifiers and checksums.

Support a dry-run/release-candidate mode that performs every step except registry publication.

## Track F — Cache and artifact safety

Review caches for:
- keys scoped by lockfile, toolchain, target, and feature set;
- no secret material;
- no reuse of untrusted build outputs in privileged release jobs;
- bounded retention;
- clear separation between dependency caches and release artifacts.

Ensure fuzz crash artifacts and test logs have appropriate retention and do not expose sensitive local data.

## Track G — Branch protection and status visibility

Identify which jobs should be required status checks. Ensure:
- check names are stable;
- failures are visible and actionable;
- skipped matrix jobs cannot accidentally satisfy release gates;
- release commits/tags cannot bypass mandatory validation without documented emergency procedure;
- CI status is attached to default-branch commits, not only pull requests.

This should also resolve the recurring inability to verify combined status on direct commits.

## Track H — Repository hygiene automation

Add checks for:
- committed wheels/binaries/build directories;
- generated files not updated where required;
- stale plan/doc counts if those remain maintained;
- version mismatch across manifests;
- undocumented public API changes;
- missing licenses/notices;
- source archive exclusions.

Keep these checks deterministic and fast.

## Track I — Security response integration

Ensure repository configuration and docs include:
- `SECURITY.md` with private reporting channel;
- supported-version policy;
- vulnerability triage and embargo procedure;
- dependency advisory response process;
- release revocation/yank procedure;
- contact ownership.

## Validation

Perform a full dry run from a clean tag-like commit:
- all required checks pass;
- artifacts build on supported targets;
- installed-artifact tests pass;
- checksums/provenance are generated;
- publication is stopped before external registry writes;
- workflow permissions are reviewed.

## Acceptance criteria

- CI covers every declared feature and supported platform class.
- Workflow permissions are minimal.
- Third-party action pinning/update policy is explicit.
- Release artifacts have checksums and traceable provenance.
- Release workflow has dry-run/RC mode and approval gates.
- Required checks run on default-branch commits and pull requests.
- Dependency/license/security policies are enforced.
- Repository hygiene checks block committed build artifacts and API drift.
- No unsupported reproducibility claim is made.

## Non-goals

- No bespoke build farm.
- No mandatory byte-for-byte reproducibility unless demonstrated.
- No expansion of product functionality.
