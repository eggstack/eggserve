# Plan 211 — Crate Boundary and Dependency Topology Refactor

## Status

**Complete — 2026-09-12.**

## Purpose

Align EggServe's physical Cargo crate boundaries with the architecture that has emerged from Plans 199–208.

EggServe now exposes canonical HTTP primitives, policy, server lifecycle, listener handling, static serving, TLS, H1/H2 support, experimental H3 support, tunneling, Rust service integration, and Python bindings.

The source tree has already received substantial internal module cleanup, but Cargo-level dependency boundaries still cause unrelated consumers to pull dependencies associated with the full server runtime.

This plan introduces enforceable dependency layers without turning EggServe into a fragmented crate ecosystem.

## Design principles

1. Split by ownership and dependency direction, not by source-file size.
2. Avoid a crate-per-module architecture.
3. Preserve the current canonical request/service model.
4. New crate boundaries must prevent dependency leakage.
5. Compatibility matters more than achieving a theoretically perfect crate graph in one release.
6. Do not introduce a generic `eggnet-core`.
7. A downstream application server should be able to use EggServe without inheriting static-server-specific concerns.
8. A consumer that needs canonical types should not have to compile Quinn or filesystem confinement code.

## Target architecture

Introduce a dependency-light leaf crate for canonical application-facing primitives.

Recommended working name:

`eggserve-primitives`

The exact name may be adjusted during implementation if repository naming conventions strongly favor another term.

It should own concepts such as:

- canonical request metadata;
- canonical response representation;
- body/service abstractions where dependency-light implementation is feasible;
- HTTP method/version/header representations or adapters;
- limits that are independent of a concrete runtime;
- trusted peer/provenance types;
- generic request policy types;
- service-facing errors;
- cross-protocol canonical contracts.

The target is no dependency on:

- Hyper;
- Hyper-util;
- Quinn;
- H3;
- Tokio where technically avoidable;
- Rustls;
- platform filesystem APIs.

A Tokio dependency may be retained only if removing it would materially damage the service abstraction and no practical runtime-neutral representation exists. This must be an explicit decision rather than an accidental dependency.

## Server crate

Introduce:

`eggserve-server`

It should own the transport/runtime server machinery:

- TCP listeners;
- accept loops;
- lifecycle and graceful shutdown;
- admission controls;
- request timeout enforcement;
- H1/H2 connection serving;
- Hyper adapters;
- canonical service execution;
- listener ownership;
- connection metadata assembly;
- generic upgrade/tunnel handoff where protocol-neutral.

Expected dependencies include Tokio, Hyper, Hyper-util, and `eggserve-primitives`.

HTTP/3 should not live here after Plan 213 unless a very small protocol-neutral interface is required.

## Static-serving boundary

Introduce:

`eggserve-static`

This crate should own the filesystem-serving specialization:

- `SecureRoot`;
- confined path resolution;
- static response planning;
- range/static file operations where applicable;
- MIME mapping;
- platform-specific filesystem primitives;
- `rustix` usage;
- directory/static resource policy.

Static serving should consume canonical primitives and server interfaces rather than being the architectural center of the generic runtime.

This allows application-server authors to use EggServe without importing static-file-serving implementation code.

## Compatibility strategy

Do not create an unnecessary public API break during the first split.

Because existing consumers may import from `eggserve_core::*`, preserve a compatibility layer during the transition.

The implementation should choose one of the following patterns after dependency-cycle analysis.

### Preferred compatibility facade

Keep `eggserve-core` as a temporary aggregate/facade crate for the 0.1 series.

It may re-export:

- `eggserve-primitives`;
- `eggserve-server`;
- static support as appropriate;
- existing public names needed for source compatibility.

New dependency-sensitive consumers should be documented to depend directly on the leaf crates.

This is preferable to circularly forcing server/runtime code back into the primitives crate merely to preserve old paths.

### 0.2 cleanup

A future semver transition may redefine the meaning of `eggserve-core` once external consumers have had a migration path.

That rename/restructuring is outside this plan unless implementation proves there are effectively no external consumers and the compatibility layer has no value.

