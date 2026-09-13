# Plan 215 — Direct Embeddable Connection Runtime Parity

## Status

Implemented 2026-09-13 — see `release/plan-215-direct-runtime-parity.md`.
The direct crate owns the mature H1 kernel (ops/errors/policy/authority,
service contract shape, connection vocabulary, H1 config/state, H1
connection driver, Hyper conversion boundary) with a 16-scenario
direct-vs-compatibility parity suite. Criteria 5/9 hold except for the
tunnel-entangled remainder (unified `Service` identity, tunnel-capable core
pipeline/context/conversion), which is explicit Plan 216 input with seam
comments and a topology gate enforcing the boundary.

## Purpose

Complete the generic HTTP/1 runtime extraction left intentionally incomplete by Plan 214 so `eggserve-server` becomes the single implementation home for EggServe's embeddable connection-serving substrate.

The direct crate already owns the simple HTTP/1 service path, but the mature compatibility runtime in `eggserve-core::server` still owns the capabilities needed by nontrivial downstream servers: caller-owned byte streams, trustworthy socket metadata, mature timeout/admission/lifecycle accounting, graceful per-connection shutdown, connection outcomes, panic containment, and the richer service/error contract.

This plan moves those generic responsibilities into `eggserve-server` without adding application-specific admission hooks and without making EggServe an application framework. A reverse proxy, WAF, local daemon, custom TLS terminator, anonymity transport, or test harness must be able to perform its own transport policy before handing an established byte stream to EggServe.

A concrete downstream such as Synvoid is motivating evidence for this boundary, not an API contract. No Synvoid types, WAF callbacks, flood-protection hooks, JA4 hooks, routing hooks, or backend concepts may enter EggServe.

## Relationship to Plan 214

Plan 214 established the direct crate topology and moved canonical/static/H1 streaming behavior into the direct crates, while explicitly leaving advanced runtime compatibility paths in `eggserve-core` pending parity work.

This plan narrows the remaining generic H1 work into a separately verifiable extraction. It does not reopen the static extraction and it does not claim HTTP/2 or tunnel parity; those are Plans 217 and 216 respectively.

Plan 215 is a prerequisite for Plans 216 and 217 because both need one authoritative direct connection pipeline rather than another compatibility-only implementation.

## Current-state findings

The repository currently has two materially different server/runtime surfaces.

`crates/eggserve-server/src/lib.rs` currently provides:

- the direct `Service` trait and simple `ServiceError`;
- a small `RuntimeConfig`;
- a `Server` that binds its own TCP listener;
- a connection semaphore and request timeout;
- an HTTP/1 Hyper driver;
- request/response conversion over the direct canonical primitives.

The mature `crates/eggserve-core/src/server/` implementation additionally owns:

- caller-owned `AsyncRead + AsyncWrite` connection driving;
- `ConnectionContext` with observed local/remote socket addresses, scheme, TLS metadata, and proxy provenance fields;
- `ConnectionShutdown` with level-triggered, pre-signal-safe shutdown semantics;
- `ConnectionOutcome` classification;
- `RuntimeState` with independent service/file/tunnel admission resources;
- connection activity and request lifecycle tracking;
- header, idle, body-read, handler, write-no-progress, and total-connection timeout semantics;
- max-requests-per-connection handling;
- panic containment at service and task boundaries;
- response commitment/cancellation accounting;
- per-runtime observability/correlation state through `OpsContext`;
- richer `ServiceError`, `service_fn`, `service_fn_head`, and `service_fn_with_policy` behavior;
- listener/handle lifecycle machinery and advanced compatibility integrations.

The direct request conversion also currently constructs `ConnectionInfo::without_socket_addrs(...)`, which means a direct consumer cannot rely on transport-authenticated local/remote socket addresses. That is unsuitable for generic reverse-proxy, virtual-host, audit, or security-policy consumers.

Plan 214's central rule remains binding here: move the mature implementation; do not grow a second approximation of it.

## Goals

