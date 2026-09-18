# Plan 217 — Direct service/request type convergence

## Purpose

Finish the convergence started by Plans 211–216 so the direct `eggserve-primitives` + `eggserve-server` path owns the canonical application-facing request/service contract, including the H2-compatible service shape, while `eggserve-core` remains a compatibility facade rather than a second implementation.

Plan 216 moved generic tunnel intent to `eggserve-primitives` and tunnel execution to `eggserve-server`, but intentionally left compatibility request/service type identity and H2 glue in core. This plan closes that remaining seam.

## Goals

- Make `eggserve-primitives::Request`, `RequestContext`, request body/lifecycle, response, authority, header, and tunnel vocabulary the canonical types used by the direct server.
- Make `eggserve-server::Service` the single service contract for H1 and all compatibility H2 delegation.
- Remove nominally distinct compatibility request/service types where they are semantically identical.
- Preserve existing `eggserve_core::primitives::*` and `eggserve_core::server::*` import paths through re-exports or thin adapters during the 0.x compatibility line.
- Keep `eggserve-primitives` Hyper/Tokio/rustls/QUIC-free.
- Do not promote H2 support tier; this is ownership convergence only.

## Non-goals

- No new protocol family.
- No ASGI/WSGI/application framework model.
- No H3 adapter move; Plan 220 owns that.
- No static-file refactor; Plan 219 owns static/confinement authority.
- No stable-API promotion.

## Work

### 1. Inventory remaining duplicate primitive types

Compare `crates/eggserve-core/src/primitives` against `crates/eggserve-primitives/src/primitives`.

Classify each module as:
1. byte-identical duplicate,
2. nominal duplicate with import-only differences,
3. compatibility wrapper with additional behavior,
4. intentionally core-only behavior that must first be moved downward.

At minimum explicitly inventory:
- `request.rs`
- `request_context.rs`
- `request_body.rs`
- `request_head.rs`
- `request_lifecycle.rs`
- `request_target.rs`
- `canonical.rs`
- `response.rs`
- `response_stream.rs`
- `header_block.rs`
- `authority.rs`
- `trailers.rs`
- `tunnel.rs`
- `proxy.rs`
- `connection_info.rs`
- `method.rs`
- `version.rs`

Do not use file-size or marker parity as the long-term guarantee where exact type identity is possible.

### 2. Unify the request/service contract

Move any remaining neutral behavior required by the direct server into `eggserve-primitives`.

Make direct and compatibility H1/H2 pipelines invoke `eggserve-server::Service` over the same request/response types. Compatibility layers may translate only where unavoidable for 0.x source compatibility.

The target is no second service error taxonomy, no second request envelope, no second response normalization state, and no second tunnel-capability state machine.

### 3. H2 service-shape convergence

Keep H2 wire/protocol mechanics in the compatibility transport layer until a later direct H2 transport plan if necessary, but make H2 dispatch through the same canonical service contract used by direct H1.

Preserve:
- request-body rejection semantics,
- request lifecycle/cancellation,
- response normalization,
- service admission,
- handler timeout,
- tunnel intent/acceptance,
- interim/trailer semantics,
- connection metadata truthfulness.

Any H2-only adapter must be explicitly transport glue, not a second application model.

### 4. Compatibility facade conversion

For each moved type/module, replace core implementation with:
- `pub use eggserve_primitives::...`, or
- a narrowly documented compatibility adapter when exact source compatibility prevents a direct re-export.

Adapters must not own parsing, validation, state machines, or security policy.

### 5. Topology enforcement

Extend `scripts/check-crate-topology.py` so the direct authority is structural:
- forbid second definitions of canonical request/service types in core,
- forbid Hyper/Tokio imports in primitives,
- forbid core/static upward references from server,
- ensure compatibility files are facades where expected.

Prefer AST/manifest-level or exact ownership checks over brittle line-count comparisons.

## Security invariants

- Request framing decisions remain transport-owned and occur before service side effects.
- Incomplete request bodies cannot make a reusable connection ambiguous.
- Response framing/normalization remains runtime-owned.
- Tunnel acceptance remains one-shot.
- No raw Hyper/H2 types enter the canonical service API.
- No transport metadata is fabricated for caller-owned/non-socket streams.

## Tests

Required:
- current direct H1 parity suite,
- compatibility H1 suite,
- H2 feature-gated suite,
- tunnel upgrade suites from Plan 216,
- trailer/interim/body lifecycle tests,
- app-server consumer contract tests,
- compile tests proving core and direct imports refer to identical or explicitly adapted types,
- topology script.

Add at least one downstream fixture implementing only `eggserve-server::Service` and exercise it through both direct H1 and compatibility H2 paths.

## Migration strategy

Land in small commits by type family:
1. headers/authority/value objects,
2. request metadata/head/context,
3. body/lifecycle,
4. response/normalization,
5. service contract/H2 glue,
6. compatibility facade cleanup.

Do not perform a flag-day rewrite of every primitive module.

## Rollback

Each family migration should remain revertible independently. Compatibility re-exports should be introduced before deleting the old implementation. If a source-compatibility blocker appears, retain a thin wrapper and document the blocker rather than restoring a second implementation.

## Acceptance criteria

- `eggserve-server::Service` is the only application service contract used by direct H1 and compatibility H2.
- Canonical request/response/context types are owned by `eggserve-primitives`.
- Core contains no second parser/state-machine implementation for migrated primitive families.
- Default, `http2,tls`, and `http3,tls` test matrices remain green.
- Existing downstream app-server examples compile with the direct crates.
- Topology CI fails if a second authority is reintroduced.
- Support tiers remain unchanged.
