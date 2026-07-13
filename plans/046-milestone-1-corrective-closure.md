# Phase 46 — Milestone 1 Corrective Closure

## Goal

Close the remaining integration and evidence-correctness gaps in Plans 043–045 before beginning Milestone 2. The release infrastructure is already implemented; this phase is a narrow corrective pass to make execution policy, evidence identity, skip semantics, aggregation behavior, and checklist authority internally consistent and demonstrably fail-closed.

This phase must not add HTTP features, alter filesystem behavior, broaden product scope, or redesign the release system.

## Starting state

The repository now has:

- reconciled product and support documentation;
- `release/criteria.toml` as the machine-readable release-gate source;
- `scripts/release_criteria.py` for criteria and evidence handling;
- `scripts/release-validate.sh` for unified local validation;
- `scripts/check-contract-consistency.py` for documentation and workflow drift checks;
- normalized CI gate names;
- per-gate structured evidence generation;
- evidence aggregation and checklist generation;
- safety, criteria, and contract-consistency tests.

Remaining concerns:

1. workflow comments, criteria, and inventory disagree about which gates run on pull requests versus main pushes;
2. `package.core` and `package.bin` may not receive distinct evidence records;
3. conditional and skipped gates may be represented as missing evidence rather than explicit state;
4. aggregate validation must fail closed for malformed, conflicting, stale, invalidated, or wrong-SHA evidence;
5. generated and manually maintained checklist responsibilities remain ambiguous;
6. a real final-SHA CI evidence bundle has not yet been inspected and recorded.

## Track A — Reconcile execution policy

Choose and document one execution policy for each gate and trigger class:

- pull request;
- push to `main`;
- scheduled run;
- manual workflow dispatch;
- tagged release.

For each gate in `release/criteria.toml`, encode or verify:

- trigger applicability;
- required versus advisory status per trigger;
- expected workflow job;
- whether absence is `MISSING`, `NOT_APPLICABLE`, or `DEFERRED`;
- whether the gate may satisfy release qualification.

Synchronize:

- `.github/workflows/ci.yml` comments and `if:` expressions;
- `release/criteria.toml`;
- `docs/ci-gate-inventory.md`;
- `docs/release-process.md`;
- generated checklist output;
- README/AGENTS references if they describe cadence.

Preferred policy:

- fast correctness gates run on pull requests and main pushes;
- expensive package, cross-platform wheel, and supply-chain gates may be main-only if explicitly declared;
- release approval requires main/release evidence, never PR-only evidence;
- skipped main-only gates on PRs are `NOT_APPLICABLE`, not failures and not passes.

Extend `check-contract-consistency.py` so it parses workflow triggers/job conditions and compares them to criteria execution policy. Add fixtures covering mismatched comments, job conditions, and criteria declarations.

Acceptance:

- no contradictory PR/main policy remains;
- workflow/criteria drift fails tests;
- branch-protection expectations are documented separately from release qualification.

## Track B — Separate Rust package evidence

Ensure `package.core` and `package.bin` produce independent structured records.

Preferred implementation:

- add explicit modes to `scripts/verify-cargo-packages.sh`, such as `core`, `bin`, and `all`; or
- split orchestration into two scripts while sharing helpers;
- invoke each gate independently through `ci-gate-evidence.sh`;
- record package filename, package version, package-list digest, command result, and temporary-registry details where applicable.

Do not report one successful wrapper execution as proof of both package gates unless the wrapper emits and validates two distinct child evidence records.

Tests must prove:

- core succeeds and bin fails => core passed, bin failed;
- bin succeeds and core fails => bin passed, core failed;
- missing child evidence fails the corresponding required gate;
- duplicate or conflicting package evidence fails closed.

Acceptance:

- both package gates have distinct IDs and files;
- both can be evaluated independently;
- checklist output reports each result separately.

## Track C — Explicit skip and applicability semantics

Define a single status model, at minimum:

- `PASSED`;
- `FAILED`;
- `SKIPPED`;
- `NOT_APPLICABLE`;
- `DEFERRED`;
- `MISSING`;
- `STALE`;
- `INVALIDATED`;
- `MALFORMED`;
- `CONFLICTING`;
- `WAIVED`.

Document which states can satisfy:

- PR checks;
- main-branch qualification;
- release approval.

Every conditional job or command must emit a record with:

- gate ID;
- commit SHA;
- trigger;
- platform/target;
- status;
- skip reason or applicability reason;
- timestamp;
- workflow/job identity.

Cover at least:

- main-only gate on PR;
- missing fuzz corpus;
- unsupported platform;
- disabled optional feature;
- manual release gate not yet run;
- human approval not yet granted.

Required release gates must never become satisfied through a generic `SKIPPED` state.

Acceptance:

- missing evidence is distinguishable from deliberate non-applicability;
- all conditional paths generate deterministic records;
- checklist rendering uses the same status vocabulary as evidence validation.

## Track D — Fail-closed aggregation

Harden evidence aggregation so the aggregate job exits non-zero when any required invariant fails.

Required checks:

- exact commit SHA match;
- valid schema/version;
- known gate ID;
- expected trigger/platform/target;
- freshness;
- invalidation-path rules;
- required artifact presence and digest;
- deterministic duplicate handling;
- no contradictory pass/fail records;
- no malformed JSON ignored silently;
- no required gate satisfied by stale, skipped, or wrong-trigger evidence;
- waiver validity and scope.

Define precedence explicitly. Recommended order:

`MALFORMED > CONFLICTING > INVALIDATED > STALE > FAILED > MISSING > DEFERRED > NOT_APPLICABLE > WAIVED > PASSED`.

A waiver must never hide malformed or conflicting evidence and must identify approver, scope, reason, expiration, and affected SHA/version.

Add end-to-end tests using multi-file evidence bundles rather than only unit-level record validation.

Acceptance:

- aggregation fails closed;
- output is deterministic regardless of input file ordering;
- failure messages identify gate and reason;
- partial evidence cannot yield a release-ready checklist.

## Track E — Establish checklist authority

Adopt one clear ownership model:

- `release/criteria.toml` defines gates;
- generated template/reference files are never manually edited;
- exact-SHA release state is generated from an evidence bundle;
- human-only fields are stored separately or in a clearly delimited approval record.

Preferred resolution:

- keep `docs/release-checklist-generated.md` as the committed generated reference;
- convert `docs/release-checklist.md` into a short operator index that points to criteria, generated reference, current evidence bundle, known limitations, and approval procedure; or generate it entirely;
- add `--check` enforcement for every generated file;
- place a generated-file header in generated documents.

Acceptance:

- there is no second manually editable gate table;
- generated-file drift fails CI;
- exact release state is tied to an evidence artifact, not repository prose.

## Track F — Execute and inspect a real evidence run

Run the final corrected CI workflow on a clean main-branch commit.

Inspect:

- every expected gate evidence artifact;
- trigger and applicability states;
- package core/bin separation;
- platform/target fields;
- aggregate manifest;
- generated checklist;
- exact-SHA consistency;
- absence of credentials or secrets;
- deterministic regeneration from downloaded evidence.

Record the workflow run and artifact identifiers in the operator documentation or release checklist evidence record. Do not claim release readiness solely from local test counts.

Acceptance:

- one real main-branch run exercises the full Milestone 1 infrastructure;
- downloaded artifacts regenerate the same checklist;
- no required gate is silently absent;
- the aggregate job reports expected status and fails when a controlled evidence mutation is introduced in tests.

## Required validation

```sh
python3 -m unittest scripts.test_release_criteria -v
python3 -m unittest scripts.test_check_contract_consistency -v
python3 -m unittest scripts.test_release_safety -v
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
python3 scripts/release_criteria.py generate-checklist --check
bash scripts/release-validate.sh metadata
bash scripts/release-validate.sh fast
```

Run focused shell tests for package split and CI wrapper behavior. Validate `.github/workflows/ci.yml` as YAML and through any existing workflow lint path.

## Completion criteria

This phase is complete only when:

- trigger policy is consistent across workflow, criteria, inventory, and docs;
- package core and bin evidence are independent;
- skip/applicability states are explicit and tested;
- aggregation fails closed on every invalid evidence class;
- checklist authority is unambiguous;
- generated files are reproducible and checked;
- a real final-SHA evidence run has been inspected;
- no Milestone 1 release-infrastructure blocker remains.

## Non-goals

- No new HTTP types or response APIs.
- No changes to static-serving policy.
- No release publication.
- No broad rewrite of release tooling.
- No requirement that every expensive gate run on every pull request.