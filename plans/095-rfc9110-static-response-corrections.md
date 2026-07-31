# Plan 095 — RFC 9110 Static Response Corrections

## Goal

Correct the known HTTP semantics defects in EggServe's existing static and canonical response paths before building the `http.server` compatibility facade.

This is a narrow correctness pass. It does not add new protocol versions, new static-serving features, or a new verification system.

## Why this plan comes first

The Python compatibility facade planned in Plan 096 must delegate to a protocol-correct core. Building compatibility behavior before correcting shared response semantics would either duplicate fixes in Python or preserve defects behind a new API.

All corrections in this plan should land at the narrowest shared Rust layer so that the CLI, native Python server, future compatibility facade, static service, and handler-generated responses converge on the same behavior.

## Required outcomes

1. Valid HTTP status codes are limited to 100 through 599 inclusive.
2. Responses with status 205 do not transmit content.
3. `If-Range` uses strong entity-tag comparison.
4. Weak, malformed, empty, or nonmatching `If-Range` values cause the Range request to be ignored and a full 200 response to be generated.
5. Applicable origin responses receive a valid `Date` field through one common finalization path.
6. Generated directory-listing HEAD responses retain the `Content-Length` of the equivalent GET representation.
7. Existing GET, HEAD, conditional, range, error, and callback response behavior remains otherwise unchanged.
8. Targeted tests cover the corrected externally observable behavior without adding a new conformance framework.

## Governing constraints

- Remain HTTP/1.1 only.
- Retain single-range support only.
- Do not add content hashing merely to create strong ETags.
- Continue emitting the existing weak metadata ETag for ordinary cache validation.
- Do not add automatic compression, trailers, upgrades, HTTP/2, or HTTP/3.
- Do not redesign the canonical response model.
- Do not add a new dependency unless the current dependency set cannot format a correct IMF-fixdate; `httpdate` is already present and should be reused.
- Do not add a new response middleware stack.
- Do not change routine CI shape.
- Do not restore scheduled fuzzing, evidence files, release automation, or platform matrices.
- Do not change package or crate versions.
- Do not broaden Windows support claims.

## Required file inspection

Before implementation, inspect at least:

- `crates/eggserve-core/src/primitives/planner.rs`
- `crates/eggserve-core/src/primitives/canonical.rs`
- `crates/eggserve-core/src/primitives/response.rs`
- `crates/eggserve-core/src/response.rs`
- `crates/eggserve-core/src/service.rs`
- `crates/eggserve-core/src/server/connection.rs`
- `crates/eggserve-core/src/server/static_service.rs`
- `crates/eggserve-core/tests/http_wire_correctness.rs`
- `crates/eggserve-core/tests/http_primitives_integration.rs`
- `crates/eggserve-core/tests/canonical_conformance.rs`
- `crates/eggserve-core/tests/canonical_wire_interop.rs`
- Python tests that assert status, conditional, range, Date, or HEAD behavior
- `docs/http-primitives.md`
- `docs/http-response-planning.md`
- `architecture/response-planning.md`

Search for all independent response construction paths:

```sh
rg -n "Response::builder|planned_response|canonical_error|normalize_response|normalize_metadata|to_hyper_response" crates/eggserve-core
rg -n "generate_etag|evaluate_if_range|If-Range|if_range" crates/eggserve-core crates/eggserve-python docs architecture
rg -n "100\.\.=999|100\.\.=599|permits_payload_body|RESET_CONTENT|205" crates/eggserve-core crates/eggserve-python
rg -n '"date"|DATE' crates/eggserve-core/src
```

The implementation must identify the actual final common path before deciding where `Date` belongs.

## Track A — Correct status-code validation

### Current defect

The canonical `StatusCode::new()` accepts values through 999. HTTP status codes are three digits in the range 100 through 599.

The Python boundary also accepts `u16` values and may normalize invalid values inconsistently.

### Required implementation

1. Change the canonical validator to accept only `100..=599`.
2. Ensure all constants remain valid.
3. Update documentation and comments that state `100–999`.
4. Ensure conversion from Python callback responses rejects or maps 600–999 consistently.
5. Prefer explicit rejection at construction time for public factories.
6. At the transport boundary, invalid handler output must produce a controlled 500 response rather than panic or serialize an invalid status.
7. Do not silently clamp invalid values.

### Required tests

At minimum:

- 99 rejected;
- 100 accepted;
- 199 accepted;
- 200 accepted;
- 599 accepted;
- 600 rejected;
- 999 rejected;
- invalid Python handler status produces the documented controlled failure;
- no invalid status reaches Hyper conversion.

Consolidate existing boundary tests rather than duplicating all cases in every language layer.

## Track B — Treat 205 as body-forbidden

### Current defect

