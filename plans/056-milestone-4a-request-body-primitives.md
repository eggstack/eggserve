# Phase 56 — Milestone 4A: Bounded Request-Body Primitives

## Goal

Introduce transport-independent, one-shot request-body abstractions that allow downstream Rust services to safely consume request bodies without exposing Hyper internals, weakening static-server defaults, or permitting unbounded buffering.

This phase defines the body contract and core types. It must not yet broaden the built-in static service beyond GET/HEAD, add multipart or upload frameworks, expose raw transfer coding, or add ASGI/WSGI semantics.

## Starting state

Eggserve currently has:

- canonical `RequestHead` and `ConnectionInfo` types;
- a reusable Rust `Service` boundary;
- Rust-owned HTTP parsing and transport;
- strict request-body rejection for the built-in static service;
- response body abstractions and file-backed streaming;
- Python callback services using the shared runtime;
- explicit request, handler, write, TLS, and shutdown limits.

Current gap:

- service handlers receive only request metadata;
- there is no public transfer-decoded body object;
- downstream services cannot safely buffer or stream request bodies;
- no one-shot consumption, body limit, timeout, cancellation, premature EOF, or partial-consumption contract exists.

## Track A — Body policy model

Define a public request-body policy with explicit modes:

```rust
pub enum RequestBodyPolicy {
    Reject,
    Buffer { max_bytes: u64 },
    Stream { max_bytes: u64 },
}
```

Requirements:

- `Reject` remains the static-service default;
- limits are mandatory for buffer and stream modes;
- zero-byte and invalid limits are handled deliberately;
- policy is selected by the service/runtime contract, not inferred from headers;
- policy can be configured globally and overridden only through a narrow service declaration if needed;
- unsupported transfer framing is rejected before service invocation;
- handlers cannot disable transport-level accounting.

Decide whether policy belongs to `RuntimeConfig`, `Service`, or a service capability method. Preferred design:

- runtime owns a hard global maximum;
- service declares `Reject`, `Buffer`, or `Stream` within that ceiling;
- static service always declares `Reject`.

Acceptance:

- body acceptance is explicit and bounded;
- no handler can request a limit above the runtime ceiling.

## Track B — Public request body type

Add a transport-independent request-body object, for example:

```rust
pub struct RequestBody { /* private */ }
```

Required properties:

- no public Hyper/body implementation types;
- one-shot consumption;
- cancellation-aware;
- transfer-decoded bytes only;
- bounded by the effective body limit;
- exact consumed-byte accounting;
- explicit completion state;
- safe to move into async handler code;
- no cloning of live body streams;
- no implicit rewind or replay.

Potential public methods:

```rust
impl RequestBody {
    pub fn declared_length(&self) -> Option<u64>;
    pub fn bytes_received(&self) -> u64;
    pub fn is_complete(&self) -> bool;
    pub async fn read_all(self) -> Result<Bytes, RequestBodyError>;
    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>, RequestBodyError>;
}
```

Avoid exposing both `read_all()` and chunk iteration in ways that permit mixed consumption without a typed error.

Preferred semantics:

- initial state: `Unread`;
- `read_all(self)` consumes the object;
- streaming iteration transitions to `Streaming`;
- after completion: `Complete`;
- after error/cancellation: terminal state;
- a second consumer attempt fails deterministically.

Acceptance:

- body consumption cannot be duplicated;
- public types remain transport-independent.

## Track C — Request envelope

Evolve the `Service` input from request-head-only to a canonical request envelope:

```rust
pub struct Request {
    head: RequestHead,
    body: RequestBody,
    connection: ConnectionInfo,
}
```

Or an equivalent structure preserving the current service ergonomics.

Requirements:

- request head remains immutable;
- connection metadata remains trustworthy and separate from headers;
- body ownership is explicit;
- existing request-head-only services receive a compatibility adapter or migration path;
- static service can ignore metadata and reject body before dispatch;
- no Hyper request type leaks into public signatures;
- request construction remains runtime-only except for test/build fixtures.

Compatibility options:

1. introduce a new experimental `Request` and update experimental `Service`;
2. retain `Service<RequestHead>` temporarily with a second body-aware trait;
3. use a request envelope while providing `service_fn_head` compatibility.

Prefer one coherent experimental service model rather than long-term parallel traits.

Acceptance:

- downstream services receive head, connection metadata, and bounded body through one canonical request object;
- migration is documented.

## Track D — Error taxonomy

Define typed body errors, at minimum:

- `RejectedByPolicy`;
- `DeclaredLengthTooLarge`;
- `LimitExceeded`;
- `ReadTimeout`;
- `PrematureEof`;
- `LengthMismatch`;
- `InvalidChunkFraming` or transport-normalized equivalent;
- `Cancelled`;
- `Disconnected`;
- `AlreadyConsumed`;
- `MixedConsumptionMode`;
- `Transport` with sanitized details.

Separate errors that occur:

