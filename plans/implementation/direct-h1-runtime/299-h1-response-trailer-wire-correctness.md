# Direct H1 Runtime Milestone 299 — H1 response-trailer wire correctness

Status: ready for handoff

Repository baseline: `c1f4348939406c02f9739cdedff3bf7568ce09c6`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-8--h1-response-trailer-wire-correctness`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Historical capability contract:

- `plans/198-canonical-trailers-and-interim-responses.md`
- `docs/http-primitives.md#canonical-trailers-and-interim-responses-plan-198`

Primary class: invariant

## 1. Objective

Repair F3: validated response trailers currently reach the canonical/Hyper body as terminal trailer frames but are silently absent from HTTP/1.1 wire output because EggServe strips application `Trailer` framing metadata and never supplies Hyper the required head-time trailer declaration.

Restore the documented H1 contract without redesigning trailers generally:

- HTTP/1.1 + client `TE: trailers` + a response with a valid head-time trailer declaration → terminal trailers appear on the wire;
- HTTP/1.1 without opt-in and HTTP/1.0 → trailer producer is suppressed/dropped without polling and no `Trailer` header is emitted;
- application code never directly owns transfer coding or the final wire `Trailer` header;
- H2/H3 terminal trailer behavior remains protocol-native and unchanged.

## 2. Why this milestone is ready

The gap has reproducible raw-wire evidence from Plan 294 and is reaffirmed in Plans 295 and 297 as a medium-severity correctness residual. Canonical `Trailers`, bounded validation, `ResponseStream` trailer futures, H1 TE negotiation, and terminal Hyper `Frame::trailers` emission already exist.

Hard dependencies are closed. No performance milestone is reopened.

## 3. Current implementation evidence

At the baseline:

- `ResponseStream::with_trailers` / `with_known_length_and_trailers` carry one terminal trailer future but no head-time field-name declaration.
- `h1_trailers_allowed` permits HTTP/1.1 trailers only when request `TE` contains `trailers`; HTTP/1.0 suppresses them.
- when policy suppresses trailers, `Response::strip_response_trailers()` drops the trailer future without polling.
- canonical normalization owns framing and strips hop-by-hop/application `Trailer` and `Transfer-Encoding`.
- `ResponseStreamAdapter` correctly converts validated terminal `Trailers` into a Hyper `Frame::trailers`.
- raw Plan-294 tests show both native and direct-Tower bodies arrive but `x-end: yes` is absent from H1 wire output.
- the Plan-294 closure attributes this to Hyper requiring the initial response head to declare trailers while EggServe never synthesizes that runtime-owned declaration.
- known-length streams can currently normalize to `Content-Length`; H1 responses that actually carry terminal trailers must instead use legal trailer-capable framing.

## 4. Invariants that must not regress

- EggServe remains the sole H1 framing authority.
- Applications cannot directly force `Transfer-Encoding` or a raw wire `Trailer` header.
- Canonical trailer field/value validation remains single-source in `eggserve-primitives::Trailers`; do not duplicate the forbidden-field registry.
- Trailer declaration metadata is bounded and validated before response commitment.
- Actual H1 trailer fields must not exceed/escape the declared head-time field-name set; violation after commitment fails closed rather than emitting undeclared metadata or a second response.
- No full-response buffering and no early polling of the terminal trailer future solely to discover names.
- HTTP/1.0 never emits response trailers.
- HTTP/1.1 without `TE: trailers` never emits response trailers and does not poll the suppressed producer.
- HEAD and body-forbidden statuses do not poll body/trailer producers and do not advertise terminal trailers.
- H2/H3 protocol-native trailer paths and failure isolation remain unchanged.
- Native Service, direct Tower/Axum, and core compatibility H1 all converge on the single direct H1 authority.

## 5. Scope

### In scope

- reproduce F3 with a dedicated raw TCP test before changing production code;
- verify the exact Hyper H1 encoder requirement at the current locked dependency version;
- add bounded transport-neutral head-time trailer-name declaration metadata if required;
- synthesize the runtime-owned H1 `Trailer` response header only when H1 policy permits trailers;
- ensure H1 trailer-bearing responses use legal framing (remove conflicting `Content-Length`; allow Hyper to select chunked transfer rather than application-controlled TE);
- validate actual terminal trailer names against the declared set;
- define the compatibility behavior for existing `ResponseStream::with_trailers` callers that do not provide declarations;
- provide a clean direct-Tower/Axum path for head-time declaration without letting ecosystem responses own raw framing;
- raw-wire native/Tower/core-compat parity tests;
- docs and semver/publication classification.

