# Plan 219 — Collapse static/path/filesystem authority onto eggserve-static

## Purpose

Remove the largest remaining duplicated security-critical implementation in the repository.

Today `eggserve-core` and `eggserve-static` both contain near-identical path and filesystem-confinement code. The duplicated surface includes descriptor-relative Unix traversal, Windows handle/reparse logic, path decoding/component validation, and static response planning. These are precisely the modules that should have one implementation authority.

## Goals

- Make `eggserve-static` the sole implementation owner of static path parsing, secure-root resolution, filesystem confinement, MIME/static planning, and resolved-file capability handling.
- Convert `eggserve-core` static/path APIs to compatibility facades.
- Preserve the existing public `eggserve_core::primitives::{SecureRoot, ConfinedPath, ...}` paths through re-export/adaptation for the 0.x line.
- Remove duplicate Unix/Windows confinement source from core.
- Make future security fixes land once.

## Non-goals

- No behavior expansion.
- No follow-symlink hardening claim beyond the documented functional profile.
- No new filesystem crate yet; Plan 224 evaluates that only after this consolidation.
- No Python API redesign.

## Baseline to verify

The review found roughly 180 KiB of duplicated filesystem source between core and static, plus byte-identical path modules such as percent decoding/component/platform/policy/rejection logic.

Before editing, generate an exact inventory and diff so the implementation handoff has a reproducible baseline.

## Work

### 1. Define ownership

`eggserve-static` owns:
- path policy and rejection vocabulary needed for static serving,
- request-path confinement parsing,
- pinned root,
- Unix descriptor-relative traversal,
- Windows handle-relative traversal,
- symlink/reparse/dotfile enforcement,
- resolved file/directory capabilities,
- MIME lookup,
- conditional/range/static response planning,
- directory listing construction.

`eggserve-primitives` continues to own transport-neutral HTTP/request/response vocabulary and `StaticPolicy`.

`eggserve-core` owns none of the above implementations.

### 2. Establish public static crate surface

Promote only the static APIs needed to preserve existing core compatibility:
- `SecureRoot`
- `ResolvedFile`
- `ResolvedDirectory`
- `ResolvedResource`
- `ResourceDeniedReason`
- `ResolveAndPlanError`
- `ConfinedPath`
- `PathPolicy`
- `PathRejection`
- planner functions required by Python/core consumers

Do not expose raw fd/handle internals.

### 3. Replace core copies

For each core module in:
- `src/fs/**`
- `src/path/**`
- static planner/MIME/secure-root compatibility modules

replace implementation with direct re-exports or minimal wrappers.

Delete duplicated platform implementations after parity tests prove the static authority is being used.

### 4. Preserve capability semantics

A resolved file must remain an opened capability. Compatibility wrappers must never reconstruct an absolute path and reopen it.

Any Python bridge that currently relies on internal constructors should be migrated to a narrowly scoped static-crate bridge feature, or preferably redesigned to move the already-opened file capability without reconstructing provenance.

### 5. Follow-symlink boundary

Preserve the existing documented limitation: link-following mode is functional-only and retains the path-based residual race. Do not accidentally broaden the hardened claim while moving code.

### 6. Topology and unsafe boundary

Extend topology checks to fail if:
- `eggserve-core/src/fs` regains production confinement code,
- `eggserve-core/src/path` regains a second path parser,
- core directly depends on `rustix` or Windows confinement dependencies solely for static serving.

Where feasible after deletion, tighten unsafe policy in non-platform crates.

## Tests

Run all existing:
- path traversal and percent-decoding tests,
- Unix symlink swap/confinement tests,
- Windows reparse/namespace tests,
- directory listing tests,
- conditional/range tests,
- static-service tests,
- Python primitive tests.

Add:
- compatibility import tests proving core paths resolve to the static authority,
- a source/topology gate that detects reintroduction of duplicate filesystem modules,
- an installed-artifact static-serving smoke test after the move.

## Migration order

1. Expose static APIs.
2. Point core wrappers at static APIs while old implementation still exists.
3. Run parity tests.
4. Delete duplicate path modules.
5. Delete duplicate fs modules.
6. Remove now-unused core dependencies/features.
7. Update docs/topology.

## Rollback

If a compatibility API cannot be expressed as a re-export, retain a wrapper around the static type. Do not restore a second filesystem resolver.

## Acceptance criteria

- One production implementation of path confinement and filesystem resolution exists.
- `eggserve-core` contains no duplicate Unix/Windows resolver.
- Core compatibility imports continue to compile.
- Python and binary behavior are unchanged.
- Hardened-profile confinement tests remain green.
- Core dependency graph drops static-only platform dependencies where possible.
- Topology CI rejects a second confinement authority.
