# Plan 179 — Canonical Runtime Limit and Configuration Model

## Status

**IMPLEMENTED / CLOSED — consolidation; no product-surface expansion.**

Prerequisite: Plan 178 closed. This plan should land before Plan 180 so the connection pipeline is decomposed around one configuration authority rather than moving duplicated validation into additional modules.

## Purpose

Remove the parallel transport-limit/default/validation logic currently spread across `Limits`, `RuntimeConfigBuilder`, `RuntimeConfig`, and the `ServeConfig` bridge while preserving the repository's useful separation between static-service configuration and transport/runtime configuration.

The objective is not to collapse every configuration type into one public struct. The objective is to have one source of truth for shared runtime constraints, defaults, and cross-field validation, with each public surface adapting to that authority.

## Current-state findings

`crates/eggserve-core/src/limits.rs` and `crates/eggserve-core/src/server/config.rs` currently repeat most transport policy fields, including:

- connection and file-stream concurrency;
- request-body ceiling;
- HTTP/1 parser buffer/header/target limits;
- in-flight service admission;
- header, TLS handshake, handler, body, total-connection, graceful-shutdown, keep-alive, and response-write timeouts;
- maximum requests per connection;
- file-stream chunk size.

`Limits::validate()` contains the same range/cross-field rules that `RuntimeConfigBuilder::build()` implements independently. The source explicitly notes that some checks mirror the builder. `try_from_serve_config()` first validates `Limits` and then manually projects nearly every runtime field into a `RuntimeConfig`.

This architecture has three maintenance costs:

1. adding/changing a runtime limit requires synchronized edits in multiple validators/default tables;
2. error wording and accepted ranges can drift between frontends;
3. `RuntimeConfig` has public fields, so callers can hand-construct values that bypass builder validation unless every runtime entry boundary protects itself separately.

At the same time, `Limits` also contains genuinely static-serving-specific controls such as directory-listing and extra-response-header budgets. Those should not be forced into a generic runtime type.

## Design constraints

- Preserve `ServeConfig` as the static-serving/product configuration model.
- Preserve the experimental `RuntimeConfig` as the runtime-facing configuration model unless a pre-1.0 migration is demonstrably simpler than compatibility.
- Preserve static-only limits as service/static concerns.
- Do not expose Hyper, Tokio semaphores, Python, CLI parsing, or platform FFI through a new canonical policy type.
- Do not add a configuration database, dynamic reloading, environment-variable framework, or generic schema system.
- Prefer an internal/shared validation kernel over a new public type if that achieves the goal with less API surface.

## Track A — Inventory and classify every limit

Before changing code, make an explicit field inventory with three categories.

### A1. Runtime/transport shared fields

These are consumed by the generic HTTP runtime and should have exactly one default/constraint authority. At current `main` they include the transport fields listed in the Purpose section.

### A2. Static-service-only fields

Keep directory-listing budgets, extra static response-header budgets, and other filesystem/static-rendering limits outside the generic runtime authority.

### A3. Frontend-only controls

Bind exposure acknowledgements, CLI logging format, Python callback concurrency, compatibility-facade response buffering, and similar frontend-specific controls stay in their owning surfaces.

If a field is ambiguous, resolve ownership based on which component enforces the invariant at runtime, not which frontend first introduced the option.

## Track B — Establish one runtime-policy validation kernel

### B1. Centralize default constants

Every shared runtime field must have one authoritative default value. `Limits::default()`, `RuntimeConfig::default()`, builders, CLI/Python adapters, and documentation should consume those values rather than duplicate numeric literals.

A small internal structure or set of shared constants is acceptable. Avoid a macro DSL unless ordinary Rust functions/types become materially repetitive after consolidation.

### B2. Centralize scalar and cross-field validation

Create one validation function/model that checks the shared runtime fields, including:

