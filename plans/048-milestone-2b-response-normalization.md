# Phase 48 — Milestone 2B: Canonical Response Construction and Normalization

## Goal

Create one transport-independent response model and one final normalization path for built-in static responses, Rust handlers, and Python callbacks. The runtime must prevent common HTTP/1 framing and semantic errors rather than requiring every consumer to implement them correctly.

This phase must preserve file streaming, range behavior, conditional responses, and existing static-server semantics.

## Starting state

Eggserve already has `StaticResponsePlan`, `BodyPlan`, file-backed body sources, Python `Response` constructors, and production HTTP tests. Remaining weaknesses include:

- generic Python headers are lossy;
- HEAD suppression is not uniformly owned by the runtime;
- body-forbidden statuses rely partly on handler discipline;
- hop-by-hop and framing headers may be emitted by handlers;
- callback response validation and static planning are not clearly one final contract;
- streaming response behavior needs a canonical public representation.

## Track A — Canonical response value types

Define public response types independent of Hyper and PyO3:

```rust
pub struct ResponseHead {
    status: StatusCode,
    headers: HeaderBlock,
}

pub struct Response {
    head: ResponseHead,
    body: ResponseBody,
}
```

Requirements:

- use the duplicate-preserving header types from Phase 47;
- no public Hyper types;
- validated construction;
- immutable response head after normalization begins;
- body ownership and one-shot semantics explicit;
- clear distinction between pre-normalized builder input and normalized wire response.

Do not collapse existing static planning types until compatibility and performance are proven. Add adapters first, then consolidate if safe.

## Track B — Status code model

Provide a validated `StatusCode` type or adopt a stable dependency type only if exposing it does not couple the API to unstable internals.

Requirements:

- valid range enforcement;
- reason phrases not treated as authoritative application data;
- helpers for informational, success, redirection, client-error, and server-error classes;
- `permits_payload_body()` and related semantics centralized;
- no invalid or out-of-range status emitted to transport.

## Track C — Response builder

Add a safe builder:

```rust
let response = Response::builder()
    .status(StatusCode::OK)
    .header("content-type", "text/plain")?
    .body(ResponseBody::bytes("ok"))?;
```

Builder validation must cover:

- invalid names/values;
- CR/LF injection;
- aggregate header-size limit;
- duplicate policy only where uniqueness is required;
- user-provided framing headers;
- status/body compatibility;
- invalid file range/body metadata;
- body already consumed.

Provide convenience constructors without making them normative:

- empty;
- bytes;
- text;
- file full;
- file range;
- stream.

A JSON convenience may be deferred unless it introduces no framework-level policy.

## Track D — Final normalization algorithm

Implement one documented normalization function invoked immediately before transport conversion.

Inputs:

- request method/version;
- response status;
- response headers;
- response body metadata;
- connection policy.

Rules must include:

1. HEAD transmits no body bytes while preserving representation headers appropriate to the equivalent GET response.
2. 1xx, 204, and 304 responses transmit no payload body.
3. `Transfer-Encoding` is runtime-owned; reject or remove handler-supplied values.
4. `Content-Length` is computed or validated by the runtime.
5. conflicting length/framing declarations are rejected.
6. hop-by-hop headers are rejected or normalized according to a documented policy.
7. connection-close semantics are runtime-owned.
8. duplicate end-to-end headers are preserved.
9. body stream/file length mismatches produce typed failures.
10. error responses do not leak handler tracebacks or sensitive internals.

Choose fail-versus-strip policy explicitly. Prefer rejecting malformed handler responses before headers are sent. For headers that must be runtime-owned, rejection is easier to audit than silent mutation unless compatibility strongly favors stripping.

## Track E — Body representation

Define `ResponseBody` variants:

- `Empty`;
- `Bytes`;
- `FileFull`;
- `FileRange`;
- bounded streaming body.

Streaming requirements:

- chunks validated as bytes;
- cancellation on disconnect/shutdown;
- write timeout enforcement;
- error propagation after headers;
- permit/resource release;
- optional known length;
- no eager Python-memory copy for file bodies;
- one-shot consumption.

