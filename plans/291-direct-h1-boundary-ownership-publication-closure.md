# Plan 291 — Direct H1 boundary-ownership publication and registry closure

Status: **COMPLETE**; `eggserve-server 0.3.1` is published and registry-qualified.

Program:
`plans/288-291-direct-h1-boundary-ownership-followup-program.md`.

Planning baseline:
`2d4bae12f8cb87c01a8496d765d18b1380871e72`.

## Goal

Publish the qualified direct-H1 parser-range, aggregate-header ownership, and
service-response metadata ownership contract; prove it from fresh registry-only
consumers; and record the exact downstream-unblock artifact.

This plan adds no new runtime design.

## Entry conditions

Plan 291 may begin only after Plan 290 records:

- proof-bearing implementation SHA;
- green hosted CI;
- combined direct-H1 qualification;
- public API compatibility classification;
- exact semver strategy.

If Plan 290 is not green, do not stage or publish a release.

## Track A — query live registry state

At execution time query crates.io for at least:

- `eggserve-server`;
- `eggserve-primitives`;
- `eggserve-core`;
- any package Plan 290 says changed.

Do not assume `0.3.1` is free merely because `0.3.0` was the planning
baseline.

If Plan 290 confirms an additive patch and `0.3.0` is still current, the
expected direct-server candidate is the next unused `0.3.x` patch.

If source compatibility requires a new minor, follow the Plan 290 decision.

## Track B — derive the minimal publish set

Expected implementation ownership is `eggserve-server` only:

- parser validation kernel lives in server;
- H1ConnectionPolicy ownership overrides live in server;
- response finalization/provenance lives in server;
- new public ownership types/methods live in server;
- `eggserve-primitives` need not change.

Do not republish sibling crates merely to synchronize versions.

### Core compatibility

Current `eggserve-core 0.3.0` declares:

```toml
eggserve-server = { version = "0.3.0", ... }
```

which is a normal compatible requirement, not an exact pin.

If a server patch is sufficient, prove a fresh registry-only
`eggserve-core 0.3.0` consumer can resolve the new server patch and pass its
compatibility build/tests where practical. That is preferable to an
unnecessary core publication.

Republish `eggserve-core` only if package metadata, API forwarding, or
qualification evidence demonstrates a real requirement.

The same rule applies to static/H3/bin/Python: no publication without a changed
artifact or dependency floor that must move.

## Track C — package/staged artifact qualification

Before publishing each selected crate:

```bash
cargo package -p <crate>
cargo package -p <crate> --allow-dirty
```

Use the repository's normal layered package/dry-run process and inspect the
actual package contents.

Required package checks for `eggserve-server`:

- new ownership type/method rustdocs included;
- no path-only dependency leakage;
- declared Rust version remains 1.89;
- default feature set unchanged;
- direct no-default-features package builds;
- `http-interop` package builds;
- `tower` package builds;
- no accidental `eggserve-core` / `eggserve-static` dependency.

Run supply-chain checks before publish.

## Track D — manual publication

Publish manually in dependency order using the repository's established release
process.

For every published crate record:

- exact version;
- crates.io timestamp;
- checksum/archive SHA-256;
- source SHA;
- package dependency metadata;
- publication command/result.

Do not create a tag/release claiming success before crates.io visibility and
registry-only consumers pass.

## Track E — registry-only direct consumer

Create a fresh temporary project outside the workspace using only crates.io.

Pin the exact new `eggserve-server` version and current required primitives
version.

No `path`, `patch`, git, or workspace dependency is allowed.

The consumer must prove:

1. `RuntimeConfig::default().h1_connection_policy()` keeps hardened defaults;
2. parser values above the former 4 MiB and 10,000 gates can be projected;
3. external aggregate-header ownership is configurable through the published
   H1ConnectionPolicy API;
4. external Date/Server service metadata ownership is configurable;
5. two service responses on one runtime retain different valid Date/Server
   metadata under External ownership;