- before service invocation;
- during handler consumption;
- during post-handler drain/close processing.

Map protocol-level failures to deterministic responses or connection closure. Do not expose parser internals to downstream services.

Acceptance:

- callers can distinguish policy, limit, timeout, disconnect, and consumption-state failures;
- client-visible responses do not leak internal error details.

## Track E — Fixed-length accounting semantics

Define behavior for `Content-Length` requests:

- parse and validate once at the transport boundary;
- reject conflicting duplicate values;
- reject invalid decimal syntax;
- reject lengths above the global/service limit before callback invocation;
- read exactly the declared number of transfer-decoded bytes;
- detect premature EOF;
- detect overrun or extra framed body data according to HTTP parser guarantees;
- track actual received bytes independently of declared length;
- ensure body timeout applies across the entire receive operation or according to a documented idle/total model.

Tests:

- zero length;
- exact length;
- one byte short;
- one byte over;
- conflicting duplicate `Content-Length`;
- comma-joined ambiguity;
- very large numeric values;
- overflow;
- connection close mid-body;
- timeout before first byte and between bytes.

Acceptance:

- fixed-length bodies cannot exceed policy limits or reach handlers with inconsistent framing.

## Track F — Chunked accounting contract

Even if runtime integration lands in Phase 57, define the public semantics now:

- handlers see transfer-decoded data, never raw chunk framing;
- every decoded byte counts toward the body limit;
- chunk extensions do not affect size accounting;
- malformed chunk syntax is rejected before or during stream consumption;
- trailers remain unsupported or explicitly discarded according to contract;
- zero-length terminal chunk completes the body;
- decoded-size limit cannot be bypassed by many small chunks;
- timeout and cancellation apply across chunk boundaries.

Tests/fixtures should include:

- one chunk;
- many tiny chunks;
- chunk extensions;
- malformed hex size;
- missing CRLF;
- premature EOF;
- decoded size exactly at limit;
- decoded size one byte over limit;
- excessive chunk count if a count limit is added;
- trailer presence according to supported policy.

Acceptance:

- the public contract is transfer-decoded and limit-safe.

## Track G — Drain-or-close outcome model

Define what happens when a handler returns without fully consuming the body.

Required deterministic options:

- drain remaining body up to a configured limit/deadline, then permit keep-alive;
- close the connection without draining;
- force close immediately for malformed/over-limit/cancelled bodies.

Recommended policy:

```rust
pub enum IncompleteBodyPolicy {
    Drain { max_bytes: u64, timeout: Duration },
    Close,
}
```

The runtime may choose a strict default of `Close` initially.

Requirements:

- handler cannot accidentally leave connection reuse ambiguous;
- draining remains bounded by bytes and time;
- over-limit bodies are never drained without a hard cap;
- cancellation/shutdown interrupts draining;
- connection outcome is observable in tests;
- keep-alive is permitted only after complete framing reconciliation.

Acceptance:

- partial consumption has a deterministic connection policy.

## Track H — Safe test constructors

Provide test-only or experimental constructors for body fixtures without exposing transport internals.

Examples:

- empty body;
- fixed bytes;
- chunk sequence;
- injected timeout/error;
- premature EOF fixture.

Keep constructors clearly separated from production transport construction.

Acceptance:

- downstream consumer tests can exercise body-aware services without spinning up sockets for every unit test;
- unsafe raw framing remains internal.

## Track I — API stability and documentation

Update:

- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- `architecture/runtime.md`;
- `architecture/primitives-api.md`;
- Rust docs and examples;
- migration guide.

Classify all new body/service-envelope APIs as experimental through Milestone 4 closure.

Document explicitly:

- static service remains bodyless;
- no multipart/form parsing;
- no automatic JSON/form decoding;
- no rewind/replay;
- no raw chunk access;
- no trailers unless explicitly supported later;
- incomplete-consumption connection policy.

## Required tests

Rust unit/property tests:

- policy validation;
- one-shot state machine;
- read-all versus streaming exclusivity;
- fixed-length accounting;
- limit boundaries;
- timeout/cancellation fixtures;
- typed errors;
- request envelope public-consumer compile tests;
- no Hyper leakage.

Conformance fixtures:

- empty;
- exact fixed-length;
- premature EOF;
- over-limit;
- chunked exact/over-limit;
- partial consumption;
- cancellation.

## Completion criteria

Phase 56 is complete when:

- bounded body policy types exist;
- canonical request envelope owns a one-shot body;
- public APIs expose no Hyper body types;
- fixed-length and chunked semantics are documented and represented in fixtures;
- partial consumption has an explicit drain-or-close policy;
- typed body errors exist;
- static service remains body-rejecting;
- migration and stability docs are updated;
- core body-state tests pass.

## Non-goals

- Runtime socket integration beyond what is needed for fixtures.
- Python body APIs.
- Multipart, forms, uploads, JSON helpers, decompression, or content decoding.
- Request replay.
- HTTP trailers.
- ASGI/WSGI semantics.