### Explicitly out of scope

- no redesign of request trailers or interim responses;
- no H2/H3 trailer redesign/promotion;
- no replacement of the deferred `Arc<Mutex>` Tower trailer rendezvous from Plan 295;
- no broad canonical request/response model rewrite;
- no buffering body bytes until the trailer future completes;
- no custom H1 encoder;
- no reopening of file/tunnel feature splits or performance work;
- no publication unless separately directed after closure evidence.

## 6. Required production changes

### Crates and ownership

Expected owners:

- `eggserve-primitives`: only neutral declaration metadata/API needed to make anticipated trailer field names available before body polling. It must remain Hyper/Tokio/Tower-free.
- `eggserve-server`: H1 negotiation, runtime-owned initial `Trailer` declaration, H1 framing adjustment, actual-vs-declared enforcement, raw-wire tests.
- `eggserve-core`: compatibility forwarding/tests only; no second H1 implementation.
- `eggserve-h3`: no behavior change except regression tests if shared primitives change.

Do not create a new crate.

### Config and policy

No new `RuntimeConfig` knob. Trailer willingness remains request-derived (`TE: trailers`) and existing suppression policy remains default.

### Protocol and compatibility

#### Head-time declaration

If Hyper requires anticipated field names before response commitment, add the minimum bounded canonical representation needed for those names.

Preferred shape is additive, e.g. declaration-aware `ResponseStream` construction or a small neutral declaration value; exact API name is implementation-dependent. Requirements:

- names use canonical header-name validation;
- declaration rejects fields forbidden by the existing trailer policy;
- declaration has explicit count/byte bounds aligned with or stricter than `TrailerLimits`;
- duplicate/case-equivalent names are canonicalized or rejected deterministically;
- declaration contains names only, never values;
- declaration survives until H1 response-head construction but is not serialized by generic canonical normalization itself.

Do not poll the trailer future early to derive names.

#### Existing no-declaration constructors

Preserve source compatibility for `with_trailers` / `with_known_length_and_trailers`.

For H2/H3 they continue to work as today.

For H1, if a trailer source exists but no valid head-time declaration is available, do not silently promise wire delivery. Suppress/drop the H1 trailer producer before commitment with a specific diagnostic (existing suppression event may be extended with a reason), or choose another equally safe behavior justified by the raw-wire/Hyper evidence. Document the migration path to declaration-aware construction.

#### Direct Tower/Axum

Tower bodies can produce trailer frames only after data, so the adapter also needs head-time declaration input. Prefer consuming a validated ecosystem `Trailer` declaration as semantic input while stripping it from ordinary application headers and regenerating the wire header under EggServe authority, or another bounded explicit adapter mechanism.

Whichever approach is chosen:
- malformed/forbidden declarations fail before commitment;
- application `Transfer-Encoding` remains ignored/rejected under existing runtime ownership;
- actual terminal trailer fields must be a subset of declared names;
- no declaration means H1 suppression, not silent frame loss;
- native and Tower semantics converge.

#### H1 framing

For an allowed trailer-bearing HTTP/1.1 response:
- the initial head contains a runtime-generated `Trailer: name[, name...]`;
- no conflicting `Content-Length` is sent, including for `with_known_length_and_trailers`;
- Hyper/runtime selects legal chunked H1 framing;
- terminal zero chunk is followed by the validated trailer section;
- actual body-byte known-length checks may still protect producer correctness internally, but must not force illegal wire `Content-Length` framing.

### Runtime and concurrency

Keep current pull/backpressure semantics. Trailer future is polled only after body completion. Cancellation/drop must release both body and trailer producer. Failure after response commitment closes/truncates the H1 connection without synthesizing a second response.

### Frontend or operator surface

No CLI/Python high-level feature expansion. Update Rust/direct-embedding docs and examples only if the declaration API is public.

### Security and confinement

Trailer declarations and emitted fields must remain bounded. No user-supplied field may smuggle framing, routing, connection, authentication, or payload-processing metadata forbidden by the canonical trailer validator.

