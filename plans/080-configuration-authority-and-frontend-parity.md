# Plan 080 — Configuration Authority and Frontend Parity

## Goal

Establish one authoritative configuration model for runtime, static-serving, CLI, and Python behavior; remove duplicate or ineffective fields; validate all resource limits before runtime construction; and prove that every documented frontend setting reaches the actual enforcing primitive.

## Preconditions

- Plan 077 has finalized timeout and shutdown semantics.
- Plan 078 has finalized custom-service construction and metadata propagation.
- Plan 079 has finalized body-policy types and behavior.
- Plan 075 has registered configuration findings and release-gate requirements.

## Non-goals

Do not:

- add new operational features merely to justify existing configuration fields;
- preserve dead fields for compatibility;
- introduce a second compatibility configuration layer with divergent defaults;
- add dynamic runtime reconfiguration;
- add environment-variable behavior not already within project policy;
- optimize runtime behavior.

## Defect statement

Configuration authority is currently split across runtime, static service, CLI, and Python layers. Some fields appear to control the same resource while only one value reaches the enforcing semaphore. In particular, file-stream limits can differ between Python-facing runtime configuration and `ServeConfig` defaults.

Direct Rust users can also construct zero-valued or otherwise invalid public limit structures, leading to panic, a zero-capacity runtime, or behavior inconsistent with CLI/Python validation.

## Track A — Configuration inventory

Create a complete table of every public or operator-facing setting.

For each field record:

- canonical name;
- owning type/module;
- semantic definition;
- default;
- valid range;
- Rust builder/constructor exposure;
- CLI flag/config exposure;
- Python constructor/property exposure;
- actual enforcing code path;
- documentation locations;
- tests;
- whether it is duplicated, dead, or mismatched.

Inventory at minimum:

- bind/listen address;
- connection concurrency;
- TLS handshake concurrency and timeout;
- header/body/handler/write/idle/shutdown timeouts;
- request/header/body limits;
- requests per connection;
- request-body and incomplete-body policy;
- file-stream concurrency;
- directory-listing limits;
- Python callback concurrency/timeouts;
- root and filesystem policies;
- logging format and level where configuration exists.

## Track B — Ownership model

Adopt an explicit split.

Recommended runtime ownership:

- listener/bind settings;
- connection and handshake limits;
- request parser/header limits;
- body acceptance, byte, and timeout policy;
- handler and callback execution limits;
- response-write/keep-alive/connection lifetime controls;
- graceful-shutdown controls.

Recommended static-service ownership:

- root/pinned-root construction;
- symlink/reparse policy;
- dotfile policy;
- directory index/listing policy;
- static file-stream concurrency;
- directory listing entry/byte limits;
- static validator/cache metadata policy.

A setting may be shared by reference, but only one final validated value may own enforcement.

## Track C — Validated configuration types

Replace public freely mutable primitive fields where they permit invalid states.

Use as appropriate:

- `NonZeroUsize` or validated newtypes for positive concurrency;
- bounded integer newtypes for counts/bytes;
- validated duration types rejecting zero where zero is nonsensical;
- enums for finite policies;
- builders returning `Result`;
- private fields plus accessors when invariants must be preserved.

Do not panic during server construction for user-supplied invalid configuration. Return a structured error identifying field, value, and constraint.

Explicitly decide whether zero means disabled, unlimited, or invalid for every numeric field. Do not infer meaning by frontend.

## Track D — Default unification

Define defaults once in the canonical owner and derive frontend defaults from that source.

Eliminate cases where:

- Python defaults differ from Rust defaults;
- CLI help embeds a separate literal;
- static service defaults override a runtime value;
- feature-gated builds silently choose another value;
- documentation contains copied defaults that drift.

Where code generation is excessive, add tests asserting all frontend defaults equal canonical defaults.

## Track E — Builder and server construction flow

Define one construction sequence:

1. collect user-facing options;
2. apply defaults;
3. validate each field and cross-field constraints;
4. construct canonical runtime and service configuration;
5. construct actual semaphores, timers, and policies from those values;
6. expose read-only effective configuration for diagnostics/tests;
7. start the server.

No frontend may construct enforcement primitives independently.

Add cross-field validation for examples such as:

- graceful shutdown shorter than required cleanup policy;
- drain timeout/limit present when drain is disabled;
- callback limit supplied when Python callback service is absent, if that should be rejected or ignored explicitly;
- TLS settings in plaintext-only builds;
- directory listing limits when listing is disabled;
- impossible or excessive values that overflow conversions.

## Track F — File-stream limit correction