1. Make `eggserve-server` the single owner of the mature generic HTTP/1 connection driver.
2. Provide a public caller-owned connection API over arbitrary established bidirectional async byte streams without exposing Hyper types.
3. Preserve truthful observed local/remote connection metadata supplied by the caller.
4. Move the mature generic runtime state, admission, timeout, lifecycle, cancellation, and connection-outcome semantics out of compatibility core.
5. Converge the direct and compatibility `Service`/`ServiceError` definitions onto one implementation.
6. Make the direct `Server` convenience runtime drive connections through the same canonical connection implementation used by caller-owned transports.
7. Preserve the current default direct dependency graph discipline: server may depend downward on primitives and transport/runtime libraries, never on core/static/application crates.
8. Leave application admission before handoff entirely caller-owned.
9. Preserve compatibility-core paths as re-exports or thin composition adapters during the 0.x line.
10. Add parity evidence strong enough that a downstream can replace its own Hyper HTTP/1 connection loop without adopting EggServe static serving or application policy.

## Non-goals

- HTTP/2 extraction or support-tier changes; Plan 217 owns that work.
- Generic tunnel/Upgrade/CONNECT extraction; Plan 216 owns that work.
- HTTP/3 or Quinn/H3 movement; Plan 213 remains authoritative.
- Moving static serving back into the server crate.
- Moving certificate parsing, SNI identity policy, client-auth policy, or reload logic out of `eggnet-tls`.
- Requiring EggServe to terminate TLS for caller-owned connections.
- Adding pre-accept/post-accept WAF, rate-limit, flood-protection, JA4, routing, authentication, or application middleware callbacks.
- Extracting inbound PROXY/trusted-forwarding policy merely for a downstream integration. Existing compatibility behavior may remain compatibility-owned until separately justified.
- Turning `Service` into Tower or requiring Tower in the default graph.
- Adding framework routing or backend/proxy semantics.
- Binary-size claims without measured before/after evidence.

## Architectural invariants

### One connection driver

At plan completion there must be one authoritative HTTP/1 connection execution path. The direct `Server`, caller-owned connection API, and compatibility facade must all reach the same driver.

There must not be one simplified direct H1 driver plus a richer compatibility H1 driver.

### Caller owns pre-HTTP policy

The caller-owned connection API begins after the downstream has an established bidirectional byte stream and validated transport context.

A downstream may therefore:

1. accept a TCP connection itself;
2. inspect/reject it using local L3/L4 policy;
3. optionally sniff protocol bytes or terminate TLS;
4. construct truthful `ConnectionContext` metadata;
5. hand the surviving stream to EggServe.

EggServe does not need a downstream-specific accept callback to support this model.

### No Hyper in public embedding signatures

Public caller-owned driver APIs must accept generic async byte streams and canonical EggServe service/config/context types. Hyper remains an implementation detail.

### No upward dependency

`eggserve-server` must never depend on `eggserve-core` or `eggserve-static`. Any mature runtime implementation moved from core must have its dependencies moved downward or replaced by a neutral server-owned contract.

### Preserve default H1 graph

This plan must not make HTTP/2, TLS, H3, static serving, Tower, or filesystem dependencies mandatory for direct H1 consumers.

## Workstream A — Ownership and parity inventory

Before moving source, produce a file/symbol ownership matrix for the mature H1 runtime.

At minimum classify:

- `server/service.rs`;
- `server/runtime.rs`;
- `server/lifecycle.rs`;
- `server/connection/{mod,context,lifecycle,activity,transport,driver,pipeline,request,response,deferred_body}.rs`;
- relevant H1 configuration from `server/config/`;
- runtime-limit projections used by those modules;
- `server/handle.rs` portions required by the simple direct server;
- `ops` types/events consumed directly by the connection runtime.

Each item must be classified as:

1. move to `eggserve-server`;
2. re-export from direct ownership;
3. remain compatibility-only with a documented reason;
4. H2/tunnel/TLS/proxy-specific and therefore deferred to another plan.

Do not copy source into the direct crate before deciding which copy will be deleted or converted to a facade.

### Acceptance

