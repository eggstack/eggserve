# Phase 51 — Milestone 3A: Reusable Runtime and Service Boundary

## Goal

Refactor eggserve’s server-side library boundary into a reusable, transport-owning HTTP runtime that downstream Rust projects can embed without importing internal modules or depending directly on Hyper.

The runtime must remain deliberately narrow. It should own listener acceptance, HTTP/1 parsing, request conversion, response normalization, body transport, limits, and cancellation. Downstream users should supply a service/handler, not a socket loop or parser.

This phase establishes the Rust runtime architecture and public service contract. It must not add routing, middleware stacks, request-body streaming, ASGI/WSGI, HTTP/2, reverse proxying, or application-framework behavior.

## Starting state

Eggserve already has:

- a production CLI accept loop;
- Rust-owned HTTP parsing and file streaming;
- canonical request and response types;
- static-service handling;
- Python callback support;
- connection and file-stream limits;
- header and response-write timeouts;
- graceful shutdown behavior;
- TLS feature support;
- production-path, raw-wire, and conformance tests.

However, the reusable runtime boundary remains coupled to implementation-specific paths. The library needs a clear public model separating:

- transport runtime;
- service invocation;
- static file service;
- response normalization;
- lifecycle handle;
- connection metadata;
- operational limits.

## Track A — Runtime architecture inventory

Document the current server path from listener bind to response completion.

Inventory:

- CLI bind/configuration;
- listener acceptance;
- TLS handshake path;
- Hyper connection serving;
- canonical request conversion;
- static service path;
- Python callback path;
- canonical response normalization;
- file/byte body transport;
- connection permit acquisition/release;
- file-stream permit acquisition/release;
- timeout enforcement;
- shutdown propagation;
- error logging and sanitization.

Classify each component as:

- reusable runtime core;
- static-service implementation;
- CLI adapter;
- Python adapter;
- internal transport helper.

Produce an architecture document showing intended dependencies and ownership. Avoid beginning implementation until duplicate loops and lifecycle paths are identified.

Acceptance:

- one documented production request path exists;
- proposed public runtime types map to existing behavior;
- no security policy is accidentally moved into downstream code.

## Track B — Public service contract

Define a minimal, transport-independent service abstraction.

Possible shape:

```rust
pub trait Service: Send + Sync + 'static {
    fn call(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>>;
}
```

Alternative designs are acceptable if they avoid unstable public async-trait assumptions and unnecessary boxing. Evaluate:

- explicit boxed future;
- associated future type where practical;
- concrete function-service wrapper;
- optional `tower-service` compatibility behind an experimental feature.

Requirements:

- public request and response types are canonical eggserve values;
- no Hyper request/response types in the stable boundary;
- service errors are typed and do not leak sensitive internals;
- handler panic containment is defined;
- service invocation is cancellable;
- service objects are safely shareable across connections;
- response normalization remains runtime-owned;
- services cannot bypass final framing policy through the safe API.

Provide convenience adapters:

- `service_fn` or equivalent for closures;
- a static-file service implementation;
- an explicit not-handled outcome only if required for later composition, without building a router.

Acceptance:

- a downstream crate can implement a service using only public types;
- the runtime can invoke static and custom services through the same boundary;
- safe services cannot write raw socket bytes.

## Track C — Runtime configuration model

Introduce or refine a public runtime configuration separate from CLI parsing.

The model should include:

- connection limit;
- in-flight request limit if supported;
- file-stream limit;
- header-read timeout;
- response-write timeout;
- handler timeout;
- graceful-shutdown timeout;
- keep-alive policy;
- TLS acceptor/configuration abstraction;
- log/observer hooks only where already supported;
- server identification/header policy if applicable.

Requirements:

- safe defaults match or strengthen current CLI defaults;
- CLI `ServeConfig` maps deterministically into runtime configuration;
- Python configuration maps to the same semantics;
- no runtime-only field is silently ignored by one frontend;
- configuration validation is typed and performed before serving.

Avoid putting filesystem policy into generic runtime configuration. Static policy belongs to the static service.

Acceptance:

- runtime and static-service configuration are cleanly separated;
- configuration parity is tested across CLI, Rust, and Python;
- invalid combinations fail before listener acceptance.

## Track D — Connection execution pipeline

Refactor the production connection path so the reusable runtime owns:

1. connection permit acquisition;
2. optional TLS handshake;
3. HTTP/1 connection setup;
4. request conversion to canonical values;
5. request-policy validation owned by the runtime;
6. service invocation;
7. canonical response normalization;
8. transport-body conversion;
9. write timeout and error propagation;
10. permit release and connection termination.