### Documentation and static guards

Update `docs/http-primitives.md`, `architecture/runtime.md`, direct-server/interop docs, and conformance inventory. Add a regression guard/test that fails if the H1 wire trailer disappears again while terminal frames still exist internally.

## 7. Ordered work packages

### Work package A — Freeze F3 reproduction and Hyper contract

Create a focused raw-TCP test that:
- sends HTTP/1.1 `TE: trailers`;
- exercises native `ResponseStream` and Tower/Axum terminal trailer responses;
- proves data arrives but terminal trailer is absent at the baseline.

Confirm against the locked Hyper version what head metadata is required for the H1 encoder to serialize trailer frames. Record the evidence in the eventual closure.

### Work package B — Neutral declaration contract

Implement the smallest head-time declaration representation/API needed for anticipated trailer names.

Acceptance:
- bounded/validated;
- transport-neutral;
- old constructors remain source-compatible;
- no trailer future poll during response-head construction;
- suppression can remove both trailer producer and declaration metadata.

If solving F3 requires redesigning `Service`, `Response`, or generic H2/H3 semantics, stop and create an ADR/new plan rather than expanding 299.

### Work package C — H1 head/framing synthesis

At the single direct H1 boundary:
- when request policy permits and a valid declaration exists, synthesize the `Trailer` head field under runtime ownership;
- prevent `Content-Length` wire framing for that response;
- preserve internal known-length byte-count validation if useful;
- allow Hyper to emit legal chunked framing and terminal trailer frames.

Do not hand-build chunked bytes.

### Work package D — Actual-vs-declared enforcement

When the terminal trailer future resolves:
- validate values through existing `Trailers`;
- enforce that every actual field name was declared for H1;
- allow declared-but-omitted fields if standards/Hyper behavior supports it; document the decision;
- on undeclared/invalid output after commitment, fail closed and close/truncate the connection with sanitized diagnostics.

### Work package E — Tower/Axum declaration bridge

Provide the minimal direct-Tower route for anticipated names. If using an ecosystem `Trailer` header as declaration input, treat it only as a validated declaration request: strip it from ordinary headers, store neutral names, and let the runtime regenerate the final wire field.

Add Axum/Tower fixture coverage.

### Work package F — Protocol/suppression matrix

Raw-wire matrix:

1. H1.1 + `TE: trailers` + declaration + unknown-length body → wire trailer present.
2. H1.1 + `TE: trailers` + declaration + known-length body → no CL on wire; chunked + trailer present.
3. H1.1 without `TE: trailers` → no Trailer head/trailer section; producer not polled.
4. H1.0 → no trailers; producer not polled.
5. HEAD / 1xx / 204 / 205 / 304 as applicable → no body/trailer producer poll and no Trailer advertisement.
6. declaration with forbidden/malformed name → reject before commitment.
7. actual undeclared trailer → committed-response failure/close, no metadata leak.
8. duplicate legal trailer values/order preservation.
9. native vs Tower vs core-compat H1 convergence.
10. H2/H3 existing trailer tests remain green and do not acquire an H1 `Trailer` header requirement.

### Work package G — Version/release disposition

Classify:
- `eggserve-primitives` semver if a new public declaration API lands;
- `eggserve-server` version interaction with the already-deferred 0.4.0 release from Plan 297;
- core/static/bin/Python impact.

Do not publish from this milestone unless the maintainer explicitly expands scope. Closure should state the exact next-release requirement.

## 8. Failure, cancellation, restart, and contention semantics

- client disconnect during data/trailer phase releases producer state;
- trailer future error/panic after commitment closes the stream/connection with sanitized diagnostics;
- undeclared actual trailer behaves like a committed producer/framing failure, never a second HTTP response;
- shutdown with active trailer-bearing streams follows existing graceful-drain timeout;
- no locks/tasks/channels are added solely for head-time declaration;
- no response waits for terminal trailer values before sending body bytes.

## 9. Compatibility and migration

Preserve existing public constructors. If a new declaration-aware constructor/builder is added, document that H1 wire delivery requires anticipated names while H2/H3 can continue carrying terminal trailers without H1 declaration semantics.

If Tower begins interpreting the ecosystem `Trailer` response header as declaration metadata, document this as an adapter contract and prove it cannot bypass runtime framing ownership.

