# Plan 185 — HTTP/2 Runtime, TLS, and Multiplexing

## Status

**PLANNED — native HTTP/2 implementation after Plan 184; Rust runtime/CLI/static first, Python compatibility facade remains HTTP/1.1-shaped.**

Prerequisites: Plans 183 and 184 complete. Do not begin by merely turning on Hyper's `http2` feature against the old HTTP/1 lifecycle pipeline.

## Purpose

Add native HTTP/2 support to EggServe's existing TCP/TLS runtime while preserving the same canonical `Service` boundary, static service, response privacy policy, admission budgets, and HTTP/1 behavior.

The implementation should use the existing Hyper/Hyper-Util family for HTTP/2 and should converge HTTP/1 and HTTP/2 on one byte-stream connection entry point where practical. It must explicitly own H2 resource limits and multiplexed lifecycle semantics rather than inheriting changing library defaults.

Initial HTTP/2 scope:

- HTTPS H2 negotiated with ALPN `h2`, falling back to HTTP/1.1 when offered/allowed;
- cleartext HTTP/2 prior knowledge if the selected Hyper-Util auto driver supports it predictably and tests prove behavior;
- no obsolete HTTP/1 `Upgrade: h2c` path;
- ordinary request/response streaming;
- built-in static service;
- generic Rust `Service` consumers;
- graceful connection drain/GOAWAY through the underlying H2 driver;
- bounded stream, header, body, service, file-stream, and flow-control resources.

Not in scope:

- server push;
- extended CONNECT/WebSocket over H2;
- generic upgrade handoff;
- reverse proxying;
- application routing/middleware;
- new Python `http.server` protocol semantics.

## Current baseline assumptions

Plan 184 should have established:

- one request-target parser;
- canonical `HttpVersion::Http2` capability without wire enablement;
- canonical authority/scheme semantics;
- one service invocation kernel;
- protocol-neutral lifecycle dispositions;
- connection versus request/response activity ownership;
- protocol-specific runtime configuration ownership;
- a non-upgradeable HTTP/1 driver.

If those assumptions are false at implementation time, stop and close Plan 184 first.

## Track A — Enable the Hyper HTTP/2 substrate deliberately

### A1. Dependency/feature changes

Enable the HTTP/2 server features required by the current compatible Hyper and Hyper-Util releases. Keep versions within the project's existing semver/dependency policy and run the supply-chain checks after lockfile changes.

Do not add an independent `h2` protocol implementation unless Hyper itself requires it transitively. EggServe should not maintain two H2 stacks.

### A2. Use an H1/H2-capable connection builder

Preferred architecture: replace the HTTP/1-only connection construction for TCP/TLS byte streams with Hyper-Util's H1/H2 auto server connection builder using Tokio executor/runtime adapters.

The target shape is:

```text
TCP / caller-owned AsyncRead+AsyncWrite
        |
        +-- optional rustls TLS + ALPN
        |
        v
Hyper-Util auto H1/H2 driver
        |
        v
canonical Hyper-to-EggServe adapter
        |
        v
shared Service kernel
```

If the auto builder cannot preserve one of EggServe's explicit H1 parser policies, keep separate internal H1/H2 builders behind one protocol-selection facade rather than weakening the H1 contract.

### A3. Preserve caller-owned byte-stream support

Generalize the current `serve_http1_connection` architecture carefully. Do not silently change its semantics/name in a patch release.

Under the Plan 183 minor transition, add an appropriately named multi-protocol connection driver (for example `serve_http_connection`) while preserving/deprecating `serve_http1_connection` as a strict H1 entry point if downstream callers depend on that guarantee.

Caller-owned arbitrary byte streams can support H2 prior knowledge, but they do not automatically gain TLS/ALPN unless the caller supplies a TLS-terminated stream/context.

## Track B — TLS ALPN and protocol selection

### B1. Stop hard-coding one ALPN list

`load_tls_config()` currently sets only `http/1.1`. Refactor TLS configuration construction so ALPN advertisement is derived from enabled runtime protocol policy.

When H2 and H1 are enabled for a TLS listener, advertise in preference order:

```text
h2
http/1.1
```

When H2 is disabled, preserve HTTP/1.1-only behavior.

### B2. Verify negotiated protocol

After TLS handshake, record the negotiated ALPN protocol in transport/protocol context and use it to select/verify the H2/H1 driver mode. Do not accept a mismatch between ALPN and the actual protocol state machine.