Requirements:

- there is one production connection executor used by CLI and embedded runtime;
- malformed input is rejected before service invocation;
- connection metadata comes from the actual socket/TLS session;
- handler failures map to deterministic responses without tracebacks or internal leakage;
- cancellation releases every permit;
- stream errors terminate correctly and are observable;
- no frontend forks protocol semantics.

Acceptance:

- CLI behavior remains wire-compatible;
- embedded runtime behavior matches CLI for the same service/configuration;
- the service is never responsible for parser or transport correctness.

## Track E — Static service extraction

Make hardened static serving a reusable service built on the generic runtime.

Suggested shape:

```rust
let static_service = StaticService::builder(root)
    .policy(policy)
    .limits(static_limits)
    .build()?;
```

Requirements:

- preserve descriptor-relative confinement on supported Unix platforms;
- preserve dotfile, symlink, and directory-listing defaults;
- preserve request-body rejection for built-in static service;
- preserve GET/HEAD-only semantics;
- preserve conditional and range handling;
- preserve Rust-owned file streaming;
- keep filesystem policy independent of generic runtime policy;
- no path reopening after secure resolution.

The CLI should become a thin adapter that builds the runtime plus `StaticService`.

Acceptance:

- embedded Rust users can run the same hardened static service as the CLI;
- no security behavior differs between CLI and library use;
- static-service tests run against both direct service invocation and production sockets.

## Track F — Error taxonomy

Define public runtime errors for:

- invalid configuration;
- bind/listener setup;
- TLS setup/handshake classification;
- lifecycle misuse;
- service failure;
- handler timeout;
- connection failure;
- shutdown timeout;
- transport conversion failure.

Separate:

- startup errors returned to the caller;
- per-connection errors handled internally/observed;
- service errors converted into responses;
- fatal runtime errors.

Do not expose raw internal error chains as stable API. Preserve source errors for diagnostics where appropriate.

Acceptance:

- callers can distinguish startup, lifecycle, and runtime failure classes;
- error responses do not leak internals;
- documentation identifies which errors terminate the server.

## Track G — Public API and feature boundaries

Decide crate/module placement for runtime APIs.

Preferred public facade:

```rust
eggserve_core::server::{
    Server,
    ServerBuilder,
    ServerHandle,
    RuntimeConfig,
    Service,
    service_fn,
    StaticService,
}
```

Implementation modules may remain internal.

Review feature flags:

- default server/runtime support;
- TLS;
- client;
- Python-internal bridge;
- optional interoperability features.

Avoid making Hyper or Tower mandatory public dependencies solely for trait exposure.

Add external-consumer compile fixtures proving:

- public runtime use without Hyper imports;
- static service construction;
- custom service construction;
- feature combinations;
- no internal module dependency.

Acceptance:

- the public import story is coherent;
- downstream adapters have the primitives needed to build on eggserve;
- feature flags remain bounded and documented.

## Track H — Tests

Add tests for:

- closure/function service;
- custom service returning bytes;
- custom service returning file/range body;
- service error conversion;
- panic containment;
- handler timeout;
- malformed request not reaching service;
- connection metadata;
- CLI versus embedded-runtime wire parity;
- TLS versus plaintext semantic parity;
- connection permit recovery;
- file-stream permit recovery;
- cancellation during service call;
- response normalization across service types;
- static-service security regression.

Use direct service tests for exhaustive behavior and real-socket tests for transport/lifecycle behavior.

## Documentation

Update:

- `architecture/overview.md`;
- `architecture/eggserve-core.md`;
- a new runtime architecture document;
- `docs/release-contract.md`;
- `docs/api-stability.md`;
- `docs/library-capability-matrix.md`;
- Rust examples;
- README embedding examples;
- non-goals.

Classify new runtime APIs as experimental until Milestone 3 conformance closure.

## Completion criteria

Phase 51 is complete only when:

- a public transport-independent service contract exists;
- one reusable connection executor owns protocol correctness;
- CLI and embedded runtime share the same production path;
- hardened static serving is a reusable service;
- public runtime configuration is validated and frontend-neutral;
- no Hyper type leaks into the public service boundary;
- cancellation and errors are typed and tested;
- existing static-serving security behavior is unchanged.

## Non-goals

- Request-body streaming.
- Routing tables or middleware stacks.
- ASGI/WSGI adapters.
- HTTP/2, WebSockets, or upgrades.
- Reverse proxying.
- Authentication/session frameworks.
- Raw socket access through the safe service API.