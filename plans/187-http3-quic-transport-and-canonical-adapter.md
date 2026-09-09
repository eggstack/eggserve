# Plan 187 — HTTP/3 QUIC Transport and Canonical Adapter

## Status

**PLANNED — native HTTP/3 implementation after the HTTP/2 phase; feature-gated QUIC/H3 dependencies.**

Prerequisites: Plans 183–186 complete, with the protocol-neutral request/lifecycle/configuration model stable enough to reuse. HTTP/3 must not fork static planning, service admission, request-target parsing, or application error semantics.

## Purpose

Add HTTP/3 as a second transport implementation sharing EggServe's canonical request/service/response substrate.

Unlike HTTP/2, this is not a Hyper connection-builder change. HTTP/3 runs HTTP semantics over QUIC/UDP and therefore requires:

- a UDP/QUIC endpoint alongside the existing TCP listener;
- TLS 1.3/QUIC configuration with `h3` ALPN;
- HTTP/3/QPACK request parsing and response emission;
- stream-oriented request-body and response-body adapters;
- stream reset/STOP_SENDING/cancellation behavior;
- QUIC/H3-specific flow-control, stream, header, handshake, idle, and memory limits;
- connection drain/GOAWAY semantics;
- runtime-owned H3 discovery advertisement where configured.

The `Service` trait and static service must remain unaware of Quinn/H3 types.

## Initial scope constraints

Supported initial H3 capability should be deliberately narrow:

- HTTP/3 request/response streams;
- GET/HEAD static serving and generic canonical Rust services;
- bounded request body Buffer/Stream policies;
- bounded streaming responses and files;
- conditional/range responses inherited from the static service;
- graceful connection drain;
- H3 discovery via runtime-owned `Alt-Svc` on TCP responses when enabled.

Explicitly out of scope for this plan:

- QUIC 0-RTT application requests;
- WebTransport;
- HTTP datagrams;
- extended CONNECT/tunneling;
- WebSocket-over-H3;
- server push;
- reverse proxying;
- arbitrary QUIC applications;
- connection migration as an application identity feature;
- new Python `http.server` protocol semantics.

## Transport-stack selection gate

At implementation time, re-evaluate the maintained Rust HTTP/3 ecosystem and select compatible audited versions. Preferred architecture is the Hyperium H3 API with a Quinn transport adapter (for example `h3` + `h3-quinn` + `quinn`) unless current maintenance/security evidence supports another stack.

Requirements for whichever stack is selected:

- maintained against current RFC 9114 behavior;
- supports server request streams and graceful shutdown/GOAWAY;
- exposes bounded configuration for H3 field/QPACK state and QUIC transport resources;
- works with Tokio without introducing a second async runtime;
- permits request and response streaming without buffering entire bodies;
- allows stream cancellation/reset and connection close to be distinguished;
- license and supply-chain policy pass `cargo deny`/audit;
- protocol types can remain internal.

Do not publish stable APIs containing `quinn`, `h3`, or adapter types.

## Track A — Feature graph and dependency isolation

### A1. Add H3 as an optional feature

Add a dedicated HTTP/3 feature (exact naming per current crate conventions) that activates only the required QUIC/H3/TLS dependencies.

A minimal HTTP/1 build must not pull Quinn/H3. If H2 remains separately feature-gated, H3 should not force H2 unless there is a measured architectural reason.

### A2. Rationalize TLS-common code

HTTP/3 always uses TLS 1.3 through QUIC, while existing direct HTTPS uses rustls over TCP. Share safe certificate/key parsing and validation where practical, but build separate protocol-appropriate TLS configs.

Do not reuse the existing TCP rustls `ServerConfig` blindly: its ALPN and protocol settings may be wrong for QUIC.

A useful internal layering is:

```text
PEM certificate/key loader
          |
          v
validated TLS identity/material (internal)
       /                  \
      v                    v
TCP rustls config      QUIC TLS config
h2/http1.1 ALPN        h3 ALPN, TLS 1.3
```

