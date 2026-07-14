# CI Gate Inventory

This document maps every validation command, CI job, and release gate to its
criteria gate ID, execution policy, and evidence requirements. It is the
authoritative inventory for Plan 045 Track A.

## Inventory entries

| Command | Workflow/Job | Platform | Features | Execution policy | Evidence type | Criteria gate ID | Notes |
|---------|-------------|----------|----------|-----------------|---------------|-----------------|-------|
| `cargo fmt --all -- --check` | ci.yml / gate/rust-full | linux, macos, windows | — | PR, main | ci-log, lint-output | `rust.format` | |
| `cargo clippy --workspace --all-targets -- -D warnings` | ci.yml / gate/rust-full | linux, macos, windows | — | PR, main | ci-log, lint-output | `rust.clippy` | |
| `cargo test --workspace` | ci.yml / gate/rust-full | linux, macos, windows | — | PR, main | ci-log, test-output | `rust.test` | |
| `cargo test --workspace --doc` | ci.yml / gate/rust-full | linux, macos, windows | — | PR, main | ci-log, test-output | `rust.doctest` | |
| `cargo test -p eggserve-core --features client` | ci.yml / gate/rust-full | linux, macos, windows | client | PR, main | ci-log, test-output | `rust.test.client` | |
| `cargo clippy -p eggserve-core --features client-tls --all-targets -- -D warnings && cargo test -p eggserve-core --features client-tls` | ci.yml / gate/rust-full | linux, macos, windows | client-tls | PR, main | ci-log, test-output | `rust.test.client-tls` | |
| `cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings && cargo test -p eggserve-bin --features tls` | ci.yml / gate/rust-full | linux, macos, windows | tls | PR, main | ci-log, test-output | `rust.test.server-tls` | |
| `cargo test -p eggserve-core --test http_wire_correctness` | ci.yml / gate/http-wire | linux | — | PR, main | ci-log, test-output | `http.raw-wire` | |
| `cargo test -p eggserve-core --test http_primitives_integration` | ci.yml / gate/http-wire | linux | — | PR, main | ci-log, test-output | `http.primitives-integration` | |
| `cargo test -p eggserve-bin --test production_path` | ci.yml / gate/production-path | linux | — | PR, main | ci-log, test-output | `http.production-path` | |
| `cargo test -p eggserve-core --test corpus_replay && cargo test -p eggserve-core --test corpus_replay --features client` | ci.yml / gate/corpus-replay | linux | — | PR, main | ci-log, test-output | `filesystem.corpus-replay` | Skipped if no fuzz/corpus |
| `bash scripts/install-cargo-tools.sh && cargo audit` | ci.yml / gate/supply-chain | linux | — | main | ci-log, audit-output | `supply-chain.audit` | |
| `cargo deny check` | ci.yml / gate/supply-chain | linux | — | main | ci-log, deny-output | `supply-chain.deny` | |
| `bash scripts/verify-cargo-packages.sh --mode core` | ci.yml / gate/package | linux | — | main | ci-log, package-output | `package.core` | Core-only mode |
| `bash scripts/verify-cargo-packages.sh --mode bin` | ci.yml / gate/package | linux | — | main | ci-log, package-output | `package.bin` | Bin-only mode (packages core first) |
| `cd crates/eggserve-python && PYTHONPATH=python python -m unittest discover ...` | ci.yml / gate/python-unit-tests | linux | — | PR, main | ci-log, test-output | `python.unit-tests` | Source-only, no wheel needed |
| Python native tests, server primitives, API stability, boundary hardening, client primitives, server integration | ci.yml / gate/python-unit-tests | linux | — | PR, main | ci-log, test-output | `python.native-tests`, `python.server-primitives`, `python.api-stability`, `python.boundary-hardening`, `python.client-primitives`, `python.server-integration` | Requires built wheel |
| `cd crates/eggserve-python/packaging-tests && bash run_all.sh ...` | ci.yml / gate/packaging-smoke | linux | — | main | ci-log, test-output | `python.packaging-smoke` | Requires built wheel |
| `bash crates/eggserve-python/packaging-tests/run_all.sh ...` | ci.yml / gate/python-${{ matrix.os }} | linux, macos, windows | — | main | ci-log, test-output, wheel | `python.wheel.linux`, `python.wheel.macos`, `python.wheel.windows` | Cross-platform wheel matrix |
| Release workflow dry run | release.yml / validate | linux | — | manual dispatch | ci-log, release-output | `release.dry-run` | Defaults to dry_run=true |
| `bash scripts/inspect-release-bundle.sh ...` | release.yml / stage-release | linux | — | tagged push | ci-log, checksum, provenance | `release.artifacts` | |
| Provenance record | release.yml / stage-release | linux | — | tagged push | provenance | `release.provenance` | |
| Human approval | release.yml / publish | linux | — | tagged push | approval-record | `release.human-approval` | Requires environment gate |
| `python3 scripts/release_criteria.py generate-checklist --check --checklist-output docs/release-checklist.md` | ci.yml / gate/evidence-aggregate | linux | — | PR, main | ci-log, lint-output | `check-generated` | Checks canonical `docs/release-checklist.md` |
| `python3 scripts/check-contract-consistency.py` | local / release-validate.sh | linux | — | PR, main | — | contract-consistency | Not a criteria gate; run via release-validate.sh |
| `python3 scripts/release_criteria.py validate release/criteria.toml` | local / release-validate.sh | linux | — | PR, main | — | criteria.validate | Not a criteria gate; run via release-validate.sh |