Trace file-stream admission from all constructors to `ServeState`.

Required behavior:

- exactly one static file-stream concurrency value constructs the semaphore;
- Rust, CLI, and Python settings modify that exact value;
- effective configuration reports the value actually in use;
- saturation tests observe the configured capacity;
- no runtime-level shadow field remains unless it is the canonical owner by explicit design.

Add tests using distinct non-default values so accidental fallback to 32, 64, or another default is visible.

## Track G — Body and timeout field consolidation

Adopt the final names and semantics from Plans 077 and 079.

Remove:

- aliases that map to materially different semantics;
- ignored drain fields;
- timeout names that imply inactivity while enforcing total duration;
- duplicate settings between server and connection builder.

If temporary deprecations remain, they must:

- warn clearly;
- map exactly to the new behavior;
- have a removal target;
- not create two effective values.

## Track H — Rust API parity

Add Rust tests covering:

- default configuration;
- every individual override;
- invalid zero/boundary/overflow values;
- cross-field errors;
- effective configuration inspection;
- builder reuse/move behavior;
- static and custom-service construction;
- feature-gated TLS configuration;
- no panic for invalid user input.

Compile-fail tests may be used where private fields/newtypes intentionally prevent invalid construction.

## Track I — CLI parity

For every operator-facing setting:

- map the CLI flag or config key to the canonical field;
- generate or test help defaults;
- return actionable validation errors;
- preserve exit-code conventions;
- ensure quiet/log settings do not hide configuration failures;
- test config-file plus command-line precedence if supported.

Use integration tests that launch the actual binary where runtime enforcement matters.

## Track J — Python parity

For every Python-exposed setting:

- map constructor arguments to canonical fields;
- use Python exceptions for validation errors;
- expose effective configuration where already within API scope;
- build and install a wheel before parity tests;
- test non-default connection, body, timeout, file-stream, and callback values;
- ensure defaults match Rust;
- ensure unsupported fields are absent rather than silently ignored.

Python tests must prove behavior, not only object attributes.

## Track K — Configuration snapshots and drift checks

Create a machine-readable configuration schema or inventory suitable for tests and docs.

Use it to detect:

- duplicate canonical names;
- mismatched defaults;
- undocumented public fields;
- CLI/Python fields without owners;
- owner fields not connected to enforcement;
- stale deprecated aliases.

Do not create a general remote configuration protocol.

## Required behavioral tests

- connection limit set to a non-default value and enforced;
- file-stream limit set to 1, 2, and another non-default value across Rust/CLI/Python;
- body limit boundaries across Rust and Python;
- timeout values produce Plan 077 semantics;
- reject/drain settings produce Plan 079 semantics;
- zero/overflow values return structured errors;
- server cannot start with invalid cross-field combinations;
- static and custom service effective runtime settings agree where shared;
- feature-disabled settings fail or are absent predictably;
- installed wheel and installed binary match source API/defaults.

## Documentation and migration

Update:

- complete configuration reference;
- Rust builder examples;
- CLI help/reference;
- Python API docs;
- timeout/body-policy migration notes;
- default-value tables;
- release/support profile mappings;
- finding registry and corrective status.

Avoid manually duplicating defaults where generated/reference data can be used.

## Acceptance criteria

- Every public configuration field has exactly one authoritative owner.
- Every field maps to an identifiable enforcing primitive or is removed.
- Rust, CLI, and Python defaults agree.
- Python file-stream configuration changes the actual `ServeState` semaphore.
- Invalid zero, range, overflow, and cross-field values return structured errors and never panic or create zero-capacity deadlocks.
- Effective configuration reflects values actually in use.
- Deprecated aliases, if any, map exactly and are time-bounded.
- Cross-frontend behavioral parity tests pass using non-default values.
- Configuration schema/drift checks fail on duplicate, undocumented, ignored, or mismatched fields.
- Release B evidence is tied to the exact implementation and installed artifact SHAs.

## Stop conditions

Stop and record a design blocker if:

- two subsystems genuinely require separate values currently exposed under one name;
- an existing compatibility alias cannot map without changing semantics;
- Python or CLI cannot reach the canonical owner without duplicating enforcement;
- public struct construction prevents validation without an intentional breaking API change;
- effective configuration cannot be inspected reliably enough to prove parity.

Breaking pre-1.0 APIs is preferable to preserving invalid or misleading configuration.

## Handoff

Release B closes after Plans 078–080 pass Rust, CLI, Python, raw-wire, lifecycle, and installed-artifact gates. Plan 081 then refactors static response planning against this stable configuration model.