Do not expose private key material through the public API.

### A3. Programmatic configuration compatibility

The current experimental runtime can receive a prebuilt TCP rustls configuration. Decide explicitly how such callers enable H3:

- add a separate experimental H3/QUIC server config field/constructor; or
- require certificate/key identity through a higher-level builder that can construct both configs.

Do not attempt to reverse-engineer certificate/private-key material from an opaque user-supplied rustls config.

## Track B — Add a QUIC listener lifecycle beside TCP

### B1. Listener model

When H3 is enabled in the ordinary native server, run a Quinn UDP endpoint in addition to the existing TCP listener. Prefer the same numeric IP/port so an HTTPS origin has one conventional authority while TCP and UDP remain separate transport namespaces.

### B2. Port-zero semantics

Binding TCP and UDP independently to port `0` can yield different ephemeral ports and break origin discovery. Define deterministic startup:

1. bind/resolve the primary TCP listener first;
2. obtain its actual local port;
3. bind the H3 UDP endpoint to the same address/port;
4. fail startup atomically if the required UDP bind cannot succeed.

If a different explicit H3 port is supported, it must be represented explicitly and used in Alt-Svc advertisement.

### B3. Existing-listener semantics

For `ServerBuilder::from_listener(TcpListener)` with H3 enabled, either:

- bind the UDP endpoint to the supplied listener's resolved local IP/port; or
- require a new explicit QUIC-endpoint/listener input.

Choose one documented rule. Do not silently disable H3 because a TCP listener was supplied.

Caller-supplied Quinn endpoint ownership may be deferred from the initial implementation if adding it would substantially widen public transport APIs; if deferred, document the limitation.

### B4. Initial H3 coexistence policy

Initial native H3 support may require an accompanying TCP HTTP/1/H2 listener rather than supporting H3-only `Server` mode. This simplifies compatibility and discovery and keeps `ServerHandle::local_addr()` truthful.

If H3-only mode is added, redesign listener-address reporting explicitly rather than letting `local_addr()` return an arbitrary protocol address.

### B5. Shared server lifecycle

One `ServerHandle` shutdown should drain/close both TCP and QUIC listeners. Startup readiness is reached only after all configured listeners are ready. A partial TCP-success/QUIC-failure startup must unwind cleanly.

## Track C — QUIC connection admission and handshake policy

### C1. Active connection budget

Define how `max_connections` applies across TCP and QUIC. Preferred operator meaning: it bounds active accepted HTTP connections across all enabled protocols rather than creating independent hidden pools.

If separate pools are required for implementation reasons, expose/document them explicitly; do not silently double the configured connection budget when H3 is enabled.

### C2. Pending handshake budget

QUIC handshake work can consume resources before a connection becomes an active HTTP connection. Add an H3/QUIC-specific bound for pending handshakes or use a transport-native equivalent.

Inspect Quinn's current endpoint/incoming/retry APIs and choose a bounded anti-amplification/handshake policy. Stateless retry may be useful under untrusted public exposure but should be enabled only after measuring interoperability/latency implications.

### C3. Handshake timeout and TLS policy

Bound handshake establishment. H3 requires TLS 1.3 and ALPN `h3`; connections that do not negotiate a supported H3 application protocol must not enter the HTTP request loop.

### C4. Disable 0-RTT initially

Do not accept early application requests in the initial H3 implementation. Generic `Service` implementations may mutate application state, and EggServe cannot infer replay safety.

A future 0-RTT feature would require an explicit replay-safe service policy and separate plan.

## Track D — Add explicit QUIC/H3 resource configuration

Create `Http3Config`/`QuicConfig` ownership under the protocol configuration model. Pin EggServe-owned defaults and bounds rather than inheriting unstable/default transport values.

Inventory and decide at least:

### QUIC transport

- maximum concurrent incoming bidirectional streams;
- maximum concurrent incoming unidirectional streams (must leave room for required H3 control/QPACK streams);
- per-stream receive window;
- connection receive window;
- send window / buffered outgoing data;
- maximum idle timeout;
- keepalive policy if any;
- handshake timeout/pending handshake budget;
- maximum UDP payload/MTU behavior if operator-relevant;
- stateless retry policy if supported/selected.