## CI job names to criteria gate mapping

| CI job name | Criteria gates covered |
|-------------|----------------------|
| `gate/rust-full-${{ matrix.os }}` | `rust.format`, `rust.clippy`, `rust.test`, `rust.doctest`, `rust.test.client`, `rust.test.client-tls`, `rust.test.server-tls` |
| `gate/http-wire` | `http.raw-wire`, `http.primitives-integration` |
| `gate/production-path` | `http.production-path` |
| `gate/corpus-replay` | `filesystem.corpus-replay` |
| `gate/supply-chain` | `supply-chain.audit`, `supply-chain.deny` |
| `gate/package` | `package.core`, `package.bin` |
| `gate/python-unit-tests` | `python.unit-tests`, `python.native-tests`, `python.server-primitives`, `python.api-stability`, `python.boundary-hardening`, `python.client-primitives`, `python.server-integration` |
| `gate/packaging-smoke` | `python.packaging-smoke` |
| `gate/python-${{ matrix.os }}` | `python.wheel.linux`, `python.wheel.macos`, `python.wheel.windows` |
| `gate/evidence-aggregate` | `check-generated` (plus aggregation of all gate evidence) |

## Evidence classes

- **LOCAL** — Command executed on a specific host with recorded tool versions, timestamps, and exit code. Produced by `release-validate.sh`.
- **GITHUB** — GitHub Actions workflow run with recorded run ID/URL, job name, runner OS, and artifact IDs. Produced by CI gate jobs.
- **ARTIFACT** — Built, downloadable artifact with recorded filename, platform, and SHA-256 digest. Produced by release workflow.
- **HUMAN** — Recorded approval decision with approver identity, date, and rationale. Not automated.
- **CONFIG** — Documentation and static review only. Never satisfies execution gates.
- **NOT-APPLICABLE** — Gate deliberately not executed for this trigger/platform/feature. Emitted via `ci-gate-evidence.sh --skip <reason>`. Recorded but does not satisfy release gates.

## Execution policy by trigger

The `triggers` field in `release/criteria.toml` is the authoritative source for
which gates run on which events. This table mirrors the criteria declarations.

| Trigger | Gates run | Evidence class |
|---------|-----------|---------------|
| Pull request | Fast gates: format, clippy, tests, doctests, feature matrix, wire, production-path, corpus, Python source/qualification tests, contract consistency, generated files | LOCAL (release-validate.sh) or GITHUB (CI) |
| Main push | All PR gates + package verification, supply-chain, installed-wheel Python tests, cross-platform wheels | GITHUB |
| Manual dispatch (release) | Full validation + artifact assembly + provenance | GITHUB, ARTIFACT |
| Tagged publication | Consumes previously qualified artifacts; requires human approval + environment gate | ARTIFACT, HUMAN |
| Scheduled | Fuzz campaigns, extended corpus, re-audit (not currently automated) | GITHUB |

Note: The `triggers` field in each `[[gate]]` block of `release/criteria.toml`
declares the authoritative execution policy. CI job `if:` conditions in
`.github/workflows/ci.yml` must be consistent with these declarations. The
`check_trigger_policy_consistency` check in `scripts/check-contract-consistency.py`
validates this alignment.

## Duplicate or mismatch notes

- `verify-cargo-packages.sh` is used for both `package.core` and `package.bin` gates. Each gate invokes the script with `--mode core` or `--mode bin` to produce independent evidence records.
- The `rust-check` CI job combines 7 criteria gates (format, clippy, tests, doctests, client, client-tls, server-tls) into a single matrix job for efficiency. Per-gate evidence would require separate jobs; the current approach emits a single artifact per OS.
- `contract-consistency` and `criteria.validate` are run by `release-validate.sh` but are not criteria gates in `release/criteria.toml`. They are infrastructure validation commands.
