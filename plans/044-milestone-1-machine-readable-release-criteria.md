# Phase 44 — Milestone 1B: Machine-Readable Release Criteria and Evidence Model

## Goal

Replace the release checklist's hand-maintained gate definitions with one machine-readable source of truth describing every release requirement, its evidence class, platform applicability, freshness, invalidation rules, dependencies, artifacts, and waiver policy.

This phase does not execute the final release or prove that all gates are green. It builds the data model and tooling required for later phases to produce a trustworthy release evidence bundle.

## Design requirements

The criteria system must be:

- checked into the repository;
- human-reviewable in ordinary diffs;
- strict enough to reject incomplete or contradictory gate definitions;
- independent of GitHub Actions YAML syntax;
- usable by local validation, CI, release workflows, and checklist generation;
- tied to exact commit and artifact identities;
- extensible without requiring a framework or service;
- deterministic and safe to run in an untrusted checkout;
- free of registry credentials and publication side effects.

Prefer TOML or JSON. TOML is recommended because the project already uses Rust/Python packaging metadata and the file should remain maintainable by humans. YAML is acceptable only if a strict parser and schema validation are used.

Recommended path:

```text
release/criteria.toml
release/schema.json              # optional if JSON Schema is used
scripts/release_criteria.py       # or a small Rust tool
```

## Track A — Define stable gate identifiers

Every release gate must have a stable machine identifier. Gate IDs must not depend on GitHub's generated matrix labels.

Recommended naming form:

```text
rust.format
rust.clippy.workspace
rust.test.workspace
rust.test.doctest
rust.msrv
rust.no-default-features
rust.feature.client
rust.feature.client-tls
rust.feature.server-tls
http.raw-wire
http.production-path
filesystem.security
filesystem.symlink-race
fuzz.corpus-replay
fuzz.campaign-current
supply-chain.audit
supply-chain.deny
package.core
package.bin
python.api
python.typing
python.wheel.linux
python.wheel.macos
python.wheel.windows
release.dry-run
release.artifacts
release.checksums
release.provenance
release.human-approval
```

Rules:

- IDs are lowercase and stable;
- an ID's semantic meaning cannot be silently repurposed;
- renaming an ID requires a migration note;
- matrix variants are expressed through attributes rather than ad hoc name suffixes where practical;
- each ID maps to one primary release assertion.

## Track B — Define the criteria schema

Each gate should support at least:

```toml
[[gate]]
id = "http.raw-wire"
title = "HTTP raw-wire correctness"
description = "Normative HTTP/1.x request and response behavior"
required = true
evidence_classes = ["GITHUB"]
command = "cargo test -p eggserve-core --test http_wire_correctness"
workflow_job = "gate/http-wire"
platforms = ["linux"]
features = []
artifacts = []
max_age_days = 30
invalidated_by = [
  "crates/eggserve-core/src/**",
  "crates/eggserve-core/tests/http_wire_correctness.rs",
  ".github/workflows/**",
  "Cargo.lock",
]
depends_on = ["rust.test.workspace"]
waiver_allowed = false
```

The exact syntax may differ, but the model must represent:

- `id`;
- title and description;
- required versus advisory;
- evidence class or classes;
- local command where applicable;
- workflow/job identity where applicable;
- applicable operating systems;
- architectures or targets where relevant;
- feature flags;
- expected artifacts;
- maximum evidence age;
- paths that invalidate evidence;
- gate dependencies;
- waiver allowance;
- waiver authority or minimum approver level;
- documentation reference;
- security relevance;
- release stage: preflight, qualification, artifact, approval, publication, post-publication.

## Track C — Define evidence classes

Implement a closed evidence-class enum.

### LOCAL

Must record:

- command;
- exit status;
- stdout/stderr summary or artifact reference;
- tool versions;
- operating system and architecture;
- commit SHA;
- dirty-tree state;
- start/end timestamps;
- relevant environment flags.

Local evidence may assist development but must not satisfy gates requiring cross-platform or GitHub execution.

### GITHUB

Must record:

- repository;
- workflow name/path;
- run ID and URL;
- job name and job ID if available;
- commit SHA;
- event type;
- conclusion;
- timestamps;
- runner OS/architecture;
- artifact IDs/digests where applicable.

Static workflow configuration is not GITHUB execution evidence.

### ARTIFACT

Must record:

- artifact logical name;
- filename;
- target platform/architecture;
- package version;
- SHA-256 or stronger digest;
- size;
- producing workflow/run;
- provenance reference;
- independent install/smoke result;
- contents inventory where relevant.

### HUMAN

Must record:

- approver identity;
- date/time;
- exact commit SHA;
- exact evidence-bundle digest;
- release version;
- accepted limitations;
- waivers and rationale;
- approval decision.

### CONFIG

CONFIG may be retained for documentation and static review, but it must never satisfy an execution gate. Tooling should reject a required gate whose only accepted evidence is CONFIG unless the gate is explicitly defined as a policy/documentation gate.

## Track D — Encode platform and language obligations

Represent support declarations in the criteria model rather than duplicating them only in prose.

At minimum encode:

- Linux x86_64: supported-hardened;
- Linux aarch64: release target with explicit artifact obligation;
- macOS arm64: supported-hardened;
- macOS x86_64: release target with explicit artifact obligation;
- Windows x86_64: supported-functional, not Unix-level filesystem hardened;
- supported CPython range;
- unsupported Python implementations;
- Rust toolchain/MSRV policy;
- server TLS and client TLS feature obligations.

The criteria validator must compare these values with package metadata and the capability matrix from Phase 43.

## Track E — Model freshness and invalidation

A green result from an unrelated or stale commit is not release evidence.

Implement rules for:

- exact-SHA gates: evidence must match the candidate SHA;
- freshness gates: evidence may predate the candidate only within a defined age and only if invalidating files did not change;
- artifact gates: evidence must refer to the exact candidate artifacts;
- human approval: invalidated by any change to candidate SHA or evidence bundle;
- fuzz campaign: maximum age plus invalidation by parser/security-critical changes;
- dependency audit: invalidated by Cargo manifests, lockfiles, deny/audit configuration, or tool version changes;
- wheel tests: invalidated by Python code, native bindings, packaging metadata, wheel scripts, binary packaging, or workflow changes;
- documentation consistency: invalidated by public docs or package metadata changes.

Provide a deterministic function:

```text
is_evidence_valid(criteria, evidence, candidate_sha, changed_paths) -> result
```

Return structured reasons for invalidity.

## Track F — Define gate dependencies

Examples:

- artifact checksum verification depends on artifact assembly;
- provenance verification depends on artifact assembly and candidate SHA determination;
- human approval depends on every mandatory pre-publication gate;
- post-publication smoke depends on successful publication;
- release dry run depends on package, wheel, supply-chain, and test gates;
- package-bin validation depends on package-core staging where a temporary registry is required.

The validator must reject:

- unknown dependencies;
- cycles;
- dependencies on advisory gates for mandatory correctness unless explicitly intended;
- approval before prerequisite gates;
- publication before approval.

Provide a deterministic topological order for reporting and orchestration.

## Track G — Define waiver policy

Waivers must be explicit, uncommon, and visible.

Each gate must state whether waiver is allowed.

For allowed waivers require:

- gate ID;
- candidate SHA;
- approver;
- rationale;
- risk classification;
- expiration;
- compensating controls;
- release-note disclosure where relevant.

Critical/high security, artifact identity, checksum, provenance, and publication-gating requirements should normally be non-waivable.

A waiver must never mutate the gate result to `passed`. Use a distinct state such as `waived`.

## Track H — Implement validator and CLI

Provide a command such as:

```sh
python scripts/release_criteria.py validate release/criteria.toml
python scripts/release_criteria.py list
python scripts/release_criteria.py explain http.raw-wire
python scripts/release_criteria.py graph
```

Or an equivalent Rust binary.

Required behaviors:

- parse criteria strictly;
- reject duplicate IDs;
- reject unknown fields unless schema evolution explicitly allows them;
- validate enums;
- validate commands and job names are non-empty where required;
- validate referenced platforms/features exist;
- validate dependency graph;
- validate waiver rules;
- validate support metadata consistency;
- emit stable JSON for machine consumers;
- emit readable diagnostics for developers;
- exit nonzero on error.