Extend TLS metadata only if necessary; avoid exposing raw rustls types.

### B3. Cleartext HTTP/2 policy

Support prior-knowledge H2 only if behavior is deterministic under the chosen builder and can be bounded by the same admission/timeout controls. Document it separately from TLS H2.

Explicitly reject/ignore attempts to negotiate H2 through HTTP/1 `Upgrade` headers. Plan 184 removed generic upgrade support intentionally.

### B4. Protocol observability

Emit/record the negotiated protocol once per connection. Do not log client-supplied pseudo-header values or raw authority/paths as part of protocol negotiation events.

## Track C — Add HTTP/2-specific runtime configuration

Define `Http2Config` or the equivalent protocol-owned configuration namespace. Pin EggServe-owned defaults explicitly rather than relying on Hyper defaults documented as unstable.

At minimum inventory and decide defaults/bounds for:

- maximum concurrent H2 streams per connection;
- maximum decoded header-list size;
- maximum frame size if exposed/needed;
- initial per-stream receive window;
- initial connection receive window;
- maximum send buffer size / pending send data;
- keepalive ping interval/timeout if enabled at all;
- maximum local error/reset stream state needed to resist reset floods;
- adaptive window behavior (prefer deterministic fixed limits initially unless evidence justifies adaptive mode).

### C1. Keep transport and application admission distinct

`max_concurrent_streams` is a per-H2-connection transport limit. `max_in_flight_requests` remains the server-wide limit on active `Service::call()` executions. Both apply.

A single H2 client must not be able to reserve all application permits by opening idle streams that have not reached service invocation.

### C2. Keep post-decode EggServe limits

Continue to enforce EggServe's canonical aggregate header-byte and request-target limits before service work, even when Hyper's H2 header-list limit already bounds compressed/decoded header handling. The layers protect different resources and preserve cross-protocol application semantics.

### C3. Validate configuration once

Add H2-specific validation to the protocol config owner and compose it with Plan 179's shared runtime validation. Do not duplicate shared handler/body/admission bounds.

## Track D — Convert HTTP/2 request metadata correctly

### D1. Version and pseudo-fields

Map Hyper HTTP/2 requests to canonical:

- `HttpVersion::Http2`;
- method;
- canonical request target/path/query;
- effective scheme;
- effective authority;
- ordinary duplicate-preserving headers.

Pseudo-header semantics must not appear in `HeaderBlock` as ordinary names.

### D2. Authority and scheme validation

Reject malformed or conflicting H2 authority/scheme metadata according to RFC 9113/RFC 9110 and the canonical policy established by Plan 184.

Do not trust forwarded headers as a replacement for transport-known scheme/authority.

### D3. H2-specific forbidden connection fields

HTTP/2 does not use HTTP/1 connection-specific fields. Ensure requests containing protocol-forbidden connection headers are rejected at the correct layer or are rejected by Hyper before service invocation.

Audit at least:

- `connection`;
- `keep-alive`;
- `proxy-connection`;
- `transfer-encoding`;
- `upgrade`;
- `te` except the protocol-permitted `trailers` value if EggServe elects to accept it.

EggServe currently does not support trailers, so the simplest hardened initial policy may reject `te`/trailers entirely after confirming standards/interoperability consequences.

Do not run HTTP/1 TE+CL framing logic blindly against H2. H2 framing validation must be protocol-specific.

## Track E — Request-body policy over independent streams

### E1. Preserve service body policy

`Reject`, `Buffer`, and `Stream` remain the application-facing body policies. H2 DATA frames are adapted into the same bounded `RequestBody` abstraction.

Declared content length, if present, remains application metadata to validate against actual consumed length/limits; H2 does not use HTTP/1 Transfer-Encoding framing.

### E2. Rejected/incomplete body behavior

When a service/runtime refuses an H2 request body, do not close the whole connection merely because unread data remains on that stream.

Use the protocol-neutral disposition from Plan 184 to:

1. emit the appropriate error response if still legal;
2. cancel/reset the inbound request stream as required;
3. keep unrelated streams usable unless the error is connection-scoped.

Prove this with concurrent-stream tests.

### E3. Deferred body lifecycle

The current downstream-app-server substrate permits a streaming request body to outlive `Service::call()` return under controlled lifecycle rules. Preserve that capability per stream.

A timeout/cancel on one deferred H2 body must wake that request's lifecycle waiters and terminate/reset that stream without cancelling every other request.

