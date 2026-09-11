# Plan 200 — Rust `http` / `http-body` / Tower Interoperability

## Status

**IMPLEMENTED / CLOSED.**

Prerequisite: Plan 197. Implemented in parallel with Plans 198–199 on the settled native request/response contract.

## Closure record

Implemented on `main`: `http-interop` feature (`dep:http`, `primitives::interop` — loss-aware method/status/version/authority/URI/header conversions via `from_bytes`, `RawTargetExt`/`ConnectionInfoExt`/`AuthorityExt`/`LifecycleExt`, `RequestBody: http_body::Body` with data+trailers/backpressure/sanitized errors, `response_from_http_body` framing-authoritative with validated trailers, `response_from_bytes`/`empty_response_from_http` conveniences) and `tower` feature (`http-interop` + `dep:tower-service`/`dep:tower-layer`, `server::tower` — `TowerToEggserve` per-request clones driving `poll_ready` with explicit body policy, `EggserveToTower` adapter-local ready via normalization + `to_hyper_response` boxing; no shared mutex, `max_in_flight_requests` stays outer ceiling); middleware boundary after parsing/validation before normalization documented in `docs/http-interop.md`; fixtures in `crates/eggserve-core/tests/interop_http_tower.rs` (full/streaming, duplicates/opaque, request/response trailers, header middleware, readiness clones, errors, HEAD non-poll, H1 TCP parity, round-trip, native-independence) plus `primitives::interop` unit tests; docs updated (`http-interop.md`, `downstream-app-server.md`, `http-primitives.md`, `public-api-boundary.md`, `primitives-api.md`, `runtime.md`, `README.md`, `AGENTS.md`, skill). Verification: workspace fmt/clippy/tests plus `http-interop,tower` matrices green locally before push.

## Purpose

Add optional compatibility adapters so Rust application servers and middleware can consume EggServe through established ecosystem abstractions without replacing EggServe's native canonical model or exposing Hyper internals.

EggServe currently has a deliberately custom canonical vocabulary because it preserves project-specific invariants: byte fidelity, duplicate ordering, framing ownership, lifecycle cancellation, request-body policy, and hardened response normalization. That native vocabulary remains authoritative.

This plan creates explicit edges to the broader Rust HTTP ecosystem:

- `http::{Request, Response, Method, Uri, HeaderMap, Extensions}` where semantics can be represented faithfully;
- `http_body::Body` / `Frame` for data and trailers;
- Tower `Service`/`Layer` integration with explicit readiness and error semantics.

## Design principle

Adapters must be loss-aware. Do not silently coerce a canonical EggServe value into a standard type if doing so loses metadata or security semantics. Where a standard representation cannot express a property exactly, expose a documented extension or return a conversion error.

Do not make Tower or `http` the internal protocol-correctness authority.

## Track A — Dependency and feature policy

Audit the current dependency graph. `http-body` is already a direct dependency and `http` arrives through Hyper; decide whether `http` should become an explicit direct dependency for public adapters. Tower should be optional and feature-gated.

Suggested features:

```text
http-interop   -> direct `http` adapter module
 tower         -> `http-interop` + optional `tower-service`/`tower`
```

Prefer `tower-service` if only the trait is required; add full `tower` only if Layers/utilities are exposed or used in tests. Do not force Tower into default/minimal builds.

## Track B — Canonical-to-`http` metadata conversions

Provide audited conversions for method, status, version, authority/URI, and header fields.

### Headers

EggServe preserves duplicate order. `http::HeaderMap` supports duplicate values but has different iteration/insertion semantics from EggServe's ordered block. Document exact round-trip guarantees and test them. If global field-line order across different names cannot round-trip through `HeaderMap`, do not claim it can; preserve the native API for consumers requiring that fidelity.

Opaque legal field-value bytes should map through `http::HeaderValue::from_bytes` rather than UTF-8/string conversion.

### URI/request target

Do not fabricate a normalized `Uri` as the only representation if exact raw target bytes would be lost. Store EggServe raw target metadata in `http::Extensions` through an EggServe-owned extension type when converting to ecosystem requests.

### Connection metadata/lifecycle

Attach typed EggServe-owned extensions for `ConnectionInfo`, lifecycle, trusted proxy metadata, and advanced capabilities only where safe. One-shot tunnel capabilities must not be cloned through `http::Request` extension cloning patterns.

If a capability cannot be represented safely, keep it available only through the native adapter wrapper rather than placing it directly in `Extensions`.

## Track C — Request body adapter

Implement an `http_body::Body` view over canonical `RequestBody` capable of yielding:

- `Frame::data(Bytes)`;
- `Frame::trailers(HeaderMap)` after Plan 198.

Requirements:

- preserve one-shot ownership;
- no `Sync` promise unless true;
- byte/body limits and read timeouts remain enforced by EggServe;
- `size_hint` is truthful and does not turn declared length into a guarantee after transport failure;
- body errors map to a typed adapter error without exposing internal sensitive strings;
- dropping the adapter preserves canonical abandoned-body/reuse semantics;
- cancellation/disconnect wakes pending polls.

Do not create an unbounded buffering compatibility path.

## Track D — Response body adapter

Accept `http_body::Body` responses and convert frames into the canonical response pipeline.

Requirements:

- EggServe remains the final framing authority;
- application `Content-Length` is validated/reconciled under existing policy rather than blindly trusted;
- hop-by-hop/protocol-forbidden fields still pass through canonical normalization;
- response trailers use the Plan 198 trailer validator;
- HEAD/body-forbidden statuses never poll body/trailer frames;
- producer panic/error/cancellation maps into existing committed-response behavior;
- bounded/no-progress timeout tracking works at the canonical poll boundary.

Provide a convenience adapter for common `Bytes`/empty/full bodies without requiring users to understand internal `ResponseBody` variants.

## Track E — Tower service adapter

Tower's `Service<Request>` has `poll_ready(&mut self)` and `call(&mut self, request)`, whereas EggServe's native `Service` is shared `&self` and runtime admission is separate. Keep those semantics distinct.

Preferred integration directions:

### E1. Run a Tower service on EggServe

Provide an adapter that accepts a Tower service/factory and exposes an EggServe native `Service`.

Important ownership issue: Tower services often require mutable readiness/call sequencing and cloning. Do not put a single `Mutex<S>` around a service merely to satisfy the trait; that can serialize unrelated requests and defeat readiness semantics.

Use a well-defined service-factory/per-connection/per-request clone policy. Evaluate established `tower::make`/service construction patterns and current Tower idioms at implementation time. Document whether readiness is per shared service clone or per request.

EggServe's server-wide `max_in_flight_requests` remains an outer hard admission ceiling. Tower readiness may further delay/reject application work but cannot raise that ceiling.

### E2. Expose EggServe service as Tower where useful

An inverse adapter may implement Tower `Service` around an EggServe native service for composition/testing. `poll_ready` should reflect adapter-local readiness, not falsely claim transport admission already acquired.

Do not make this inverse adapter a prerequisite for server operation.

## Track F — Middleware boundary and security

Tower Layers should operate on standard HTTP request/response objects only after EggServe has completed protocol parsing/validation and before final EggServe response normalization.

This means middleware may add ordinary application headers/content, but cannot bypass:

- body hard limits;
- canonical framing validation;
- final header denylist/privacy policy;
- response body/no-progress timeouts;
- connection lifecycle/shutdown.

Document this order explicitly so users know which policies are security boundaries versus middleware conveniences.

## Track G — Advanced capabilities

Decide how trailers, interim responses, and tunnels project into the adapter:

- trailers map naturally through `http_body::Frame::trailers`;
- interim response capability is likely an EggServe extension/accessor, because `http::Response` represents the final response only;
- tunnel capability must remain one-shot and must not become an ordinary clonable extension if that weakens ownership.

Provide an adapter-specific request wrapper if necessary rather than corrupting `http::Request` semantics.

## Track H — Interoperability fixtures

Add dev-only fixtures with representative middleware/services:

- simple Tower service returning full body;
- streaming body with backpressure;
- duplicate/opaque headers;
- request and response trailers;
- middleware that adds headers;
- readiness saturation;
- service errors/panics;
- disconnect/shutdown during stream;
- H1/H2/H3 parity through the same Tower application where supported.

Avoid bringing an entire web framework into normal dependencies. A dev-only Axum or comparable smoke test is acceptable only if it materially proves ecosystem compatibility and remains outside production feature graphs.

## Documentation

Add a concise interoperability guide explaining when to use:

1. native EggServe `Service` for maximum fidelity/control;
2. `http`/`http-body` adapters for ecosystem body/message compatibility;
3. Tower adapter for middleware/application stacks.

State all known round-trip limitations, especially header order and exact target bytes.

## Acceptance criteria

- [ ] optional adapters accept/produce standard `http` messages without leaking Hyper;
- [ ] request and response bodies implement/use `http-body` incrementally with real backpressure;
- [ ] trailers map through frames without full-body buffering;
- [ ] legal opaque header values use byte conversion, never mandatory UTF-8;
- [ ] exact target/connection metadata that standard types cannot express remains available through documented EggServe metadata or the native API;
- [ ] Tower readiness semantics are honored without globally serializing requests behind a naive mutex;
- [ ] EggServe hard admission/framing/privacy/timeout boundaries remain authoritative after middleware;
- [ ] optional features do not enter the default dependency graph unnecessarily;
- [ ] representative Tower middleware/service fixtures pass across qualified transports;
- [ ] native consumers remain independent of Tower.

## Handoff

Plan 207 must include at least one standard-HTTP/Tower consumer in the application-server contract matrix, while retaining native canonical tests as the normative correctness baseline.