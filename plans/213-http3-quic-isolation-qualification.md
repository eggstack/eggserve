# Plan 213 — HTTP/3 and QUIC Isolation, Qualification, and Promotion Gates

## Status

Planned.

## Purpose

Contain EggServe's experimental HTTP/3 and QUIC dependency surface behind a dedicated crate and establish explicit criteria for when HTTP/3 can be treated as hardened rather than experimental.

This work is intentionally an isolation and qualification effort, not an attempt to build a common Eggstack QUIC framework.

## Background

EggServe currently uses:

- Quinn;
- H3;
- H3-Quinn

for optional HTTP/3 support.

EggFetch and Eggress also use this ecosystem, but for materially different roles:

- EggServe: inbound HTTP server;
- EggFetch: outbound HTTP client;
- Eggress: proxy/CONNECT/transport behavior.

The common dependency names do not make the implementations interchangeable.

The H3 ecosystem also remains less mature than Hyper's H1/H2 path. Relevant upstream issues include unresolved stream/error-handling behavior, including cases around buffered bytes during connection failure and request-stream reset semantics.

EggServe should therefore preserve HTTP/3 as an optional experimental transport until both upstream and EggServe-specific qualification improve.

## Proposed crate

Introduce:

`eggserve-h3`

This crate owns:

- Quinn endpoint integration;
- H3 connection handling;
- H3 request adaptation;
- H3 response writing;
- QUIC/H3 stream mechanics;
- H3 tunnel/CONNECT adaptation where supported;
- H3-specific error translation.

It consumes the canonical EggServe primitives/service contract.

It should not own application semantics that are already shared by H1/H2.

## Dependency boundary

Only `eggserve-h3` should need direct production dependencies on:

- Quinn;
- H3;
- H3-Quinn.

The dependency-light primitives crate must not depend on them.

The main H1/H2 server crate should not require them when HTTP/3 is disabled.

A facade feature may re-export HTTP/3 support for compatibility, but the Cargo package boundary must remain real.

## Shared service behavior

Continue translating HTTP/3 requests into the same canonical request/service model used by H1/H2.

Protocol-specific code may handle:

- stream lifecycle;
- pseudo-header interpretation;
- QUIC errors;
- response framing;
- connection-level signaling.

Protocol-specific code must not independently reimplement:

- trusted forwarding semantics;
- application request policy;
- service dispatch semantics;
- generic response policy;
- common application timeout rules

unless a protocol-specific requirement makes the distinction necessary.

## Extended CONNECT

Retain current supported CONNECT behavior only where it is valid in the current H3 library stack.

Do not claim generic WebSocket-over-H3 support while the upstream H3 parser rejects the relevant extended CONNECT `:protocol` before EggServe receives it.

Document this as an upstream/stack capability limitation rather than implementing a second parser around the library.

## Upstream issue tracking

Maintain a short HTTP/3 qualification section in the threat model or H3 documentation listing upstream issues that materially affect EggServe's security/correctness assurance.

At minimum, track the currently identified issues involving:

- loss/discard risk for already-buffered stream bytes when a connection error is observed in the same polling cycle;
- stream reset behavior when request streams are dropped.

Do not duplicate entire upstream issue discussions in EggServe docs.

Record:

- dependency version;
- affected behavior;
- whether EggServe has mitigation;
- whether promotion is blocked.

Remove entries when upstream fixes are adopted and regression-tested.

## Qualification matrix

HTTP/3 should be tested independently of H1/H2.

Required coverage should include:

### Protocol conformance

- basic GET/HEAD;
- request body;
- response body;
- trailers if supported;
- cancellation;
- malformed request rejection;
- connection shutdown;
- stream reset;
- concurrency;
- large/bounded headers;
- body limits;
- request timeouts;
- service errors.

### Cross-protocol equivalence

Where application semantics are intended to be protocol-independent, run the same canonical service test vectors over:

- HTTP/1.1;
- HTTP/2;
- HTTP/3.

Differences must be documented rather than silently normalized in tests.

### Adversarial behavior

Exercise:

- partial request delivery;
- abrupt connection loss;
- reset streams;
- concurrent stream churn;
- slow body delivery;
- service cancellation;
- graceful shutdown under active streams;
- connection/stream errors occurring around buffered application data.

### Interoperability

Test against at least one implementation outside the exact EggServe H3 test stack where practical.

Avoid tests that merely instantiate both client and server from identical helper code and therefore reproduce the same bug on both sides.

## Dependency updates

Treat Quinn/H3/H3-Quinn as a coordinated compatibility set.

When changing one:

- inspect all three versions;
- rerun H3 qualification tests;
- review upstream release notes/issues;
- retest H3/TLS integration.

Do not introduce a shared dependency-version crate solely to enforce this.

Repository automation may group these dependencies for updates if the existing maintenance tooling supports that cleanly.

## Cross-repository coordination

EggServe, EggFetch, and Eggress should align on test vectors where useful, not on a shared high-level runtime crate.

Candidate shared assets include:

- protocol fixtures;
- malformed-frame cases;
- certificate fixtures;
- QUIC failure scenarios;
- interoperability scripts.

Do not yet share:

- H3 connection drivers;
- client/server request state machines;
- proxy CONNECT implementations;
- routing;
- transport ownership.

Revisit code consolidation only after two implementations contain substantially identical neutral code and the upstream H3 API stabilizes enough that the shared layer will not simply magnify churn.

## Hardened promotion gate

HTTP/3 must remain documented as experimental until all of the following are true:

- no known upstream correctness issue remains that the maintainers consider incompatible with the project's hardened-server claims, or a tested local mitigation exists;
- adversarial stream/error tests pass reliably;
- cross-protocol application conformance passes;
- external interoperability coverage is established;
- TLS/QUIC configuration boundaries are documented;
- graceful shutdown and reset behavior are qualified;
- dependency updates no longer require ad hoc architecture changes for routine patch/minor releases.

Promotion should be a deliberate documentation/release decision, not an automatic consequence of test count.

## Non-goals

- QUIC protocol implementation from scratch.
- Forking H3 without a critical, demonstrated reason.
- Sharing EggFetch's client connection driver.
- Sharing Eggress's proxy H3 protocol implementation.
- WebTransport.
- Generic browser application protocol support.
- Claiming WebSocket-over-H3 support that the upstream stack cannot expose.
- Making HTTP/3 a default feature.

## Exit criteria

The plan is complete when:

- Quinn/H3/H3-Quinn production code is isolated in `eggserve-h3`;
- disabling HTTP/3 removes those runtime dependencies from the server package graph;
- H3 uses the canonical EggServe service model;
- a dedicated qualification matrix exists and passes;
- upstream risk tracking is explicit;
- HTTP/3 remains experimental unless the promotion gate is separately satisfied.
