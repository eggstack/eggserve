# Plan 217 — Direct HTTP/2 Runtime Parity and Qualification

## Status

Proposed — follows Plan 215; tunnel/Extended CONNECT integration depends on the capability boundary established by Plan 216.

## Purpose

Move EggServe's mature HTTP/2 server/runtime behavior from the `eggserve-core` compatibility implementation into `eggserve-server` behind the existing optional `http2` feature, while preserving the direct crate's small default HTTP/1 graph and current HTTP/2 support tier.

Plan 214 deliberately stopped with an HTTP/1-shaped direct runtime even though `eggserve-server` already declares an `http2` feature. As a result, a downstream that needs the mature caller-owned connection boundary and HTTP/2 still has to consume compatibility core or maintain its own Hyper H2 driver. That defeats the intended direct ownership model and leaves duplicate server maintenance in downstreams.

This plan closes that gap without moving HTTP/3, without requiring EggServe-owned TLS identity, and without promoting HTTP/2 merely because its source ownership changes.

A downstream TLS terminator or reverse proxy must be able to negotiate ALPN itself, retain certificate/JA4/application security policy, and hand an already-established TLS stream to the direct EggServe connection driver with an explicit H2 selection. EggServe should own HTTP/2 semantics, not the downstream's TLS policy.

## Prerequisites

Plan 215 must provide the direct authoritative:

- `Service` contract;
- `RuntimeConfig`/`RuntimeState` needed by the generic connection pipeline;
- caller-owned H1 connection driver;
- `ConnectionContext`;
- `ConnectionShutdown`;
- `ConnectionOutcome`;
- connection lifecycle/admission/timeout/observability machinery.

Plan 216 should define the dependency-correct generic tunnel capability before H2 Extended CONNECT is moved. HTTP/2 extraction may begin in parallel for non-tunnel behavior, but Plan 217 cannot close with a second H2-specific tunnel API.

Plan 213 remains authoritative for HTTP/3/QUIC isolation and support status.

## Current-state findings

The repository already contains mature HTTP/2 behavior in compatibility core:

- `server/config/http2.rs` owns H2-specific resource configuration;
- `server/connection` has an optional protocol-selecting caller-owned driver;
- the connection pipeline shares canonical service/body/response machinery across H1/H2;
- TLS listeners select H2 through ALPN before entering the protocol pipeline;
- cleartext prior-knowledge selection is supported by the mature compatibility path;
- H2 has protocol-specific stream/resource limits and timeout behavior;
- Plan 199 adds Extended CONNECT/tunnel behavior;
- the H2 qualification program includes deterministic tests and external/interoperability evidence, but H2 remains experimental per Plan 208/HTTP2 architecture documentation.

By contrast, the direct `eggserve-server` implementation remains HTTP/1-shaped. Its Cargo feature enables Hyper/Hyper-util H2 dependencies, but source ownership and direct public runtime parity have not yet moved.

The correct fix is to move the mature H2 implementation onto the Plan 215 direct connection pipeline, not to add another small H2 builder beside it.

## Goals

1. Make `eggserve-server`, behind `feature = "http2"`, the single owner of mature generic HTTP/2 server behavior.
2. Preserve the HTTP/1-only default graph when `http2` is disabled.
3. Move `Http2Config` and its validation/defaults into the direct server ownership layer.
4. Reuse the Plan 215 direct canonical request/service/response/lifecycle pipeline for H2.
5. Expose a caller-owned H2 path suitable for already-established plaintext or TLS byte streams without exposing Hyper types.
6. Preserve cleartext prior-knowledge protocol selection where it is part of the existing mature contract.
7. Allow external TLS terminators to select H2 explicitly based on their own ALPN result without requiring EggServe certificate/identity ownership.
8. Preserve stream/resource limits, body/trailer semantics, interim behavior, cancellation, graceful shutdown, and protocol-appropriate failure isolation.
9. Integrate Plan 216's generic tunnel contract for H2 Extended CONNECT rather than inventing an H2-specific application API.
10. Convert compatibility-core H2 server behavior into re-exports/thin adapters over direct ownership.
11. Re-run the existing H2 qualification program against the direct path.
12. Keep the current H2 support tier unchanged unless a separate promotion gate explicitly passes.

## Non-goals

- HTTP/3/QUIC extraction or promotion.
- Moving Quinn/H3 dependencies into `eggserve-server`.
- Requiring `tls` for cleartext HTTP/2.
- Requiring EggServe to own certificate loading, SNI, mTLS, ACME, JA4, or application TLS policy for caller-owned streams.
- Replacing `eggnet-tls` or changing its neutral role.
- Implementing h2c upgrade from HTTP/1 unless the mature existing contract already supports and qualifies it; prior knowledge is the baseline.
- Adding application routing, WAF, proxy, authentication, or framework semantics.
- Promoting HTTP/2 from experimental solely because source moves to the direct crate.
- Rewriting mature H2 behavior from scratch.
- Changing HTTP/3 tunnel limitations to make H2 extraction easier.

