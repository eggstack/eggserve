# Phase 45 — Milestone 1C: Unified Validation, CI Gate Normalization, and Checklist Generation

## Goal

Connect the reconciled product contract from Phase 43 and the machine-readable release criteria from Phase 44 to actual developer and CI workflows.

This phase must provide:

- one local validation entry point;
- stable gate-oriented CI job names;
- structured evidence output;
- deterministic checklist generation;
- explicit separation among local, GitHub, artifact, and human evidence;
- a migration path from the current manually maintained release checklist;
- release-safe defaults with no publication side effects.

The result should make it straightforward to answer, for one exact commit: which gates were required, which were executed, which passed, which evidence is stale or invalid, which artifacts were produced, and what remains before approval.

## Preconditions

Before implementation:

- Phase 43's scope, support, stability, platform, and language decisions must be merged;
- Phase 44's criteria schema and validator must be available;
- the current GitHub Actions workflows, scripts, package verification tools, fuzz workflows, and release workflow must be inventoried;
- existing release safety controls must be preserved.

Do not silently change support claims in this phase. If implementation exposes a contradiction, return to the Phase 43 source of truth and update it deliberately.

## Track A — Inventory existing validation commands and workflows

Create a mapping from current commands/jobs to Phase 44 gate IDs.

Inventory at least:

- formatting;
- workspace clippy;
- workspace tests;
- doctests;
- client feature tests;
- client TLS lint/tests;
- server TLS lint/tests;
- raw-wire tests;
- production-path tests;
- corpus replay;
- property tests;
- filesystem security tests;
- symlink swap stress;
- Python source tests;
- Python native tests;
- Python server primitive tests;
- API stability tests;
- clean installed-wheel smoke tests;
- package/list/publish-dry-run checks;
- cargo-audit and cargo-deny;
- action pinning verification;
- documentation/metadata validation;
- release dry run;
- artifact assembly;
- checksums;
- provenance;
- publication gating.

For every item record:

- existing command;
- existing workflow and job;
- platform;
- features;
- expected runtime/cost;
- whether it is appropriate for every PR, main-only, scheduled, or release-only execution;
- evidence type;
- current duplication or mismatch;
- target criteria gate ID.

Store this inventory in a temporary implementation note or permanent operator document as appropriate.

## Track B — Build a unified local validation entry point

Add a command such as:

```sh
./scripts/release-validate.sh
```

A small Rust or Python driver is acceptable if it improves portability and structured output. A thin shell wrapper may invoke the driver.

Required modes:

```text
release-validate fast
release-validate full
release-validate gate <gate-id>
release-validate list
release-validate explain <gate-id>
release-validate evidence --output <path>
release-validate check-generated
```

### Fast mode

Suitable for routine development. It should run a bounded subset such as:

- criteria validation;
- contract consistency;
- formatting;
- core clippy;
- workspace unit tests;
- source-only Python unit tests;
- generated-file cleanliness.

Fast mode is not release evidence unless each selected gate's criteria explicitly permits LOCAL evidence.

### Full mode

Run every locally executable pre-release gate, including:

- formatting;
- workspace lint/tests/doctests;
- feature matrices supported on the host;
- raw-wire and production-path tests;
- corpus replay;
- package verification;
- supply-chain tools;
- Python tests;
- local wheel build/install smoke where supported;
- documentation/metadata checks.

Full mode must clearly report gates skipped because the host cannot satisfy another platform or architecture.

### Single-gate mode

Resolve the gate command from the criteria model. Do not maintain an independent hardcoded mapping unless generated from or verified against the criteria file.

### Safety rules

The local validator must:

- never publish;
- never require production registry credentials;
- never create a public release;
- never execute a gate marked GitHub-only or artifact-only as though local execution satisfied it;
- display dirty-tree state;
- refuse to label a dirty-tree run as candidate evidence;
- preserve command exit codes;
- terminate on invalid criteria before running gates;
- sanitize secrets and environment output.

## Track C — Emit structured local evidence

For each attempted gate emit a record containing:

- schema version;
- gate ID;
- result: passed, failed, skipped, not-applicable, error;
- evidence class;
- exact command;
- exit code;
- start/end timestamps and duration;
- commit SHA;
- dirty-tree state;
- OS, architecture, and target triple;
- Rust/Python/tool versions;
- selected features;
- stdout/stderr log artifact path or digest;
- skip reason;
- invalidation information where computable.

Recommended output:

```text
target/release-evidence/local/<timestamp>/manifest.json
target/release-evidence/local/<timestamp>/gates/<gate-id>.json
target/release-evidence/local/<timestamp>/logs/<gate-id>.log
```

