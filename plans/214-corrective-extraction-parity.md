# Plan 214 — Corrective Extraction and Parity Closure

## Status

Planned.

## Purpose

Correct the Plan 211 crate-boundary implementation so that the new workspace crates become the actual homes of EggServe's mature production behavior instead of parallel simplified implementations.

The current topology is directionally correct, but the implementation leaves two overlapping architectures:

- the mature canonical request/response/service/static-serving implementation remains inside `eggserve-core`;
- `eggserve-primitives`, `eggserve-server`, and `eggserve-static` contain smaller independent implementations that do not yet provide parity with the mature core.

This plan closes that gap without reverting the Plan 211 topology, without weakening Plan 212 TLS extraction, and without changing Plan 213's experimental HTTP/3 boundary.

## Background

Plan 211 successfully introduced these dependency boundaries:

- `eggserve-primitives` as a dependency-free leaf;
- `eggserve-server` as a generic Hyper/Tokio runtime;
- `eggserve-static` as a static-serving specialization;
- `eggserve-core` as a 0.1 compatibility aggregate;
- `scripts/check-crate-topology.py` as a machine-enforced dependency gate.

However, the implementation did not yet move the mature production implementation into those crates.

Examples of the resulting overlap include:

- `eggserve-core::primitives` still owns the mature canonical request/body/lifecycle/context/proxy/planner implementation while `eggserve-primitives` defines a second, smaller canonical model;
- `eggserve-core::server` still owns the mature generic service/runtime path while `eggserve-server` implements a separate simplified HTTP/1 runtime that eagerly buffers request bodies;
- `eggserve-core` still owns the hardened descriptor/handle-relative filesystem confinement while `eggserve-static` contains a separate pathname-based implementation using `canonicalize`, `symlink_metadata`, `metadata`, and `read`;
- documentation currently suggests using `eggserve-server` directly for generic application services even though the mature downstream application-server substrate remains in `eggserve-core::server`.

This is acceptable as an intermediate migration scaffold, but not as the final Plan 211 architecture.

The goal of Plan 214 is to converge to one implementation per architectural responsibility.

## Goals

1. Make `eggserve-primitives` own the mature canonical application-facing type system.
2. Make `eggserve-server` own the mature generic application-server/runtime implementation.
3. Make `eggserve-static` own the mature hardened static-serving and filesystem-confinement implementation.
4. Convert `eggserve-core` into a real compatibility facade/aggregate rather than the primary implementation plus parallel re-exports.
5. Preserve existing Rust and Python compatibility paths during the 0.1 line where practical.
6. Preserve all current security properties, protocol semantics, lifecycle behavior, and qualification coverage.
7. Remove or explicitly deprecate the temporary simplified implementations after parity is proven.

## Non-goals

- Redesigning the canonical service contract.
- Adding framework routing, middleware, ASGI, WSGI, templating, or application semantics.
- Promoting HTTP/3 from experimental status.
- Reworking the neutral `eggnet-tls` extraction.
- Extracting inbound PROXY/trusted-forwarding logic into a cross-repository crate.
- Creating a generic `eggnet-core`.
- Rewriting working production behavior merely to make file moves aesthetically cleaner.

## Principle — Move mature code, do not reimplement it

The central rule for this plan is:

> New leaf crates must become owners of the existing qualified implementation, not alternate implementations with similar names.

Where practical, move source modules with minimal semantic changes first. Refactoring internals should follow only after parity is established.

A code move that preserves behavior is preferred over a rewrite that must rediscover existing edge cases.

## Workstream A — Canonical primitives convergence

### Objective

Establish `eggserve-primitives` as the single canonical request/response/service-facing model.

### Required work

Inventory the current mature `eggserve-core::primitives` surface and classify modules into:

1. runtime-neutral canonical/domain types that belong in `eggserve-primitives`;
2. transport adapters that belong in `eggserve-server` or optional adapter modules;
3. static-serving planning/policy that belongs in `eggserve-static`;
4. compatibility-only wrappers that may remain in `eggserve-core` temporarily.

Move the mature runtime-neutral implementation rather than reproducing it.

Candidate ownership includes, where dependency-neutral:

- method/version types;
- request target and authority validation;
- validated header block/name/value types;
- canonical request head;
- response/status/header/body metadata;
- connection/proxy provenance domain values;
- request context metadata that does not require a concrete runtime;
- generic limits and policy vocabulary;
- canonical normalization rules that can remain transport-neutral;
- lifecycle/cancellation value types where they can be expressed without Tokio/Hyper.

### Dependency rule

`eggserve-primitives` must remain free of direct production dependencies on:

- Hyper;
- Hyper-util;
- Tokio;
- Rustls;
- Quinn;
- H3/H3-Quinn;
- filesystem/platform crates.

If a currently canonical type has a hard runtime dependency, separate its pure state/contract from the runtime mechanism rather than importing the runtime into the primitives crate.

### Compatibility

After each migrated group:

- `eggserve_core::primitives::*` should re-export or thinly wrap the exact types from `eggserve-primitives`;
- avoid maintaining duplicate structs/enums with conversion glue unless a short-lived compatibility shim is unavoidable;
- add compile-time identity tests where feasible to prove facade imports and direct-crate imports resolve to the same underlying types.

### Exit criteria

There must no longer be two independently implemented canonical request/response type systems.

## Workstream B — Mature server/runtime extraction

### Objective

Make `eggserve-server` own the existing mature generic server and application-service substrate.

### Required behavior to preserve

The extraction must retain the mature runtime characteristics already qualified in `eggserve-core::server`, including as applicable:

- canonical `Service` dispatch;
- streaming request bodies;
- one-shot body semantics;
- request lifecycle and cancellation propagation;
- `RequestContext` and connection metadata;
- handler/body timeout semantics;
- runtime admission controls;
- graceful shutdown;
- caller-owned connection serving;
- H1 behavior;
- optional H2 behavior;
- TLS transport integration through `eggnet-tls`/consumer-owned Tokio-Rustls boundaries;
- interim 1xx handling;
- trailers;
- generic tunnel/upgrade handoff;
- response normalization;
- write/no-progress timeouts;
- existing observability/OpsContext boundaries where those are part of the server contract.

The current small `eggserve-server/src/lib.rs` implementation must not remain as a second lower-fidelity runtime once the mature implementation is extracted.

### Migration strategy

Prefer a staged module move:

1. move or share foundational service/request lifecycle modules;
2. move connection/runtime state;
3. move H1/H2 drivers and listener/lifecycle machinery;
4. move transport-neutral server configuration;
5. reconnect `eggserve-core::server` as facade/re-export;
6. delete the temporary simplified runtime only after parity tests pass.

Use temporary `pub(crate)` bridge modules if necessary to keep intermediate commits buildable, but remove them by plan completion.

### Service type identity

`eggserve_server::Service` and `eggserve_core::server::Service` should converge to one trait definition.

Do not leave parallel service traits requiring adapters between the direct crate and compatibility facade.

### Body model

Do not regress to mandatory request-body buffering.

The direct `eggserve-server` path must expose the same streaming and lifecycle semantics used by the mature downstream application-server qualification.

### Protocol feature ownership

`eggserve-server` should own mature H1/H2 runtime behavior.

HTTP/3 remains behind `eggserve-h3` and the Plan 213 compatibility boundary until a separately scoped move is justified.

The result should be:

- H1/H2 generic server behavior in `eggserve-server`;
- H3 direct Quinn/H3 dependencies isolated in `eggserve-h3`;
- compatibility wiring in `eggserve-core` where needed during 0.1.

## Workstream C — Hardened static-serving extraction

### Objective

Replace the temporary pathname-based `eggserve-static` implementation with the existing mature hardened static-serving implementation.

### Security requirement

Do not ship two static-serving implementations with materially different confinement guarantees under the same EggServe project.

The production `eggserve-static` crate must retain the mature security model, including platform-appropriate confinement behavior already qualified in core.

On Unix, preserve descriptor-relative confinement and the existing no-follow/openat-style protections rather than checking a path and then reopening it by pathname.

On Windows, preserve the current handle-relative implementation and documented qualification boundaries.

### Required ownership

Move the mature implementation of:

- `SecureRoot`;
- path normalization/confinement;
- filesystem access helpers;
- MIME detection;
- static policy;
- static response planning;
- conditional/range/static semantics that are service-specific;
- directory handling;
- symlink/dotfile policy;
- relevant platform-specific `rustix`/Windows implementation;
- static-specific tests and adversarial fixtures.

