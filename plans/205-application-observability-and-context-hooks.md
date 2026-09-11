# Plan 205 — Application Observability and Context Hooks

## Status

**PLANNED.** Prerequisite: Plan 197 context/lifecycle shape. Builds on Plan 181 per-runtime `OpsContext` rather than replacing it.

## Purpose

Provide enough structured hooks for downstream application servers to correlate requests, connections, protocol streams, TLS/proxy provenance, lifecycle transitions, and application work without forcing a logging/tracing/metrics framework into EggServe core.

The current `OpsContext`/event sink/counter model is deliberately lightweight and appropriate. A full server foundation needs a few additional request-scoped hooks and stable identifiers so an embedder can integrate tracing/OpenTelemetry/metrics externally without parsing EggServe log text or importing transport internals.

## Non-goals

Do not add an OpenTelemetry SDK/exporter, Prometheus HTTP endpoint, `tracing-subscriber`, global logger, distributed trace propagation policy, W3C Trace Context parser, framework middleware, or application metrics registry to core.

## Track A — Stable runtime/connection/request identities

Define opaque identifiers with clear scope:

- runtime/server instance ID;
- listener/endpoint ID when multiple sources exist;
- connection ID;
- request ID unique within the runtime;
- internal protocol stream correlation ID where useful for H2/H3.

Requirements:

- identifiers contain no client-controlled text;
- generation is cheap and does not require randomness with cryptographic guarantees unless a public unpredictability requirement exists;
- no collision within the documented scope under normal lifetime;
- request ID is available through the canonical request context and operation events;
- internal H2/H3 stream ID may remain observability-only rather than stable application API.

Do not automatically accept an incoming `X-Request-ID` as the trusted native request ID. Applications may separately preserve client correlation headers.

## Track B — Structured request lifecycle events

Extend the existing event vocabulary only for meaningful server-foundation states:

- request accepted/dispatched;
- service started/response-start returned;
- final response committed;
- response completed;
- request body complete/abandoned/failed;
- request/response trailers completed where Plan 198 applies;
- tunnel accepted/closed/reset where Plan 199 applies;
- cancellation/disconnect reason;
- overload/admission rejection;
- proxy metadata accepted/rejected provenance;
- TLS identity/client-auth selection outcomes;
- graceful drain/forced termination.

Events must use fixed categories and bounded sanitized metadata. Never include body/tunnel payloads or raw hostile header values by default.

## Track C — Per-request observer hook

Provide an optional Rust observer interface suitable for downstream instrumentation without blocking the runtime.

Possible shape:

```rust
pub trait RequestObserver: Send + Sync + 'static {
    fn on_event(&self, event: &RequestEvent);
}
```

or reuse/extend the existing event sink with request context. Prefer the smallest extension that avoids a second observability system.

Requirements:

- callback is synchronous/non-async and must be cheap; document that it runs on runtime worker paths;
- panics are contained or observer contract makes panic consequences explicit and safe;
- no arbitrary async waiting inside transport critical sections;
- observer can correlate native request IDs with downstream tracing spans;
- event schemas are non-exhaustive/versioned where growth is expected;
- disabled path has negligible allocation/locking overhead.

If the existing `OpsSink` can express this cleanly, extend it instead of adding `RequestObserver`.

## Track D — Downstream context propagation seam

Native EggServe should expose request metadata/correlation so adapters can create their own context:

- Plan 200 may insert request ID/connection info into `http::Extensions`;
- Plan 204 may expose it to Python;
- downstream ASGI/Tower middleware decides how to create tracing spans and propagate trace headers.

Do not interpret traceparent/baggage or mutate application headers in core.

A small native type map for application state is not required solely for observability; use adapter/framework extension facilities where available.

## Track E — Counters and gauges

Audit existing operational counters against a general server base. Add only server-owned resource metrics with unambiguous semantics, such as:

- active connections by negotiated protocol;
- active H2/H3 streams/requests;
- active request-body streams;
- active response streams;
- active tunnels;
- TLS handshakes in flight/failures;
- proxy preamble failures;
- application-service admission in use/rejections;
- Python bridge application tasks in use if exposed through Python-specific ops rather than core.

Avoid high-cardinality labels in the core model. Hostnames, paths, client IPs, SNI names, and arbitrary methods should not become metric-label dimensions by default.

## Track F — Timing model

Expose monotonic durations/timestamps for server-owned phases without conflating semantics:

- accept -> protocol ready;
- request dispatch -> response-start;
- response-start -> response completion;
- request-body completion;
- total request lifetime where definable;
- tunnel lifetime;
- TLS handshake duration.

Use monotonic clocks for durations. Wall-clock timestamp formatting belongs to sinks. Do not claim socket-wire completion when the protocol library only exposes body polling/queue progress; preserve the current truthful distinction for H2/H3 response progress.

## Track G — Privacy policy

Review every new field against the existing privacy/fingerprint policy.

Default events must not include:

- request/response bodies;
- tunnel payloads;
- authorization/cookie/header values;
- full request query strings;
- certificate private material;
- raw client certificates unless explicitly configured in an application-owned layer;
- arbitrary exception messages from Python/downstream services.

Document which endpoint/client address fields are emitted and how trusted-proxy provenance affects them.

## Track H — Tests/performance

Required tests:

- event order for normal, early-response, failure, timeout, disconnect, shutdown, trailer, tunnel paths;
- exactly-once terminal event/counter release;
- H2 multiplexed request IDs remain distinct and sibling events are not conflated;
- observer panic/failure does not violate transport safety;
- disabled observability path allocation/performance sanity;
- sanitized metadata contains no hostile/raw header/body content;
- counter snapshots return to baseline after cancellation/failure.

No absolute performance CI gate; record same-machine before/after evidence if hot-path overhead changes materially.

## Acceptance criteria

- [ ] runtime/connection/request identifiers are stable within documented scope and available to embedders;
- [ ] request lifecycle/commit/trailer/tunnel states emit structured fixed-category events;
- [ ] downstream instrumentation can correlate events without parsing log text or importing Hyper/H3 internals;
- [ ] disabled hooks add negligible hot-path complexity;
- [ ] core remains independent of OpenTelemetry, Prometheus exporters, tracing subscribers, and global logging frameworks;
- [ ] counters represent bounded server-owned resources and recover exactly once;
- [ ] timing fields are semantically truthful about what EggServe actually observes;
- [ ] privacy policy prevents payload/credential/high-cardinality leakage by default;
- [ ] Tower and Python adapters can project correlation metadata without changing the native security model.

## Handoff

Plan 207 uses these identifiers/events to make cross-protocol failure and resource qualification diagnosable, but release qualification must not require a particular observability backend.