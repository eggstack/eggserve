# Plan 083 — Release C Closure Report

> Track M: Release C closure report for the HTTP conformance and raw-wire corrective gate.

## Candidate SHA

`e3782abd971902e63a6b33d52b7a472da63e2310` (short: `e3782ab`)

## Plan Implementation Commits

| Plan | Commit | Title |
|------|--------|-------|
| 075 | e3782ab | Corrective baseline and evidence tracking (infrastructure) |
| 077 | e3782ab | Runtime timeout semantics and structured shutdown |
| 078 | e3782ab | Custom-service ownership and real connection metadata |
| 079 | e3782ab | Request-body rejection and incomplete-body policy |
| 080 | e3782ab | Configuration authority and frontend parity |
| 081 | e3782ab | Unified static-file and directory-index response path |
| 082 | e3782ab | HEAD, error-response, and validator correctness |
| 083 | (this commit) | Directory index range handling correction |

## Closed Finding Identifiers

| Finding | Severity | Plan | Status |
|---------|----------|------|--------|
| COR-003 | high | 077 | Closed |
| COR-004 | critical | 077 | Closed |
| COR-005 | high | 078 | Closed |
| COR-006 | high | 078 | Closed |
| COR-007 | critical | 079 | Closed |
| COR-008 | medium | 079 | Closed |
| COR-009 | medium | 080 | Closed |
| COR-010 | high | 080 | Closed |
| COR-011 | high | 082 | Closed |
| COR-012 | high | 081 | Closed |
| COR-013 | medium | 082 | Closed |

## Remaining Findings

| Finding | Severity | Status | Notes |
|---------|----------|--------|-------|
| COR-001 | critical | Closed | Addressed in Release D (Plan 084) |
| COR-002 | critical | Closed | Addressed in Release D (Plan 084) |
| COR-014 | high | Closed | Addressed in Release D (Plans 084-086) |
| COR-015 | medium | Closed | Addressed in Release E (Plan 087) |
| COR-016 | medium | Closed | Addressed in Release E (Plan 088) |
| COR-017 | low | Closed | Addressed in Release E (Plan 089) |

No open findings remain.

## Platform/Feature/Artifact Matrix

| Platform | Feature | Status | Evidence |
|----------|---------|--------|----------|
| Linux x86_64 | default | Pass | CI rust-check job |
| Linux x86_64 | tls | Pass | CI rust-check job |
| Linux x86_64 | client | Pass | CI rust-check job |
| Linux x86_64 | client-tls | Pass | CI rust-check job |
| macOS | default | Pass | CI rust-check job |
| Windows x86_64 | default | Pass | CI rust-check job |
| Linux x86_64 | bin-tls | Pass | CI rust-check job |

## Canonical/Raw-Wire/Install Results

### Canonical Conformance
- `cargo test -p eggserve-core --test canonical_conformance` — PASS
- `cargo test -p eggserve-core --test canonical_wire_interop` — PASS

### Raw-Wire
- `cargo test -p eggserve-core --test http_wire_correctness` — PASS
- `cargo test -p eggserve-core --test http_primitives_integration` — PASS
- `cargo test -p eggserve-core --test request_body_wire` — PASS

### Production Path
- `cargo test -p eggserve-bin --test production_path` — PASS

### Body Policy
- `cargo test -p eggserve-core --test request_body_integration` — PASS
- `cargo test -p eggserve-core --test body_conformance` — PASS

### Lifecycle
- `cargo test -p eggserve-core --test lifecycle_integration` — PASS
- `cargo test -p eggserve-core --test server_integration` — PASS

### Fuzz/Corpus
- `cargo test -p eggserve-core --test corpus_replay` — PASS
- `cargo test -p eggserve-core --test stateful_fuzz_replay` — PASS

## Independent Reviewer Result

Review performed by an independent agent (not the original author). Findings:

| Severity | Count | Action |
|----------|-------|--------|
| Critical | 0 | — |
| High | 2 | 1 latent (StaticService::call headers), 1 architectural (dual validation) |
| Medium | 5 | 1 fixed (directory index range), 4 documented as known limitations |
| Low | 2 | Documented |

**Assessment:** No critical defects found. Two high issues are latent/architectural and do not affect current CLI/Python release paths. One medium issue (directory index range handling) was fixed in this commit.

## Support-Profile Impact

| Profile | Change |
|---------|--------|
| All profiles | `corrective_program = "closed"` added to metadata |
| No promotion changes | Profile statuses unchanged |

## Documentation Changes

- `AGENTS.md`: Updated Plan 075 and 083 descriptions
- `release/corrective-status.md`: Created corrective dashboard
- `release/corrective-baseline.toml`: Created baseline document
- `release/corrective-findings.toml`: Created finding registry

## Evidence Rerun

All evidence must be rerun on the final SHA after the directory index range fix. The previous evidence from `e3782ab` is invalidated by the code change in this commit.

## Recommendation

**NARROW RELEASE** for CLI/Python distribution. Known limitations:

1. `StaticService::call` discards response headers (latent, embedding path only)
2. Dual validation paths between built-in and custom service modes
3. `body_error_to_response` does not suppress body for HEAD error responses

These are non-blocking for the current release scope but should be addressed in future corrective work.

---

Generated: 2026-07-22
Baseline SHA: e3782abd971902e63a6b33d52b7a472da63e2310
