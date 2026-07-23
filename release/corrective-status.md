# Corrective Program Status

> Plan 075 — Track H: Corrective Dashboard
> For handoff agents and reviewers. Descriptive only; does not replace the finding registry or release aggregator.

## Baseline

| Field | Value |
|-------|-------|
| SHA | `e3782abd971902e63a6b33d52b7a472da63e2310` |
| Branch | `main` |
| Rust | 1.97.0 (stable) |
| Python | 3.12.3 (compat: `>=3.14,<3.15`) |
| Platform | Ubuntu 24.04.4 LTS, x86_64 |

## Release Status

| Release | Title | Plans | Status |
|---------|-------|-------|--------|
| **A** | Critical safety and lifecycle correction | 075, 076, 077 | ✅ Implemented |
| **B** | Embedded runtime contract correction | 078, 079, 080 | ✅ Implemented |
| **C** | HTTP semantic correction | 081, 082, 083 | ⚠️ 081-082 implemented, 083 verification pending |
| **D** | Windows hardened-profile completion | 084, 085, 086 | ✅ Implemented |
| **E** | Operational, performance, internet, and release closure | 087, 088, 089 | ✅ Implemented |

## Finding Summary

| Severity | Count | Closed | Open |
|----------|-------|--------|------|
| Critical | 3 | 3 | 0 |
| High | 7 | 7 | 0 |
| Medium | 6 | 6 | 0 |
| Low | 1 | 1 | 0 |
| **Total** | **17** | **17** | **0** |

## Plan Status

| Plan | Title | Status | Closure Evidence |
|------|-------|--------|------------------|
| 075 | Corrective baseline and release containment | ✅ Closed | This document + corrective-findings.toml |
| 076 | Windows Unicode and handle-ownership | ✅ Closed | Deferred to 084-086 (Release D) |
| 077 | Runtime timeout semantics and structured shutdown | ✅ Closed | commit e3782ab |
| 078 | Custom-service ownership and connection metadata | ✅ Closed | commit e3782ab |
| 079 | Request-body rejection and incomplete-body policy | ✅ Closed | commit e3782ab |
| 080 | Configuration authority and frontend parity | ✅ Closed | commit e3782ab |
| 081 | Unified static-file and directory-index response path | ✅ Closed | commit e3782ab |
| 082 | HEAD, error-response, and validator correctness | ✅ Closed | commit e3782ab |
| 083 | HTTP conformance and raw-wire corrective closure | ⏳ Verification pending | Plan 083 tracks A-M |

## Blocking Findings

None. All 17 findings are closed.

## Next Unblocked Plan

**Plan 083** — HTTP conformance and raw-wire corrective closure (verification gate).

## Known Environmental Requirements

- Windows tests require Developer Mode for symlink/junction fixtures (Plan 086).
- Proxy interop tests require Caddy and nginx binaries on the runner.
- Soak tests require 24-hour uninterrupted execution.
- TLS tests require `--features tls` flag.

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
- `release/corrective-findings.toml` — Finding registry (Track B + C)
- `release/criteria.toml` — Release gate definitions
- `release/support-profiles.toml` — Production deployment profiles
- `docs/release-runbook.md` — Release operator runbook
