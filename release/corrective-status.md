# Corrective Program Status

> Plan 075 — Track H: Corrective Dashboard (updated Plan 090)
> For handoff agents and reviewers. Descriptive only; does not replace the finding registry or release aggregator.

## Baseline

| Field | Value |
|-------|-------|
| SHA | `3484ffad5d5411ec8954a0b74f163cd2085b3ba9` |
| Branch | `main` |
| Rust | 1.97.0 (stable) |
| Python | 3.12.3 (compat: `>=3.14,<3.15`) |
| Platform | Ubuntu 24.04.4 LTS, x86_64 |

## Release Status

| Release | Title | Plans | Implementation | Evidence |
|---------|-------|-------|----------------|----------|
| **A** | Critical safety and lifecycle correction | 075, 076, 077 | Implemented | Passed (cross-platform gates); Windows evidence partial |
| **B** | Embedded runtime contract correction | 078, 079, 080 | Implemented | Passed |
| **C** | HTTP semantic correction | 081, 082, 083 | Implemented | Passed |
| **D** | Windows hardened-profile completion | 084, 085, 086 | Implemented | Partial (requires dedicated NTFS environment) |
| **E** | Operational, performance, internet, and release closure | 087, 088, 089 | Implemented | Partial (requires proxy/TLS/soak/review environments) |
| **F** | Corrective evidence and production qualification closure | 090 | Implemented | Partial (4 findings pending environment-dependent evidence) |

## Finding Summary

| Severity | Count | Implementation Complete | Evidence Passed | Evidence Partial | Evidence Pending |
|----------|-------|------------------------|-----------------|------------------|------------------|
| Critical | 4 | 4 | 2 | 2 | 0 |
| High | 8 | 8 | 6 | 2 | 0 |
| Medium | 6 | 6 | 5 | 1 | 0 |
| Low | 1 | 1 | 0 | 1 | 0 |
| **Total** | **19** | **19** | **13** | **6** | **0** |

## Evidence Status Detail

### Evidence Passed (13 findings)

These findings have both implementation and qualification evidence at the same SHA:

- COR-003: connection_total_timeout rename (runtime lifecycle gates)
- COR-004: Forced shutdown JoinSet migration (lifecycle gates)
- COR-005: Custom-service ownership (server integration gates)
- COR-006: Real connection metadata (server integration gates)
- COR-007: Body rejection before service invocation (body wire gates)
- COR-008: Incomplete-body drain removal (body gates)
- COR-009: Configuration authority (workspace tests)
- COR-010: Zero-valued limits validation (workspace tests)
- COR-011: HEAD normalization (conformance gates)
- COR-012: Directory index parity (conformance gates)
- COR-013: ETag nanosecond precision (conformance gates)
- COR-015: Structured logging (ops gates)
- COR-019: PinnedRoot panic-capable clone (Plan 090 Track B; workspace tests)
- COR-020: StaticService::call header/body loss (Plan 090 Track H; workspace tests)

### Evidence Partial (6 findings)

Implementation complete; qualification evidence requires dedicated environments:

- COR-001: Windows Unicode string lengths (requires NTFS VM)
- COR-002: Windows handle ownership (requires NTFS VM; Plan 090 Track B removes panic path)
- COR-014: Windows hardened traversal (requires NTFS VM with Developer Mode)
- COR-016: Streaming allocation (benchmark gates, optional)
- COR-017: Proxy/TLS/soak/artifact evidence (requires proxy runners, 24h soak, independent review)

## Plan Status

| Plan | Title | Implementation | Evidence | Notes |
|------|-------|----------------|----------|-------|
| 075 | Corrective baseline and release containment | Implemented | Passed | This document + corrective-findings.toml |
| 076 | Windows Unicode and handle-ownership | Implemented | Partial | Deferred to 084-086; requires NTFS VM |
| 077 | Runtime timeout semantics and structured shutdown | Implemented | Passed | commit 92a7486 |
| 078 | Custom-service ownership and connection metadata | Implemented | Passed | commit b935859 |
| 079 | Request-body rejection and incomplete-body policy | Implemented | Passed | commit ccf3cd1 |
| 080 | Configuration authority and frontend parity | Implemented | Passed | commit dd0ce8b |
| 081 | Unified static-file and directory-index response path | Implemented | Passed | commit 1d12b23 |
| 082 | HEAD, error-response, and validator correctness | Implemented | Passed | commit 8e567d9 |
| 083 | HTTP conformance and raw-wire corrective closure | Implemented | Passed | Independent review, no critical defects |
| 084 | Windows directory-handle retention | Implemented | Partial | Requires NTFS VM qualification |
| 085 | Windows handle-relative enumeration | Implemented | Partial | Requires NTFS VM qualification |
| 086 | Windows adversarial filesystem qualification | Implemented | Partial | 114 tests established; requires NTFS VM |
| 087 | Structured logging and operational error closure | Implemented | Passed | commit 522e12a |
| 088 | Streaming allocation and buffer performance | Implemented | Passed | commit 522e12a |
| 089 | Production-readiness roadmap | Implemented | Partial | Infrastructure exists; evidence pending |
| 090 | Corrective evidence and production qualification closure | Implemented | Partial | 4 findings pending environment-dependent evidence |

## Blocking Evidence Requirements

The following evidence blocks profile promotion:

### Production profiles (unix-reverse-proxy, unix-direct-https)

- `proxy.caddy-interop` — Caddy reverse-proxy interop tests
- `proxy.nginx-interop` — nginx reverse-proxy interop tests
- `proxy.desync-corpus` — Proxy desynchronization corpus
- `native-tls.abuse-limits` — Native TLS abuse and resource limits
- `stateful.fuzz-replay` — Stateful live-socket fuzz replay
- `filesystem.unix-race` — Unix filesystem race qualification
- `fault.injection` — Fault injection and degraded environments
- `soak.unix-reverse-proxy` — 24-hour reverse-proxy soak
- `soak.unix-direct-https` — 24-hour direct-HTTPS soak
- `artifact.installed-binaries` — Installed binary validation
- `supply-chain.sbom` — SBOM and provenance
- `release.independent-review` — Independent security review
- `release.profile-decision` — Profile promotion decision

### Windows profiles (windows-reverse-proxy)

- All windows.* gates require dedicated NTFS environment with Developer Mode
- `windows.independent-safety-review` — Windows independent safety review
- `windows.profile-decision` — Windows profile promotion decision

## Known Environmental Requirements

- Windows tests require Developer Mode for symlink/junction fixtures (Plan 086).
- Proxy interop tests require Caddy and nginx binaries on the runner.
- Soak tests require 24-hour uninterrupted execution.
- TLS tests require `--features tls` flag.
- Independent review requires a qualified external reviewer.
- SBOM/provenance requires artifact generation pipeline.

## Evidence Location

```
target/release-evidence/
├── ci/                     # CI-generated evidence (per gate)
│   ├── rust.format.json
│   ├── rust.clippy.json
│   ├── rust.test.json
│   └── ...
└── local/                  # Locally-generated evidence
    └── <timestamp>/
        ├── manifest.json
        └── <gate-id>.json
```

## Reference Documents

- `release/corrective-baseline.toml` — Pinned baseline (Track A)
- `release/corrective-findings.toml` — Finding registry (Track B + C, schema v2)
- `release/criteria.toml` — Release gate definitions
- `release/support-profiles.toml` — Production deployment profiles
- `docs/release-runbook.md` — Release operator runbook
- `plans/090-corrective-evidence-and-production-qualification-closure.md` — This plan
