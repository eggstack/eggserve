# Release Infrastructure — Deep Dive

eggserve uses an evidence-driven release process where every gate is defined in a machine-readable source of truth and validated by automated tooling.

## Components

| Component | Location | Purpose |
|-----------|----------|---------|
| Gate definitions | `release/criteria.toml` | Machine-readable source of truth for all release gates |
| Criteria validator | `scripts/release_criteria.py` | Validates criteria file, generates checklists, aggregates evidence |
| Local validation | `scripts/release-validate.sh` | Unified local validation (fast/full/gate modes) |
| Contract consistency | `scripts/check-contract-consistency.py` | Validates cross-document claims (TLS, versions, platforms, API) |
| CI evidence collection | `scripts/ci-gate-evidence.sh` | Collects evidence artifacts from CI runs |
| Release checklist | `docs/release-checklist.md` | Generated canonical checklist (do not edit by hand) |
| Cargo package gates | `scripts/verify-cargo-packages.sh` | Package and publish dry-run validation |

## Gate Structure (`release/criteria.toml`)

Each gate is a `[[gate]]` block with these fields:

| Field | Purpose |
|-------|---------|
| `id` | Unique gate identifier (e.g. `rust.format`, `python.unit-tests`) |
| `title` / `description` | Human-readable identity |
| `required` | Whether it's a release blocker |
| `command` | Exact validation command |
| `workflow_job` | Which CI job runs it |
| `platforms` | Target platforms (linux, macos, windows) |
| `triggers` | Which CI events run it (pull_request, push, manual_dispatch, tagged_push) |
| `evidence_classes` | What output artifacts are produced |
| `max_age_days` / `invalidated_by` | Staleness and invalidation rules |
| `depends_on` | Gate dependency graph |
| `waiver_allowed` | Always `false` (fail-closed) |
| `security_relevance` | Marks security-critical gates |
| `release_stage` | preflight, qualification, artifact, approval |

## Gate Categories (53 gates)

### Rust Correctness (7)
`rust.format`, `rust.clippy`, `rust.test`, `rust.doctest`, `rust.client-feature`, `rust.client-tls`, `rust.server-tls`

### HTTP/Filesystem Correctness (5)
`http.wire-correctness`, `http.production-path`, `http.primitives-integration`, `http.corpus-replay`, `http.canonical-corpus`

### Conformance (7)
`conformance.canonical-corpus`, `conformance.public-api`, `conformance.api-inventory`, `conformance.perf-regression`, `conformance.wire-interop`, `conformance.normalization`, `conformance.request-body`

### Supply Chain (2)
`supply-chain.cargo-audit`, `supply-chain.cargo-deny`

### Package (2)
`package.core-dry-run`, `package.bin-dry-run`

### Python (14)
`python.unit-tests`, `python.native-primitives`, `python.server-primitives`, `python.api-stability`, `python.boundary-hardening`, `python.client-primitives`, `python.server-integration`, `python.packaging-smoke`, `python.lifecycle-parity`, `python.resource-qualification`, `python.contract-consistency`, `python.wheel-linux`, `python.wheel-macos`, `python.wheel-windows`

### Runtime Service Boundary (5)
`runtime.public-consumer`, `runtime.service-dispatch`, `runtime.listener-lifecycle`, `runtime.graceful-shutdown`, `runtime.tls-parity`

### Request Body (10)
`body.public-api`, `body.fixed-length`, `body.chunked-accounting`, `body.limit-enforcement`, `body.timeout-cancellation`, `body.partial-consumption`, `body.static-rejection`, `body.corpus-replay`, `body.wire-tests`, `body.tls-parity`

### Python Runtime Parity (6)
`parity.runtime-parity`, `parity.callback-timeout`, `parity.lifecycle-linux`, `parity.lifecycle-macos`, `parity.lifecycle-windows`

### Release (4)
`release.dry-run`, `release.artifacts`, `release.provenance`, `release.human-approval`

### Generated File Cleanliness (1)
`generated.checklist-clean`

## Evidence Model

### Evidence Classes
- **LOCAL** — Command executed on a specific host with recorded tool versions
- **GITHUB** — GitHub Actions workflow run with run ID, job, SHA, conclusion
- **ARTIFACT** — Built downloadable artifact with SHA-256 digest
- **HUMAN** — Recorded approval decision with approver identity
- **CONFIG** — Documentation/static review only (never satisfies execution gates)

### Evidence Status
`passed`, `failed`, `skipped`, `not-applicable`, `error`

### Fail-Closed Aggregation

Evidence aggregation uses severity-ordered precedence:
```
MALFORMED > CONFLICTING > INVALIDATED > STALE > FAILED > MISSING
```
Waivers cannot hide malformed or conflicting evidence. The `aggregate` command validates an evidence bundle against all criteria gates.

## Local Validation Modes

```sh
./scripts/release-validate.sh fast          # routine dev check
./scripts/release-validate.sh full          # pre-release validation
./scripts/release-validate.sh gate <id>     # run a single gate
./scripts/release-validate.sh metadata      # contract consistency
./scripts/release-validate.sh evidence --output <path>  # copy evidence
```

## Release Operator Sequence

1. Choose candidate version and exact SHA
2. Verify clean tree (`git status --porcelain`)
3. Run fast validation locally
4. Push to trigger CI — all gate jobs run
5. Collect evidence from CI runs
6. Run `release_criteria.py aggregate` against evidence bundle
7. Review checklist — all required gates must be PASSED
8. Human approval gate (recorded decision with rationale)
9. Tag and push for release artifact build
10. Verify artifacts (digests, provenance, install/smoke)

## See Also

- [../release/criteria.toml](../release/criteria.toml) — Gate definitions (source of truth)
- [../docs/release-process.md](../docs/release-process.md) — Full operator guide
- [../docs/release-criteria.md](../docs/release-criteria.md) — Alpha/Beta/1.0 gate categories
- [../docs/ci-gate-inventory.md](../docs/ci-gate-inventory.md) — CI job-to-gate mapping
- [overview.md](overview.md) — Architecture overview
