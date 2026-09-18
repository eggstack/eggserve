# Plan 220 — Move the HTTP/3 adapter implementation into eggserve-h3

## Purpose

Turn `eggserve-h3` from a dependency-version fence into the actual optional H3/QUIC transport adapter.

The current architecture isolates direct Quinn/H3 dependencies in `eggserve-h3`, but the protocol state machine and canonical request/response bridge still live under `eggserve-core::server::http3`. This leaves the compatibility core owning experimental transport implementation despite the dedicated H3 crate.

## Goals

- Make `eggserve-h3` own H3/QUIC endpoint, connection, request, response, tunnel-stream, and shutdown mechanics.
- Keep canonical service semantics in `eggserve-primitives` / `eggserve-server`.
- Keep H3 optional and absent from default dependency closure.
- Make `eggserve-core` consume/re-export a narrow H3 adapter rather than implement it.
- Preserve the experimental H3 support tier and all Plan 192–195 blockers.

## Non-goals

- No H3 promotion.
- No upstream h3 fork.
- No WebTransport/datagram expansion.
- No generic QUIC abstraction shared with eggfetch/eggress in this plan.

## Target dependency direction

```
eggserve-primitives
       ^
       |
eggserve-server
       ^
       |
eggserve-h3  -> h3 / h3-quinn / quinn / rustls QUIC glue

eggserve-core -> optional eggserve-h3 compatibility facade
```

`eggserve-server` must not depend on H3.

## Work

### 1. Move adapter modules

Move the implementation equivalent of:
- endpoint lifecycle,
- request conversion,
- response streaming/trailers,
- Extended CONNECT/tunnel stream bridging,
- H3 connection shutdown/drain,
- QUIC close classification,
- H3-specific limits/config translation

from core into `eggserve-h3`.

### 2. Define a narrow adapter API

The H3 crate may depend on canonical primitives/server interfaces but should not expose Quinn/H3 types as stable EggServe application APIs.

Prefer entry points expressed in:
- canonical `Service`,
- `RuntimeState`,
- neutral runtime configuration values or an H3-owned config,
- standard socket/listener ownership where possible,
- canonical shutdown/result types.

Quinn/H3 types may remain crate-internal or doc-hidden.

### 3. Configuration ownership

Separate generic runtime limits from H3-only transport configuration.

Avoid making `eggserve-server::RuntimeConfig` directly depend on Quinn types. If the compatibility core currently owns an H3 configuration struct, move the implementation authority to H3 and preserve compatibility aliases as needed.

### 4. TLS/identity boundary

Use `eggnet-tls` only for reusable identity parsing where applicable. QUIC TLS assembly remains H3-owned because ALPN/TLS1.3/QUIC transport policy are protocol-specific.

Preserve:
- TLS 1.3,
- `h3` ALPN,
- no application 0-RTT,
- bounded pending handshakes/streams/windows,
- migration policy,
- same-port TCP/UDP validation where required.

### 5. Topology update

Revise Plan 213-era topology assumptions:
- H3 is allowed to depend downward on primitives/server.
- primitives/server are forbidden from depending upward on H3.
- core may optionally depend on H3 only as compatibility composition.
- default graph must remain H3/QUIC-free.

### 6. Qualification evidence

Move/update H3 tests so they exercise the new crate directly where possible. Keep compatibility integration tests in core/bin only for facade parity.

Do not erase current blocked/pending evidence in conformance metadata.

## Tests

- all H3 deterministic tests from Plans 187–195,
- producer timeout/write-stall tests,
- shutdown race/drain tests,
- body Reject probe tests,
- stream cancellation/sibling isolation,
- H3 tunnel tests,
- TLS/ALPN startup tests,
- default dependency graph absence test,
- packaging checks for optional H3 features.

## Migration strategy

First introduce the H3 crate adapter API and call it from core while old modules remain behind a temporary internal feature. Once parity is proven, delete the core implementation.

## Rollback

If an API boundary proves awkward, keep a small core orchestration wrapper; do not move protocol mechanics back into core.

## Acceptance criteria

- `crates/eggserve-core/src/server/http3/**` no longer contains the H3 protocol state machine.
- `eggserve-h3` owns the actual H3/QUIC adapter.
- Default builds contain no H3/QUIC dependency.
- H3 feature tests are behaviorally unchanged.
- H3 remains experimental with prior blockers intact.
- Topology CI enforces downward-only dependency direction.