6. absent service metadata remains absent under External ownership;
7. invalid service Date fails safely;
8. runtime-generated rejection still uses runtime metadata policy;
9. streaming response works;
10. caller-owned Tokio H1 stream works.

Retain output and resolved dependency graph in release evidence.

## Track F — registry-only real TLS-H1 embedding consumer

Re-run the generic caller-owned Rustls/Tokio-Rustls fixture from Plan 290 using
only registry crates:

```text
caller Rustls handshake + ALPN http/1.1
  -> established TlsStream
  -> published eggserve-server H1 policy
  -> external aggregate header + Date/Server ownership
  -> generic Service
```

This ensures the new seams compose with the exact direct embedding model that
Plan 286 published.

No EggServe TLS/core orchestration should enter this fixture.

## Track G — registry-only Tower/Axum consumer

Use the published server crate with `features = ["tower"]` and prove:

- Axum response Date/Server survives under External ownership;
- default ownership remains unchanged;
- streaming body works;
- duplicate application headers work;
- dependency graph excludes `eggserve-core`, `eggserve-static`, and PHF.

## Track H — compatibility-core resolution proof

If `eggserve-core` is not republished:

- create a fresh registry-only consumer pinned to the currently published
  core version;
- update/resolve `eggserve-server` to the new compatible patch;
- prove the core consumer still builds under its ordinary supported profile;
- record the resolved server version.

This is the evidence that a synchronized core patch was not required.

If Cargo cannot resolve the new server patch because of actual metadata
constraints, stop and publish the minimal required core patch only after its
package qualification is added to this evidence.

## Track I — publication docs/evidence

Create:

`release/plan-291-direct-h1-boundary-ownership-publication-closure.md`.

Record:

- Plan 290 proof SHA and CI run;
- live registry state before publication;
- semver decision;
- minimal publish set rationale;
- exact versions/checksums/timestamps;
- direct registry-only consumer result;
- TLS-H1 consumer result;
- Tower consumer result;
- core-resolution result;
- dependency graph;
- known residuals.

Update:

- `CHANGELOG.md`;
- direct embedding docs;
- `plans/ROADMAP.md`;
- skill/agent text only if the newly published version would otherwise leave
  a concrete stale instruction.

Do not rewrite historical Plan 286 evidence.

## Track J — downstream-unblock rule

Only Plan 291 may state that downstream direct embedders are unblocked.

The closure must state the exact published artifact that provides all three
required capabilities:

- wider explicit H1 parser range;
- external aggregate-header policy ownership;
- external service Date/Server ownership.

Source-only implementation or a queued publication is not sufficient.

## Verification

Run the full release-relevant repository matrix, including at minimum:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo check -p eggserve-server --no-default-features
cargo check -p eggserve-server --no-default-features --features http-interop
cargo check -p eggserve-server --no-default-features --features tower
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
```

Require green hosted CI for the proof-bearing source SHA and for any
post-publication metadata-only correction SHA before final closure.

## Acceptance criteria

- [x] live registry state was queried.
- [x] release version follows Plan 290 compatibility evidence.
- [x] minimal publish set is justified.
- [x] package dry-runs pass.
- [x] selected crates are visible on crates.io.
- [x] exact checksums/timestamps are recorded.
- [x] direct registry-only consumer proves all new ownership/range seams.
- [x] real registry-only caller-owned TLS-H1 fixture passes.
- [x] registry-only Tower/Axum fixture passes.
- [x] direct Tower graph excludes core/static/PHF.
- [x] core compatibility resolves without unnecessary republish, or the
      minimal required core patch is explicitly qualified/published.
- [x] release evidence is complete.
- [x] roadmap/docs are truthful.
- [x] exact downstream-unblock artifact is named.

Evidence: `release/plan-291-direct-h1-boundary-ownership-publication-closure.md`.

## Non-goals

- No new API design.
- No H2/H3 promotion.
- No listener/TLS/static migration.
- No synchronized version bump for aesthetic consistency.
- No project-specific downstream adapter.