Do not commit generated evidence.

Provide a human-readable terminal summary grouped by release stage and a stable JSON summary for automation.

## Track D — Normalize CI jobs around stable gate IDs

Refactor workflow job display names so required checks map predictably to criteria gate IDs.

Recommended display names:

```text
gate/rust-format
gate/rust-clippy
gate/rust-tests
gate/rust-doctests
gate/http-wire
gate/production-path
gate/filesystem-security
gate/corpus-replay
gate/client
gate/client-tls
gate/server-tls
gate/supply-chain
gate/package-core
gate/package-bin
gate/python-linux
gate/python-macos
gate/python-windows
gate/docs-metadata
```

Constraints:

- stable names should not depend on matrix formatting details;
- job names must remain unique;
- existing branch protection expectations must be documented before renaming;
- no required coverage may be lost during consolidation;
- avoid combining unrelated gates merely to reduce job count;
- expensive jobs may remain scheduled/main/release-only if the criteria model states that execution policy;
- action pins and least-privilege permissions must remain intact.

Where a matrix job covers multiple variants, emit per-variant evidence and one aggregate gate result only when every mandatory variant passes.

## Track E — Add CI evidence artifacts

Each gate job should emit a compact structured evidence record even when the command fails, where GitHub Actions permits artifact upload under `always()`.

Required fields should mirror LOCAL evidence and add:

- workflow name/path;
- run ID and URL;
- job ID/name;
- event type;
- runner image;
- repository;
- head SHA;
- base SHA where relevant;
- artifact IDs or uploaded names.

Add an aggregation job that:

- runs under `always()` after relevant gate jobs;
- downloads or reads gate evidence;
- validates records against the criteria schema;
- verifies exact candidate SHA;
- produces one CI evidence manifest;
- does not convert failed gates into success;
- fails if mandatory evidence records are missing or malformed;
- uploads the aggregate manifest as an artifact.

For ordinary PR CI, aggregation may report incomplete release evidence without requiring release-only gates. For release dry runs, all mandatory pre-publication gates must be present.

## Track F — Map execution policy to event types

Define and implement an explicit cadence:

### Pull requests

Run fast, high-signal gates:

- criteria and contract consistency;
- formatting/lint;
- workspace tests/doctests;
- raw-wire and production-path tests;
- bounded Python tests;
- representative platform checks;
- package metadata checks.

### Main branch

Run broader gates:

- complete OS matrix;
- installed wheels;
- package verification;
- full feature matrix;
- corpus replay;
- supply-chain checks.

### Scheduled

Run time-based qualification:

- fuzz campaigns;
- extended corpus replay;
- dependency audits if cadence differs;
- soak/resource tests in later milestones.

### Manual release dry run

Run every mandatory pre-publication and artifact gate, with publication disabled.

### Tagged publication

Consume previously qualified artifacts and evidence; require approval and environment gating.

Document why every non-PR gate is omitted from PRs and how release evidence remains complete.

## Track G — Generate and verify release checklist output

Use the Phase 44 criteria tool to generate the gate table. Extend it to merge an evidence manifest when provided.

Commands should support:

```sh
release-criteria generate-checklist --criteria release/criteria.toml
release-criteria generate-checklist --evidence evidence/manifest.json
release-criteria generate-checklist --check
```

The checklist should show:

- candidate version and SHA;
- evidence bundle/schema version;
- gate ID and title;
- required/advisory state;
- applicability;
- result;
- evidence class;
- run/artifact reference;
- timestamp and freshness;
- invalidation state;
- waiver state;
- known limitations;
- approval placeholders or approval record.

Statuses must distinguish:

- pending;
- passed;
- failed;
- skipped;
- not applicable;
- stale;
- invalidated;
- waived;
- error.

Do not collapse stale, invalidated, or waived into passed.

Generated output must be deterministic for identical inputs. Normalize ordering and avoid embedding volatile timestamps in `--check` output unless supplied through evidence.

## Track H — Migrate the existing release checklist

Preserve useful explanatory material from `docs/release-checklist.md`, including:

- evidence philosophy;
- dry-run publication safety;
- package verification notes;
- Windows limitation;
- follow-symlink limitation;
- accepted protocol/client limitations.

Move hand-maintained gate rows to generated output.

Recommended structure:

```text
docs/release-process.md              # human policy/operator guide
docs/release-checklist.md            # generated current skeleton or candidate output
release/criteria.toml                 # authoritative gates
```

