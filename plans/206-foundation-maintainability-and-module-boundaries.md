# Plan 206 — Foundation Maintainability and Module Boundaries

## Status

**IMPLEMENTED / CLOSED (behavior-preserving).**

## Closure record

Implemented 2026-09-11: Track A ownership/coupling map drove extraction order
(lowest coupling first). Track B split `eggserve-python/src/server.rs` (3851
lines) into `server/` (`errors`/`body_bridge`/`request_bridge`/`tunnel_bridge`/
`response_bridge`/`static_responder`/`sync_handler`/`runtime` +
`lifecycle`/`async_handler` pointers; sync/async conversion shares helpers,
async Plan 204 stays Python-side in `lowlevel.py`, PyO3 registration stays
small in facade). Track C split `primitives/canonical.rs` (2335 lines) into
`canonical/` (`status`/`headers`/`response_body`/`response`/`adapters`;
`Response.body` + `remove/strip` are `pub(super)`; tests stay in facade).
Track D confirmed the explicit one-way `StaticService::canonical_response()`
planner→canonical adapter (planner stays pure, no duplicate validation).
Track E split `server/config.rs` (1847 lines) into `server/config/`
(`runtime` single validation authority + `http1`/`http2`/`http3`/`tls`
protocol owners; `Http2/3::validate` are `pub(super)`; no new knobs).
Track F split `server/http3.rs` (1771 lines) into `server/http3/`
(`endpoint`/`request`/`response`/`tunnel`; `accept_loop` qualifies as
`endpoint::`/`request::`/`response::`/`tunnel::`; one shared kernel).
Track G extracted `server/runtime.rs` (`RuntimeState`) + `server/accept.rs`
(`accept_loop_multi`/handlers/sources/TLS helpers) from `server/mod.rs`
(2062 lines); facade keeps `Server`/`ServerBuilder` + re-exports. Track H
split `ops.rs` (1090 lines) into `ops/` (`mod` authority +
`events`/`sinks`/`counters`). Track I found no deprecated adapters to
remove (single `TODO` in Windows test is legitimate; `config.rs`/`canonical.rs`
facades preserve paths so existing references stay valid; no new
dependencies). Track J updated architecture module maps, AGENTS.md, and both
skill copies; README needed no changes (no internals). Visibility was never
widened beyond `pub(super)` for extraction; public import compatibility is
preserved (`primitives::canonical::X`, `primitives::X`,
`server::RuntimeState`, `server::Py*` still resolve). No wire/security/
lifecycle behavior changes. Qualification: `cargo fmt --check`, workspace
clippy/tests incl. `http2,tls` + `http3,tls` feature combos, Python crate
`cargo check --locked`, plus full `verify.sh` / wheel suites before commit
(see commit). No line-count gates added.

## Purpose

Reduce change coupling and maintenance risk in the modules that have grown substantially as EggServe evolved from a hardened static server into a multi-protocol reusable HTTP runtime.

This is not a rewrite and not a line-count exercise. The objective is to make security/protocol ownership auditable before stabilizing the application-server foundation.

Plans 179 and 180 already removed duplicated runtime-limit validation and decomposed the old monolithic connection pipeline. Do not repeat or undo that work.

## Current hotspots

At the Plan 196 baseline, several files remain large and span multiple responsibilities, notably:

- `crates/eggserve-python/src/server.rs` (~110 KiB);
- `primitives/planner.rs` (~85 KiB);
- `primitives/canonical.rs` (~79 KiB);
- `server/config.rs` (~65 KiB);
- `server/mod.rs` (~47 KiB);
- `server/http3.rs` (~44 KiB);
- `server/static_service.rs` (~39 KiB);
- `ops.rs` (~36 KiB).

Some size is legitimate because tests/documentation are co-located. Refactor only where multiple invariant owners are entangled.

## Track A — Produce an ownership/coupling map

Before moving code, record for each hotspot:

- public types/functions it owns;
- crate-private protocol/security invariants;
- dependencies on other modules;
- tests that directly depend on private placement;
- feature gates;
- high-churn areas from Plans 185–205.

Use the map to choose extraction order. Avoid creating circular imports that are then solved by widening visibility.

## Track B — Python native bridge decomposition

The Python server bridge is the highest-priority maintainability target, especially after Plan 204.

Prefer a structure resembling:

```text
crates/eggserve-python/src/
  lib.rs
  server/
    mod.rs
    runtime.rs
    sync_handler.rs
    async_handler.rs
    request_bridge.rs
    body_bridge.rs
    response_bridge.rs
    tunnel_bridge.rs
    lifecycle.rs
    static_responder.rs
    errors.rs
```

Exact files may differ. Required ownership:

- compatibility sync handler logic is separate from async Plan 204 logic;
- body/response channel state machines each have one owner;
- Python exception mapping is centralized;
- static responder composition is not mixed with event-loop scheduling;
- public PyO3 registration remains small/auditable;
- GIL acquisition/release sites become easy to review.

Do not duplicate conversion logic between sync/async bridges. Extract shared canonical/Python conversion helpers where semantics are actually identical.

## Track C — Canonical primitives decomposition

