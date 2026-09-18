# Plan 218 — Dependency security remediation and advisory automation

## Purpose

Close the immediate dependency advisory discovered during the September 2026 review and make the repository detect newly published RustSec advisories even when no commit or pull request occurs.

This plan is independent of the architectural cleanup and should land first.

## Current finding

The reviewed closures resolve:
- root workspace: `rustls 0.23.41`
- excluded Python wheel closure: `rustls 0.23.43`

RUSTSEC-2026-0285 requires `rustls >= 0.23.45`.

Sibling networking repositories already resolve 0.23.45, so this update also reduces cross-repository version drift.

## Goals

- Set an explicit secure rustls floor of 0.23.45 or newer wherever eggserve directly constrains rustls.
- Regenerate and audit both distributed lockfiles.
- Add scheduled advisory scanning independent of source changes.
- Make critical networking dependency floors explicit rather than relying only on whatever the lockfile happens to contain.
- Preserve the existing narrow rustls/ring TLS stack.

## Non-goals

- No TLS feature redesign.
- No H2/H3 support-tier promotion.
- No broad dependency refresh unless required by the patched rustls resolution.
- No replacement of cargo-audit/cargo-deny.

## Work

### 1. Manifest floors

Inspect every manifest directly specifying:
- `rustls`
- `tokio-rustls`
- `rustls-pki-types`
- `rustls-webpki`
- `quinn` feature combinations that unify rustls

Set direct rustls constraints so Cargo cannot legitimately resolve below 0.23.45 after a future lock regeneration.

Do the same in `crates/eggserve-python/Cargo.toml`, which has an independent lockfile.

### 2. Regenerate both closures

Update:
- root `Cargo.lock`
- `crates/eggserve-python/Cargo.lock`

Avoid unrelated churn. Record any unavoidable transitive changes in the implementation PR/commit.

### 3. Audit

Run:
- `cargo audit --file Cargo.lock`
- `cargo deny check`
- `cargo audit --file crates/eggserve-python/Cargo.lock`
- Python-manifest `cargo deny check --config ../../deny.toml`

Also run default/TLS/H2/H3 build and test closures because rustls is exercised differently across TCP TLS and QUIC TLS.

### 4. Scheduled security workflow

Add a scheduled GitHub Actions workflow or a schedule trigger to the existing supply-chain workflow.

Requirements:
- at least daily,
- read-only permissions,
- pinned GitHub actions,
- pinned cargo-audit/cargo-deny installation via the existing script,
- audits both lockfiles,
- no release credentials,
- concurrency cancellation to avoid duplicate jobs.

The scheduled job may omit the full test matrix; its purpose is early advisory detection.

### 5. Dependency policy tightening

Evaluate and, unless a documented blocker exists:
- change cargo-deny wildcard policy from `allow` to `deny`,
- explicitly ban unintended native TLS/OpenSSL stacks (`native-tls`, `openssl-sys`) from production closures,
- explicitly ban an unintended alternate crypto backend if the project contract remains rustls/ring-only,
- retain multiple-version warnings rather than forcing single-version closure globally.

Any ban must be based on architecture, not aesthetics.

### 6. Security-sensitive floor documentation

Add a short dependency-policy section documenting that:
- advisory response can require minimum versions newer than broad semver declarations,
- both lockfiles are distributed security boundaries,
- H3 has a coordinated version set,
- rustls-family floors are security-sensitive.

## Tests and evidence

- both audit closures clean,
- cargo-deny clean,
- TLS integration tests pass,
- H2+TLS tests pass,
- H3+TLS tests pass,
- Python wheel build/install/smoke passes,
- scheduled workflow syntax validated,
- no unintended OpenSSL/native TLS packages in the resolved production graphs.

## Rollback

Do not roll back the rustls security floor. If a regression is found in 0.23.45, move forward to a later patched rustls release or disable the affected optional feature while investigating.

The scheduled advisory workflow can be temporarily disabled only if it demonstrably causes infrastructure failure, with a replacement monitoring path documented first.

## Acceptance criteria

- No distributed eggserve closure resolves rustls below 0.23.45.
- Both lockfiles pass the repository's advisory and deny policies.
- Advisory scanning runs on a schedule without requiring a push/PR.
- Security-sensitive version floors are documented.
- Default builds do not acquire an alternate TLS implementation.