The canonical payload-permission helper suppresses content for 1xx, 204, and 304, but not 205 Reset Content.

### Required implementation

1. Add `RESET_CONTENT` if a constant improves clarity.
2. Update `permits_payload_body()` so 205 returns false.
3. Ensure `normalize_response()` discards a provided body for 205.
4. Ensure `normalize_metadata()` does not emit `Content-Length` for 205.
5. Preserve all non-framing response fields.
6. Do not add special 205 behavior outside the canonical path.

### Required tests

- buffered 205 body becomes empty;
- 205 has no `Content-Length` after normalization;
- Python handler returning 205 plus bytes sends no body;
- adjacent statuses such as 204 and 206 retain their existing behavior.

## Track C — Correct `If-Range`

### Current defect

The planner currently delegates entity-tag evaluation to weak `If-None-Match` comparison. EggServe emits weak ETags, so the existing code can incorrectly authorize a partial response from a weak validator.

The caller also distinguishes only `FullResponse` from other conditional outcomes, so malformed or empty values can accidentally permit the Range request.

### Required semantics

For a satisfiable Range request:

- no `If-Range`: serve 206;
- strong entity-tag exactly matching a current strong ETag: serve 206;
- weak entity-tag: ignore Range and serve full 200;
- nonmatching entity-tag: ignore Range and serve full 200;
- valid HTTP-date exactly matching the selected last-modified validator under the repository's documented date policy: serve 206;
- stale date: full 200;
- malformed date: full 200;
- empty `If-Range`: full 200.

EggServe currently emits only weak metadata ETags. Therefore, an `If-Range` entity-tag cannot match the current generated ETag under strong comparison. This is acceptable and correct. Date-based `If-Range` remains usable.

### Required implementation

1. Add a dedicated `If-Range` evaluation result or use a clear boolean that means "range authorized".
2. Do not reuse the weak `If-None-Match` helper.
3. Implement strong entity-tag comparison directly and narrowly.
4. Reject weak tags as nonmatching.
5. Treat malformed and empty values as nonmatching.
6. Ensure an unsatisfiable Range remains 416 independently of `If-Range` only where the existing planner and RFC ordering require it; verify this behavior against the selected normative interpretation and existing tests.
7. Keep unsupported range units and multipart ranges under the repository's existing documented fallback behavior unless a separate demonstrated defect is found.
8. Update comments and docs that currently say weak ETags can satisfy `If-Range`.

### Required tests

Planner-level tests:

- weak current ETag plus identical weak `If-Range` -> full 200;
- weak current ETag plus syntactically strong equivalent tag -> full 200;
- malformed tag -> full 200;
- empty header -> full 200;
- matching Last-Modified date -> 206;
- stale Last-Modified date -> 200;
- malformed date -> 200;
- no `If-Range` -> 206;
- HEAD follows the same status/header selection but transmits no body.

Wire-level tests:

- weak-tag `If-Range` response is 200 with full representation length;
- matching-date `If-Range` response is 206 with correct `Content-Range` and length.

Do not introduce a separate JSON corpus for these cases.

## Track D — Add `Date` centrally

### Current defect

Static, error, range, conditional, and handler-generated response paths do not consistently add `Date`.

### Required design

`Date` is origin-server metadata and should be added once, late, after producer-specific response construction but before transport serialization.

The implementation should use the narrowest common finalization point shared by:

- canonical in-memory responses;
- file-backed static responses;
- planned empty responses;
- error responses;
- callback responses;
- range responses;
- HEAD responses;
- TLS and plaintext connections.

If no single common function currently covers file-backed and in-memory responses, extract one small internal helper such as:

```rust
fn finalize_origin_headers(headers: &mut HeaderBlock, now: SystemTime)
```

or an equivalent name. Do not create a general middleware abstraction.

### Required behavior

1. Generate a valid IMF-fixdate using `httpdate`.
2. Add `Date` when absent on applicable origin responses.
3. The runtime, not a Python handler, owns `Date` for the normal server path.
4. Decide one policy for handler-supplied `Date`:
   - preferred: replace it with runtime-generated Date;
   - acceptable only with explicit rationale: preserve a syntactically valid supplied Date.
5. Apply the selected policy consistently to all response producers.
6. Do not emit duplicate `Date` fields.
7. Do not add a global clock service. Tests can call a helper with a fixed `SystemTime` or assert parseability and bounded proximity at the wire layer.
8. Do not add Date to non-HTTP internal planner values unless doing so is necessary for the finalization architecture.

### Required tests

- full static 200 has one parseable Date;
- 206 has one Date;
- 304 has one Date;
- 404 and 405 have one Date;
- handler-generated 200 has one Date;
- HEAD has one Date;
- TLS and plaintext share the same finalization path where existing TLS tests can cover this cheaply;
- fixed-time unit test verifies exact formatting;
- no duplicate Date when a producer attempted to add one.

