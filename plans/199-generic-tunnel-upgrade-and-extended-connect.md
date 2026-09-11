# Plan 199 — Generic Tunnel, HTTP/1 Upgrade, and Extended CONNECT

## Status

**IMPLEMENTED / CLOSED.**

Prerequisite: Plans 197–198 settled (contract + trailers/interim). Supersedes deferred Plan 176 (linked, unchanged).

## Closure record

Implemented on `main`: canonical `tunnel.rs` (`TunnelKind` non-exhaustive `Http1Upgrade`/`Connect`/`ExtendedConnect`, `ProtocolName` token 1–64 generic, `TunnelRequest`, `TunnelError`, `TunnelIo` 32 KiB duplex `AsyncRead+Write`, one-shot `TunnelCapability::accept(headers, handler)` returning handshake `Response` with crate-private token + `AfterCommit`/`AlreadyAccepted`/`NoCapability`, H1 strict `Connection`/`Upgrade` classification, `classify_extended_protocol` generic); `Response::is_tunnel()` + `take_tunnel_acceptance()` (mutation via `head_mut`/`take_body` drops tunnel → safe denial); `RequestContext::take_tunnel()`/`tunnel_request()` sharing one slot (second take `None`) + `tunnel_shared` commitment; `RuntimeConfig::max_active_tunnels` (default 64, `>0`, `<= MAX_PERMITS`, builder + `SharedRuntimeValues` kernel + `RuntimeState::tunnel_semaphore`); ops `TunnelAccepted`/`TunnelRejected`/`TunnelClosed`/`TunnelUpgradeFailed` + `tunnels_accepted`/`tunnels_rejected`/`active_tunnels`/`tunnel_upgrade_failures`; H1 `.with_upgrades()` (read-ahead preserved) + H2 `enable_connect_protocol()` + H3 `enable_extended_connect(true)`; H1/H2 pipeline classification (no body → no smuggle, `OnUpgrade` required, H2 `:protocol` present-but-invalid → no fallback, CONNECT authority-form allowed with placeholder `/` + Host consistency + target-length bound) + `invoke_service` commitment/admission/spawning via `connection::tunnel::admit_and_spawn_h1_h2` (503 on exhaustion, `copy_bidirectional` bounded, lifecycle-aware, no payload logged, permit+gauge exactly once); driver drains tunnels (idle excludes tunnels, total outer bound, shutdown budget, no detached survivors); H3 `handle_h3_connect` (plain `CONNECT` + `webtransport`/`connect-udp` Extended, no probe so early DATA preserved, `200` no `101`/body/`FIN` yet, directional 16 KiB tasks + half-close, stream-local, siblings survive; generic `:protocol` e.g. `websocket` blocked by `h3` 0.0.8 pre-resolution, documented); `tunnel_upgrade.rs` (H1 echo+readahead/denial/malformed/body/after-commit/budget-recover/shutdown/no-log, H2 Extended echo+sibling, H3 CONNECT echo+sibling, `tokio-tungstenite` WS interop dev-only, no WS in core) + `tunnel.rs` unit (token/bounds/H1 strict/generic/echo/commit/framing). Docs updated (`README`, `AGENTS`, skill, `non-goals`, `downstream-app-server`, `http-primitives`, capability matrix, `timeout-reference`, `configuration`, `ops-logging`, `overview`, `runtime`, `http2`, `http3`, `eggserve-core`, `error-taxonomy`). Verification: `cargo fmt`, workspace clippy/tests, `http2,tls` + `http3,tls` matrices green locally before push (see Handoff). Python facade unchanged (Plan 204 owns async projection).

## Product decision

Plan 176 was correctly deferred because no concrete upgraded-protocol consumer existed. Plan 196 changes the product target: EggServe should be a full HTTP server foundation capable of supporting downstream WebSocket/application servers. A generic tunnel handoff is therefore now required.

This plan supersedes Plan 176's deferral decision while retaining its core design constraints. It expands the problem beyond HTTP/1 Upgrade because HTTP/2 and HTTP/3 WebSockets/tunnels use Extended CONNECT on a single multiplexed stream rather than converting the entire transport connection.

EggServe still does **not** implement WebSocket framing, ping/pong, fragmentation, close codes, permessage-deflate, ASGI `websocket.*` events, SOCKS, CONNECT proxy policy, or arbitrary application tunneling policy. It provides a safe, generic duplex capability after a validated HTTP transition.

## Standards constraints