- non-zero concurrency/timeouts where required;
- Tokio semaphore maximums without exposing semaphore types in public APIs;
- parser minimum/maximums;
- request body and target/header ceilings;
- stream chunk bounds;
- `Some(0)` max-requests rejection;
- `header_read_timeout <= connection_total_timeout`;
- `handler_timeout <= connection_total_timeout`;
- `body_read_timeout <= connection_total_timeout`;
- any other currently enforced shared relationship.

The validation kernel should produce structured violations or another reusable representation that can be adapted into existing `LimitsError` and `ServerError` shapes.

Do not make operator-facing error text less specific merely to deduplicate code.

### B3. Adapt existing public surfaces

`Limits::validate()` should delegate shared checks to the canonical kernel and append static-only validation.

`RuntimeConfigBuilder::build()` should build the candidate shared values once, call the same kernel, and adapt the resulting error to the experimental runtime error type.

`try_from_serve_config()` should validate/project through the same path rather than replicate field-by-field correctness assumptions.

## Track C — Protect runtime boundaries from hand-constructed invalid configs

Because `RuntimeConfig` fields are currently public, builder validation is not a sufficient invariant boundary.

### C1. Add reusable runtime validation

Provide a single internal or experimental method that validates a complete `RuntimeConfig`, including `ResponsePolicy` and feature-gated TLS invariants where applicable.

### C2. Enforce validation at ownership boundaries

Ensure invalid hand-constructed configurations are rejected before they reach operations that can panic, allocate pathological resources, or construct invalid semaphores/Hyper builders.

Audit at least:

- `Server`/`ServerBuilder` startup;
- `RuntimeState::new` or its safe successor;
- `serve_http1_connection` caller-owned entry;
- conversion from `ServeConfig`;
- feature-gated TLS startup.

If an existing constructor is currently infallible and changing its signature would create disproportionate churn, use a validated constructor plus a narrowly documented compatibility path; do not silently retain a public way to trigger runtime panics with invalid values.

The server module is experimental, so a small pre-1.0 signature correction is acceptable if it materially improves the invariant. Document it clearly.

## Track D — Remove duplicated projection without collapsing abstractions

### D1. Shared runtime value projection

Once validation is centralized, make the `ServeConfig` → `RuntimeConfig` conversion consume one shared runtime value group/helper rather than manually reproducing every constraint/default.

### D2. Keep service policy separate

`StaticPolicy`, root ownership, directory listing behavior, MIME behavior, and static response limits must not enter generic `RuntimeConfig` solely for convenience.

### D3. Keep response policy explicit

`ResponsePolicy` is runtime final-boundary behavior and should remain explicit. Consolidation must not cause the CLI/Python compatibility profiles to silently inherit advanced Rust-only privacy settings they did not request.

## Track E — Conformance tests for configuration authority

Add table-driven tests proving that each shared constraint has identical semantics through the supported construction paths.

Required classes:

- defaults match between `Limits`/`ServeConfig` bridge and `RuntimeConfig`;
- minimum and maximum accepted parser values;
- zero and semaphore-overflow concurrency;
- invalid timeout relationships;
- request body ceiling;
- request-target/header ceilings;
- max requests per connection;
- valid non-default configuration round-trips/projected values exactly;
- hand-constructed invalid `RuntimeConfig` is rejected at the runtime boundary rather than panicking.

Do not duplicate the entire test matrix once per frontend. Prefer a shared case table applied to adapters.

## Track F — Documentation and API truthfulness

Update Rustdoc and current-state docs so operators/embedders can identify:

- which limits are runtime/transport policy;
- which are static-service policy;
- the authoritative defaults;
- when configuration is validated;
- the fact that services may lower request-body ceilings but cannot raise the runtime hard ceiling.

Do not introduce new user-facing knobs as part of this cleanup.

## Verification