If generated files are committed, CI must enforce regeneration cleanliness. If candidate-specific checklists are artifacts only, keep a committed generated blank/template checklist and document the distinction.

## Track I — Integrate metadata and support checks

The unified validator must run the consistency checks added in Phase 43 and schema checks added in Phase 44.

At minimum fail on:

- Python metadata/support mismatch;
- unsupported platform classifier without criteria coverage;
- stable API inventory drift;
- criteria job name missing from workflows;
- workflow gate job missing from criteria, unless explicitly internal;
- stale generated checklist;
- version disagreement;
- missing license/README package content;
- stale TLS/non-goal claims;
- release workflow publication default not set to dry run;
- publication job lacking required environment/condition safeguards.

The check should inspect workflow structure without pretending that configuration is execution evidence.

## Track J — Preserve and test release safety

Add tests proving:

- local full mode cannot publish;
- dry-run workflow cannot publish to crates.io or PyPI;
- dry-run cannot create a public GitHub release;
- publication jobs require explicit non-dry-run conditions;
- production registry credentials are referenced only in protected publication stages;
- artifacts are built before publication;
- evidence aggregation does not mask failed dependencies;
- missing evidence fails release aggregation;
- dirty local trees cannot produce candidate-valid evidence;
- generated checklist reports stale/invalid evidence correctly.

Where static workflow tests are used, keep them narrow and parse YAML structurally rather than using fragile regex alone.

## Track K — Documentation and operator workflow

Document the expected developer sequence:

```sh
./scripts/release-validate fast
./scripts/release-validate full
```

Document the release operator sequence:

1. choose candidate version and exact SHA;
2. verify a clean tree;
3. run/inspect required CI;
4. trigger manual dry run;
5. download aggregate evidence and artifacts;
6. verify evidence manifest against criteria;
7. generate checklist;
8. inspect checksums/provenance/artifact contents;
9. record human approval;
10. enter protected publication path;
11. run post-publication smoke tests.

Document failure handling:

- rerun versus invalidate;
- candidate SHA changed;
- workflow changed;
- artifact mismatch;
- stale fuzz evidence;
- failed platform wheel;
- partial publication;
- revoked approval.

## Required tests

Add tests for:

- fast/full/single-gate command selection;
- unknown gate ID;
- host-inapplicable gate;
- dirty-tree evidence;
- command failure propagation;
- structured evidence schema;
- deterministic output;
- missing tool handling;
- skipped platform reason;
- CI evidence aggregation;
- absent mandatory record;
- wrong commit SHA;
- duplicate record;
- stale record;
- invalidated record;
- aggregate failure when a required gate fails;
- generated checklist statuses;
- workflow/criteria job mapping;
- publication safety assertions.

Use test doubles for GitHub metadata. Unit tests must not require network access or production credentials.

## Required deliverables

- unified local validation command;
- fast, full, gate, list, explain, evidence, and generated-check modes;
- structured local evidence schema/output;
- normalized CI gate names;
- per-job CI evidence artifacts;
- aggregate CI evidence manifest;
- event/cadence documentation;
- generated release checklist integration;
- migrated release process documentation;
- metadata/workflow/criteria consistency checks;
- release safety tests;
- operator guide;
- updated contributor/AGENTS guidance.

## Required validation

Run at minimum:

```sh
./scripts/release-validate fast
./scripts/release-validate full
./scripts/release-validate list
./scripts/release-validate explain http.raw-wire
./scripts/release-validate check-generated
python scripts/release_criteria.py validate release/criteria.toml
python scripts/release_criteria.py generate-checklist --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
```

Also validate the GitHub workflow files with the repository's chosen workflow linter/parser and execute representative CI jobs from a clean branch.

## Completion criteria

This phase is complete only when:

- one local command can run and report all locally executable gates;
- each attempted gate emits schema-valid structured evidence;
- dirty-tree and host limitations are represented honestly;
- required CI jobs have stable criteria-aligned names;
- CI emits and aggregates exact-SHA gate evidence;
- checklist gate rows are generated from criteria and evidence;
- workflow and criteria drift fails CI;
- release safety protections have automated tests;
- ordinary PR, main, scheduled, dry-run, publication, and post-publication responsibilities are documented;
- no release is declared ready solely from workflow configuration or local-only results.

## Non-goals

- no final RC approval;
- no final package publication;
- no production dashboard or external evidence service;
- no new server/library HTTP capabilities;
- no ASGI/WSGI work;
- no replacement for GitHub Actions;
- no weakening of platform security classifications or artifact requirements.