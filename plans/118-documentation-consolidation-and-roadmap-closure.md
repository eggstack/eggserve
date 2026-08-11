# Plan 118 — Documentation Consolidation and Roadmap Closure

## Status

**COMPLETE — 2026-08-11.**

Closure gate for the Plan 112 consolidation roadmap. All acceptance criteria met.

Final phase of Plan 112. Execute only after Plans 113–117 have landed sufficiently that active architecture, supported API, dependency ownership, verification policy, timeout semantics, and Python packaging decisions are stable.

This is the closure gate for the consolidation roadmap. It should leave the repository easier to understand and hand off, not create another documentation-maintenance subsystem.

---

## Goal

Reduce overlapping normative documentation, reconcile all active docs with the simplified implementation, verify the final supported surface, and close Plans 112–118 without another follow-up plan for minor prose drift.

The desired end state is that a maintainer can understand EggServe from a small authoritative document set and use deeper documents only when needed.

---

## Documentation principles

### One fact, one normative owner

Each major policy area should have one primary authoritative document.

Recommended ownership:

```text
README.md
  product definition, quick start, supported platforms, high-level security defaults

docs/security-policy.md
  normative safe defaults and opt-in weakening behavior

docs/python-http-server-compatibility.md
  Python http.server-shaped compatibility contract

docs/python-api.md
  Python API reference

docs/cli.md
  CLI contract

docs/deployment.md / docs/tls.md
  deployment and native TLS limits

architecture/overview.md
  implementation architecture entry point and index

architecture/filesystem-confinement.md
  detailed platform confinement design

architecture/runtime.md
  runtime/service ownership and timeout model

architecture/primitives-api.md
  reusable Rust primitive boundary

architecture/testing-and-conformance.md
  concise verification/test architecture
```

Other documents may remain when they contain unique technical depth, but they must link to rather than restate normative policy whenever practical.

### Historical plans are records, not active architecture

Plans 000–118 remain useful implementation history. They should not be edited to pretend old implementation details never existed.

Active docs must not rely on historical plan prose as their only explanation of current behavior.

---

## Non-goals

Do not:

- delete historical plan files;
- rewrite all prose for stylistic consistency;
- create generated documentation;
- add a documentation linter/framework;
- add a docs CI job;
- create a new architecture taxonomy;
- expand scope while documenting it;
- reopen hardened filesystem or HTTP behavior without a concrete defect;
- create Plan 119 for minor wording preferences after this closure pass.

If a blocking runtime defect is discovered, record it explicitly and stop closure. Do not disguise source changes as documentation cleanup.

---

# Track A — Define the post-roadmap product statement

Before editing docs, write a short internal truth statement from the code and landed plans.

It should answer:

1. What does EggServe serve?
2. Which HTTP versions are supported?
3. Which Python facade is supported?
4. Which Rust primitives are intentionally public?
5. Is a full HTTP client still part of the product?
6. Is the deprecated pre-runtime service adapter still present?
7. What are the safe filesystem defaults?
8. What is Windows' qualification level?
9. How is TLS packaged for Rust CLI and Python wheel?
10. Which Python versions are actually supported?
11. What does routine CI run?
12. What is manual/release-only verification?
13. Does GitHub Actions publish anything?
14. What does `connection_total_timeout` mean?

Use this as the reconciliation checklist for active docs.

### Acceptance criteria

- all fourteen questions have one concrete answer before broad doc edits;
- answers come from current source/manifests/workflows, not plan intent alone;
- unresolved contradictions are fixed in their implementation-owning phase before closure.

---

# Track B — Remove known truthfulness defects

At minimum reconcile the known issue from the review:

```text
docs/security-review.md
```

It must not claim `cargo audit` or `cargo deny` runs in routine CI unless `.github/workflows/ci.yml` actually runs them.

Search broadly:

```sh
rg -n "cargo audit|cargo deny|routine CI|release CI|publish|publication|GitHub Actions" \
  README.md docs architecture AGENTS.md .opencode
```

Also reconcile:

- removed client/current client claims after Plan 113;
- deprecated service adapter claims after Plan 113;
- direct dependency/feature inventories after Plan 114;
- verification tier names after Plan 115;
- timeout/write-timeout wording after Plan 116;
- Python version and TLS-wheel policy after Plan 117.

### Acceptance criteria

- active docs match actual workflow behavior;
- no deleted surface is described as current;
- no plan-specific feature name is presented as a permanent product concept when the feature was removed/renamed;
- package support claims match metadata and tests.

---

# Track C — Consolidate architecture documentation

Review the architecture deep-dive set for duplication.

Do not delete a document simply because there are many documents. Delete or merge when the document:

- substantially repeats another normative owner;
- exists mainly to preserve a historical phase taxonomy;
- describes a removed subsystem;
- has become a second source of truth for the same field/enum/test inventory;
- requires frequent synchronized updates with no independent explanatory value.