Run the ordinary Rust/Python/TLS checks because configuration crosses all surfaces:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --features tls
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
```

Run `scripts/verify-conformance-matrix.py` if configuration/capability documentation is changed.

Do not add another CI lane. These tests belong in existing routine CI.

## Acceptance criteria

- [ ] shared runtime defaults exist in one authoritative location.
- [ ] shared runtime scalar and cross-field constraints are implemented once.
- [ ] `Limits::validate()` and `RuntimeConfigBuilder::build()` no longer carry parallel copies of the same constraint logic.
- [ ] static-only limits remain outside the generic runtime configuration model.
- [ ] valid `ServeConfig` projection produces exactly the expected `RuntimeConfig` values.
- [ ] invalid hand-constructed `RuntimeConfig` values are rejected before semaphore/Hyper/runtime operations can panic or widen resource policy.
- [ ] response-policy and TLS validation remain intact.
- [ ] error messages remain actionable and identify the invalid field/constraint.
- [ ] no new runtime feature, protocol, dependency framework, routing abstraction, or Python API capability is introduced.
- [ ] CLI, Rust, Python, TLS, and Plan 175 consumer behavior remain green.

## Suggested implementation order

1. Produce the runtime/static/frontend field ownership inventory.
2. Introduce the shared runtime default/validation kernel with tests.
3. Migrate `Limits::validate()` to it.
4. Migrate `RuntimeConfigBuilder::build()` and full-config validation to it.
5. Simplify `try_from_serve_config()` projection.
6. Enforce full validation at server/caller-owned runtime boundaries.
7. Run frontend/TLS/conformance tests and update narrow documentation.
8. Add a closure record identifying the removed duplicated validators and any experimental API signature correction.

## Closure record

- Track A: ownership inventoried in `architecture/configuration.md` and
  `runtime_limits.rs` docs — runtime kernel owns 18 transport fields;
  static-only listing/extra-header budgets stay in `Limits`/`config`;
  frontend-only controls stay in CLI/Python surfaces.
- Track B: new crate-private `runtime_limits.rs` owns all shared defaults
  (`DEFAULT_*`) and `SharedRuntimeValues::validate()` (`Violation`
  field/value/constraint). `limits.rs` re-exports the authority and keeps only
  listing constants; `Limits::default()` and `RuntimeConfig::default()` consume
  it. Removed ~300 lines of duplicated range/cross-field checks.
- Track C: added `RuntimeConfig::validate()` (shared kernel + response
  policy) and `RuntimeState::try_new()` (preferred); `new()` validates +
  panics with context. `ServerBuilder::build()`/`static_service()`,
  `Server::start_with_service()`, and `serve_http1_connection_with_id`
  enforce before semaphore/Hyper use (caller-owned logs + `Internal`).
  Documented in `docs/migration-guide.md` as additive experimental change.
- Track D: `try_from_serve_config()` validates `Limits` once then projects
  via single `RuntimeConfig::from_shared_runtime`; static policy/root/MIME
  untouched; response policy stays explicit.
- Track E: new `tests/runtime_config_authority.rs` (9 tests) — defaults match,
  kernel defaults pinned, shared invalid table across Limits/builder/bridge/
  hand-built, parser boundaries, non-default round-trip, static separation,
  boundary rejection, caller-owned `Internal` without panic.
- Track F: `README.md`, `AGENTS.md`, skill, `architecture/configuration.md`,
  `architecture/runtime.md`, `architecture/eggserve-core.md`,
  `docs/migration-guide.md` updated; `verify-conformance-matrix.py` green.
- Verification: `cargo fmt --check`, `cargo clippy --workspace -D warnings`,
  `cargo test --workspace` (1725 passed), TLS bins/core, `cargo check`
  python crate, `cargo test --doc`, dist builds, `bash
  scripts/test-python-wheel.sh` (781 passed) all green locally.

## Handoff

After closure, proceed to Plan 180. The connection-pipeline decomposition should consume the consolidated runtime configuration authority and must not recreate local copies of timeout/limit validation inside the new driver modules.