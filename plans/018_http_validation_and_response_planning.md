# Plan 018: HTTP validation and response planning

## Status

Complete. This plan builds on Plans 016 and 017 by turning existing request validation and response construction behavior into reusable primitives.

## Objective

Centralize HTTP method/body/framing validation and static response metadata planning so Rust and Python users can safely compose dynamic sites or higher-level adapters without reimplementing tricky HTTP/static-file semantics.

The output of this plan should be pure, testable planning primitives first. Streaming integration can continue to use the existing server path until the planner is stable.

## Scope

In scope:

- Public request validation primitive for static/read-only serving.
- Public static response metadata planner for resolved files.
- Public directory listing response planner.
- Conditional request support.
- Range request support.
- HEAD parity.
- Error/status mapping tests.

Out of scope:

- ASGI/WSGI adapters.
- Python request callback server.
- Middleware.
- Routing.
- Full HTTP/2 or HTTP/3 semantics.
- Full cache policy framework.
- Reverse proxy behavior.
- General HTTP client.

## Current state

`service::handle_request` currently enforces `GET` and `HEAD`, validates no request body for those methods, parses the request target, resolves the resource, and returns responses built by internal helpers.

`response.rs` currently emits useful file headers: `Content-Length`, `Content-Type`, `Last-Modified`, weak `ETag`, and `X-Content-Type-Options: nosniff`. Directory listings escape visible text, percent-encode link segments, and add `Content-Security-Policy` plus `Referrer-Policy`.

Missing or not yet centralized:

- Public method/body/framing validation.
- `If-None-Match` handling.
- `If-Modified-Since` handling.
- Correct `304 Not Modified` response planning.
- `Range` request handling.
- `If-Range` handling.
- `206 Partial Content` planning.
- `416 Range Not Satisfiable` planning.
- A pure response-plan object that Python can consume without Hyper body types.

## Design constraints

Response planning must not depend on Hyper body types. A planner should produce a value object that can be mapped into Hyper, Python stdlib server responses, test assertions, or later adapter layers.

Suggested objects:

```rust
pub struct HeaderMapPlan { /* stable serializable header pairs */ }

pub enum BodyPlan {
    Empty,
    FullBytes(Vec<u8>),
    FileFull,
    FileRange { start: u64, end_inclusive: u64 },
}

pub struct StaticResponsePlan {
    pub status: StatusCodeLike,
    pub headers: HeaderMapPlan,
    pub body: BodyPlan,
}
```

Do not expose Hyper's `Response<BoxBodyInner>` as the primitive output. The existing server can translate `StaticResponsePlan` into Hyper internally.

## Implementation steps

### 1. Add request validation primitives

Create a public primitive module such as `primitives::http` or `primitives::request`.

Define:

- `RequestMethod` or method validation over string/bytes.
- `ReadOnlyMethod` with `Get` and `Head` variants.
- `RequestHeaderView` or a simple borrowed header adapter for Rust.
- `BodyFramingPolicy` with current default zero-body behavior.
- `RequestValidationError` with structured variants.

The public validator should answer:

- Is the method allowed for static/read-only handling?
- Is the request target syntactically supported?
- Do `Content-Length` and `Transfer-Encoding` indicate an unsupported body?
- Is `Content-Length` malformed, negative, overflowing, or above limit?
- Are both `Content-Length` and `Transfer-Encoding` present?

Keep the current behavior: unsupported methods map to 405, body-bearing read-only requests map to 413 or 400 depending on malformed versus oversized/framed-body condition.

### 2. Define response planning data structures

Create a public response planning module, e.g. `primitives::response`.

Define value objects independent of Hyper:

- `ResponseStatus` or use `http::StatusCode` if already a dependency and acceptable.
- `ResponseHeader` / `ResponseHeaders`.
- `StaticResponsePlan`.
- `StaticBodyPlan`.
- `FileRange`.
- `ConditionalRequestOutcome`.
- `RangeRequestOutcome`.

Headers should be serializable to Python-friendly `(name, value)` pairs later. Avoid exposing `HeaderValue` lifetimes or non-owned header values in public primitive objects.

### 3. Move current file metadata planning into the planner

Given a `ResolvedFile` from Plan 017 and a method, generate current baseline response metadata:

- `200 OK` for GET/HEAD.
- `Content-Length` matching file length.
- `Content-Type` from safe relative components.
- `Last-Modified` when available.
- Weak `ETag` using the current size/mtime format unless replaced by a documented stronger scheme.
- `X-Content-Type-Options: nosniff`.
- Empty body plan for HEAD.
- Full file body plan for GET.

The existing Hyper response builder should consume this plan where possible, or at least tests should prove equivalence before migration.

### 4. Implement conditional request support

Add support for:

- `If-None-Match`
- `If-Modified-Since`

For static files, the planner should return `304 Not Modified` when validators match for GET/HEAD. It should include correct headers for `304`: at minimum validators and headers relevant to caches. Do not include a body.

Be conservative with malformed conditional headers. Prefer ignoring invalid dates rather than rejecting the request unless there is a strong reason otherwise. Document the behavior.

Implement ETag matching rules carefully:

- Existing ETags are weak validators, formatted as `W/"size-mtime"`.
- `If-None-Match: *` matches any existing resource.
- Weak comparison is acceptable for `If-None-Match` on GET/HEAD.
- Multiple ETags in one header must be parsed or conservatively handled.

If full RFC coverage is too large for the first pass, implement a small, documented subset and add TODOs. Do not silently claim complete RFC compliance.

### 5. Implement range request support

Add support for simple byte ranges:

- `Range: bytes=start-end`
- `Range: bytes=start-`
- `Range: bytes=-suffix_length`

Return:

- `206 Partial Content` for valid satisfiable single ranges.
- `416 Range Not Satisfiable` with `Content-Range: bytes */<len>` for unsatisfiable ranges.
- `200 OK` if range syntax is unsupported or multiple ranges are intentionally not supported, depending on documented policy.

Recommended first pass: support exactly one byte range and reject or ignore multiple ranges explicitly. Multi-range MIME responses are not necessary for this project now.

For `If-Range`:

- If validator matches current ETag or Last-Modified, serve range.
- If it does not match, serve full `200 OK`.
- If malformed, prefer full response.

HEAD parity:

- HEAD with a satisfiable range should return the same status and headers as GET would, but empty body.
- HEAD with `304` or `416` should also have empty body.

### 6. Directory listing response planning

Move directory listing metadata into a planner:

- `200 OK`.
- `Content-Type: text/html; charset=utf-8`.
- `Content-Length` based on generated escaped HTML.
- `X-Content-Type-Options: nosniff`.
- restrictive CSP.
- `Referrer-Policy: no-referrer`.
- empty body for HEAD.

The HTML generation may remain internal, but the plan should be Python-friendly. Since directory listing is opt-in, keep the response conservative and test escaping.

### 7. Integrate with current service carefully

After planners are implemented and tested, update `service::handle_request` to use them if it reduces duplication. Keep streaming implementation internal.

Do not introduce behavior changes without tests. If migration is risky, leave service response construction in place and add equivalence tests plus a follow-up plan.

## Tests

Required request validation tests:

- GET allowed.
- HEAD allowed.
- POST rejected with method-not-allowed result.
- `Content-Length: 0` allowed for GET/HEAD.
- positive content length rejected with 413-equivalent result under zero-body policy.
- invalid content length rejected with 400-equivalent result.
- negative content length rejected.
- overflowing content length rejected.
- non-empty transfer encoding rejected.
- content length plus transfer encoding rejected.

Required conditional tests:

- file response includes ETag and Last-Modified.
- matching `If-None-Match` returns 304.
- nonmatching `If-None-Match` returns 200.
- `If-None-Match: *` returns 304 for existing resource.
- matching `If-Modified-Since` returns 304 when ETag condition is absent.
- stale `If-Modified-Since` returns 200.
- invalid `If-Modified-Since` does not crash and follows documented behavior.
- HEAD conditional behavior matches GET status/headers with empty body.

Required range tests:

- `bytes=0-0` returns 206 and one-byte range plan.
- `bytes=0-` returns range to EOF.
- `bytes=-10` returns suffix range.
- unsatisfiable range returns 416 with `Content-Range: bytes */len`.
- malformed range follows documented behavior.
- multiple ranges follow documented behavior.
- `If-Range` matching validator serves range.
- `If-Range` nonmatching validator serves full 200.
- HEAD range behavior has empty body but correct range status/headers.

Required directory listing tests:

- listing response escapes HTML special chars.
- listing response percent-encodes href segments.
- listing response includes CSP, referrer policy, nosniff.
- HEAD listing response has same content length and empty body.

## Documentation acceptance criteria

Add `docs/http-response-planning.md` documenting:

- request validation policy;
- status mapping;
- conditional request support and limitations;
- range support and limitations;
- HEAD parity;
- how response plans are intended to be consumed by Python or Rust callers;
- explicit non-goal of full framework/runtime behavior.

## Validation

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
```

If available:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This plan is complete when:

- Request validation is available as a public primitive independent of the server loop.
- Static file response planning is available as a Hyper-independent value object.
- Conditional GET/HEAD behavior is implemented and tested.
- Single-range byte serving is implemented and tested.
- Directory listing planning remains safe and tested.
- Existing CLI/static server behavior is preserved or intentionally improved with tests.