The inventory must identify every remaining duplicate `Service`, `ServiceError`, H1 connection-driver, runtime-state, and H1 timeout implementation.

## Workstream B — Converge the service contract

Move the mature service abstraction into `eggserve-server` and make compatibility core use the exact same trait/types.

Required direct behavior includes:

- `Service: Send + Sync + 'static`;
- body-policy selection before service invocation;
- the mature `ServiceError` categories and sanitized transport mapping;
- `service_fn`;
- `service_fn_head`;
- `service_fn_with_policy`;
- panic containment semantics;
- request-body-error conversion;
- no `poll_ready` requirement on the native trait;
- no raw transport access through `Service`.

Where compatibility code still needs a Hyper response helper, keep that transport conversion outside the canonical direct `ServiceError` definition rather than retaining a second error implementation in core.

### Source compatibility

Prefer additive/move-and-re-export changes. Existing direct `Service` implementations should continue compiling when possible. Any unavoidable pre-1.0 source break must be documented in the migration guide and must remove, rather than add, duplicate authority.

### Acceptance

`eggserve_server::Service` and `eggserve_core::server::Service` resolve to the same trait definition. The same requirement applies to `ServiceError` and service helper constructors.

## Workstream C — Extract connection context, shutdown, and outcomes

Move the mature generic connection vocabulary into `eggserve-server`:

- `ConnectionContext`;
- `ConnectionShutdown`;
- `ConnectionOutcome`.

Preserve the existing semantics for:

- optional local address;
- optional remote address;
- semantic HTTP/HTTPS scheme;
- optional transport-authenticated TLS metadata;
- truthful absence for non-socket transports;
- level-triggered idempotent shutdown;
- pre-signal-safe shutdown observation;
- clean/error/timeout/shutdown outcome classification.

Proxy provenance fields may remain in `ConnectionContext` if they are already transport-neutral values from `eggserve-primitives`; this plan does not require moving the policy/parser that decides whether a PROXY preamble or forwarded header is trusted.

### Important boundary

Caller-supplied `ConnectionContext` is asserted transport metadata. EggServe must not silently derive trusted client identity from untrusted HTTP forwarding headers in this plan.

### Acceptance

A direct caller can supply real local/remote socket addresses and observe those exact values in the canonical request's `ConnectionInfo` without importing `eggserve-core`.

## Workstream D — Extract the caller-owned HTTP/1 driver

Move the mature caller-owned connection path into `eggserve-server`.

The direct crate must expose the equivalent of the compatibility runtime's:

- `serve_http1_connection`;
- `serve_http1_connection_with_id` where explicit correlation IDs remain part of the mature contract.

The public signature must remain generic over a suitable bidirectional async byte stream and must not expose `hyper::Request`, `hyper::body::Incoming`, `TokioIo`, or other Hyper implementation types.

The driver must preserve:

- canonical request construction;
- streaming request bodies;
- one-shot consumption semantics;
- trailers;
- request lifecycle/disconnect propagation;
- service admission;
- handler timeout;
- body-read timeout;
- header-read timeout;
- keep-alive idle timeout;
- total connection timeout;
- response write no-progress timeout;
- max request count per connection;
- response normalization and commitment;
- producer/deferred-body cleanup;
- graceful shutdown;
- task/permit release on every exit;
- panic containment currently qualified by the mature core path.

Tunnel-specific behavior is excluded from this plan except for preserving seams needed by Plan 216. HTTP/2 selection is excluded except for ensuring Plan 217 can reuse the same extracted pipeline.

### Acceptance

A downstream test must be able to create an arbitrary caller-owned stream, provide a service/config/context/runtime state, request per-connection shutdown, and receive a classified `ConnectionOutcome` entirely through `eggserve-server`.

## Workstream E — Extract runtime state and admission authority

Move the H1-generic runtime state required by the mature driver into `eggserve-server`.

Preserve independent resource accounting where applicable for:

- connection admission;
- in-flight service executions;
- response/deferred producer work;
- file-stream admission only if the generic response body/file contract requires it;
- tunnel admission as an inert/reserved seam only if required for source movement, with active tunnel behavior deferred to Plan 216.