Avoid assertions that require exact wall-clock equality in live tests.

## Track E — Correct directory-listing HEAD metadata

### Current defect

The directory listing response computes the GET representation bytes, but passes zero as the body length to canonical metadata normalization for HEAD. This removes the `Content-Length` that should describe the equivalent GET response.

### Required implementation

1. Continue generating the same safe escaped HTML representation.
2. Compute its length once.
3. Pass that representation length to metadata normalization for both GET and HEAD.
4. Suppress only the body bytes for HEAD.
5. Preserve listing security fields, including `Content-Security-Policy`, `Referrer-Policy`, and `X-Content-Type-Options`.
6. Do not enable directory listing by default.
7. Do not redesign the listing template in this plan.

### Required tests

- listing GET and HEAD have the same status;
- listing GET and HEAD have the same `Content-Type`;
- listing GET and HEAD have the same nonzero `Content-Length`;
- HEAD body is empty;
- GET body length matches the field;
- unsafe names remain escaped and encoded under the existing tests.

## Track F — Response-finalization audit

After implementing Tracks A–E, audit all response producers for bypasses.

The goal is not to refactor every path. The goal is to prove that the corrected invariants apply everywhere they need to.

Create a temporary table during implementation:

```text
producer
status source
header representation
body representation
normalization function
Date insertion point
transport conversion
covered test
```

Include at least:

- static full file;
- static range;
- static 304;
- static 416;
- directory listing;
- canonical error;
- service timeout/error;
- Python callback bytes;
- Python callback empty body;
- TLS path.

Delete the temporary table before completion unless it materially improves an existing architecture document. Do not create a new permanent registry.

If one producer bypasses the finalization helper, make the smallest correction necessary. Do not use this audit to redesign unrelated server modules.

## Documentation updates

Update active documentation that describes affected semantics:

- `docs/http-primitives.md`
- `docs/http-response-planning.md`
- `architecture/response-planning.md`
- `docs/python-api.md` only where callback behavior is currently described
- any API stability or capability matrix statement that says status 100–999

Required documentation corrections:

- status range 100–599;
- 205 body prohibition;
- weak ETags remain valid for `If-None-Match` but cannot satisfy `If-Range`;
- date-based `If-Range` behavior;
- Date generation ownership;
- HEAD listing metadata parity.

Do not rewrite unrelated deployment or platform documentation.

## Suggested commit sequence

Keep commits reviewable and independently testable:

1. `fix: constrain status codes and suppress 205 bodies`
2. `fix: enforce strong If-Range semantics`
3. `fix: centralize Date response finalization`
4. `fix: preserve directory listing HEAD metadata`
5. `test/docs: close RFC 9110 response corrections`

Combining adjacent commits is acceptable when the implementation seam makes separation artificial. Do not mix Python compatibility facade work into this phase.

## Verification

Run targeted tests during implementation, then the existing standard checks.

Example targeted commands; adjust exact test names to the repository:

```sh
cargo test -p eggserve-core planner
cargo test -p eggserve-core canonical
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test canonical_wire_interop
cargo test -p eggserve-core --test http_primitives_integration
```

Then:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Do not add a new verification mode.

## Acceptance criteria

Plan 095 is complete only when all of the following are true on the same final commit:

- `StatusCode::new(599)` succeeds.
- `StatusCode::new(600)` fails.
- Python handler statuses above 599 cannot reach the wire.
- 205 responses send no body and no `Content-Length`.
- Weak `If-Range` never authorizes a 206 response.
- Empty or malformed `If-Range` causes full 200 for an otherwise satisfiable Range request.
- Matching valid date `If-Range` can authorize 206.
- Static, error, callback, range, conditional, and HEAD responses contain exactly one parseable Date where required.
- Directory-listing HEAD has the same representation `Content-Length` as GET and no body.
- Existing full-file, range, conditional, error, and HEAD tests remain green.
- No new dependency was added without explicit necessity.
- No new routine workflow or CI job was added.
- `verify.sh fast` passes.
- `verify.sh full` passes.
- Both current routine CI jobs pass on the final commit.

## Explicit non-goals

This plan does not implement:

- the Python compatibility facade;
- `index.htm` lookup;
- trailing-slash redirects;
- TLS Python classes;
- multi-range responses;
- strong content-hash ETags;
- cache-control policy;
- compression;
- authentication;
- redirects outside directory canonicalization;
- HTTP/2 or HTTP/3;
- general header middleware;
- public API cleanup;
- release publication.

Those boundaries are mandatory. Any additional defect discovered should be fixed only if it is directly required to make Tracks A–F correct; otherwise record it for the next applicable plan.