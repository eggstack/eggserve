# Plan 210 — Dependency and Release Security Closure

## Status

Complete — 2026-09-12.

## Purpose

Close the remaining supply-chain, dependency-audit, compiler-reproducibility, unsafe-code-policy, and security-documentation gaps identified during the EggServe dependency and security review.

This plan is deliberately limited to security and release hygiene. It must not introduce a large new verification framework, mandatory cargo-vet/crev workflow, or materially heavier CI without a demonstrated EggServe-specific benefit.

## Background

EggServe currently has a strong dependency-security baseline:

- root `Cargo.lock` is checked with `cargo audit`;
- the root dependency graph is checked with `cargo deny`;
- GitHub Actions are pinned;
- release artifacts are built with locked dependencies;
- PyPI publishing uses trusted publishing;
- release artifacts include SHA-256 manifests;
- the current root dependency graph has no known RustSec blocker.

Several remaining gaps exist.

The most important is that `crates/eggserve-python` is excluded from the root workspace and maintains its own `Cargo.lock`. The root supply-chain job therefore does not prove that the Python wheel dependency closure receives the same advisory and policy checks as the main workspace.

The release workflow also tracks the floating `stable` Rust toolchain. This is useful for compatibility CI but is undesirable for a release build, particularly given the Rust 1.98.0 compiler regression that was corrected in Rust 1.98.1.

The workspace also lacks an explicit unsafe-code lint policy comparable to the stricter policies already used in EggFetch and Eggress.

Finally, `SECURITY.md` still describes EggServe substantially as a static-file server and no longer reflects the actual security boundary represented by the reusable HTTP runtime, TLS/mTLS support, trusted proxy metadata, HTTP/2, HTTP/3, tunneling, and Python callback substrate.

## Goals

1. Ensure every lockfile used to produce a distributed EggServe artifact is explicitly security-audited.
2. Make release compiler selection deterministic.
3. Establish an explicit unsafe-code policy.
4. Bring security documentation in line with the current product and threat model.
5. Preserve the current lightweight CI philosophy.

## Non-goals

- Adding cargo-vet or cargo-crev as a mandatory release gate.
- Maintaining an internal vulnerability database.
- Replacing Cargo's dependency resolver.
- Introducing a custom dependency BOM crate.
- Treating experimental HTTP/3 as having the same assurance level as mature HTTP/1.1 or HTTP/2 support.
- Large CI expansion unrelated to a demonstrated security property.

## Workstream A — Audit every shipped dependency closure

Determine whether `crates/eggserve-python` should continue to maintain an independent lockfile.

### Preferred outcome

If Maturin packaging and the existing release workflow can safely use a shared workspace lockfile without complicating wheel builds or publication, bring the Python package into the workspace dependency/lock model.

This is only preferred if it materially simplifies ownership.

Do not force workspace membership if it makes the Python release path more brittle.

### Supported alternative

If the Python binding remains excluded:

- retain its independent `Cargo.lock`;
- add explicit `cargo audit` coverage for that lockfile;
- add explicit `cargo deny` coverage for that package/dependency graph;
- use the repository's existing audit and deny configuration rather than maintaining divergent policy files;
- ensure wheel CI continues to build with `--locked`.

The CI output must make it obvious that both the root dependency graph and Python wheel dependency graph have been evaluated.

### Acceptance

A dependency present only in `crates/eggserve-python/Cargo.lock` must be within the scope of the supply-chain job.

The implementation should include either automated evidence or a documented command showing that the excluded lockfile is independently checked.

## Workstream B — Pin release compiler versions

Separate compatibility testing from release reproducibility.

### CI

Retain a floating `stable` lane where useful to detect upcoming ecosystem/compiler compatibility issues.

Retain the project's supported minimum Rust version checks where currently applicable.

### Release

Pin release builds to an exact Rust patch release.

At implementation time, select the current known-good compiler patch after validating the entire release matrix. Based on the current review, Rust 1.98.1 is the minimum acceptable 1.98 release because it contains the correction for the 1.98.0 compiler regression.