## Move sequencing

### Phase 1 — Introduce primitives crate

Move only stable canonical/domain concepts.

Add compile-time dependency checks or CI inspection that proves the crate does not acquire server-transport dependencies.

Keep behavior unchanged.

### Phase 2 — Introduce server crate

Move:

- listener;
- lifecycle;
- H1/H2 serving;
- transport-specific runtime limits;
- connection handling.

Adapt imports to consume primitives rather than current monolithic core internals.

### Phase 3 — Extract static concerns

Move confinement/filesystem/MIME/static planner logic.

Ensure generic server tests do not require filesystem-serving setup.

### Phase 4 — Compatibility facade

Restore existing public paths where practical through re-exports.

Document direct leaf-crate usage for downstream application-server authors.

### Phase 5 — Feature cleanup

Reduce feature propagation complexity.

Features should map to real optional capabilities rather than act as broad aliases for unrelated transport stacks.

In particular, no `http3` feature should cause H3/Quinn code to appear inside the dependency-light primitives crate.

## Binary and Python layering

`eggserve-bin` remains a presentation layer.

The Python crate should consume library crates directly for functional behavior.

Its dependency on `eggserve-bin::run_cli` is acceptable only for the CLI compatibility entry point.

Do not move CLI parsing or process orchestration into primitives simply to remove this edge.

A later naming cleanup from reusable `eggserve-bin` library logic to an `eggserve-cli` crate may be considered, but it is not required for this plan.

## Tests

Maintain all existing protocol and integration tests.

Add architecture-focused checks:

- `eggserve-primitives` builds independently;
- primitives do not depend on Hyper, Hyper-util, Quinn, H3, Rustls, or filesystem-specific crates unless an explicit reviewed exception exists;
- `eggserve-server` builds without static support;
- `eggserve-static` builds independently against primitives/interfaces;
- the old facade imports continue to compile where compatibility is promised;
- Python bindings still build and pass wheel tests;
- CLI behavior is unchanged;
- feature combinations remain compilable.

## Non-goals

- Moving TLS into the primitives crate.
- Creating one crate for every transport submodule.
- Sharing this new primitives crate with EggFetch.
- Creating common client/server request internals merely because both projects use HTTP.
- Rewriting canonical service semantics during the crate move.
- Application framework features, routing, middleware stacks, templating, ASGI implementation, or framework-level abstractions.

## Exit criteria

The plan is complete when EggServe has enforceable layers between:

1. canonical application-facing primitives;
2. server runtime/transport ownership;
3. static-file specialization;

and downstream server authors can consume the generic runtime without static-serving concerns or experimental HTTP/3 dependencies.

## Implementation record

The plan was implemented as a staged boundary refactor:

- `eggserve-primitives` is a dependency-free workspace crate for canonical
  request, response, policy, limits, and proxy-domain values.
- `eggserve-server` is a generic Hyper/Tokio HTTP runtime that consumes the
  primitives crate and has no dependency on `eggserve-core` or
  `eggserve-static`.
- `eggserve-static` owns the direct static-serving specialization and consumes
  only the primitives and server layers.
- `eggserve-core` remains the 0.1 compatibility aggregate while existing
  production-grade H2/H3, TLS, filesystem-confinement, and Python-facing
  implementations remain available through their established compatibility
  paths. It exposes the new layers under `eggserve_core::layers` so migration
  can proceed without a source break.
- `scripts/check-crate-topology.py` and CI/`verify.sh` enforce the dependency
  direction and leaf dependency restrictions.
- README, agent guidance, dependency/public-API policy, and crate architecture
  pages document the staged migration and its compatibility boundary.

## Validation record

The workspace and excluded Python manifest were checked with the stable and
Rust 1.88 toolchains, including default, `http2,tls`, and `http3,tls` feature
graphs. Workspace clippy (`-D warnings`), workspace tests, doctests, direct
layer tests, conformance and release metadata checks, topology checks, and
the excluded Python `--locked` check passed locally. The H3/TLS tests were
rerun sequentially after reclaiming local Cargo build artifacts to avoid
parallel-build disk exhaustion.