Do not add raw socket access. Do not add trailers unless they can be modeled and tested cleanly; otherwise document them as deferred.

## Track F — Static response integration

Adapt existing static response planning into the canonical response path.

Preserve:

- ETag and Last-Modified behavior;
- conditional request outcomes;
- range and If-Range behavior;
- 304 and 416 semantics;
- MIME fallback;
- file capability ownership;
- streaming I/O error propagation;
- GET/HEAD equivalence.

Add tests proving byte-for-byte and header-for-header compatibility for normative fixtures. Any intentional change must update the release contract and fixtures explicitly.

## Track G — Rust handler integration

Update the reusable handler/service boundary to return canonical responses.

Requirements:

- handler errors remain distinct from invalid response construction;
- invalid responses become deterministic 500 or connection-close behavior according to whether headers were sent;
- panics are contained at the runtime boundary;
- response normalization is not bypassable through stable APIs;
- a clearly experimental raw path, if retained, is feature-gated and documented unsafe at the protocol level.

## Track H — Python response parity

Update Python `Response` to project the canonical Rust response model.

Required behavior:

- ordered duplicate-preserving headers;
- typed status and construction errors;
- runtime-owned HEAD suppression;
- runtime handling of 1xx/204/304 bodies;
- rejection of user framing headers;
- file-backed responses without eager copy;
- byte and streaming responses;
- deterministic callback return validation;
- no traceback leakage.

Compatibility:

- preserve existing constructors where correct;
- deprecate flat dictionary-only header input only if necessary;
- accept ordered pairs as the canonical representation;
- define lossy dictionary conversion explicitly;
- update type stubs and examples.

## Track I — Limits and denial-of-service controls

Add or consolidate response limits:

- maximum response-header bytes;
- maximum header count;
- maximum single header value length if justified;
- bounded queued streaming chunks;
- write timeout;
- optional maximum handler-produced buffered body;
- no unbounded conversion of iterables to memory.

Defaults must remain safe and documented. Static file size must not be limited by buffered-body limits because files stream.

## Track J — Error taxonomy

Define stable or experimental typed errors for:

- invalid status;
- invalid header;
- forbidden framing header;
- body-forbidden status;
- content-length mismatch;
- invalid range body;
- response already consumed;
- streaming producer failure;
- normalization failure.

Map errors consistently across Rust and Python.

## Required tests

Normative matrix:

- GET and HEAD for every body variant;
- 1xx, 204, and 304 with attempted bodies;
- 200/206/304/416 static responses;
- duplicate `Set-Cookie` and other duplicate headers;
- invalid names and newline values;
- handler-supplied `Content-Length` and `Transfer-Encoding`;
- wrong known stream length;
- stream failure before and after first chunk;
- client disconnect;
- slow reader/write timeout;
- callback exception;
- handler panic;
- file truncation/read error;
- HTTP/1.0 versus HTTP/1.1 close behavior.

Use raw-wire assertions for exact framing. Add Rust/Python parity fixtures and property tests for normalization invariants.

## Documentation

Update:

- `docs/release-contract.md`;
- `docs/api-stability.md`;
- Python API docs;
- architecture response/server docs;
- capability matrix;
- examples;
- non-goals for trailers/raw response access if deferred.

Document the normalization algorithm as normative behavior.

## Completion criteria

- all response producers converge on one final normalization path;
- HEAD and body-forbidden statuses are runtime-correct;
- handlers cannot emit conflicting framing through stable APIs;
- duplicate headers survive Rust and Python construction;
- static response behavior remains compatible;
- file responses remain capability-backed and streaming;
- malformed callback responses fail deterministically without leaking internals;
- raw-wire and parity tests cover the contract.

## Non-goals

- Compression middleware.
- Cookie/session framework.
- HTTP/2.
- WebSockets or upgrades.
- Raw socket response writers.
- General middleware stack.
- Trailers unless explicitly approved during design review.