## Track F — Multiplexed response progress and timeout semantics

### F1. Do not reuse aggregate TCP-write progress as per-stream truth

A busy H2 connection may continue writing data for stream B while stream A is flow-control blocked. Aggregate socket progress cannot satisfy stream A's response no-progress budget.

Implement a per-response-stream progress signal suitable for H2. Candidate signals include successful body-frame demand/consumption by the H2 transport and explicit adapter callbacks. Validate the chosen signal under real flow-control stall tests.

Do not call a producer "making progress" merely because another stream writes bytes.

### F2. Bound a stalled response stream

If a response stream makes no transport-relevant progress for `response_write_timeout`, terminate/reset that stream and cancel its request lifecycle. Do not kill the connection unless the H2 library cannot safely recover or a connection-level invariant fails.

If Hyper's public API cannot provide a safe stream-specific reset from the response body error path, document the limitation and choose a conservative bounded alternative before declaring H2 hardened. Do not silently retain the incorrect aggregate timeout.

### F3. Connection-level idle versus stream-level activity

Define H2 connection idle as no active/request/deferred/response streams and no meaningful connection activity for the configured idle period.

Do not fire keep-alive idle while any stream remains legitimately active.

### F4. Hard connection lifetime

Re-evaluate `connection_total_timeout` for multiplexed connections. A fixed 60-second hard lifetime may remain a defense-in-depth option, but it should not be accidentally inherited as an undocumented H2 policy.

Choose and document one of:

- retain the hard lifetime for all protocols;
- provide a protocol-specific H2 lifetime/default;
- allow an explicitly bounded/disabled lifetime under a validated setting.

Any change to shared semantics must be deliberate and covered by migration documentation.

## Track G — H2 response normalization and forbidden fields

### G1. Preserve canonical response construction

Services continue to return the same canonical `Response`. Do not add an H2-specific response type.

### G2. Protocol-specific finalization

Before handing a response to the H2 driver:

- remove/reject connection-specific fields;
- do not emit `Transfer-Encoding`;
- retain valid `Content-Length` semantics where canonical length is known;
- preserve duplicate end-to-end headers such as `Set-Cookie`;
- preserve `HEAD`, 1xx/204/205/304 body suppression rules;
- apply runtime `Date`/`Server`/denylist privacy policy exactly once.

The canonical unknown-length stream must map to H2 DATA framing without inventing `Content-Length: 0`.

### G3. Error mapping after response commitment

A response-body producer failure after H2 headers are committed cannot be converted into a second 500 response. Reset/terminate only the affected stream and record a sanitized internal event.

## Track H — Connection draining, request thresholds, and shutdown

### H1. Graceful server shutdown

Map server/caller shutdown to H2 graceful shutdown/GOAWAY semantics through Hyper/Hyper-Util, then allow accepted in-flight streams to drain within EggServe's bounded shutdown grace.

After the drain deadline, cancel remaining stream lifecycles and close the connection.

### H2. Max requests per connection

The existing H1 behavior inserts `Connection: close` on the response that reaches `max_requests_per_connection`.

For H2, map the same operator policy to connection drain/GOAWAY: stop accepting new streams at/after the threshold while allowing already accepted streams to complete within the drain budget.

Define race behavior for concurrently arriving streams around the threshold and test it deterministically.

### H3. Connection-scoped protocol errors

Only errors that violate H2 connection state or make continued decoding unsafe should terminate the entire connection. Ordinary application rejection, service timeout, request-body refusal, and response producer failure should normally remain stream-scoped.

## Track I — Static service and file streaming

Run the existing static service unchanged through H2 and prove:

- GET;
- HEAD;
- index redirect;
- directory policy;
- ETag/Last-Modified conditionals;
- single-range 206/416 behavior;
- large file streaming;
- known and unknown-length canonical streams;
- file-stream admission exhaustion;
- response privacy policy.

No static planner fork is permitted for H2.

## Track J — Public/runtime API surface

### J1. Protocol selection policy

Expose only the minimum experimental Rust configuration required to enable/disable H2 and tune hardened H2 limits.

Do not expose Hyper builder types.

### J2. Existing H1 connection API

Preserve `serve_http1_connection` as an H1-specific API if already consumed downstream. Add a separate auto/multi-protocol driver rather than changing a function whose name promises H1 unless the 0.2 migration explicitly replaces it.

### J3. Python surfaces

