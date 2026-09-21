# Plan 245 — Static-service authority convergence

## Purpose

Finish the static ownership model by converging the two service-layer
implementations without changing any existing API or capability.

Plan 219 correctly made `eggserve-static` the sole authority for path parsing,
filesystem confinement, MIME selection, secure root capabilities, and response
planning. However, the repository still has:

- `eggserve-static::StaticService`, the direct leaf service;
- `eggserve-core::server::StaticService`, the compatibility/composed service.

The compatibility implementation currently carries behavior beyond the direct
leaf, including extra response headers, error-representation policy,
per-runtime ops attachment, `ServeConfig` projection, configurable directory
listing budgets, and richer compatibility directory response construction.

That division is documented, but it leaves two static request-to-response
implementations and makes the recommended direct leaf less capable than
EggServe's own compatibility service.

## Desired end state

`eggserve-static` owns reusable static request/service behavior.

`eggserve_core::server::StaticService` remains available with the exact
current builder/public surface but becomes a compatibility/composition wrapper
around the direct authority plus any genuinely core-only configuration
projection.

No path/filesystem implementation moves back into core.

## Compatibility freeze

Preserve:

- `eggserve_static::StaticService` and `StaticServiceBuilder`;
- `eggserve_core::server::StaticService` and `StaticServiceBuilder`;
- current builder methods and defaults on both;
- `ServeConfig` behavior;
- static metadata/extra-header behavior;
- error representation behavior;
- directory listing semantics and limits;
- ops behavior;
- GET/HEAD, conditional, range, ETag/Last-Modified behavior;
- safe symlink/dotfile/listing defaults.

No public method may be removed to force callers onto a new type.

## Work

### 1. Inventory service-layer differences

Create a behavior matrix comparing both service implementations for:

- build/init validation;
- content-type fallback;
- extra response headers;
- response-policy/error bodies;
- conditional/range evaluation;
- file body capability ownership;
- index selection;
- directory listing rendering and limits;
- policy denial/error mapping;
- HEAD parity;
- ops events/counters;
- `ServeConfig` construction.

Mark each row DIRECT, COMPAT-ONLY, or FRONTEND-PROJECTION.

### 2. Extend the direct authority without breaking it

Add reusable internal/additive configuration to `eggserve-static` for
behavior already exposed by core.

Prefer an internal options/state structure consumed by the leaf
`StaticService` implementation. Public additions are allowed only when they
are genuinely useful to direct consumers and do not narrow existing behavior;
private hooks/features are preferable for compatibility-only configuration.

Do not make `eggserve-static` depend upward on `eggserve-core`.

If ops attachment is needed, depend on the neutral/direct server authority
rather than the compatibility facade.

### 3. Unify request-to-response logic

There should be one implementation of:

- static method validation;
- confined resolution;
- conditional/range header collection;
- file response construction;
- directory/index resolution;
- directory listing generation;
- static error mapping;
- extra metadata application.

Core should project `ServeConfig`/compat settings and invoke that authority.

Avoid reserializing/reopening file paths; preserve the existing opened
capability through response construction.

### 4. Keep limits with their correct owners

Do not push generic runtime limits into the static crate.

Static-only limits may move to `eggserve-static` implementation authority if
needed:

- listing entry count;
- listing response byte budget;
- extra static response header count/bytes.

Existing `eggserve_core::limits::Limits` fields and validation behavior remain
unchanged and project into the direct static configuration.

### 5. Directory renderer

Converge on the compatibility renderer rather than silently reducing behavior.
Preserve escaping, percent-encoding/link semantics, deterministic ordering, and
current byte/entry bounds.

Add adversarial tests for control characters, special HTML characters,
non-UTF8/platform-specific names where currently supported, and budget
boundaries before deleting either renderer.

### 6. Topology enforcement

Update the topology gate to assert:

- one `StaticService` request/response authority in `eggserve-static`;
- core static service is facade/projection/delegation, not a second resolver or
  renderer;
- core does not reacquire `phf`/filesystem authority;
- static continues to depend only downward on primitives/server + platform fs.

## Tests

Move direct implementation tests into `eggserve-static` and retain core
compatibility parity tests.

Required parity cases:

- file GET and HEAD;
- custom/default content type;
- conditional 304 and preconditions;
- byte range/416;
- index resolution;
- listing disabled/enabled;
- listing entry/response byte ceilings;
- dotfile/symlink denial/follow behavior;
- extra headers including order and forbidden-owned header rejection;
- `ErrorRepresentationPolicy::{Minimal,Empty}`;
- policy-denied/not-found/I/O mapping;
- opened-handle/capability continuity.

The existing static authority/confinement suites must remain unchanged and
green.

## Qualification

At minimum:

```sh
cargo test -p eggserve-static
cargo test -p eggserve-core --test static_authority_conformance
cargo test -p eggserve-core
python3 scripts/check-crate-topology.py
cargo clippy --workspace --lib --bins --tests -- -D warnings
```

Python static fast-path and callback-backed static suites must also remain
green because both consume this behavior indirectly.

## Acceptance criteria

- [ ] one static request-to-response implementation authority remains;
- [ ] core static public APIs remain source-compatible;
- [ ] direct static public APIs remain source-compatible;
- [ ] all compatibility-only behavior is projected/delegated, not duplicated;
- [ ] confinement/opened-handle invariants remain intact;
- [ ] listing and extra-header budgets retain current semantics;
- [ ] Python stock-static fast path remains eligible under the same contract;
- [ ] topology rejects a second core static renderer/service implementation;
- [ ] no dependency/capability/support-tier regression.

## Non-goals

Do not extract another filesystem crate, redesign MIME policy, add templating,
add compression/caching/CDN behavior, or change default directory-listing,
dotfile, or symlink policy.