Do not automatically turn every normal CI job into an exact-version job.

### Acceptance

A release generated twice from the same commit must not silently change compiler patch versions because `stable` moved.

The exact release compiler version must be visible in release workflow configuration or an equivalent checked-in release configuration.

## Workstream C — Establish an unsafe-code policy

Add an explicit workspace policy for unsafe Rust.

Preferred default:

`unsafe_code = "forbid"`

If any existing crate cannot satisfy `forbid`, use `deny` temporarily and document the exact exception rather than weakening the entire workspace.

Because `eggserve-python` is currently excluded from the workspace, make sure it receives an equivalent local policy.

PyO3 internals do not justify arbitrary application-level unsafe blocks in EggServe source.

If future platform-specific implementation requires unsafe FFI, place that code behind a very narrow module or dedicated crate with:

- documented safety invariants;
- minimal public surface;
- platform gating;
- targeted tests;
- explicit review.

### Acceptance

An unapproved new `unsafe` block in normal EggServe Rust code must fail CI or compilation.

## Workstream D — Reconcile security documentation

Update `SECURITY.md` to represent EggServe as it exists now.

Security-relevant surfaces should include:

- HTTP/1.1 and HTTP/2 server behavior;
- experimental HTTP/3/QUIC;
- TLS identity handling;
- SNI;
- optional and required client authentication;
- trust stores and CRLs;
- TLS configuration reload;
- trusted forwarding and PROXY protocol;
- listener and admission behavior;
- request/response framing;
- tunnel handling;
- custom Rust services;
- Python handlers/callbacks;
- static-file confinement.

Clarify what classes of upstream issue remain reportable to EggServe even when the root defect lies in Hyper, Rustls, Quinn, H3, Tokio, or another dependency.

Reconcile Windows confinement wording between `SECURITY.md` and the current threat model. The documentation must distinguish between implemented handle-relative confinement and any remaining adversarial qualification or rename/race limitations rather than claiming either full Unix-equivalent assurance or no confinement at all.

## Workstream E — Small release-hardening improvements

Evaluate, but do not automatically require:

- pinning the ARMv7/QEMU build image by digest;
- GitHub artifact attestations;
- Sigstore-compatible provenance;
- storing compiler/tool versions in release metadata.

These may be adopted if the implementation is small and does not materially complicate local-project maintenance.

They are not blockers for completion of this plan unless the implementation review identifies a concrete unresolved release-integrity risk.

## Tests and verification

Run:

- root dependency audit;
- root dependency policy check;
- Python dependency audit/policy check;
- root Rust tests;
- Python wheel build/test;
- TLS feature build;
- HTTP/2 feature build;
- HTTP/3 feature build;
- release workflow syntax validation.

Verify manually that `SECURITY.md` matches the current threat-model documentation.

## Exit criteria

This plan is complete when:

- every distributed Rust dependency closure is audited;
- the Python lockfile can no longer escape advisory scanning;
- releases use an exact compiler patch;
- floating stable compatibility testing remains available separately;
- unsafe-code policy is explicit;
- security documentation reflects the current server/runtime scope;
- all existing release and wheel workflows remain functional.

## Completion record

**Status: Complete (2026-09-12).** The excluded Python closure now upgrades to
PyO3 0.29.2 and is audited/policy-checked by
`scripts/check-supply-chain.sh` alongside the root lockfile. Release wheel
jobs use exact Rust 1.98.1, while routine compatibility lanes retain floating
stable. Workspace `unsafe_code = "deny"` is inherited by the two workspace
crates and declared locally by the excluded Python crate, with only the
documented Windows/systemd/test-fixture exceptions.
`SECURITY.md`, the threat-model architecture index, dependency/toolchain/release
docs, README, AGENTS.md, and the EggServe development skill now describe the
current runtime scope and verification boundary. ARMv7 image pinning,
attestations, and Sigstore provenance were evaluated and left out because the
plan makes them optional and they would add release complexity without a
demonstrated EggServe-specific security property.
