# Phase 57 — Milestone 4B: Runtime Body Ingestion, Limits, and Connection Semantics

## Goal

Integrate the bounded request-body primitives from Phase 56 into the shared Rust runtime so fixed-length and chunked request bodies are transfer-decoded, accounted, timed out, cancelled, and delivered to services safely while the built-in static service remains bodyless.

This phase owns transport integration and connection behavior. It must not add framework-level decoding, multipart handling, uploads, request decompression, routing, middleware, or async Python callbacks.

## Starting state

Expected Phase 56 outputs:

- `RequestBodyPolicy` with reject/buffer/stream modes;
- canonical request envelope containing `RequestHead`, `ConnectionInfo`, and `RequestBody`;
- one-shot read-all and chunk-iteration semantics;
- typed body errors;
- fixed-length and chunked accounting contract;
- incomplete-body drain-or-close policy;
- test body fixtures;
- experimental service-envelope migration path.

The runtime already owns:

- listener acceptance;
- HTTP/1 parsing;
- canonical request conversion;
- service dispatch;
- handler timeout;
- connection and task accounting;
- graceful/forced shutdown;
- plaintext and TLS transports.

## Track A — Select body policy before service invocation

Define one deterministic policy-selection path.

Requirements:

- static service always selects `Reject`;
- custom services declare or are configured with a body policy;
- runtime enforces a hard global `max_request_body_bytes` ceiling;
- service-specific limits may only lower the ceiling;
- policy selection occurs after request-head validation but before body consumption;
- invalid framing is rejected before callback/service invocation where possible;
- methods are not globally restricted solely because a body exists;
- method/body policy belongs to the selected service, not the HTTP parser.

Potential service contract:

```rust
trait Service {
    fn request_body_policy(&self, head: &RequestHead) -> RequestBodyPolicy;
    fn call(&self, request: Request) -> ServiceFuture;
}
```

Keep the API minimal and experimental.

Acceptance:

- body policy is visible, bounded, and selected exactly once;
- static GET/HEAD behavior remains unchanged.

## Track B — Hyper incoming-body adapter

Implement an internal adapter from Hyper’s incoming body to the public `RequestBody` abstraction.

Requirements:

- Hyper types remain private;
- transfer coding is decoded by the transport stack before public delivery;
- each decoded data frame updates byte accounting;
- non-data frames are handled according to the trailer policy;
- errors map to typed `RequestBodyError` variants;
- body stream is cancellation-aware;
- no eager buffering for stream policy;
- buffer policy uses bounded preallocation and never trusts `size_hint` as an authority;
- declared length is metadata, not a bypass around actual accounting.

Acceptance:

- services consume only public body primitives;
- no raw Hyper frame escapes the runtime.

## Track C — Fixed-length preflight and ingestion

Before service invocation:

- parse and reconcile `Content-Length` through the canonical header model;
- reject conflicting or ambiguous duplicates;
- reject values above effective limit with a deterministic response;
- reject unsupported `Transfer-Encoding`/`Content-Length` combinations;
- select keep-alive/close behavior for rejected bodies.

For `Buffer` policy:

- read exactly the declared body under body timeout;
- reject premature EOF;
- enforce actual-byte limit even when declared length is smaller or absent;
- construct a completed in-memory body for the service;
- avoid service invocation when buffering fails.

For `Stream` policy:

- expose the stream after preflight;
- preserve declared-length metadata;
- continue enforcing actual received bytes.

Acceptance:

- fixed-length limits and framing cannot be bypassed;
- buffer-mode failures occur before handler execution.

## Track D — Chunked ingestion and decoded-byte accounting

Integrate chunked bodies through the same public body stream.

Requirements:

- services never see chunk boundaries unless chunk iteration happens to align with decoded frames; no semantic reliance on transport chunk sizes;
- decoded byte count is authoritative;
- cumulative count uses checked arithmetic;
- limit exceedance terminates consumption immediately;
- malformed chunk framing maps to protocol failure without service-visible parser details;
- chunk extensions are ignored safely;
- trailers are rejected, discarded, or exposed only according to the Phase 56 contract;
- many-small-chunk attacks are bounded by existing parser/runtime behavior and optionally a frame-count ceiling;
- timeout applies across the total body or idle intervals according to documented semantics.

Tests must use raw TCP requests, not only Hyper client helpers.

Acceptance:

- chunked transfer coding cannot bypass byte limits;
- malformed chunking never reaches the service as valid data.

## Track E — Body timeout semantics

Add explicit runtime configuration for request-body timeout.

Decide and document whether the timeout is:

- total body deadline;
- idle-between-data deadline;
- or both.

Preferred model:

- total `body_read_timeout` for bounded completion;
- optional internal progress timeout only if required to prevent slowloris behavior already not covered by header timeout.

Requirements:

- timeout starts at the documented point;
- buffer and stream modes use the same accounting model;
- timeout errors are deterministic;
- timeout cancels pending body reads;
- connection behavior after timeout is explicit, normally close;
- shutdown cancellation takes precedence and does not wait for body timeout;
- TLS and plaintext behave identically.

Acceptance:

- slow bodies cannot occupy connections indefinitely;
- timeout behavior is covered over plaintext and TLS.

## Track F — Cancellation and shutdown

Body operations must react to:

- client disconnect;
- handler cancellation;
- graceful server shutdown;
- forced server shutdown;
- connection task abortion;
- service timeout.

Requirements:

- pending body reads wake promptly on cancellation;
- no body task survives its connection task;
- permits and buffers are released;
- graceful shutdown follows the configured drain deadline;
- forced shutdown aborts body ingestion immediately;
- callback/body interaction does not deadlock during shutdown;
- cancellation errors do not leak transport details to clients.

Tests:

- shutdown before first body byte;
- shutdown mid-fixed body;
- shutdown between chunks;
- forced shutdown with blocked body reader;
- client disconnect while handler awaits next chunk;
- service future dropped without complete consumption.

Acceptance:

- body ingestion is fully contained by the runtime task lifecycle.

## Track G — Partial consumption and connection reuse

Implement the Phase 56 incomplete-body policy.

For `Close`:

- stop reading after handler completion;
- mark connection non-reusable;
- close deterministically after response transmission or immediately according to safety requirements.

For bounded `Drain` if enabled:

- drain no more than configured bytes;
- enforce drain timeout;
- cancel drain on shutdown;
- permit keep-alive only after complete successful drain;
- close on malformed, over-limit, timeout, or disconnect outcomes.

Requirements:

- response completion does not imply body completion;
- keep-alive decisions are runtime-owned;
- handler cannot force reuse after incomplete framing;
- metrics/test hooks can observe reused versus closed outcome without adding a public observability framework.

Raw-wire tests:

- partial body then pipelined second request;
- unread exact small body under drain policy;
- unread body above drain cap;
- partial chunked body;
- malformed remainder;
- handler error before consumption;
- timeout during drain.

Acceptance:

- second requests are never parsed from leftover body bytes;
- connection reuse occurs only after safe reconciliation.

## Track H — Error response and close policy

Define deterministic protocol outcomes for:

- rejected by service policy;
- declared body too large;
- decoded body exceeds limit;
- body timeout;
- malformed framing;
- premature EOF;
- unsupported transfer coding;
- handler returns before body completion;
- internal transport error.

Recommended mappings, subject to existing contract:

- 400 for malformed framing/length mismatch;
- 408 for request body timeout where a response is safe;
- 413 for body too large;
- 501 for unsupported transfer coding only if parser accepts it far enough to respond safely;
- close without response when framing state is unsafe.

Requirements:

- error responses pass through canonical normalization;
- `Connection: close` or transport close is applied as needed;
- no detailed parser errors are reflected;
- HTTP/1.0 and HTTP/1.1 behavior is tested.

Acceptance:

- failure responses and close behavior are consistent and documented.

## Track I — Static service preservation

Prove the built-in static service remains bodyless.

Requirements:

- GET/HEAD with any body framing is rejected according to existing policy;
- static service does not buffer or stream request bodies;
- custom body support does not change CLI defaults;
- static rejection happens before filesystem resolution and file opening;
- body-bearing requests cannot consume file-stream permits;
- keep-alive behavior after rejection is deterministic.

Tests:

- GET/HEAD with fixed-length body;
- GET/HEAD with chunked body;
- zero-length `Content-Length` according to chosen semantics;
- malformed body framing;
- request smuggling-oriented CL/TE combinations;
- plaintext and TLS parity.

Acceptance:

- Milestone 4 broadens only downstream dynamic service capability.

## Track J — Service and runtime migration

Update the experimental `Service` API and all implementations:

- `service_fn`;
- `StaticService`;
- Python callback service;
- examples;
- tests;
- CLI runtime integration.

Provide compatibility helpers if needed, but avoid preserving two permanent dispatch paths.

Potential helper:

```rust
service_fn_head(|head| ...)
```

which internally selects `Reject` and discards the empty body envelope.

Acceptance:

- one canonical request-envelope dispatch path exists;
- legacy request-head-only adapters are thin and documented.

## Track K — Fuzzing and corpus

Add fuzz targets for:

- fixed-length accounting;
- chunked decoded-size accounting;
- body state machine;
- partial-consumption connection decision;
- CL/TE reconciliation;
- timeout/cancellation state transitions where model-based fuzzing is practical.

Add seed corpora:

- boundary lengths;
- conflicting headers;
- malformed chunks;
- many tiny chunks;
- premature EOF;
- partial bodies;
- exact-limit and one-over-limit inputs.

Add corpus replay tests that do not require indefinite network waits.

Acceptance:

- body accounting and framing boundaries receive continuous adversarial coverage.

## Track L — Performance and resource qualification

Measure:

- empty-body overhead;
- small buffered body;
- body at common limits;
- streamed body chunk processing;
- many-small-chunk overhead;
- cancellation cleanup;
- partial-consumption close/drain cost;
- allocation counts for buffer mode.

Requirements:

- no unbounded allocation from declared length or size hints;
- buffer capacity grows under explicit ceiling;
- stream mode remains O(chunk size) memory;
- benchmark jobs remain advisory unless deterministic thresholds exist;
- resource tests verify permits/buffers return to baseline.

Acceptance:

- body support has documented performance and memory characteristics.

## Required tests

```sh
cargo test -p eggserve-core --test request_body_integration
cargo test -p eggserve-core --test request_body_wire
cargo test -p eggserve-core --test server_integration
cargo test -p eggserve-core --test lifecycle_integration
cargo test -p eggserve-core --features tls --test request_body_tls
cargo test -p eggserve-core --test public_api_consumers
cargo test --workspace
```

Add exact test targets according to implementation naming.

## Completion criteria

Phase 57 is complete when:

- runtime selects bounded body policy before service invocation;
- fixed-length and chunked bodies are transfer-decoded and limit-accounted;
- body timeout and cancellation are enforced;
- buffer and stream modes work through public body primitives;
- partial consumption has deterministic drain-or-close behavior;
- unsafe framing never permits connection reuse;
- static service remains body-rejecting;
- TLS/plaintext behavior matches;
- one request-envelope service path is used;
- fuzz, raw-wire, resource, and performance coverage exists.

## Non-goals

- Python body projection.
- Multipart, forms, upload storage, JSON helpers, compression decoding, or trailers.
- HTTP/2 flow control.
- Framework-specific receive channels.