### HTTP/3/QPACK

- maximum request field-section/header-list size;
- QPACK dynamic table capacity;
- blocked-stream limit;
- any implementation-specific retained-state/reset limits needed to resist abuse.

### Application/runtime

Continue to apply the shared EggServe limits:

- aggregate canonical header bytes;
- request-target bytes;
- request body bytes;
- handler timeout;
- body read timeout;
- response progress timeout;
- service admission;
- file-stream admission.

Do not multiply QUIC stream count by large receive windows without calculating worst-case memory exposure. Document the approximate configured upper-bound relationship.

## Track E — H3 request acceptance and canonical adaptation

### E1. Connection request loop

Use the selected H3 server API to accept request streams from each QUIC connection. Spawn/drive per-request work under bounded task/admission rules; do not allow every peer-created stream to become an unbounded Tokio task before transport limits apply.

### E2. Pseudo-field conversion

Translate H3 request metadata into the same canonical fields established for H2:

- `HttpVersion::Http3`;
- method;
- request target/path/query;
- scheme;
- authority;
- ordinary duplicate-preserving headers.

Do not expose literal `:method`, `:path`, `:scheme`, `:authority` fields to services.

### E3. Header validation

Apply H3/HTTP semantics for forbidden connection-specific fields. Do not run HTTP/1 Transfer-Encoding framing logic on H3.

Apply both transport/H3 field-section limits and EggServe's canonical post-decode aggregate header/target limits before service invocation.

### E4. Connection metadata

Populate `ConnectionInfo` from truthful transport knowledge:

- scheme is HTTPS;
- protocol version is H3 in the request head;
- local/remote socket addresses may be recorded as snapshots when available;
- TLS metadata reports TLS 1.3/SNI only when known through the QUIC stack.

QUIC peer addresses must not become a stable authentication identity. If migration can change the remote address, document the snapshot semantics and avoid using address changes to fabricate multiple application identities.

## Track F — Adapt H3 request bodies to canonical `RequestBody`

### F1. Pull/backpressure bridge

Convert H3 DATA reception into the existing one-shot bounded `RequestBody` stream abstraction without buffering whole request bodies unless `Buffer` policy requests it.

Transport flow control should naturally follow service/body consumption where possible.

### F2. Declared length

Validate `Content-Length` if present against configured limits and actual consumed body length. H3 has no HTTP/1 Transfer-Encoding framing.

### F3. Reject policy

For a rejected request body:

- produce the appropriate canonical error response if allowed;
- stop receiving/cancel the request stream with H3/QUIC stream control;
- do not terminate the entire QUIC connection unless the protocol state is connection-corrupt.

### F4. Deferred body lifecycle

Preserve downstream deferred streaming semantics per H3 request stream. Body timeout/cancellation must wake only that request lifecycle and stop/reset that stream unless a connection-level error occurs.

## Track G — Keep service execution protocol-neutral

Reuse the Plan 184 shared service kernel. The H3 request adapter should produce the same canonical `Request` and receive the same canonical service result/lifecycle disposition as H1/H2.

If the shared kernel still returns Hyper response types at this point, split it into:

1. canonical application execution/result normalization;
2. protocol-specific response encoding.

Do not copy service admission, panic containment, handler timeout, or error mapping into the H3 module.

## Track H — Make final response policy transport-neutral

The current H1/H2 path may apply privacy policy on a Hyper response. H3 cannot depend on Hyper response types merely to share `Date`/`Server`/denylist behavior.

Extract or reuse a canonical final-response-policy step that can be consumed by both:

- Hyper H1/H2 response conversion; and
- H3 response emission.

Preserve exactly one authority for:

- `Server` suppression/fixed value;
- `Date` generation/suppression;
- stripped response headers;
- `Last-Modified <= Date` invariant;
- hop-by-hop/framing/header validation;
- generic runtime error representation.

