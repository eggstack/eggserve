# Plan 163 — Transport-Neutral Canonical Connection Driver

## Status

**IMPLEMENTED / CLOSED.**

Prerequisite: Plan 161. Coordinate public-type changes with Plan 162 if both are implemented together.

## Goal

Expose the existing HTTP/1 connection machinery through an EggServe-native API that can serve a canonical `Service` over any suitable bidirectional async byte stream, without requiring a TCP listener, `SocketAddr` peer identity, or a Hyper service.

This is the key embedding boundary for downstream runtimes and an I2P router: the caller supplies an already-established stream; EggServe supplies HTTP parsing, request conversion, body policy, service dispatch, response normalization/framing, timeouts, and connection closure semantics.

## Current gap

Two useful capabilities exist today but do not meet at one public boundary:

- the low-level connection executor is generic over `AsyncRead + AsyncWrite`, but its service side is Hyper-native;
- the canonical `Service` executor uses EggServe request/response types, but requires concrete local/remote `SocketAddr` metadata and is reached through the TCP-oriented runtime path.

A downstream non-TCP transport should not have to fabricate socket addresses or reimplement the Hyper-to-canonical adapter.

## Target API

Exact naming is provisional. The public capability should resemble:

```text
serve_http1_connection(
    io: AsyncRead + AsyncWrite,
    service: Service,
    config: ConnectionConfig/RuntimeConfig subset,
    context: ConnectionContext,
    runtime/admission state,
    cancellation/shutdown signal,
) -> ConnectionOutcome
```

Requirements:

- no public Hyper request/response/service/body types;
- no requirement that the stream is TCP;
- caller remains owner of listener/tunnel/transport establishment;
- EggServe owns HTTP/1 semantics once the byte stream is handed over;
- the same canonical pipeline serves TCP, TLS, and caller-owned streams.

## Layering

Refactor toward one execution path:

```text
TcpListener accept ----+
TLS accept ------------+--> canonical connection driver --> Service
caller-owned stream ---+
```

The existing `Server` remains a convenience runtime that owns TCP listener acceptance and lifecycle. It should call the canonical driver rather than maintaining a semantically separate request pipeline.

Do not expose a generic listener registry or transport plugin framework. One connection-driving function/type is sufficient.

## Connection metadata evolution

`ConnectionInfo` is currently a stable primitive with mandatory `local_addr` and `remote_addr` `SocketAddr` values. That contract is too TCP-specific.

Evolve it additively where practical. Preferred conceptual shape:

```text
ConnectionInfo
  socket_endpoints: Option<SocketEndpoints>
  scheme: Scheme
  tls: Option<TlsInfo>
  transport metadata: minimal, bounded, non-identifying
```

or equivalent optional local/remote endpoint accessors.

Rules:

- real TCP/TLS connections continue to expose actual socket endpoints;
- non-socket transports expose no fabricated IP/port;
- forwarded/X-Forwarded fields remain ordinary untrusted HTTP headers;
- do not add I2P `Destination`, tunnel IDs, router identities, or LeaseSet types to EggServe;
- do not add an arbitrary `Any`/extension map solely to carry downstream transport state;
- if downstream code needs peer identity, it should retain that identity outside EggServe and associate it with its own service wrapper/session state.

Because `ConnectionInfo` is treated as stable, update API snapshots, docs, examples, Python mappings, and migration notes in the same implementation.

## Scheme and TLS semantics

Do not infer `https` solely from concrete Tokio TLS types. Caller-owned streams need an explicit trustworthy connection context.

Provide a bounded way for the caller/runtime to describe:

- HTTP vs HTTPS semantic scheme when meaningful;
- optional TLS metadata when EggServe performed/knows the TLS session;
- absence of TLS metadata for opaque encrypted transports such as an I2P stream.

An anonymity-network transport is not automatically `https`; it is an application transport carrying HTTP unless the caller explicitly terminates HTTPS on it.

## Shared admission state

A caller driving many external streams must still be able to share EggServe runtime budgets. Do not make each `serve_http1_connection()` invocation create independent file/response/service limits.

Refactor the currently internal runtime state into a small reviewed public/experimental admission context if necessary. It may own:

- file-stream permits;
- in-flight request/service permits from Plan 164;
- counters/metrics handles where already global/shared;
- other strictly transport-runtime state required to enforce server-wide bounds.

