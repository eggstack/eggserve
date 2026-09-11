# Plan 197 — Application Service Contract Stabilization

## Status

**PLANNED.** Prerequisite: Plan 196 accepted. Builds on Plans 173–175 and 184–195.

## Purpose

Promote the experimental downstream application-server seam from a successful qualified shape into a deliberately designed native contract that can support the remaining server-foundation features without repeatedly breaking `Service`, `Request`, `Response`, or lifecycle ownership.

This plan does not make EggServe an application framework and does not promise a 1.0 release. It establishes the contract that Plans 198–205 extend.

## Current-state findings

The existing architecture has the right central seam: a transport-independent `Service` receives a canonical `Request` and returns a canonical `Response`; request bodies can be delegated and streamed; response bodies can be streamed; `RequestLifecycle` exposes cancellation; and Plan 175 proves a bounded external consumer.

The remaining problem is that several future capabilities have no obvious attachment point without distorting the ordinary request/response model:

- trailers are terminal message metadata, not ordinary header mutations;
- interim responses occur before the final `Response` exists;
- upgrade/tunnel ownership is duplex and cannot be modeled as a response body;
- trusted peer/proxy/TLS metadata needs a typed request context rather than ad hoc headers;
- ecosystem adapters need a place for typed extensions without making EggServe depend on Tower/framework concepts.

## Design goals

The final contract should make ordinary services simple while allowing advanced services to opt into capabilities. Prefer additive request context/capability objects over a giant service trait with many methods.

A target conceptual shape is:

```text
Request
  head
  body
  context
    connection info
    lifecycle
    optional typed capabilities

Service::call(Request) -> final response/outcome

Response
  status
  headers
  body/message stream
```

Do not expose transport implementation handles merely because an advanced capability needs internal state.

## Track A — Inventory the public/experimental seam

Produce a source-level API inventory for:

- `Service`, `service_fn`, `ServiceError`;
- `Request`, `RequestHead`, `RequestBody`, `RequestBodyPolicy`;
- `RequestLifecycle` and cancellation reasons;
- `ConnectionInfo`/TLS/protocol metadata;
- `Response`, `ResponseBody`, `ResponseStream`;
- `Server`, `ServerBuilder`, `ServerHandle`, caller-owned connection entry points;
- runtime/listener ownership APIs used by external consumers.

Classify each as one of: stabilize unchanged, stabilize after correction, keep experimental/internal, or compatibility adapter.

Check current Rustdoc/API snapshots and external consumer tests before changing anything.

## Track B — Introduce a typed request context/capability container

The request needs a stable place for transport-authenticated metadata and one-shot optional capabilities without continuously adding top-level fields.

Prefer an EggServe-owned type such as:

```rust
pub struct RequestContext {
    connection: ConnectionInfo,
    lifecycle: RequestLifecycle,
    // opaque typed extensions/capabilities internally
}
```

or equivalent accessors on `Request` backed by an internal context.

Requirements:

- ordinary metadata access remains cheap;
- context cloning never clones one-shot ownership capabilities;
- typed values cannot be forged by untrusted request headers;
- downstream application state is not stored here by default; Tower/framework extension maps belong in adapters unless a narrowly scoped native extension map is justified;
- transport capabilities remain opaque and capability-based, not raw socket references.

If a general type map is considered, benchmark and document its allocation/cost. Prefer explicit typed accessors for security-sensitive metadata.

## Track C — Separate final response from advanced service outcome only if required

Plans 198–199 will determine whether `Service::call()` can continue returning `Response` or needs an experimental `ServiceOutcome`.

Decision rule:

- trailers that belong to a final response should live in the response/message-body abstraction, not force an outcome enum;
- interim responses should use a request-scoped sender/capability or equivalent so the final return remains ordinary;
- upgrades/tunnels may require a distinct accepted-tunnel outcome if pairing a continuation with a final response cannot be made type-safe otherwise.