The tool must not execute arbitrary commands merely to validate the criteria file.

## Track I — Generate the release checklist skeleton

Generate `docs/release-checklist.md` or a generated companion from the criteria model.

The generated checklist should contain:

- release/candidate metadata placeholders;
- gates grouped by release stage;
- stable gate IDs;
- required/advisory status;
- expected evidence class;
- status placeholder;
- evidence reference placeholder;
- known platform limitations;
- approval section;
- invalidation statement.

Generated content must be clearly marked. Avoid maintaining a second hand-edited list of gates.

If the repository needs hand-written explanatory text, split it into:

- generated gate table;
- manually maintained policy preamble or release operator guide.

Add a CI check that regenerated output is clean.

## Track J — Add tests

Unit tests must cover:

- minimal valid criteria file;
- full repository criteria file;
- duplicate gate ID;
- missing required field;
- unknown evidence class;
- unknown platform or feature;
- dependency cycle;
- unknown dependency;
- invalid waiver configuration;
- invalid exact-SHA/freshness combination;
- invalid CONFIG-only execution gate;
- deterministic gate ordering;
- checklist generation stability;
- metadata mismatch detection;
- path invalidation matching;
- evidence validity reasons.

Golden tests are acceptable for generated JSON and Markdown, but normalize timestamps and platform-specific paths.

## Required initial gate inventory

The first criteria file must include the current release obligations, even if some later remain pending:

### Rust correctness

- format;
- workspace clippy;
- workspace tests;
- doctests;
- client feature;
- client TLS feature;
- server TLS feature;
- package build from packaged contents;
- optional MSRV/no-default-feature gates according to Phase 43 policy.

### HTTP/filesystem correctness

- raw-wire tests;
- production-path tests;
- request/body framing corpus;
- filesystem security tests;
- symlink race stress;
- conditional/range conformance.

### Fuzz/property

- deterministic property tests;
- corpus replay;
- current fuzz campaign.

### Supply chain

- pinned cargo-audit installation and execution;
- pinned cargo-deny installation and execution;
- action pinning verification;
- package content verification.

### Python

- source-only unit tests where appropriate;
- native API tests;
- API stability tests;
- installed-wheel tests;
- platform wheel matrix;
- version metadata check;
- clean environment/no source import check.

### Release artifacts

- Rust core crate;
- Rust binary crate;
- Unix binary archives;
- Windows archive;
- Python wheels;
- checksums;
- provenance;
- artifact inventory;
- release dry run;
- publication gating;
- human approval;
- post-publication smoke.

## Required deliverables

- `release/criteria.toml` or approved equivalent;
- schema/parser;
- criteria validator CLI;
- evidence-class definitions;
- platform/language declarations;
- freshness/invalidation implementation;
- dependency graph validation;
- waiver model;
- generated checklist skeleton;
- unit and golden tests;
- contributor/operator documentation;
- CI check for criteria validity and generated-file cleanliness.

## Required validation

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
python scripts/release_criteria.py validate release/criteria.toml
python scripts/release_criteria.py list --format json
python scripts/release_criteria.py graph
python scripts/release_criteria.py generate-checklist --check
```

Adjust command names to the selected implementation language, but preserve equivalent capabilities.

## Completion criteria

This phase is complete only when:

- one checked-in criteria file contains all current mandatory release gates;
- every gate has a stable ID and complete schema-valid definition;
- support declarations are represented and cross-checked;
- evidence classes are closed and validated;
- freshness and invalidation rules are executable;
- dependency cycles and unknown dependencies are rejected;
- waiver policy is explicit;
- the release checklist gate table is generated from criteria;
- CI fails on criteria errors or stale generated output;
- no gate is considered passed merely because a workflow job exists.

## Non-goals

- no final evidence collection;
- no release publication;
- no GitHub API integration beyond what is needed to define the model;
- no broad server/library feature work;
- no replacement of GitHub Actions;
- no hosted release dashboard;
- no weakening of existing dry-run or publication protections.