Split `primitives/canonical.rs` by stable semantic type while preserving public import paths through re-exports where practical:

```text
primitives/canonical/
  mod.rs
  status.rs
  headers.rs
  response.rs
  response_body.rs
  adapters.rs
```

Do not mechanically move request types that already have coherent dedicated modules merely to create symmetry.

Keep byte/header validation authority singular. New trailer types from Plan 198 should reuse header primitives rather than creating another field implementation.

API snapshot tests must prove source paths intentionally retained or document any pre-1.0 migration.

## Track D — Static planner versus canonical response overlap

Revisit the compatibility debt identified by Plan 184: static planning types (`ResponseStatus`, header/plan/body plan vocabulary) coexist with canonical response types.

Goal:

- one runtime response representation;
- pure static planning APIs may remain stable compatibility/value objects;
- explicit one-way adapter from planner result to canonical response;
- no duplicate status/header/body validation;
- static service internals should not convert back and forth repeatedly.

Do not delete useful planner APIs solely to reduce type count. Measure actual duplication and migrate only internal paths that reduce correctness/maintenance risk.

## Track E — Server configuration module ownership

Plan 179 already created one runtime default/validation authority. This plan may split `server/config.rs` by protocol ownership without reintroducing duplicate validation:

```text
server/config/
  mod.rs
  runtime.rs
  http1.rs
  http2.rs
  http3.rs
  tls.rs
```

Shared constraints continue using `runtime_limits.rs`; protocol modules own only protocol-specific fields/defaults/validation.

Keep stable/public construction paths re-exported. Do not add new knobs as part of the split.

## Track F — H3 adapter decomposition

After Plans 198–199 add trailers/tunnels, `server/http3.rs` may otherwise become a second monolith.

Split by invariant where warranted:

- endpoint/listener lifecycle;
- request metadata conversion;
- request-body/field receive adapter;
- response send/body/trailer adapter;
- stream activity/timeout/reset mapping;
- tunnel/Extended CONNECT adapter.

There must remain one shared service invocation kernel and canonical normalization path from Plan 184. No H3-specific service semantics may emerge during refactor.

Apply equivalent extraction to H2 only if its implementation has become similarly entangled; do not manufacture parallel module trees for aesthetic symmetry.

## Track G — Server facade simplification

Keep `server/mod.rs` focused on public exports, server construction/startup orchestration, and high-level runtime ownership. Move feature-specific listener/protocol startup details behind internal modules where Plans 201–203 add complexity.

Do not expose internal structs merely to reduce `mod.rs` imports.

## Track H — Ops decomposition if needed

If Plan 205 materially expands `ops.rs`, separate schema/event vocabulary from sink implementations/counters:

```text
ops/
  mod.rs
  events.rs
  counters.rs
  sinks.rs
```

Preserve the single `OpsContext` authority from Plan 181. Do not create request-observer and ops systems that compete for the same lifecycle events.

## Track I — Remove stale compatibility/dead code

After all extractions:

- search for deprecated adapters that are no longer used by public compatibility surfaces;
- remove duplicate helper wrappers and stale feature gates;
- remove comments referring to historical source line numbers or old module locations;
- run `cargo machete`/equivalent dependency inspection if already accepted by project workflow, otherwise audit Cargo manifests manually;
- verify Python crate's separate lockfile remains intentional and synchronized for shared dependency security fixes.

Do not remove deprecated APIs before their documented compatibility window solely for cleanliness.

## Track J — Documentation/source maps

Update architecture module maps, contributor/agent guidance, and rustdoc links after moves. Avoid duplicating implementation details across many docs; architecture docs should state ownership and invariants, source files carry local mechanics.

Historical plan paths remain historical; update only current-state docs and explicitly necessary stale references.

## Verification

Behavior-preserving refactor requires broad regression evidence:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features 'tls,http2,http3'
cargo test -p eggserve-bin --features 'tls,http2,http3'
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
```

Run API snapshot/external consumer tests after every public module move. Run focused H2/H3/Python async qualification after their owning modules move.

Do not add arbitrary source-file-size CI gates.

## Acceptance criteria

- [ ] Python sync/async/runtime/body/response/tunnel responsibilities have coherent module owners and share conversion logic where semantics match;
- [ ] canonical response/header types are split into auditable modules without semantic duplication;
- [ ] static planning and canonical runtime response logic have one validation/conversion authority;
- [ ] server protocol configuration is modular without recreating shared default/constraint tables;
- [ ] H3 feature growth remains decomposed around endpoint/request/response/stream/tunnel invariants;
- [ ] `server/mod.rs` and PyO3 registration remain understandable facades rather than implementation buckets;
- [ ] visibility is not widened merely to make extraction convenient;
- [ ] public import compatibility is preserved or explicitly migrated under pre-1.0 policy;
- [ ] no intentional wire/security/lifecycle behavior changes occur;
- [ ] full Rust/Python/H1/H2/H3 regression suites remain green;
- [ ] no line-count or abstraction-count metric is treated as the objective.

## Handoff

Plan 207 should run after this cleanup or against the finalized module layout. Any behavioral bug discovered during refactor must be fixed in a narrow corrective commit/plan rather than hidden inside mechanical movement.