- HTTP/1 Upgrade transitions the connection after a validated 101 handshake; buffered post-handshake bytes must not be lost.
- RFC 8441 defines Extended CONNECT for HTTP/2. A successful tunnel is one H2 stream and remains subject to that stream's flow control/cancellation while sibling streams continue normally.
- RFC 9220 applies Extended CONNECT to HTTP/3; orderly/error closure maps to the H3 request stream rather than a whole QUIC connection.
- CONNECT/Extended CONNECT authority/protocol metadata must be validated separately from ordinary origin-form requests.

The canonical API must represent the common semantic concept—an accepted duplex stream—without pretending H1, H2, and H3 have the same wire transition.

## Track A — Phase-zero capability audit

Re-run Plan 176's Hyper upgrade spike against current dependencies and add H2/H3 capability audits.

Record:

### H1
- current Hyper `OnUpgrade` behavior;
- whether `.with_upgrades()` must be restored and at which driver boundary;
- preservation of read-ahead bytes;
- TLS and caller-owned IO behavior;
- driver completion/accounting after handoff.

### H2
- current Hyper/h2 support for Extended CONNECT and `:protocol` metadata;
- required SETTINGS_ENABLE_CONNECT_PROTOCOL configuration;
- how to obtain a duplex stream abstraction without exposing h2 internals;
- flow-control/backpressure and stream reset APIs.

### H3
- current `h3`/`h3-quinn` support for Extended CONNECT/`:protocol` and settings;
- request-stream send/receive split ownership;
- FIN/reset/STOP_SENDING mapping and shutdown behavior.

If a dependency cannot safely expose a required primitive, document the blocked protocol capability rather than bypassing it with raw wire code.

## Track B — Canonical tunnel request metadata

Represent validated tunnel intent separately from raw headers/pseudo-headers.

A conceptual type:

```rust
pub struct TunnelRequest {
    kind: TunnelKind,
    protocol: Option<ProtocolName>,
    authority: Option<Authority>,
}

#[non_exhaustive]
pub enum TunnelKind {
    Http1Upgrade,
    Connect,
    ExtendedConnect,
}
```

Exact naming may differ. Requirements:

- H2/H3 pseudo-headers never appear as ordinary application headers;
- protocol token bytes are validated and bounded;
- an ordinary request cannot fabricate a transport-backed tunnel capability by constructing headers;
- WebSocket-specific handshake fields remain ordinary canonical headers for the downstream codec/adapter to validate;
- trusted transport facts and tunnel request syntax remain distinguishable.

Do not hard-code a `WebSocket` enum variant as the only protocol; the capability must remain generic.

## Track C — One-shot acceptance capability

Attach a non-cloneable, one-shot tunnel capability to the request context from Plan 197.

Possible API:

```rust
if let Some(tunnel) = request.context().take_tunnel() {
    return Ok(tunnel.accept(response_metadata, handler));
}
```

or an explicit `ServiceOutcome::Tunnel` if type safety requires it.

Requirements:

- ordinary request/context clones do not clone ownership;
- accepting twice is impossible or deterministic error;
- dropping/ignoring the capability uses a normal HTTP denial/final-response path safely;
- capability becomes unusable after final response commitment;
- the runtime, not the application, writes transition/framing bytes;
- accepting a tunnel does not expose the original raw socket/QUIC connection.

## Track D — Protocol-neutral `TunnelIo`

Expose an EggServe-owned duplex abstraction suitable for downstream protocol codecs.

Preferred contract:

```rust
pub trait TunnelIo: AsyncRead + AsyncWrite + Unpin + Send { ... }
```

or a concrete opaque type implementing Tokio `AsyncRead`/`AsyncWrite` where all supported protocol streams can be adapted truthfully.

If H2/H3 flow-controlled streams cannot correctly implement byte-oriented Tokio IO without semantic loss, use a small EggServe-owned async read/write trait instead of forcing the wrong abstraction. The public type must not name Hyper/h2/h3/Quinn types.

Requirements:

- bounded backpressure;
- single-owner by default; splitting requires an explicit supported operation;
- H1 read-ahead bytes preserved;
- H2/H3 stream flow control respected;
- shutdown of one H2/H3 tunnel affects only its stream unless the transport itself fails;
- lifecycle cancellation can wake idle tunnel tasks;
- no implicit buffering proportional to attacker input.

## Track E — Handshake/outcome validation

### HTTP/1

Runtime validates the transition outcome and narrowly permits the required hop-by-hop handshake fields only on the explicit accepted-upgrade path. Ordinary response normalization continues stripping/rejecting them.