The implementation must not create a second set of semaphores alongside still-authoritative core semaphores.

### Runtime configuration

Rehome the generic runtime configuration and validation needed by H1 into the direct server layer while retaining one defaults/validation authority.

Do not fork shared limits between:

- direct `RuntimeConfig`;
- compatibility `RuntimeConfig`;
- `Limits`;
- CLI/Python projection.

If shared runtime defaults currently live in core, move the generic authority downward or introduce a single neutral authority that both paths consume. Do not duplicate constant values.

### Acceptance

Direct and compatibility construction reject the same invalid H1 runtime configurations and project the same defaults.

## Workstream F — Resolve observability dependency direction

The mature connection runtime currently depends on the Plan 181 per-runtime `OpsContext`. `eggserve-server` cannot solve this by depending on core.

Perform a dependency audit of `crate::ops` and choose one of two acceptable outcomes:

1. move the transport-generic observability vocabulary/context into `eggserve-server`, with compatibility core re-exporting it; or
2. define a small server-owned event-sink/correlation contract and adapt the existing compatibility `OpsContext` to it.

Prefer moving the existing implementation when its dependencies fit the direct server graph. Introduce an abstraction only when it materially prevents an upward dependency or compatibility break.

Required properties:

- no global-only observability regression;
- per-runtime connection ID sequencing remains available;
- connection/timeouts/panics/rejections retain bounded structured event kinds;
- sink failures remain contained;
- no mandatory `tracing` dependency merely to satisfy the extraction unless separately justified by dependency policy.

### Non-goal

Do not resurrect Plan 205 request-observer/application hooks. This work is runtime observability plumbing only.

## Workstream G — Make the direct `Server` use the canonical driver

Refactor the simple direct `Server::start_with_service` path so it becomes a convenience owner of listener acceptance around the same direct connection driver.

At minimum:

- preserve real accepted local/remote socket addresses;
- construct a truthful `ConnectionContext`;
- use the same `RuntimeState` and driver as caller-owned streams;
- use the same shutdown semantics and task cleanup;
- stop silently dropping accepted connection errors without the runtime's existing outcome/ops accounting.

This plan does not require moving every Plan 201 listener-adoption mode out of core. Prebound Unix/systemd/H3 socket integration may remain compatibility-owned until separately justified. If simple prebound TCP listener adoption moves cleanly with no extra dependency or semantic coupling, it may move here, but it must feed the same canonical accept/driver path rather than introduce another path.

## Workstream H — Compatibility facade conversion

After direct parity is proven:

- convert H1-generic `eggserve_core::server` modules to re-exports or thin adapters over `eggserve-server`;
- remove the duplicate H1 `Service` and `ServiceError` definitions;
- remove the duplicate generic caller-owned H1 driver;
- remove duplicate generic runtime-state/lifecycle code that no longer owns H2/H3/tunnel compatibility behavior;
- leave protocol-specific compatibility modules only where Plans 216/217/213 still require them.

Every substantial H1 implementation left in core must have a documented reason.

## Workstream I — Downstream-neutral embedding example

Add or update a Rust example that demonstrates the intended embedding boundary without application-framework behavior.

The example should:

1. create or receive a bidirectional stream;
2. construct explicit connection metadata;
3. optionally reject a synthetic connection before handoff to demonstrate caller-owned admission;
4. invoke the direct caller-owned H1 driver;
5. expose a simple canonical service;
6. request graceful per-connection shutdown;
7. inspect `ConnectionOutcome`.

Do not use Synvoid-specific code in the example.

## Workstream J — Parity and anti-duplication qualification

### Differential behavior

Run the existing mature H1 conformance cases against the direct runtime and compatibility facade.

At minimum cover:

- valid GET/HEAD;
- request bodies in reject/buffer/stream modes;
- chunked bodies and trailers;
- malformed framing;
- request-target and header ceilings;
- handler timeout;
- body read timeout;
- header timeout;
- idle timeout;
- response write stall;
- total connection timeout;
- max requests per connection;
- service panic;
- peer disconnect while service is running;
- peer disconnect while body is being consumed;
- shutdown before driver waiter registration;
- shutdown during active request;
- streaming response cleanup;
- real local/remote address propagation;
- non-socket context with truthful missing endpoints.