Keep the six-class `eggserve.server` compatibility facade HTTP/1.1-only for this plan. Do not make `protocol_version = "HTTP/2"` meaningful.

If the native CLI/static path is shared with the Python fast path and would accidentally begin negotiating H2, explicitly choose/document whether that is allowed. Default preference for compatibility stability: Python compatibility HTTPS types continue advertising only HTTP/1.1 even if the Rust CLI can advertise H2.

## Verification

Run all ordinary existing checks plus H2 feature checks. The exact feature command depends on the selected feature names; expected shape:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo audit
cargo deny check
bash scripts/test-python-wheel.sh
```

Also verify the minimal/default build still works with H2 disabled if H2 remains feature-gated.

## Required focused test classes

- TLS ALPN H2 selection and HTTP/1.1 fallback.
- H2 prior-knowledge cleartext if supported.
- Explicit absence of `Upgrade: h2c` support.
- HTTP/1 regression through the new connection builder/facade.
- Multiple concurrent H2 streams using one TCP connection.
- Per-connection H2 stream limit.
- Server-wide service admission under streams from multiple connections.
- Decoded header-list/aggregate header/request-target limits.
- H2 forbidden connection headers.
- Request body Reject/Buffer/Stream.
- One rejected/incomplete body stream while a sibling stream completes normally.
- Per-stream handler timeout.
- Flow-control-stalled response timeout without sibling-stream false progress.
- Response stream producer error after commitment.
- max-requests-per-connection mapped to GOAWAY/drain.
- graceful shutdown with in-flight streams.
- forced shutdown/cancellation.
- large static file and range response.
- privacy headers and duplicate response headers.
- caller-owned byte-stream H2 behavior if that public API is enabled.

Plan 186 adds broader interoperability/conformance/release qualification; Plan 185's tests are implementation acceptance and must be deterministic in the repository.

## Acceptance criteria

- [ ] Hyper/Hyper-Util H2 support is enabled without a second H2 implementation stack.
- [ ] TCP/TLS runtime can serve HTTP/1.1 and HTTP/2 through one canonical service pipeline.
- [ ] TLS advertises `h2` and `http/1.1` only when configured to support both; H1-only configurations remain H1-only.
- [ ] negotiated protocol is recorded and canonical requests report `HttpVersion::Http2` correctly.
- [ ] H2 authority/scheme/pseudo-fields map into canonical metadata without pseudo-header leakage.
- [ ] HTTP/1 framing checks are not blindly applied to H2; H2 forbidden header rules are explicitly enforced.
- [ ] H2 protocol limits are EggServe-owned, validated, bounded, and distinct from server-wide application admission.
- [ ] rejected/incomplete bodies are stream-scoped when safe; unrelated streams survive.
- [ ] response no-progress behavior is stream-aware and cannot be masked by writes on sibling streams.
- [ ] response normalization/privacy remains canonical and no connection-specific/transfer-encoding field is emitted on H2.
- [ ] graceful shutdown and max-request thresholds use GOAWAY/drain semantics rather than `Connection: close`.
- [ ] static serving works without an H2-specific planner/service fork.
- [ ] existing H1/TLS behavior remains covered and green.
- [ ] `serve_http1_connection` semantics remain truthful; any new multi-protocol driver has an explicit API/migration story.
- [ ] Python `http.server` compatibility is not expanded to H2 by accident.
- [ ] no server push, extended CONNECT, WebSocket, reverse proxy, middleware, or routing capability is added.

## Suggested implementation order

1. Enable H2 features and prototype the auto H1/H2 builder behind internal tests.
2. Add validated `Http2Config` with pinned resource defaults.
3. Refactor TLS ALPN construction and protocol observation.
4. Adapt H2 request metadata/forbidden headers into the canonical request model.
5. Map request-body rejection/deferred lifecycle to stream-scoped behavior.
6. Implement stream-aware response progress/timeout tracking.
7. Map neutral lifecycle dispositions to H2 reset/GOAWAY/drain semantics.
8. Run static and generic services unchanged over H2.
9. Add/adjust public experimental protocol-selection APIs and preserve strict H1 entry points.
10. Run the complete deterministic H1/H2/TLS/Python regression suite.
11. Proceed to Plan 186 for interoperability and release qualification.

## Handoff

Plan 185 is complete when native H2 is functionally correct and hardened under deterministic tests. It is not yet the release-support declaration. Plan 186 owns cross-implementation conformance, operational qualification, documentation finalization, and the decision to label H2 supported.