The service must not obtain a raw writer for the 101 response.

### HTTP/2 and HTTP/3

Extended CONNECT success is a normal successful response head followed by bidirectional stream data. Do not synthesize 101. Validate that response framing/body rules match protocol semantics and that `:protocol` support was actually negotiated/advertised before accepting.

### Denial

A service may return an ordinary canonical HTTP response instead. The unused capability is dropped safely; request-body and stream/connection reuse semantics remain normal.

## Track F — Lifetime, admission, timeout, and shutdown policy

Define tunnel accounting explicitly:

- H1 tunnel continues counting against the owning connection permit;
- H2/H3 tunnels continue counting against protocol stream/connection resource limits;
- normal pre-response `max_in_flight_requests` permit releases after tunnel acceptance unless implementation requires a narrower transition guard;
- a separate configurable `max_active_tunnels` server-wide budget should be considered to prevent long-lived tunnels from bypassing application admission entirely;
- ordinary HTTP response-write timeout does not apply after tunnel transition;
- hard connection/stream lifetime policy, if enabled, remains an outer bound;
- downstream protocol implementation owns protocol heartbeat/idle semantics unless EggServe adds an explicitly generic tunnel-idle timeout;
- graceful shutdown stops new tunnel admission, signals active tunnel lifecycles, waits within the existing drain deadline, then aborts remaining streams/connections.

No detached tunnel task may survive `ServerHandle::wait()`/forced shutdown without explicit caller ownership semantics.

## Track G — Tunnel lifecycle/cancellation

Expose a generic cancellation observer associated with the same connection/request lifecycle source.

Distinguish at least:

- peer closed normally;
- peer/reset/transport failure;
- server shutdown;
- configured hard timeout;
- local tunnel close/drop.

Do not expose H2/H3 numeric reset codes through the ordinary API unless a concrete downstream protocol requires them; keep protocol detail available to observability internally.

## Track H — Security hardening

Test explicitly:

- malformed/duplicate Connection/Upgrade tokens cannot produce H1 tunnel capability;
- smuggled CL/TE/body bytes cannot cross the transition incorrectly;
- H1 read-ahead boundary is byte-exact;
- capability cannot be accepted after final response commitment;
- capability cannot be accepted twice;
- ordinary responses cannot emit Upgrade/Connection fields through the tunnel exception path;
- unsupported H2/H3 Extended CONNECT protocol is rejected without affecting siblings;
- tunnel count/resource limits recover exactly once on reset/drop;
- shutdown cannot leave untracked raw transports alive;
- no tunnel payload bytes are logged by default;
- tunnel metadata lengths are bounded before allocation/service dispatch.

Fuzz handshake classification/token parsing where EggServe performs parsing beyond the protocol library.

## Track I — Qualification consumers

Add two test-only consumers:

1. a generic byte echo/tunnel protocol proving the EggServe abstraction without a WebSocket dependency;
2. an optional dev-only WebSocket interoperability fixture using a maintained WebSocket library, purely to prove that the generic handoff is sufficient.

Exercise H1, H2 Extended CONNECT, and H3 Extended CONNECT wherever dependencies expose the required mechanism. WebSocket codec logic must remain outside production EggServe modules.

## Documentation

Update `docs/downstream-app-server.md`, capability matrix, non-goals, H2/H3 docs, timeout reference, and lifecycle documentation. Historical Plan 176 remains unchanged and should be linked as the prior deferral.

## Acceptance criteria

- [ ] a service can receive a genuine one-shot transport-backed tunnel capability without importing Hyper/h2/h3/Quinn;
- [ ] H1 accepted Upgrade performs a validated transition and preserves buffered bytes;
- [ ] H2/H3 Extended CONNECT uses one stream and does not convert/own the whole connection;
- [ ] ordinary denial remains an ordinary canonical HTTP response;
- [ ] runtime framing/handshake authority cannot be bypassed by application headers;
- [ ] tunnel IO is backpressured, bounded, cancellation-aware, and single-owner by default;
- [ ] active tunnel resources are bounded and released exactly once;
- [ ] graceful/forced shutdown accounts for tunnels;
- [ ] sibling multiplexed streams survive one tunnel reset/failure where protocol state permits;
- [ ] a real downstream WebSocket fixture works without adding WebSocket semantics/dependencies to production core;
- [ ] the synchronous Python compatibility facade remains unchanged; Plan 204 owns async Python projection.

## Handoff

Plan 204 consumes this generic capability to implement an ASGI-ready downstream bridge. Do not add ASGI event names or WebSocket framing here.