If a `ServiceOutcome` becomes necessary, provide an ergonomic conversion from ordinary `Response` and preserve `service_fn` simplicity. Do not add variants for H1/H2/H3 or framework-specific concepts.

## Track D — Define commitment and cancellation semantics

Write a normative contract for when a response is considered:

1. not started;
2. interim metadata emitted;
3. final response head committed to the runtime;
4. body streaming;
5. terminal metadata/trailers emitted;
6. complete/cancelled/failed;
7. transitioned into a non-HTTP tunnel where applicable.

The contract must specify what happens if:

- service errors/panics before final response;
- service attempts an interim response after final commitment;
- response producer errors after commitment;
- request body remains delegated after response-start;
- peer disconnect/reset occurs at each stage;
- shutdown/timeout races with service completion.

There must never be an attempt to synthesize a second HTTP error response after final commitment.

## Track E — Clarify concurrency and readiness

Keep the native `Service` sharing model (`Send + Sync`) unless evidence requires change. Document that server-wide service admission and downstream application-task admission are separate concepts, as established by Plan 175.

Do not copy Tower's `poll_ready` semantics into the native trait. Tower readiness belongs in the adapter plan; native EggServe admission remains runtime-owned and deterministic.

## Track F — Stabilize error taxonomy at the boundary

Review `ServiceError`, body errors, connection errors, cancellation reasons, and adapter-visible errors.

Requirements:

- client-facing errors remain sanitized;
- service/library callers can distinguish rejection, internal failure, timeout, cancellation, and committed-stream failure where useful;
- transport-specific H2/H3 reset codes are not required for ordinary applications;
- optional advanced tunnel APIs may expose a small generic close/cancellation reason without leaking implementation enums;
- errors are non-exhaustive where future categories are plausible.

## Track G — API compatibility strategy

Because the server module is currently experimental, prefer correcting the shape now rather than carrying permanent compatibility baggage. Nevertheless:

- preserve the Plan 175 common path where feasible;
- add migration notes for signature/type changes;
- use `#[non_exhaustive]` where consumers should tolerate growth;
- avoid exposing concrete Tokio/Hyper/H3/rustls types in semver-considered APIs;
- keep the stable primitives facade compatible unless a correctness issue requires a planned minor transition.

Add/update API snapshot tests so future plans know which changes are intentional.

## Track H — Documentation/examples

Create or update a minimal application-service example showing:

- buffered request -> bytes response;
- streamed request -> streamed response;
- lifecycle cancellation;
- no static filesystem dependency.

Document the ownership contract in `docs/downstream-app-server.md`, `architecture/runtime.md`, and the public API boundary docs.

## Verification

At minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test app_server_consumer
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test api_stability
cargo test -p eggserve-core --test deferred_lifecycle
cargo test -p eggserve-core --features 'tls,http2,http3'
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Add compile fixtures for the intended native application-server API without importing Hyper.

## Acceptance criteria

- [ ] the native application-facing request/service/response/lifecycle ownership model is documented normatively;
- [ ] advanced capability attachment has one deliberate API location rather than ad hoc top-level fields;
- [ ] ordinary `Service` implementations remain simple and transport-neutral;
- [ ] commitment/cancellation semantics are explicit enough for trailers, interim responses, and tunnels to build on without contradiction;
- [ ] downstream admission remains distinct from EggServe service admission;
- [ ] security-sensitive connection/TLS/proxy metadata cannot be forged via ordinary headers;
- [ ] API snapshots/migration notes cover all intentional changes;
- [ ] the Plan 175 external consumer remains possible using only public APIs;
- [ ] no Tower, ASGI, WebSocket codec, routing, middleware, or worker semantics enter the native service contract.

## Handoff

Plans 198–200 consume this contract. If implementation discovers that trailers/interim responses/upgrades cannot fit without a `ServiceOutcome`, make that change here or as the first explicitly documented step of Plan 199 rather than letting each protocol adapter invent its own outcome model.