`eggserve-static` may depend on `eggserve-primitives` and `eggserve-server`, plus platform/filesystem dependencies needed by the mature implementation.

The Plan 211 topology check must be updated accordingly: it should enforce forbidden architectural edges, not require `eggserve-static` to have exactly two total dependencies.

### Remove scaffold behavior

Delete temporary fixture/scaffold behavior such as the placeholder directory-listing response once the mature implementation is in place.

### Async behavior

Avoid introducing blocking `std::fs::read`/metadata work directly on async runtime worker threads where the mature implementation already has safer or more appropriate behavior.

## Workstream D — Convert `eggserve-core` into a real compatibility facade

### Objective

Reduce `eggserve-core` implementation ownership after the leaf crates achieve parity.

### Target role

During the 0.1 compatibility window, `eggserve-core` should primarily:

- preserve historical module paths;
- re-export mature leaf-crate types/functions;
- provide thin composition/configuration glue where required;
- own compatibility-only adapters that cannot yet move without a source break;
- retain HTTP/3 compatibility adapter code that Plan 213 intentionally left in core;
- retain only implementation that has an explicit documented reason not to move yet.

### Avoid

Do not retain full duplicate implementations in core merely because re-export migration is inconvenient.

Every substantial implementation left in core at plan completion should have a documented reason and a future ownership target if applicable.

## Workstream E — Documentation correction during migration

Until parity is complete, correct documentation that overstates the readiness of the direct crates.

In particular, do not recommend the current simplified `eggserve-server` as the preferred downstream application-server substrate while the mature implementation remains under `eggserve_core::server`.

During implementation, documentation should clearly distinguish:

- migration/scaffold state;
- mature compatibility path;
- final direct-crate path after parity.

At plan completion, update:

- README;
- `architecture/crate-topology.md`;
- direct-crate architecture pages;
- downstream app-server guide;
- agent/development guidance;
- dependency/public-API policy docs.

The final documentation should describe one canonical implementation with compatibility re-exports, not two parallel stacks.

## Workstream F — Parity and anti-duplication tests

### Required parity tests

Move or duplicate test harnesses temporarily so direct crates are validated against the same behavior currently expected from core.

At minimum, direct-crate tests should cover:

#### Primitives

- request/response canonicalization;
- header validation/order behavior;
- request target validation;
- authority/proxy provenance semantics;
- limits/policy behavior;
- type/path compatibility through the core facade.

#### Server

- H1 application-service contract;
- streaming body semantics;
- lifecycle cancellation;
- handler/body timeout split;
- admission behavior;
- graceful shutdown;
- caller-owned connections;
- interim responses;
- trailers;
- tunnel/upgrade behavior;
- H2 parity where enabled;
- TLS listener parity where enabled.

#### Static

- traversal rejection;
- symlink escape rejection;
- root rename/descriptor/handle qualification cases already covered by the mature implementation;
- ranges and conditional responses;
- MIME behavior;
- directory policy;
- dotfile policy;
- Windows qualification boundaries.

### Anti-duplication gate

Add a lightweight repository check or explicit review gate that fails or flags obvious reintroduction of parallel canonical/server/static implementations.

This does not need to be an AST-level duplicate detector.

Acceptable approaches include assertions that:

- core compatibility modules are predominantly re-exports/thin wrappers after migration;
- canonical type definitions exist only in the expected leaf crate;
- `eggserve-core` does not define a second `Service` trait;
- the mature `SecureRoot` implementation exists only in `eggserve-static`.

## Workstream G — Update topology enforcement

`scripts/check-crate-topology.py` currently validates direct Cargo edges but does not prove implementation ownership.

Retain the dependency checks and extend them where useful to reflect the final graph.

Expected high-level invariants:

- `eggserve-primitives` remains the dependency-light leaf;
- `eggserve-server` depends on primitives, never on static/core;
- `eggserve-static` depends downward on primitives/server and may own filesystem/platform dependencies;
- `eggnet-tls` remains neutral;
- `eggserve-h3` remains the H3/QUIC dependency boundary;
- `eggserve-core` aggregates/re-exports these layers without direct H3/Quinn/H3-Quinn dependencies.

Do not encode brittle exact-dependency-set checks for crates such as `eggserve-static` when legitimate mature functionality requires additional dependencies.

## Implementation sequencing

