# Plan 201 — Listener Ownership and Process-Manager Integration

## Status

**PLANNED.** Prerequisite: Plan 197 request/connection contract. Independent of application message feature plans.

## Purpose

Make the server runtime usable as a production embedding substrate when socket/listener ownership belongs to a parent process, service manager, test harness, or downstream application server.

EggServe already supports address-driven startup and caller-owned individual streams. A full server foundation also needs first-class listener injection so embedders do not have to reproduce EggServe's accept/admission/TLS/lifecycle loop merely because they already own the listening socket.

This plan adds listener adapters, not a second accept loop.

## Target use cases

- downstream Rust application server pre-binds TCP sockets and hands them to EggServe;
- zero-downtime process manager passes inherited/listening descriptors;
- systemd socket activation on Linux;
- Unix-domain sockets for local reverse-proxy/application-server deployment;
- test harnesses bind port 0 with custom socket options before runtime startup;
- supervised processes choose `SO_REUSEPORT`/network namespace/socket policy externally;
- Python downstream runtime can eventually consume an inherited/prebound descriptor through Plan 204 without Python reimplementing HTTP transport.

## Track A — Define a listener abstraction

Introduce an internal/experimental accept-source abstraction that supplies accepted bidirectional transports plus trustworthy local/peer metadata.

Do not expose a broad generic trait until concrete implementations prove its necessary methods. The minimum semantic operations are:

- obtain bound/local address/identity where available;
- asynchronously accept a transport;
- surface peer/local metadata;
- stop accepting on shutdown;
- classify transient versus fatal accept errors;
- integrate with existing connection admission and backoff policy.

`Server` must continue owning connection admission, TLS handshake (when configured), protocol selection, runtime state, task tracking, graceful drain, and observability after accept.

## Track B — Prebound TCP listener API

Add a supported embedding path accepting `std::net::TcpListener` and/or `tokio::net::TcpListener` without rebinding.

Prefer accepting `std::net::TcpListener` at the public boundary if this reduces runtime coupling: validate/set nonblocking as required and convert internally. If accepting Tokio listener directly is useful, document runtime-handle requirements precisely.

Requirements:

- no duplicate bind;
- actual listener local address becomes server metadata/readiness output;
- socket options already set by the caller are not silently reset unless required for correctness;
- ownership transfer is explicit; if borrowed/shared ownership is supported, semantics must be clear;
- startup failure never closes a descriptor the caller still owns under the selected ownership model;
- same accept/admission/TLS/H1/H2 pipeline as address-bound server.

Do not implement a parallel `PreboundServer` runtime.

## Track C — Unix-domain listener support

On Unix, support `UnixListener` as an optional/local transport where the HTTP protocol drivers can operate over the stream.

Connection metadata must not fabricate IP socket addresses. Add/extend transport endpoint metadata with a non-IP endpoint representation only if downstream consumers need it; preserve existing `Option<SocketAddr>` truthfulness for callers relying on it.

Requirements:

- filesystem socket path creation/removal ownership is explicit;
- EggServe does not unlink arbitrary existing paths silently;
- abstract namespace sockets, if supported, are capability/platform documented;
- TLS over Unix sockets is not automatically enabled merely because TLS is configured for TCP; define explicit behavior;
- H3 is not available over Unix streams because it is QUIC/UDP; do not force protocol symmetry where it is false.

Windows named pipes are not required by this plan. A future transport adapter can reuse the same abstraction if justified.

## Track D — Socket activation / inherited descriptor adapter

Add a small optional adapter for systemd-style socket activation rather than a permanent dependency on a service-manager crate if standard-library/rustix descriptor adoption is straightforward.

Requirements:

- validate descriptor type/domain/listening state before adoption;
- support multiple inherited descriptors only through explicit selection/mapping; never silently take fd 3 because it exists;
- clear/handle inheritance environment state according to socket-activation convention;
- ownership transfer and close-on-exec behavior documented;
- reject datagram descriptors from the TCP/H1/H2 listener path;
- if an inherited UDP socket is later used for H3, treat that as an explicit H3 endpoint adapter rather than overloading TCP logic.