### Anti-duplication guard

Extend `scripts/check-crate-topology.py` or a focused repository check so regressions are detectable.

At minimum assert that:

- `eggserve-core` does not define a second canonical `Service` trait;
- the generic caller-owned H1 driver is defined in `eggserve-server` only;
- direct server source does not import `eggserve-core` or `eggserve-static`;
- compatibility H1 paths resolve to the direct implementation.

Avoid brittle line-count rules.

## Validation matrix

Routine implementation validation must include:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test -p eggserve-primitives
cargo test -p eggserve-server
cargo test -p eggserve-core
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Also run the direct downstream/application-server conformance subset that exercises H1 streaming/lifecycle behavior.

Feature checks for HTTP/2/TLS/H3 must continue compiling during migration, but this plan's parity claim is H1 only. Plan 217 owns direct H2 behavior.

## Documentation updates

Update at least:

- `README.md`;
- `AGENTS.md`;
- `architecture/crate-topology.md`;
- downstream application-server/extension documentation;
- public API stability/migration documentation where paths move.

The final documentation must say that the direct server owns the mature H1 embeddable connection runtime and that advanced tunnel/H2/H3 behavior has its own support/extraction status.

## Implementation sequencing

Recommended order:

1. Produce the ownership/parity inventory.
2. Converge `Service` and `ServiceError` into the direct crate.
3. Move connection context/shutdown/outcome types.
4. Resolve observability dependency direction.
5. Move runtime state/config authority needed by H1.
6. Move the mature caller-owned H1 connection driver and supporting modules.
7. Switch the direct listener-based server to that driver.
8. Run differential H1 qualification.
9. Convert compatibility core H1 paths to re-exports/thin adapters.
10. Add anti-duplication enforcement and update docs.
11. Only after Plan 215 closes should Plans 216/217 remove their remaining protocol-specific compatibility ownership.

Keep intermediate commits buildable. Temporary compatibility bridges are acceptable during the move but must have an explicit deletion step.

## Acceptance criteria

Plan 215 is complete only when all of the following are true:

1. `eggserve-server` owns the single mature HTTP/1 connection driver.
2. A caller can serve an already-established arbitrary async byte stream through the direct crate without importing Hyper or `eggserve-core`.
3. `ConnectionContext`, `ConnectionShutdown`, and `ConnectionOutcome` are available through the direct server API with mature semantics.
4. Real local/remote socket addresses supplied by a caller or observed by the direct TCP server reach canonical requests unchanged.
5. The mature `Service`, `ServiceError`, and service helper definitions have one implementation authority in `eggserve-server`.
6. H1 runtime limits, admission, timeout, lifecycle, cancellation, panic-containment, and response-commitment behavior match the previously qualified compatibility implementation.
7. The direct convenience `Server` and caller-owned connection API share the same driver and runtime state.
8. `eggserve-server` has no dependency on `eggserve-core`, `eggserve-static`, HTTP/3, application frameworks, or downstream application code.
9. Compatibility-core H1 paths are re-exports or thin adapters rather than an independent H1 implementation.
10. No downstream-specific admission callback or WAF/proxy/router concept was added to EggServe.
11. Default/H1 dependency footprint does not grow with H2/H3/static/TLS dependencies.
12. Topology, routine CI, H1 conformance, and direct embedding tests pass.

## Closure evidence

When implemented, add a release/qualification record that captures:

- source-ownership before/after matrix;
- direct vs compatibility H1 conformance results;
- direct crate dependency graph before/after;
- proof of real socket metadata propagation;
- caller-owned connection lifecycle/shutdown tests;
- anti-duplication/topology gate output;
- any intentional pre-1.0 migration notes.

Do not claim binary-size reduction from this plan unless measured artifacts demonstrate it. The primary success metric is removal of duplicate runtime ownership and maintenance/audit burden.