## Architectural invariants

### One service pipeline

H1 and H2 must share the direct canonical service/body/response pipeline established by Plan 215. Protocol-specific code should terminate at transport framing, flow control, stream lifecycle, and H2 configuration boundaries.

Do not maintain separate application-service implementations for H1 and H2.

### Feature isolation

With `default-features = false` and no `http2` feature, direct H1 consumers must not compile H2-specific dependency/features or code paths.

The topology check should prove this with Cargo metadata rather than relying only on source review.

### TLS policy remains optional/caller-owned

The direct H2 driver must be usable with an already-established TLS stream where the caller has already selected H2 via ALPN.

EggServe may use its own optional TLS convenience runtime, but that must not be the only route into direct H2 serving.

### Stream-scoped correctness

Where HTTP/2 permits stream-local failure/reset, one malformed/stalled/cancelled stream must not unnecessarily destroy healthy sibling streams unless the existing Hyper/EggServe limitation requires a documented connection-level fallback.

## Workstream A — H2 ownership inventory

Inventory all mature H2-specific implementation in compatibility core and classify it as:

1. direct `eggserve-server` ownership;
2. shared direct connection-pipeline logic already moved by Plan 215;
3. tunnel-specific logic owned by Plan 216 integration;
4. TLS convenience glue that may remain compatibility/optional;
5. H3-only code that remains under Plan 213.

At minimum inspect:

- `crates/eggserve-core/src/server/config/http2.rs`;
- H2 branches in `server/connection/driver.rs`;
- H2 request/body/framing behavior in `server/connection/request.rs`;
- protocol selection in `server/connection/mod.rs`;
- H2-specific response/deferred-body handling;
- H2 timeout/no-progress behavior;
- H2 lifecycle/stream cancellation registration;
- H2 Extended CONNECT hooks;
- TLS ALPN dispatch that selects H2;
- H2 ops/events and qualification tests;
- `architecture/http2.md` and H2 conformance/qualification scripts.

### Acceptance

The inventory must identify every H2-specific source block that would otherwise leave core as a second server implementation after direct extraction.

## Workstream B — Move `Http2Config` and validation authority

Move the mature H2-specific configuration to `eggserve-server` behind `feature = "http2"`.

Preserve all existing validated resource controls, including those currently represented by the compatibility `Http2Config` contract. Inventory exact fields/defaults before moving; do not infer new defaults.

Typical categories include:

- maximum concurrent streams;
- maximum header-list size;
- initial connection/stream window sizes where exposed;
- adaptive/flow-control behavior where configured;
- keep-alive/ping settings where part of the existing API;
- CONNECT protocol enablement when Plan 216 integration is active;
- other H2-specific ceilings already qualified.

### One validation authority

Compatibility `Http2Config`, direct `Http2Config`, CLI/Python bridges, and any `ServeConfig` projection must not retain independent defaults or validation implementations.

### Acceptance

Direct and compatibility construction accept/reject the same H2 configuration values and expose the same documented defaults.

## Workstream C — Extract explicit caller-owned H2 driving

Expose a direct caller-owned H2 connection driver over the same generic byte-stream constraints as Plan 215.

The public API must support an explicit H2 choice for downstreams that already performed protocol negotiation, for example a dedicated H2 entry point or a typed protocol-selection argument. The implementation plan should preserve the existing mature public shape where practical rather than invent a redundant API.

Required properties:

- no Hyper request/body/server-builder type in the public embedding signature;
- explicit `ConnectionContext` remains authoritative for socket/scheme/TLS metadata;
- direct `RuntimeState` and `ConnectionShutdown` are reused;
- `ConnectionOutcome` remains meaningful for H2 connection-level termination;
- per-stream lifecycle remains request-scoped;
- no certificate identity is required merely to call the H2 driver.

### External TLS terminator case

Add a direct integration test using an already-established async stream marked with appropriate HTTPS/TLS metadata and explicit H2 selection. The test does not need to duplicate a full certificate manager; its purpose is to prove that HTTP/2 is not coupled to EggServe's TLS identity runtime.

## Workstream D — Move cleartext protocol selection

Where the mature compatibility path supports HTTP/1 vs HTTP/2 prior-knowledge classification on caller-owned cleartext streams, move that selector into `eggserve-server` behind `http2`.

Preserve:

- HTTP/2 connection-preface recognition;
- no destructive loss/reordering of bytes used for classification;
- strict H1-only entry point for callers that require it;
- explicit H2 entry point for already-negotiated transports;
- clear documentation that automatic cleartext selection is not TLS ALPN negotiation.