Add cross-protocol golden tests proving equivalent canonical responses yield equivalent semantic headers on H1/H2/H3 subject only to protocol-forbidden framing differences.

## Track I — H3 response streaming and per-stream progress

### I1. Response head

Translate canonical status/headers into H3 response fields. Do not emit connection-specific fields or `Transfer-Encoding`.

### I2. Buffered/file/stream bodies

Support:

- empty responses;
- buffered bytes;
- file-backed bodies under the existing file-stream semaphore;
- known-length response streams;
- unknown-length response streams.

The H3 adapter should pull canonical body chunks and send H3 DATA under QUIC flow-control backpressure.

### I3. Per-stream write/no-progress timeout

H3 send operations are stream-specific. Use successful stream-level send progress to implement the existing response no-progress policy accurately.

A client that stops extending one stream's flow-control window must not be kept alive by traffic on sibling streams.

On timeout, reset/terminate only the affected stream and cancel its lifecycle unless the transport reports a connection-level failure.

### I4. Producer failures after commitment

If a canonical stream errors after response headers are sent, stop/reset the H3 response stream and emit a sanitized event. Do not attempt to send a second status response.

## Track J — H3 connection drain and shutdown

### J1. Graceful GOAWAY

On server shutdown or max-request drain policy, send H3 GOAWAY through the H3 implementation where supported, stop accepting new request streams, and allow already accepted work to complete within the configured grace.

### J2. Request threshold semantics

Map `max_requests_per_connection` to H3 connection drain rather than any connection header. Define concurrent stream races exactly as for H2.

### J3. Forced closure

After the graceful deadline:

- cancel all remaining request lifecycles for that QUIC connection;
- reset/stop request streams as appropriate;
- close the QUIC connection with a non-sensitive application/protocol reason;
- release connection/service/file/task admission exactly once.

## Track K — Runtime-owned H3 discovery advertisement

### K1. Alt-Svc on TCP responses

When a working H3 listener is active and advertisement is enabled, add an EggServe-owned `Alt-Svc` value to HTTP/1.1/H2 responses identifying the H3 endpoint.

Do not ask application services to inject the advertisement manually.

### K2. Use actual bound port

If the server bound port `0` or a distinct H3 port, construct advertisement from the actual active H3 endpoint, not requested configuration.

### K3. Privacy/operator policy

Make advertisement policy explicit. A privacy/minimal-fingerprint profile may suppress it even when direct H3 is active. Application-provided `Alt-Svc` must remain subordinate to the runtime-owned transport advertisement policy if EggServe claims sole authority.

### K4. No DNS automation

HTTPS/SVCB DNS record management is deployment guidance, not an EggServe capability.

## Track L — Observability

Add bounded protocol events/counters only where useful:

- QUIC handshake success/failure/timeout;
- H3 connection accepted/closed;
- negotiated H3 version/ALPN;
- request stream accepted/reset/cancelled;
- stream-limit/admission rejection;
- H3/QUIC connection drain;
- response stream timeout;
- Alt-Svc advertisement state only as startup/config summary, not per-response log spam.

Never log QUIC connection IDs, TLS secrets, tokens, raw packet contents, or untrusted request values by default.

## Track M — Static service and downstream reuse

Run the existing static `Service` and a generic downstream `Service` over H3 without H3-specific application code.

Required functional cases:

- GET/HEAD;
- index redirect;
- directory policy;
- conditional requests;
- range 206/416;
- large file streaming;
- file-stream admission;
- request-body rejection;
- generic buffered/streaming application response;
- privacy/error policy.

No H3-specific static planner is permitted.

## Verification