Likely candidates after earlier phases:

- client deep dive if the client subsystem was removed;
- duplicated error-taxonomy inventories if Plan 116 simplified them;
- configuration inventories repeated across runtime/docs;
- multiple documents describing verification commands rather than linking to the authoritative development/release section.

Keep deep technical documents for filesystem confinement, canonical response planning, runtime ownership, Python compatibility, and security model where they provide real audit value.

### Preferred overview shape

`architecture/overview.md` should function as an index, not duplicate every enum, field, and plan-history detail.

Each deep dive should answer:

- why this subsystem exists;
- its security/correctness invariants;
- key public/ownership boundaries;
- where implementation lives;
- important limitations.

Avoid exhaustive source inventories that immediately become stale unless they materially help auditing.

### Acceptance criteria

- each retained architecture doc has a distinct purpose;
- overview remains concise enough to be an entry point;
- no removed subsystem retains an active deep dive;
- duplicate field/variant inventories are reduced where they cause maintenance burden.

---

# Track D — Consolidate reference documentation

Inspect `docs/` for repeated product/security statements.

Retain dedicated docs when users need a stable reference, especially:

- CLI;
- Python API;
- compatibility contract;
- security policy;
- deployment/TLS;
- threat model;
- release process;
- non-goals.

Prefer cross-links over copying the same security-default table into many documents.

For security review material, distinguish:

```text
normative policy
current qualification evidence
known limitations
historical review notes
```

If `docs/security-review.md` is mostly a snapshot, label it clearly as such and avoid making it the normative owner of CI policy.

### Acceptance criteria

- security defaults have one normative owner;
- release/CI policy has one normative owner;
- Python compatibility has one normative owner;
- snapshot/review docs are labeled as snapshots rather than permanent truth sources.

---

# Track E — Reconcile AGENTS.md and skill guidance

Agent guidance should be high-signal and operational.

Update `AGENTS.md` and repository skill files so they:

- identify Plans 112–118 as the consolidation track after Plan 111;
- describe only current supported product surfaces;
- do not tell agents to preserve removed experimental/deprecated modules;
- state the simplified verification commands;
- preserve security non-negotiables;
- link to authoritative docs instead of embedding long duplicated inventories;
- update the plan-number range.

Remove quirks that can be discovered trivially from source and that have repeatedly drifted.

Keep warnings an agent could plausibly violate without explicit guidance, such as:

- two-layer dotfile policy if still present;
- descriptor/handle-relative confinement constraints;
- Python wheel installed-test requirement;
- runtime-owned file-stream admission;
- no raw-socket exposure to Python handlers;
- manual release policy.

### Acceptance criteria

- agent guidance does not preserve deleted code;
- plan range and current roadmap status are correct;
- operational guidance is shorter or no larger without a concrete reason;
- duplicated architecture inventories are reduced.

---

# Track F — Final supported-surface verification

This closure pass must verify behavior, not merely prose.

Run the final routine check exactly as documented after Plan 115.

Run the final release check exactly as documented where required tooling is available.

At minimum verify:

### Rust/static server

- default CLI build;
- TLS CLI build;
- workspace tests;
- representative raw-wire static correctness;
- production `Server` / `StaticService` path;
- no deleted legacy module is referenced by production code.

### Python

- installed wheel builds and imports;
- bundled CLI exists and launches;
- `HTTPServer` / `ThreadingHTTPServer` work;
- `SimpleHTTPRequestHandler` uses hardened static resolution;
- HTTPS classes work if retained;
- supported Python-version claim matches actual compatibility tests.

### Scope searches

Run targeted searches such as:

```sh
rg -n "PyHttpClient|HttpClient|client-tls|primitives::client" \
  crates README.md docs architecture AGENTS.md

rg -n "eggserve_core::service|pre-runtime|deprecated compatibility adapter" \
  crates README.md docs architecture AGENTS.md

rg -n "response-write timeout|write timeout" \
  crates README.md docs architecture AGENTS.md

rg -n "cargo audit.*CI|cargo deny.*CI" \
  README.md docs architecture AGENTS.md
```

Remaining hits must be intentional current references or clearly historical context.

### Acceptance criteria

- final routine/release checks use the documented paths;
- no stale current-surface references remain;
- Python installed-wheel verification passes;
- no known roadmap defect remains open.

---

# Track G — Record closure without creating another bureaucracy

Update Plans 112–118 with concise completion metadata as each implementation lands, following existing repository convention.

For final Plan 118 closure, record:

- implementation commit SHA(s);
- which plans were completed;
- final supported product statement;
- any intentionally retained surface that a plan originally expected to remove and the evidence/rationale;
- final verification commands/results;
- any environment-limited test that could not be run locally and the existing CI/manual evidence used instead.

Do not add:

- generated evidence files;
- gate registries;
- completion databases;
- another roadmap document;
- a Plan 119 solely to polish Plan 118 prose.

