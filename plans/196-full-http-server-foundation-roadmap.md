# Plan 196 — Full HTTP Server Foundation Roadmap

## Status

**PLANNED — umbrella roadmap for Plans 197–208.**

Baseline: `main` after Plans 172–195. Plans 173–175 already established byte-preserving canonical metadata, deferred request-body ownership/lifecycle, and an external application-server consumer. Plans 179–184 already consolidated runtime configuration, decomposed the HTTP/1 connection pipeline, and established a protocol-neutral runtime. Plans 185–195 added and qualified experimental HTTP/2 and HTTP/3. This roadmap must build on those results rather than recreating them.

## Product decision

EggServe should become a hardened, reusable HTTP server foundation that can be embedded directly from Rust and can support downstream application servers in Rust or Python. EggServe itself remains a transport/runtime library and static-server product, not a web framework or a maintained ASGI/WSGI application server.

The target boundary is:

```text
Rust application / Tower service / Python app-server bridge
                         |
                adapter/service boundary
                         |
              EggServe canonical HTTP API
                         |
        limits / lifecycle / framing / privacy
                         |
             H1 / H2 / H3 protocol adapters
                         |
 TCP / TLS / QUIC / caller-owned or prebound transports
```

EggServe owns protocol parsing, framing, canonical message semantics, transport lifecycle, resource limits, timeout policy, cancellation, graceful drain, final response normalization, and security-sensitive transport metadata. Downstream consumers own routing, middleware semantics, framework loading, worker/process strategy, application lifespan, WebSocket framing, authentication, sessions, templates, and application business logic.

## Why a second roadmap is required

Plan 172 intentionally closed an HTTP-only application-server substrate and deferred generic upgrades because no concrete consumer existed. The current product goal is broader: the server base should be capable of supporting full application servers. That changes the evidence threshold for features that were previously optional.

Current gaps that cannot be implemented cleanly entirely above EggServe are:

1. request/response trailers and interim response events;
2. safe generic HTTP/1 upgrade handoff plus HTTP/2 and HTTP/3 Extended CONNECT/tunnel ownership;
3. standard Rust ecosystem adapters (`http`, `http-body`, Tower) without replacing the native canonical API;
4. production listener ownership (prebound listeners, Unix sockets where supported, socket activation) rather than only address-driven startup/caller-owned individual streams;
5. trusted proxy metadata and optional HAProxy PROXY protocol handling without trusting spoofable headers implicitly;
6. production TLS identity selection, optional client authentication, reloadable identity state, and explicit handshake policy;
7. a real async Python bridge substrate sufficient to build ASGI-class servers without routing Python through the synchronous `http.server` compatibility path;
8. protocol-neutral application-server conformance across H1/H2/H3 and Python/Rust consumers;
9. API/module cleanup needed before declaring the runtime boundary stable.

## Standards and ecosystem inputs

Implementation must re-check current specifications and crate APIs when each child plan starts. The design baseline is:

- RFC 9110 HTTP semantics: message trailers are distinct from headers; one or more 1xx responses may precede a final response.
- RFC 8441: WebSockets over HTTP/2 use Extended CONNECT and remain one multiplexed stream, not a connection-wide HTTP/1 upgrade.
- RFC 9220: the Extended CONNECT mechanism is adapted to HTTP/3 with H3 stream cancellation/closure semantics.
- ASGI HTTP/WebSocket 2.5: useful qualification target for incremental bodies, disconnect events, WebSockets, and optional response trailers; ASGI-specific vocabulary remains downstream.
- Rust `http`/`http-body` plus Tower `Service`: ecosystem interoperability boundary; EggServe's native canonical types remain the security/correctness authority.
- HAProxy PROXY protocol v1/v2 and systemd socket activation: optional deployment adapters, never implicit trust mechanisms.

## Dependency graph

