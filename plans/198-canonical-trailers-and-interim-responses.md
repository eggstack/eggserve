# Plan 198 — Canonical Trailers and Interim Responses

## Status

**IMPLEMENTED / CLOSED.**

Prerequisite: Plan 197 contract shape settled. Protocol adapters from Plans 185–195 remain experimental inputs.

## Closure record

Implemented on `main`: canonical `Trailers` (`trailers.rs`, denylist + 32/8KiB limits, one validator for H1/H2/H3) + `TrailerLimits`; `RequestBody::trailers()`/`read_all_with_trailers()`/`from_bytes_with_trailers()` with wire-slot bridge populated only from protocol trailer frames (H1 chunked trailers via `BodyStream`, H2 terminal HEADERS via same path, H3 bounded `recv_trailers` probe; repeated/data-after-trailers fail with `InvalidTrailers`, new `RequestBodyError::InvalidTrailers`/`TrailersNotReady`); `ResponseStream::with_trailers`/`with_known_length_and_trailers` (one terminal block, no data after, HEAD/body-forbidden never poll, known length counts data only, adapters map without buffering; H1 `Frame::trailers`, H2 terminal HEADERS stream-local, H3 `send_trailers` stream-local with same no-progress deadline); H1 policy (`TE: trailers` required, HTTP/1.0 suppressed, `Trailer`/`Transfer-Encoding` runtime-owned, producer failure truncates, no second error); bounded `InterimSender` in `RequestContext::interim()` (only 1xx no 101, no body/trailers, no post-commit, 4/8KiB bounds, HTTP/1.0 suppressed, single 100, `103` allowed); `100 Continue` deterministic with body policy (`Reject` → 413 without inviting, `Buffer`/`Stream` → Hyper owns wire `100`, unknown `Expect` → 417, app 100 deduped); Python `_native.validate_trailers`/`validate_interim` text-only bounded projection without changing sync facade; `trailers_interim.rs` hostile/acceptance suite (26 tests) + existing matrices green. Docs updated (`http-primitives.md`, `downstream-app-server.md`, `runtime.md`, `primitives-api.md`, `http2.md`, `http3.md`, `error-taxonomy.md`, `README.md`, `AGENTS.md`, skill). Verification: `cargo fmt`, workspace clippy/tests, `http2,tls` + `http3,tls` matrices, Python crate check green locally before push.

## Purpose

Add transport-neutral HTTP message capabilities that are currently missing from the canonical application boundary: request trailers, response trailers, and bounded interim 1xx responses before a final response.

These are HTTP semantics, not ASGI features. The design must map correctly onto HTTP/1.1, HTTP/2, and HTTP/3 while preserving EggServe's ownership of framing, body limits, response normalization, and privacy policy.

## Standards constraints

RFC 9110 models trailers as a distinct trailer section, not ordinary headers discovered late. Trailers must not be merged blindly into the header section, and fields whose semantics require early processing are not valid trailer candidates. One or more informational 1xx responses may precede the final response; 1xx responses have no content or trailers. HTTP/1.0 must not receive 1xx responses.

Protocol adapters own the wire mapping:

- H1: chunked framing/trailer section where legal; runtime owns `Transfer-Encoding` and `Trailer` mechanics.
- H2: terminal HEADERS after DATA/end-of-stream semantics.
- H3: terminal field section on the request/response stream.

Application services must never manufacture protocol framing headers to force these behaviors.

## Track A — Canonical trailer representation

Use the existing byte-preserving, duplicate/order-preserving header vocabulary for trailer fields, but keep trailers structurally distinct from initial headers.

Requirements:

- trailer values preserve legal octets and duplicate ordering;
- trailer field validation reuses canonical header-name/value rules;
- runtime rejects fields forbidden in trailers by EggServe's generic safety policy;
- initial headers and trailers cannot be accidentally merged by convenience APIs;
- limits exist for trailer field count and decoded aggregate bytes, with shared safe defaults and protocol-specific lower bounds where required;
- trailer limits are enforced before unbounded allocation and before exposing data to services.

Define a reusable validator for forbidden trailer fields. At minimum framing/routing fields controlled by the runtime must never be accepted as application trailers. Document whether the policy is an allowlist, denylist, or semantic registry subset; prefer a conservative denylist plus protocol validation if full field semantics are impractical.

## Track B — Request trailer consumption

Extend `RequestBody` so terminal trailer metadata can be obtained without changing the incremental byte API into framework events.

A plausible model is:

```rust
while let Some(chunk) = body.next_chunk().await? { ... }
let trailers = body.trailers().await?;
```

or a terminal frame API if that produces cleaner one-shot ownership.

Requirements:

- trailers become available only after content completion;
- calling `read_all()` must have a defined way to retrieve trailers rather than silently discarding them;
- body byte limits remain byte limits; trailer metadata has separate bounds;
- malformed/oversized trailers fail the request body and lifecycle safely;
- dropping the body before trailers preserves existing abandoned-body connection safety;
- H1 requests without a valid trailer framing mechanism cannot inject post-body header-like bytes;
- services that ignore trailers pay minimal overhead.

## Track C — Response trailers

Extend the canonical streaming response model with one terminal trailer source. Avoid representing trailers as an arbitrary body chunk.

Possible shapes:

```rust
ResponseStream::with_trailers(stream, trailer_future)
```

or a small canonical body-frame abstraction:

```rust
BodyFrame::Data(Bytes)
BodyFrame::Trailers(HeaderBlock)
```

Decision criteria:

- exactly one terminal trailer block;
- no data after trailers;
- HEAD/body-forbidden responses do not poll the body/trailer producer;
- producer cancellation/drop remains deterministic;
- known-length response semantics remain coherent;
- adapters can map trailers without buffering the entire body;
- ordinary byte streams remain ergonomic.

If a body-frame abstraction is chosen, keep a convenience constructor for byte-only streams so the common path does not become noisy.

## Track D — Trailer negotiation and H1 policy

Define explicit H1 behavior. Do not let application code set `Transfer-Encoding`.

Decide and document:

- whether response trailers are emitted only when the request indicates willingness via `TE: trailers`, except where RFC semantics permit otherwise;
- when the runtime emits a `Trailer` header naming anticipated fields;
- whether trailer names must be declared before streaming starts or may be omitted when not knowable;
- how HTTP/1.0 behaves (trailers unavailable);
- how connection close/error is handled if a trailer producer fails after response commitment.

For H2/H3, omit H1 negotiation artifacts and use protocol-native terminal fields.

## Track E — Interim response capability

Add a request-scoped, bounded interim-response sender/capability rather than changing `Service::call()` to return a list of responses.

A conceptual API:

```rust
request.context().interim().send(status, headers).await?;
```

Requirements:

- only 1xx statuses accepted;
- no body or trailers;
- final response cannot be sent through this capability;
- no interim response after final commitment;
- bounded count and aggregate header bytes per request;
- final response remains the ordinary `Service` result;
- runtime-owned/forbidden response fields still pass through normalization/privacy rules appropriate to interim messages;
- HTTP/1.0 rejects/suppresses application interim responses according to the documented contract rather than emitting invalid wire behavior.

## Track F — `100 Continue`

Do not force every application to manually send `100 Continue`. Establish a safe runtime policy for `Expect: 100-continue` that interacts with request-body policy:

- `Reject`: reject without encouraging the client to send the body;
- `Buffer`/`Stream`: runtime may emit `100 Continue` once body acceptance is known, subject to protocol rules;
- unknown expectations map to the correct final error path;
- an application-generated 100 cannot race/duplicate a runtime-generated 100 without a defined outcome.

If the current Hyper APIs own part of `100 Continue`, characterize the actual behavior first and prevent double emission.

`103 Early Hints` should be allowed through the generic interim mechanism if valid, but EggServe does not invent preload/link policy.

## Track G — Protocol adapters

Implement one semantic core and thin adapters:

### H1

- request trailer extraction from Hyper body frames;
- response trailer emission through `http-body` frame support;
- interim response support only if Hyper server APIs permit controlled emission without bypassing the canonical pipeline; if Hyper cannot expose it safely, record the limitation and do not create raw-socket fallback code.

### H2

- stream-local request/response trailers;
- 1xx field blocks before final headers if supported by the driver API;
- trailer/reset failures affect the stream rather than sibling streams.

### H3

- stream-local terminal field sections;
- interim field sections if supported by current `h3` APIs;
- map producer failure/cancellation to bounded H3 stream termination without widening whole-connection failure.

No protocol adapter may maintain a second trailer validation policy.

## Track H — Python low-level projection

Expose generic trailer/interim primitives needed by Plan 204, but do not add them to the synchronous `http.server` facade unless compatibility semantics require it.

Keep Python conversion byte-preserving and bounded. Async event naming belongs to Plan 204/downstream ASGI mapping.

## Security tests

Add hostile cases for:

- forbidden framing/routing fields in trailers;
- oversized trailer block/count;
- duplicate legal trailers preserving order;
- malformed H1 chunk trailers;
- trailer producer error after final response commitment;
- data-after-trailers attempt;
- repeated trailer block attempt;
- interim response flooding bounded by count/bytes;
- invalid non-1xx interim status;
- interim response after commitment;
- 100-continue rejection without reading attacker body;
- reset/disconnect while waiting for trailers.

Fuzz canonical trailer validation and any new body-frame state machine.

## Verification

Run the full feature matrix plus focused H1/H2/H3 wire tests. Extend the existing conformance corpus where transport-neutral semantics can be shared.

No absolute timing CI gates.

## Acceptance criteria

- [x] request trailers are available as distinct terminal metadata with explicit bounds;
- [x] response trailers stream without full-body buffering and cannot be followed by data;
- [x] H1/H2/H3 use one canonical trailer validation model;
- [x] application code never controls transfer coding to obtain trailers;
- [x] interim 1xx responses have a bounded request-scoped API and cannot carry content/trailers;
- [x] `100 Continue` behavior is deterministic with request-body acceptance policy;
- [x] HEAD/body-forbidden semantics never poll discarded producers;
- [x] stream-local H2/H3 failures do not unnecessarily terminate siblings;
- [x] Python low-level bindings can project the capability without changing the synchronous compatibility surface;
- [x] docs clearly distinguish initial headers, trailers, interim responses, and final responses.

## Handoff

Plan 199 may reuse the commitment/capability machinery established here, but tunnel handoff remains structurally distinct from trailers or body frames.