Use small, reversible phases rather than one giant source move.

Recommended order:

1. Correct documentation to mark direct crates as migration boundaries while parity work is in progress.
2. Converge canonical primitives first.
3. Extract mature generic server/service runtime onto the canonical leaf.
4. Extract mature static-serving/confinement implementation.
5. Switch `eggserve-core` compatibility modules to re-exports/thin wrappers.
6. Delete temporary simplified implementations.
7. Tighten topology/anti-duplication checks.
8. Run full cross-feature and platform qualification.
9. Update docs to recommend the direct crates after parity is proven.

Avoid moving static serving before the canonical/service type identity is settled, otherwise adapters will be written only to be removed later.

## Validation matrix

At minimum run:

- `cargo fmt --all -- --check`;
- workspace MSRV checks;
- workspace stable checks;
- clippy with warnings denied;
- workspace tests;
- doctests;
- default feature graph;
- `http2` graph;
- `tls` graph;
- `http2,tls` graph;
- `http3,tls` compatibility graph;
- excluded Python crate `--locked` check;
- Python wheel build/install/smoke/tests;
- supply-chain checks;
- crate-topology check;
- existing conformance matrix;
- downstream application-server qualification;
- static confinement/adversarial tests;
- manual H2/H3 qualification where required by existing release policy.

Where platform-sensitive confinement code moves crates, ensure Linux/macOS/Windows CI or qualification coverage still exercises the same behavior rather than only proving compilation.

## Acceptance criteria

Plan 214 is complete only when all of the following are true:

1. `eggserve-primitives` contains the mature canonical implementation used by the server and static layers.
2. `eggserve_core::primitives` is a compatibility facade over those exact types rather than a second canonical implementation.
3. `eggserve-server` contains the mature generic service/runtime implementation, including streaming/lifecycle semantics needed by downstream application servers.
4. `eggserve_core::server` re-exports or thinly wraps the direct server implementation wherever compatibility permits.
5. `eggserve-static` contains the mature hardened confinement/static-serving implementation rather than the current pathname-based scaffold.
6. No lower-security alternate static-serving implementation remains exposed as a production EggServe API.
7. Existing H1/H2/TLS application behavior and qualification remain green.
8. HTTP/3 remains isolated/experimental per Plan 213 and continues to consume the canonical shared service model.
9. `eggnet-tls` remains neutral and unchanged in architectural role.
10. Rust compatibility paths promised for the 0.1 line continue to compile or have an explicitly documented pre-1.0 migration decision.
11. Python wheel behavior remains unchanged unless a separately documented compatibility change is required.
12. Documentation recommends direct crates only after their mature parity is established.
13. The temporary duplicate/scaffold implementations are removed.

## Failure conditions

Do not mark this plan complete if any of the following remain:

- two independent canonical request/response models;
- two independent `Service` contracts;
- a direct `eggserve-server` path that lacks the mature streaming/lifecycle semantics while being documented as the preferred application-server substrate;
- pathname check-then-open static confinement exposed as the hardened direct static crate;
- mature implementation remaining in core solely because the new crates were implemented independently rather than extracted;
- compatibility tests passing only through adapters between duplicate type systems.

## Rollback strategy

Each extraction phase should be independently revertible.

If a moved subsystem reveals an unexpected compatibility issue:

- keep the mature implementation authoritative;
- temporarily restore the compatibility module in core;
- do not fall back to the lower-fidelity scaffold as the production implementation;
- document the blocker and narrow the next corrective phase.

The Plan 211 crate declarations themselves do not need to be reverted unless an extraction proves structurally impossible.

## Expected end state

The target graph after Plan 214 is conceptually:

```text
eggserve-primitives
        │
        ▼
eggserve-server ───────────────┐
        │                       │
        ▼                       │
eggserve-static                 │
                                │
eggnet-tls ─────────────────────┤
                                │
eggserve-h3  (experimental) ────┤
                                ▼
                         eggserve-core
                    compatibility facade
```

The arrows above represent composition/re-export relationships, not permission for lower layers to depend upward on `eggserve-core`.

There should be one canonical model, one mature generic server implementation, one hardened static implementation, one neutral TLS substrate, and one isolated experimental H3 dependency boundary.

That is the architecture Plan 211 intended to establish; Plan 214 closes the implementation-parity gap.