It must not own static filesystem state or application routing state.

Provide an ergonomic constructor from `RuntimeConfig`/limits so downstream callers cannot accidentally omit mandatory budgets.

## Cancellation and lifecycle

Caller-owned streams need a per-connection cancellation/shutdown mechanism independent of the TCP `ServerHandle`.

Requirements:

- caller can request graceful connection shutdown;
- runtime can terminate on hard timeout or protocol error;
- dropping/cancelling the driver does not leak permits or producer tasks;
- in-flight service/body work observes cancellation through existing future/body drop semantics;
- connection outcome distinguishes normal EOF, protocol/client error, timeout, shutdown, and internal failure at least for internal observability.

Avoid a large public lifecycle state machine unless required. A small cancellation token/signal plus returned outcome is preferable.

## HTTP parser/runtime invariants

The transport-neutral path must retain every existing protection:

- Hyper HTTP/1 parsing;
- body framing validation/defense in depth;
- TRACE/body policy;
- canonical request conversion;
- global/service request body ceilings;
- handler/body/header timeouts;
- panic containment;
- canonical response normalization/framing;
- server/header privacy policy from Plan 165 when implemented;
- file/response admission;
- incomplete-body close semantics;
- shutdown/drain behavior applicable at the connection level.

There must not be a weaker “embedded” parser path.

## TCP/TLS runtime refactor

After the driver is stable:

- make `Server::start_with_service()` and built-in static serving use it;
- keep listener bind/pre-bound listener and TLS handshake concerns above the driver;
- preserve current TCP/TLS wire behavior and `ServerHandle` lifecycle;
- eliminate duplicate conversion/finalization logic where possible;
- keep raw Hyper connection helpers crate-private unless tests/fuzzing require a hidden escape hatch.

## Tests

Add tests using `tokio::io::duplex` or an equivalent controlled non-socket stream for:

- GET/HEAD canonical service success;
- request body buffering and streaming;
- response streaming after Plan 162;
- malformed request/framing rejection;
- keep-alive multiple requests;
- connection close rules;
- header/body/handler timeout behavior;
- caller cancellation;
- client half-close/disconnect;
- shared admission saturation across multiple independent streams;
- no `SocketAddr` requirement;
- connection metadata absence/presence semantics;
- byte-for-byte parity with the TCP path for equivalent HTTP input where transport metadata is irrelevant.

Keep existing TCP/TLS conformance suites as regression gates.

## Python impact

Do not expose arbitrary Python file-like/socket objects as raw async transports in this plan. Plan 166 should decide what Python runtime construction surface is useful without creating an unsafe generic I/O bridge.

Update Python `ConnectionInfo`-like views if exposed so non-socket endpoints are representable as `None`, not fake addresses.

## Non-goals

Do not add:

- I2P protocol logic or tunnel management;
- Unix-domain listener management beyond what a downstream caller can already adapt into an async stream;
- QUIC/HTTP/3;
- HTTP/2;
- arbitrary per-request transport extensions;
- socket hijacking/upgrades;
- a generic transport registry;
- WAF/rate-limiting logic.

## Acceptance criteria

- [ ] A Rust caller can hand EggServe a non-TCP `AsyncRead + AsyncWrite` stream and a canonical `Service` and receive correct HTTP/1 service behavior.
- [ ] The caller does not import Hyper or fabricate socket addresses.
- [ ] TCP/TLS convenience servers use the same canonical connection pipeline.
- [ ] Server-wide admission can be shared across caller-owned connections.
- [ ] Cancellation/timeout/protocol exits release all permits and tasks.
- [ ] Stable `ConnectionInfo` evolution has migration documentation and API snapshot coverage.
- [ ] Existing static/Python/TCP/TLS wire behavior remains qualified.

## Handoff

Plan 164 strengthens the shared runtime budgets used by this driver. Plan 165 adds final response privacy policy. Plan 168 uses this API to build a deterministic I2P-like transport harness without requiring a live I2P network in CI.

## Closure record

Implemented in commit `5aa126183b1859d290a0d67713fd5668e424d9c7`.

No material deviation from the proposed transport-neutral service boundary;
Plan 169 verified the relaxed response-stream producer bound across the same
TCP/caller-owned runtime path.