### Acceptance criteria

- closure metadata is sufficient for handoff/audit;
- no new process mechanism is introduced;
- historical plan text is preserved except normal completion metadata.

---

## Complete roadmap acceptance criteria

Plan 118 and the Plan 112 roadmap are complete when all of the following are true.

### Scope

- EggServe's current product definition is narrow and internally consistent;
- no full client product survives accidentally;
- no deprecated parallel serving architecture survives accidentally;
- canonical reusable HTTP/security primitives remain available.

### Correctness/security

- hardened filesystem confinement remains unchanged except for independently justified bug fixes;
- static GET/HEAD/range/conditional behavior remains correct;
- runtime resource admission remains centralized;
- Python static handlers do not reopen translated paths;
- fail-closed handler-response behavior remains.

### Dependencies/artifacts

- direct dependency ownership is understandable;
- test-only dependencies are not promoted without reason;
- artifact measurements are recorded without adding a size gate;
- no feature was removed solely for size.

### Verification

- routine CI is small and accurate;
- release verification is manual/explicit;
- diagnostic security suites remain available but targeted;
- GitHub Actions does not publish.

### Runtime/API

- timeout semantics are truthful and tested;
- error taxonomy contains only current useful boundaries;
- no dead exception/API type is documented as active.

### Python

- wheel TLS policy is explicit;
- interpreter support is truthful and tested;
- installed-wheel behavior remains authoritative;
- no complex release matrix was introduced.

### Documentation

- active docs agree on current behavior;
- normative owners are clear;
- duplicate architecture/reference inventories are reduced;
- `AGENTS.md` and skill guidance point at current surfaces and Plans 112–118;
- no known issue from the Plan 112 roadmap requires another corrective planning phase.

---

## Rejection conditions

Reject closure if:

- a removed surface is still imported/exported by supported code;
- routine CI/docs disagree materially;
- active docs claim audit/deny automation that does not exist;
- timeout docs still advertise nonexistent behavior;
- Python metadata claims versions not actually supported;
- filesystem security was simplified for maintainability or size;
- release automation was added;
- a new generalized framework/process was introduced during consolidation;
- a known roadmap defect is deferred merely to mark the roadmap complete.

If only minor wording preferences remain after all behavioral and truthfulness criteria pass, close the roadmap and handle future documentation edits as ordinary maintenance rather than creating another plan.

---

## Closure Metadata

### Plans completed

| Plan | Status | Summary |
|------|--------|---------|
| 112 | Complete | Consolidation roadmap (this plan) |
| 113 | Complete | Product surface: removed client subsystem, deprecated service adapter |
| 114 | Complete | Dependency slimming, artifact measurement |
| 115 | Complete | CI simplification: two-job routine CI, manual release |
| 116 | Complete | Timeout semantics (`connection_total_timeout`), error taxonomy cleanup |
| 117 | Complete | Python TLS policy (unconditional in wheel), abi3 version broadening |
| 118 | Complete | Documentation consolidation and roadmap closure |

### Product statement

EggServe is a hardened, Rust-backed static file server with safe-by-default behavior. It serves static files via HTTP/1.1 only. The supported surfaces are: CLI binary (`eggserve`), Python `http.server`-shaped facade (`eggserve.server` with six classes), and reusable Rust primitives (`eggserve-core::primitives`). There is no HTTP client. There is no deprecated service adapter. TLS is optional in the CLI (`tls` feature) and unconditional in the Python wheel.

### Verification results

- `cargo fmt --all -- --check`: clean
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean
- `cargo test --workspace`: 1353 passed, 11 ignored
- `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings`: clean
- `cargo test -p eggserve-bin --features tls`: 88 passed
- Scope searches for stale references: all clean (no removed surfaces described as current, no false CI claims)

### Documentation changes

- Fixed `cargo audit`/`cargo deny` claims in `docs/security-review.md`, `docs/dependency-policy.md`, `docs/release-criteria.md`
- Removed deprecated `service` adapter references from `README.md`, `docs/architecture.md`
- Removed client subsystem references from `docs/toolchain-support.md`, `docs/extension-contract.md`, `docs/library-capability-matrix.md`, `docs/dependency-policy.md`
- Removed stale client test references from Python test files
- Updated `AGENTS.md` and `docs/architecture/overview.md` plan ranges to 000–118
- Updated skill file (`eggserve-dev/SKILL.md`) plan range and removed deprecated adapter note
- Cleaned up `architecture/eggserve-python.md` and `docs/python-packaging.md`

### Intentionally retained surfaces

No surfaces were retained that a plan originally expected to remove. All removals from Plans 113–117 were completed.

### Environment-limited tests

All routine CI checks were run locally and passed. The Python wheel build/test (`scripts/test-python-wheel.sh`) was not run locally as it requires a clean CPython 3.14 venv; existing CI evidence from prior merges is used.