A lightweight helper such as `ListenerSource::from_systemd(...)` is acceptable. Do not add process supervision, notification/watchdog, service installation, or unit generation to core.

## Track E — Optional UDP/QUIC endpoint injection

Audit the H3 server path. A full embedding API should eventually permit a caller-owned/prebound UDP socket or Quinn-compatible endpoint without exposing Quinn types in the ordinary service contract.

If current Quinn APIs can adopt `std::net::UdpSocket` cleanly, provide an H3-specific endpoint constructor. Otherwise document the gap and defer rather than creating unsafe descriptor duplication.

Port-coordinated TCP+UDP startup for H1/H2/H3 must preserve current same-port semantics where requested. Prebound callers may provide explicit matching TCP/UDP sockets and should receive validation if the pair is inconsistent.

## Track F — Accept-loop hardening

Use the existing accept loop; audit production failure behavior while adding new listener sources:

- connection semaphore acquisition ordering should not let accepted sockets accumulate unboundedly;
- transient `EMFILE`/`ENFILE`/resource failures use bounded backoff and observability rather than a hot error loop;
- fatal listener errors transition server state deterministically;
- shutdown while admission is saturated wakes the accept loop promptly;
- accepted-but-not-dispatched transports are closed promptly on shutdown;
- TLS handshake concurrency/timeout remains bounded independently of accept rate.

Do not add an unbounded accepted-socket queue.

## Track G — Readiness and handle semantics

`ServerHandle` readiness should mean all configured listener/endpoint sources have been successfully adopted and protocol configuration is valid, not merely that a task was spawned.

Expose actual bound endpoints in a protocol-neutral way sufficient for tests/downstream servers. Preserve the simple existing `local_addr` path for the common one-TCP-listener case.

Multiple listeners, if enabled, should have stable IDs/names rather than positional assumptions in logs/metrics.

## Track H — Python projection

Plan 204 may expose prebound descriptors/listeners through a low-level Python API, but this plan should design Rust ownership so Python can transfer/duplicate an fd safely later.

Do not alter `eggserve.server.HTTPServer` compatibility constructor semantics here.

## Security review

Test:

- descriptor type confusion;
- adopting a connected socket as a listener;
- unexpected nonblocking/cloexec state;
- Unix socket path replacement races/unsafe unlink behavior;
- shutdown under accept/admission saturation;
- inherited listener with wildcard bind still obeys explicit exposure policy at the frontend that owns that policy;
- local/peer metadata cannot be spoofed by request headers;
- no fd leaks across failed startup/restart cycles.

## Verification

Add deterministic integration tests for address-bound and prebound parity, Unix sockets on supported platforms, inherited descriptor adoption on Linux, and H3 prebound endpoint behavior if implemented. Platform-specific activation tests can be focused/manual where CI runners lack systemd; descriptor-level unit/integration behavior must remain automated.

## Acceptance criteria

- [ ] caller can hand EggServe a prebound TCP listener and use the same runtime/Service pipeline as normal startup;
- [ ] listener injection does not duplicate the accept/connection driver;
- [ ] Unix-domain HTTP is supported on Unix with truthful endpoint metadata and documented cleanup ownership;
- [ ] Linux socket-activation descriptors can be validated/adopted without adding process supervision to core;
- [ ] accepted connection/resource queues remain bounded under saturation/failure;
- [ ] graceful shutdown wakes accept/admission waits and closes undispatched transports;
- [ ] H3 prebound UDP/endpoint injection is either implemented safely or explicitly documented as the remaining protocol-specific gap;
- [ ] minimal/default builds do not acquire unnecessary service-manager dependencies;
- [ ] Python `http.server` compatibility behavior remains unchanged.

## Handoff

Plans 202–203 may attach trusted transport/TLS metadata to connections created through these listener sources. They must not create alternate listener loops.