Do not add heuristic protocol sniffing beyond the qualified H2 preface contract.

### Acceptance

The direct crate offers distinct, documented ways to:

- serve strict H1;
- serve explicit H2 when enabled;
- use the existing mature cleartext auto/prior-knowledge behavior when enabled.

## Workstream E — Reuse canonical request/body semantics

H2 request conversion must use the same direct canonical model and body-policy authority as H1.

Preserve:

- streaming request bodies;
- declared/observed size limits;
- one-shot consumption;
- trailers;
- request lifecycle cancellation;
- handler timeout;
- body-read timeout;
- service admission;
- canonical authority/header validation;
- response normalization;
- HEAD/body-forbidden semantics;
- sanitized service/runtime error responses where an HTTP response is still legal.

Do not add an H2-specific `Service` trait or body type exposed to downstream application code.

## Workstream F — H2 lifecycle, flow control, and timeout parity

Move the mature protocol-specific lifecycle behavior rather than reusing H1 close semantics mechanically.

Qualification must cover:

- multiple concurrent streams;
- sibling stream survival when one request is cancelled/rejected where supported;
- request-body flow control under slow consumption;
- response streaming under slow receiver pressure;
- peer reset;
- service cancellation on stream/connection loss;
- graceful server shutdown;
- connection-level forced shutdown after drain deadline;
- max-requests/stream semantics where applicable;
- handler/body/header/idle/total timeout interaction;
- response no-progress timeout behavior.

The existing architecture notes that H2 response no-progress observation cannot always prove stream-level wire progress after Hyper accepts a frame and may use a conservative connection-shutdown fallback. Preserve/document the current qualified limitation rather than claiming stronger isolation without evidence.

## Workstream G — H2 Extended CONNECT through Plan 216

When Plan 216's direct generic tunnel contract is available, connect H2 Extended CONNECT to that same capability.

Required behavior:

- enable `SETTINGS_ENABLE_CONNECT_PROTOCOL` only according to the existing validated H2 configuration/contract;
- validate `:protocol` into the same bounded generic `ProtocolName` vocabulary;
- represent intent as the same `TunnelRequest`/`TunnelKind::ExtendedConnect` contract;
- successful acceptance uses H2-appropriate `2xx` semantics rather than H1 `101`;
- flow control and reset stay stream-scoped where supported;
- downstream protocol codec remains outside EggServe;
- no H2-specific WebSocket API.

If Plan 216 is not complete, H2 extraction may land first with Extended CONNECT retained behind a temporary compatibility bridge, but Plan 217 cannot close until there is one shared direct service-facing tunnel contract.

## Workstream H — Direct convenience server and optional TLS integration

Teach the direct convenience `Server` to use the same H2 driver when the `http2` feature is enabled.

For cleartext TCP:

- preserve existing prior-knowledge behavior and configuration.

For optional EggServe-owned TLS convenience paths:

- preserve ALPN ordering/selection already qualified;
- reuse `eggnet-tls` identity/trust/reload substrate rather than reintroducing duplicate certificate logic;
- select the direct H1/H2 connection driver after handshake;
- keep TLS optional.

This work must not make the direct caller-owned H2 API depend on EggServe-owned TLS.

## Workstream I — Compatibility-core facade conversion

After direct H2 parity is proven:

- re-export direct `Http2Config` from compatibility paths;
- route core H2 caller-owned connection APIs to the direct driver;
- route compatibility listener/TLS H2 handling through the direct driver where ownership permits;
- remove duplicate core H2 builder/config/request-pipeline implementations;
- retain only thin H3/compatibility glue that has an explicit ownership reason.

Any H2 server implementation left in core at plan closure must have a documented blocker.

## Workstream J — Dependency and feature isolation checks

Extend topology/feature checks to prove:

- direct default/H1 builds do not enable Hyper H2 features;
- `eggserve-server --features http2` enables only the expected H2 dependency surface;
- H3/Quinn/H3-Quinn are not pulled by `http2`;
- static serving is not pulled by generic H2 server use;
- TLS remains separately optional;
- direct H2 does not depend on `eggserve-core`;
- compatibility core delegates rather than becoming a hidden required dependency.

Record `cargo tree -e features` evidence for:

1. direct server default;
2. direct server `http2`;
3. direct server `http2,tls`;
4. core `http2` compatibility;
5. core `http3,tls` to prove H3 isolation remains unchanged.

## Workstream K — H2 qualification

Run the existing H2 qualification program against the direct implementation, not merely compatibility aliases.

At minimum retain or add deterministic coverage for:

- preface/protocol selection;
- concurrent requests;
- large/invalid header handling;
- authority/framing validation;
- streaming request body;
- request trailers;
- streaming response;
- response trailers where supported;
- HEAD/body-forbidden response behavior;
- peer stream reset;
- sibling stream survival;
- body/read timeout;
- service timeout;
- flow-control backpressure;
- response no-progress timeout;
- graceful shutdown;
- connection shutdown under active streams;
- service panic isolation;
- H2 Extended CONNECT after Plan 216 integration;
- already-established TLS stream with explicit H2 selection;
- compatibility/direct parity.

Also run the existing external/manual interoperability tooling (`scripts/qualify-http2.sh`) according to repository release policy.

### No automatic support-tier promotion

Source extraction and parity are not sufficient to promote H2. Existing browser/platform/trailer-scope/reset-hook or other documented promotion gaps remain authoritative until separately closed.

The Plan 217 closure record must state the support tier truthfully.

## Validation matrix

At minimum run:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo +1.88 check --workspace --all-targets --features http2
cargo +1.88 check --workspace --all-targets --features http2,tls
cargo clippy -p eggserve-server --features http2 --lib --tests -- -D warnings
cargo test -p eggserve-server --features http2
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/qualify-http2.sh
```

Continue to run the HTTP/3 compatibility graph/tests required by routine CI so H2 extraction does not regress Plan 213 isolation.

## Documentation updates

Update:

- `README.md`;
- `AGENTS.md`;
- `architecture/crate-topology.md`;
- `architecture/http2.md`;
- downstream application-server documentation;
- public API/migration documentation;
- feature/dependency documentation;
- H2 qualification/conformance records.

Documentation must distinguish:

- direct H1 default;
- opt-in direct H2;
- explicit caller-owned H2 for externally negotiated TLS;
- cleartext prior-knowledge selection;
- H2 support tier;
- H3's separate experimental dependency/support boundary.

## Implementation sequencing

Recommended order:

1. Complete the Plan 215 direct connection-runtime prerequisites.
2. Inventory H2-specific core ownership and existing qualification coverage.
3. Move `Http2Config` and validation authority to the direct server.
4. Move explicit caller-owned H2 driving onto the Plan 215 direct pipeline.
5. Move cleartext prior-knowledge selection.
6. Move H2 lifecycle/flow-control/timeout behavior and tests.
7. Integrate Plan 216 generic Extended CONNECT capability.
8. Route direct convenience listener/TLS paths through the direct H2 driver.
9. Run deterministic direct H2 parity tests.
10. Run external/manual H2 qualification.
11. Convert compatibility-core H2 paths to re-exports/thin adapters.
12. Add feature-isolation/anti-duplication checks and update docs.
13. Publish a truthful closure record without changing support tier unless the independent promotion gate also passes.

## Acceptance criteria

Plan 217 is complete only when all of the following are true:

1. `eggserve-server`, behind `http2`, owns the single mature generic H2 server/connection implementation.
2. Direct callers can serve an already-established H2 byte stream without importing `eggserve-core` or Hyper server types.
3. External TLS terminators can select H2 explicitly after their own ALPN/security policy and pass the established stream into EggServe.
4. Cleartext prior-knowledge selection matches the previously qualified compatibility behavior.
5. `Http2Config` has one defaults/validation authority in the direct server ownership layer.
6. H1 and H2 share the same canonical direct `Service`, request body, lifecycle, response normalization, admission, and observability pipeline where protocol-neutral.
7. H2-specific flow-control, reset, timeout, and shutdown behavior matches the mature compatibility implementation and documented limitations.
8. H2 Extended CONNECT uses Plan 216's generic tunnel contract; no H2-specific application service API exists.
9. Compatibility-core H2 paths are re-exports/thin adapters rather than a second H2 runtime.
10. The direct default/H1 graph remains free of H2 activation, and the `http2` feature does not pull H3/Quinn/static dependencies.
11. HTTP/3 remains isolated and experimental according to Plan 213.
12. H2's support tier is unchanged unless a separate documented promotion gate passes.
13. Direct deterministic H2 tests, compatibility parity tests, topology checks, and manual/external H2 qualification pass at the level required by existing release policy.

## Closure evidence

When implemented, add a release/qualification record containing:

- H2 ownership before/after matrix;
- `Http2Config` identity/default/validation evidence;
- direct caller-owned plaintext and established-TLS H2 test results;
- cleartext protocol-selection evidence;
- concurrent-stream/flow-control/reset/shutdown test results;
- Plan 216 Extended CONNECT integration evidence;
- `cargo tree -e features` output for default/H1/H2/H2+TLS/H3 compatibility graphs;
- external H2 qualification results;
- anti-duplication/topology gate output;
- explicit statement of the retained/promoted support tier and any remaining qualification gaps.