```text
196  Roadmap/scope gate
 |
 +--> 197  Stable application service contract and extension context
 |      |
 |      +--> 198  Trailers + interim responses
 |      +--> 199  Upgrade/tunnel/Extended CONNECT capability
 |      +--> 200  Rust http/http-body/Tower adapters
 |
 +--> 201  Listener and process-manager integration
 +--> 202  Trusted proxy + PROXY protocol metadata
 +--> 203  Production TLS identity/client-auth/reload
 |
 +--> 204  Async Python/ASGI-ready bridge (depends on 197–199 semantics)
 +--> 205  Request/transport observability extension points
 +--> 206  Maintenance/module/API cleanup after feature shapes settle
 +--> 207  Cross-protocol application-server contract qualification
 +--> 208  Release/API stabilization closure
```

Plans 201–203 may proceed in parallel after 197 identifies the final connection/request metadata contract. Plan 204 must not freeze Python event shapes before Plans 198–199 settle message and tunnel semantics. Plan 206 should happen after the feature-bearing APIs settle so refactoring does not fight active design changes.

## Global invariants

Every child plan must preserve these properties:

- Hyper, h2, h3, Quinn, rustls session internals, and raw sockets remain below the ordinary application-facing service API.
- EggServe remains the only authority for HTTP framing (`Content-Length`, transfer coding, H2/H3 end-stream semantics, body-forbidden statuses, and trailer placement).
- Request-body and response-body backpressure stays bounded; no unbounded queue may appear in Rust/Python adapters.
- Services may reduce but never increase server hard resource ceilings.
- Duplicate header/trailer order and legal opaque bytes are preserved where the underlying protocol permits them.
- Lifecycle cancellation is transport-neutral and wakes downstream consumers on disconnect, reset, timeout, forced drain, or fatal transport failure.
- H2/H3 stream failures do not unnecessarily terminate sibling streams; H1 connection safety remains conservative after framing/body failure.
- Proxy-derived metadata is never trusted merely because a forwarding header exists.
- Optional deployment/protocol features do not inflate the minimal H1 dependency graph without a documented reason.
- The synchronous Python `http.server` compatibility facade remains a compatibility product; it must not become the async application-server abstraction.

## Explicit non-goals

Do not add routing, middleware policy, dependency injection, framework loading, application worker/process supervision, ASGI lifespan ownership, WSGI thread pools, authentication/session systems, templates, ORM/database integration, reverse proxying, caching, ACME, WebSocket frame codecs, permessage-deflate, WebTransport, arbitrary QUIC application APIs, or a generic plugin system.

Server push remains out of scope unless a future concrete consumer justifies it. HTTP/2 and HTTP/3 support should focus on ordinary requests, trailers, and tunnel semantics rather than obsolete/low-value breadth.

## Workstream acceptance

This roadmap is complete only when:

- the native Rust service/request/response/lifecycle contract is stable enough for downstream application servers;
- trailers and interim responses are represented canonically and correctly mapped by H1/H2/H3;
- HTTP/1 upgrade and H2/H3 Extended CONNECT can hand a bounded duplex capability to a downstream protocol implementation without exposing transport internals;
- a Rust consumer can use `http`/`http-body` and Tower through optional adapters;
- prebound listeners/socket activation can feed the same runtime without a second accept loop;
- proxy-derived connection metadata has an explicit trust policy and optional PROXY protocol support;
- TLS supports production identity selection/client-auth/reload needs without leaking key material or weakening defaults;
- Python has an async, backpressured low-level bridge sufficient for a downstream ASGI implementation;
- one application-server contract suite runs the same semantic cases across supported H1/H2/H3 transports;
- the large implementation modules touched by this program are split by invariant ownership, not arbitrary line-count gates;
- documentation describes exactly which APIs/protocols are stable, experimental, and intentionally downstream-owned;
- no framework/application-server product has been smuggled into `eggserve-core`.

## Handoff

Implement Plan 197 first. Treat Plans 173–195 as historical prerequisites: do not rewrite their closure records. Where this roadmap intentionally reopens a previously deferred product decision (especially Plan 176 upgrades), reference that decision explicitly in the successor plan.