Exact commands depend on selected feature names and H3 stack. Expected repository checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
cargo audit
cargo deny check
bash scripts/test-python-wheel.sh
```

Also prove the minimal feature graph does not include QUIC/H3 dependencies when H3 is disabled (`cargo tree` or an existing dependency-policy assertion).

## Required deterministic test classes

- TCP+UDP same-port startup and atomic failure cleanup.
- port-0 binding resolves one usable origin port.
- TLS 1.3 + `h3` ALPN success/failure.
- 0-RTT not accepted.
- QUIC handshake timeout/pending-handshake admission.
- H3 request metadata/pseudo-field conversion.
- forbidden connection-header handling.
- header/target/field-section limits.
- per-connection bidi/uni stream limits.
- request body Reject/Buffer/Stream.
- sibling-stream isolation after body rejection/timeout/reset.
- global service/file-stream admission across H1/H2/H3.
- H3 buffered/file/known-stream/unknown-stream responses.
- per-stream response flow-control timeout.
- response producer failure after commitment.
- GOAWAY/graceful shutdown/max-request drain.
- Alt-Svc uses actual active endpoint and respects suppression policy.
- H1/H2 regressions after canonical response-policy extraction.
- feature-disabled build has no QUIC/H3 graph.

Plan 188 owns independent-client interoperability, adversarial QUIC qualification, performance/resource measurements, platform evidence, and final support-tier declaration.

## Acceptance criteria

- [ ] H3/QUIC dependencies are optional, audited, and absent from the minimal build when disabled.
- [ ] one native server lifecycle can own the configured TCP listener and H3 UDP endpoint with atomic startup/readiness/shutdown.
- [ ] port-zero and existing-listener semantics are deterministic and documented.
- [ ] QUIC uses TLS 1.3 and `h3` ALPN; 0-RTT application requests are disabled.
- [ ] active connection and pending handshake resources are explicitly bounded.
- [ ] QUIC stream/window and H3/QPACK field-state defaults are EggServe-owned and validated.
- [ ] H3 pseudo-fields map into the same canonical request metadata used by H2; no pseudo-header leakage occurs.
- [ ] request bodies reuse canonical bounded body/lifecycle semantics and ordinary stream rejection does not kill sibling streams.
- [ ] service admission/invocation/panic/timeout/error handling is shared with H1/H2, not copied into H3.
- [ ] response privacy/finalization is transport-neutral and semantically consistent across H1/H2/H3.
- [ ] buffered, file, and streaming canonical responses emit correctly over H3 with per-stream flow-control backpressure.
- [ ] response no-progress timeout is truly stream-specific.
- [ ] graceful shutdown/request thresholds use H3 GOAWAY/drain semantics.
- [ ] Alt-Svc advertisement, when enabled, is runtime-owned, uses the actual H3 endpoint, and can be suppressed by policy.
- [ ] static and generic Rust services need no H3-specific application implementation.
- [ ] no WebTransport, datagrams, extended CONNECT, WebSocket, server push, reverse proxy, 0-RTT application semantics, or DNS automation is added.

## Suggested implementation order

1. Re-evaluate/select the maintained H3/QUIC stack and feature graph; run license/audit checks.
2. Extract shared TLS identity parsing needed to build separate TCP and QUIC configs.
3. Add dual TCP/UDP listener lifecycle with deterministic port semantics.
4. Add bounded QUIC handshake/connection/stream/QPACK configuration.
5. Implement H3 request acceptance and canonical metadata/body adaptation.
6. Ensure the shared service kernel yields a protocol-neutral canonical result.
7. Move final response privacy policy to a transport-neutral boundary if still Hyper-specific.
8. Implement H3 response/file/stream emission with stream-level progress timeout.
9. Map lifecycle dispositions to H3 reset/STOP_SENDING/GOAWAY/drain behavior.
10. Add runtime-owned Alt-Svc based on the actual active endpoint.
11. Run all deterministic H1/H2/H3/TLS/Python/dependency tests.
12. Proceed to Plan 188 for external interoperability and release qualification.

## Handoff

Plan 187 is complete when H3 is functionally implemented behind an optional feature with deterministic resource/lifecycle tests. It must still be described as experimental until Plan 188 demonstrates independent-client interoperability, adversarial QUIC robustness, platform behavior, and acceptable footprint.