Because Plan 297 already reserved the next `eggserve-server` release as 0.4.0 for a feature-edge change, 299 must record whether its changes can ride that release. Do not automatically bump unrelated crate versions.

## 10. Required tests

### Focused unit tests

- declaration validation/bounds/forbidden fields/case handling;
- strip/suppression drops declaration + future without polling;
- actual-vs-declared subset enforcement;
- known-length internal accounting remains correct.

### Integration tests

- dedicated raw H1 wire suite covering the matrix in WP-F;
- direct Tower/Axum trailer response;
- core compatibility H1 parity;
- keep-alive/connection-close behavior after successful and failed trailers.

### Restart and recovery tests

- server shutdown with an active trailer-bearing stream;
- repeated connections after successful trailer responses.

### Contention and cancellation tests

- concurrent trailer-bearing streams under normal service admission;
- disconnect before terminal trailers;
- trailer future pending at shutdown.

### Security and negative tests

- forbidden `content-length`, `transfer-encoding`, `trailer`, `connection`, auth/routing-sensitive fields per existing validator;
- undeclared actual field;
- malformed ecosystem `Trailer` declaration if Tower bridge uses it;
- no header injection via declaration formatting.

### Migration and compatibility tests

- old `with_trailers` constructors compile;
- H2/H3 existing tests unchanged/green;
- direct server `tower` no-default profile green;
- core compatibility forwarding green.

## 11. Required verification commands

```bash
cargo test -p eggserve-server --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
./scripts/verify.sh fast
```

If shared primitives/H2/H3 code changes, also run the affected `http2,tls` / `http3,tls` lanes or `./scripts/verify.sh full` as required by the diff. Raw-TCP wire output is mandatory evidence; a Hyper client abstraction alone is insufficient.

## 12. Documentation updates

At minimum:
- `docs/http-primitives.md`;
- `architecture/runtime.md`;
- direct server / HTTP interop documentation;
- any public primitives API docs for declaration-aware constructors;
- conformance/test inventories;
- source roadmap + registry + 299 closure record.

Correct the old statement “Trailer header naming is omitted when not knowable (allowed)” if the implemented Hyper/H1 contract proves head-time declaration is required for EggServe's supported wire behavior.

## 13. Acceptance criteria

- Raw HTTP/1.1 wire contains the expected terminal trailer section when the client opts in and the response supplies valid anticipated names.
- The initial head contains a runtime-generated Trailer declaration and no conflicting Content-Length.
- Known-length response streams retain byte-count safety but use legal H1 trailer framing.
- HTTP/1.0, H1.1 without TE opt-in, HEAD/body-forbidden responses, and undeclared H1 trailer sources do not silently lose a polled trailer producer; they are suppressed before polling with explicit semantics.
- Native, Tower/Axum, and core compatibility H1 converge.
- Actual trailer fields cannot escape the declared/validated field-name set.
- H2/H3 existing protocol-native trailer behavior remains green.
- No second framing authority, full-body buffering, custom H1 encoder, or early trailer-future polling is introduced.
- Semver/release impact is explicitly recorded.

## 14. Stop conditions

Stop and report rather than improvise if:
- Hyper cannot safely serialize trailers without replacing/customizing the H1 encoder;
- the fix requires buffering the response body or polling the trailer future before body completion;
- a solution requires weakening trailer validation/framing ownership;
- the public declaration model would require a broad `Service`/canonical-response redesign;
- H2/H3 semantics would need incompatible changes;
- the apparent F3 cause differs materially from the Plan-294 raw-wire finding.

A durable cross-protocol contract redesign requires a new ADR/milestone.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md` containing:

- baseline F3 raw-wire reproduction;
- Hyper encoder requirement evidence;
- final declaration/framing design;
- native/Tower/core-compat raw-wire transcripts or byte assertions;
- suppression/non-polling evidence;
- actual-vs-declared negative evidence;
- H2/H3 regression results;
- exact verification commands/outcomes;
- semver/publication disposition;
- residual findings and roadmap/registry closure decision.

## 16. Handoff notes

F3 is correctness, not a performance opportunity. Do not combine it with the deferred Tower trailer-rendezvous optimization from Plan 295. The goal is the smallest standards-correct H1 wire repair that preserves EggServe's single framing authority.
