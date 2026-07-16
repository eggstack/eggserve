# Phase 59 — Milestone 4 Final Closure and Verification

## Goal

Fully close Milestone 4 by resolving the remaining HTTP/1 framing strictness, duplicate length handling, stream-consumption, drain-or-close, public API hygiene, Python iterator cancellation, error-mapping, and release-evidence gaps in Plans 056–058.

The bounded request-body architecture already exists. This phase is a corrective protocol and verification pass. It must not add multipart parsing, form decoding, upload persistence, decompression, routing, middleware, ASGI/WSGI channels, async Python handlers, HTTP/2, reverse proxy behavior, or application-framework semantics.

## Starting state

The repository now has:

- transport-independent `Request`, `RequestBody`, `RequestBodyPolicy`, `RequestBodyError`, and `IncompleteBodyPolicy` types;
- explicit reject, bounded-buffer, and bounded-stream policies;
- fixed-length and chunked transfer-decoded body ingestion;
- byte limits, body timeout, cancellation, and shutdown integration;
- partial-consumption close/drain behavior;
- static service body rejection by default;
- Python `RequestBody` read and chunk-iteration APIs backed by Rust;
- shared Rust/Python body conformance fixtures;
- raw-wire, TLS, property, cancellation, fuzz, benchmark, and installed-wheel tests;
- dedicated body release criteria and generated checklist entries.

Remaining concerns:

1. requests containing both `Transfer-Encoding` and `Content-Length` are currently represented as “TE wins” rather than being rejected;
2. duplicate `Content-Length` policy is permissive but not sufficiently explicit or proven at the raw-header boundary;
3. a partial-consumption conformance fixture uses buffer mode, where no unread transport bytes remain;
4. drain ownership, bounds, and keep-alive sequencing need a final architecture audit;
5. Hyper `Incoming` adaptation may be exposed more broadly than the public transport-independent contract permits;
6. repeated body consumption may silently return empty data rather than a typed `AlreadyConsumed` error;
7. body error-to-HTTP mappings need role-appropriate review;
8. Python iterator drop, timeout, and shutdown cancellation need focused resource tests;
9. Milestone 4 has not yet been qualified through one inspected final-SHA evidence bundle.

## Track A — Reject ambiguous request framing

Adopt a strict hardened HTTP/1 framing policy.

Required invariant:

> A request containing both `Transfer-Encoding` and any `Content-Length` field is rejected before service invocation.

Required behavior:

- return `400 Bad Request`;
- do not invoke the service;
- do not expose a `RequestBody` to downstream code;
- close the connection after the error response;
- apply the same behavior to plaintext and TLS;
- apply the same behavior to Rust and Python callback services;
- do not depend on Hyper silently preferring one framing signal.

Implementation audit:

- determine whether Hyper rejects, normalizes, or preserves the conflicting framing before eggserve sees the request;
- if Hyper rejects first, add a raw-wire production-path test proving the resulting wire behavior;
- if both signals remain visible, add an explicit pre-service framing validator;
- ensure no alternate path constructs a body after conflict detection;
- ensure the error is typed distinctly enough for conformance and logging.

Update `conformance/body_corpus.json` so every TE+CL fixture expects:

- status `400`;
- `handler_called: false`;
- connection closure.

Add cases for:

- `Transfer-Encoding: chunked` plus one `Content-Length`;
- chunked plus duplicate identical lengths;
- chunked plus conflicting lengths;
- mixed header casing;
- comma-combined length values where the transport exposes them;
- whitespace variants;
- TLS and plaintext parity.

Acceptance:

- no TE+CL request reaches a service;
- corpus, raw-wire, TLS, Rust, and Python tests agree;
- the release contract explicitly documents strict rejection.

## Track B — Finalize duplicate Content-Length policy

Choose one deterministic policy and apply it before body construction.

Preferred hardened policy:

- reject every request with more than one `Content-Length` field or more than one parsed length value, even when values are identical.

This policy is preferred because it minimizes intermediary disagreement and simplifies auditability.

If the project deliberately retains identical-value acceptance, requirements are stricter:

- values must be strictly decimal;
- no sign, leading plus, embedded whitespace, or overflow;
- all values must be byte-for-byte or numerically identical according to one documented rule;
- comma-combined values must be handled explicitly;
- conflicting values must always be rejected;
- acceptance must be proven before any duplicate-collapsing map conversion;
- the security rationale must be documented.

Tests must cover:

- one valid length;
- duplicate identical fields;
- duplicate conflicting fields;
- comma-combined identical values;
- comma-combined conflicting values;
- empty value;
- leading/trailing optional whitespace;
- leading zeros;
- overflow;
- negative and signed forms;
- non-decimal values;
- mixed casing;
- interaction with `Transfer-Encoding`.

Acceptance:

- one policy is encoded in code, corpus, docs, and tests;
- raw-header behavior is proven before lossy normalization;
- invalid or ambiguous lengths never invoke a service.

## Track C — Separate buffer and stream consumption semantics

Correct conformance fixtures and documentation so transport consumption is not confused with application-level inspection.

### Buffer mode

Required semantics:

- the runtime consumes and validates the complete body before service invocation;
- size limit, framing, premature EOF, and timeout errors occur before the handler;
- the handler receives a finite in-memory body abstraction;
- partial application reads do not leave unread network bytes;
- connection reuse is unaffected by the handler reading only part of the buffered value;
- repeated consumption follows the one-shot API contract defined in Track E.

### Stream mode

Required semantics:

- the handler consumes transfer-decoded chunks incrementally;
- the runtime tracks completion separately from “some bytes read”;
- returning before EOF triggers `IncompleteBodyPolicy`;
- connection reuse occurs only after complete consumption or successful bounded drain;
- malformed remainder, timeout, cancellation, or drain failure closes the connection.

Replace the current `partial_consumption_close` buffer fixture with distinct fixtures:

1. buffer body partially inspected, then response returned — connection may remain reusable;
2. stream body partially consumed with `Close` — response completes and connection closes;
3. stream body partially consumed with `Drain` — bounded drain succeeds, then next request is accepted;
4. drain timeout — connection closes;
5. malformed chunk remainder during drain — connection closes;
6. body over limit during drain — connection closes and limit error is recorded;
7. fully consumed stream — keep-alive remains available.

Acceptance:

- corpus language distinguishes object consumption from transport consumption;
- partial stream behavior is tested over a two-request keep-alive connection;
- buffer mode never closes solely because application code read only part of the buffered value.

## Track D — Prove drain ownership and boundedness

Audit the connection/body architecture for a single unambiguous owner of unread body frames after service completion.

Required invariants:

- the service and drain task cannot read the body concurrently;
- ownership transfers exactly once after service return;
- draining begins before the connection parser accepts the next request;
- drain is bounded by remaining byte budget and timeout;
- decoded-byte limits continue to apply during drain;
- malformed framing, premature EOF, timeout, or cancellation terminates the connection;
- shutdown cancels drain promptly;
- forced shutdown cannot leave a drain task detached;
- body permits and connection permits are released on every terminal path.

Recommended implementation shape:

- represent body state explicitly, such as `Available`, `Borrowed`, `Consumed`, `ReturnedIncomplete`, `Draining`, `Closed`;
- retain a runtime-owned completion/consumption token independent of application wrappers;
- when possible, keep the transport body receiver in a shared internal state rather than moving it irretrievably into an application object;
- prevent a second reader through type/state enforcement rather than convention.

Add tests for:

- service returns without reading;
- service reads one chunk then returns;
- service drops body object;
- service panics/errors during streaming;
- handler timeout while body remains unread;
- graceful shutdown during drain;
- forced shutdown during drain;
- client disconnect during drain;
- many small chunks with drain limit;
- keep-alive second request after successful drain;
- second request is never parsed after failed drain.

If safe drain cannot be guaranteed for all stream cases:

- keep `Close` as the default;
- classify `Drain` as experimental;
- narrow supported drain cases;
- document the limitation rather than claiming general reuse.

Acceptance:

- drain ownership is explainable from code and docs;
- no concurrent body reader exists;
- every drain path is bounded and cancellation-safe;
- connection reuse is proven only after successful completion.

## Track E — Enforce explicit one-shot consumption errors

Make one-shot semantics observable and deterministic.

Required API behavior:

- the first read or iterator acquisition claims consumption ownership;
- a second incompatible consumption attempt returns a typed error;
- EOF after a successful first stream is distinct from “already consumed”;
- reading an actually empty body returns empty data successfully on the first read;
- dropping a partially consumed iterator leaves the body incomplete and triggers policy handling;
- Rust and Python expose equivalent semantics.

Add or confirm a `RequestBodyError::AlreadyConsumed` variant.

Rust expectations:

- `read_all()` after iteration ownership returns `AlreadyConsumed`;
- `next_chunk()` after `read_all()` returns `AlreadyConsumed`;
- acquiring two stream consumers is impossible or errors;
- repeated polling after terminal EOF follows standard stream semantics without reopening consumption.

Python expectations:

- `body.read()` after iteration raises a typed body exception;
- `iter(body)` after `read()` raises;
- a second iterator acquisition raises;
- ordinary end-of-iteration still raises `StopIteration`, not `AlreadyConsumed`;
- exception hierarchy is included in `.pyi` and API snapshots.

Update mixed-consumption corpus fixtures to expect typed failures rather than silent empty results.

Acceptance:

- application misuse is never silently interpreted as an empty request body;
- Rust/Python conformance agrees on second-consumption behavior.

## Track F — Keep transport adapters out of the stable public API

Audit the visibility and documentation of all body constructors and adapters.

Required stable/public contract:

- transport-independent `RequestBody` inspection and consumption APIs;
- safe constructors for empty or in-memory test bodies only if intentionally supported;
- no public signature containing `hyper::body::Incoming`, Hyper frame types, Tokio channels, PyO3 types, or internal connection types.

Actions:

- make `RequestBody::from_incoming` and equivalent adapters `pub(crate)` where possible;
- if fuzz/integration tests require access, use a test-only module, crate feature, internal adapter namespace, or public transport-independent test constructor;
- add compile fixtures ensuring downstream users can consume bodies without importing Hyper;
- extend `no_hyper_in_public_api.rs` to inspect body-related exports;
- run rustdoc/API inventory checks for leaked transport types.

If an adapter must remain public temporarily:

- place it in an explicitly experimental `server::hyper_adapter` or equivalent module;
- exclude it from stable API claims;
- document removal or stabilization criteria.

Acceptance:

- stable body APIs remain transport-independent;
- no accidental Hyper exposure is required for fuzzing or tests.

## Track G — Audit body error taxonomy and HTTP mapping

Review every `RequestBodyError` variant and separate protocol, timeout, limit, disconnect, cancellation, and internal failures.

Recommended categories:

- malformed framing;
- conflicting framing;
- invalid content length;
- premature EOF;
- body too large;
- body timeout;
- already consumed;
- incomplete consumption;
- client disconnected;
- runtime cancelled;
- transport adapter failure;
- internal invariant failure.

Recommended wire mapping:

- malformed/conflicting framing: `400 Bad Request`;
- invalid or conflicting Content-Length: `400`;
- premature EOF before complete request: `400` when a response is still possible;
- body timeout while receiving request: `408 Request Timeout`;
- configured body limit exceeded: `413 Content Too Large`;
- already consumed: service-side/internal error, not a client protocol status;
- client disconnect: terminate without attempting a response;
- runtime cancellation/shutdown: terminate according to lifecycle state;
- internal adapter/invariant failure: `500 Internal Server Error`;
- avoid `502 Bad Gateway` unless eggserve is explicitly acting as a gateway in that path.

Requirements:

- handler-visible errors remain typed even when no HTTP response is possible;
- client-caused and server-caused failures are distinguishable;
- error messages sent to clients are sanitized;
- logs/observer events retain enough category detail without exposing sensitive data;
- Rust and Python exception mappings remain stable or experimental as documented.

Acceptance:

- every error variant has one documented semantic category and wire behavior;
- proxy-specific status codes are not used for ordinary origin-server failures without rationale.

## Track H — Harden Python iterator cancellation and backpressure

Verify the synchronous Python iterator over the async Rust body stream cannot leak workers, deadlock the runtime, or retain resources after abandonment.

Required invariants:

- producer work does not require the Python consumer to release the GIL in order to make progress;
- channel capacity is finite and documented;
- dropping the iterator signals cancellation to the producer;
- producer exits if the receiver is dropped;
- body timeout wakes a blocked iterator with a typed exception;
- graceful shutdown wakes or terminates blocked iteration deterministically;
- forced shutdown does not wait indefinitely for iterator producer tasks;
- connection and body permits return to baseline;
- no unbounded thread or task creation occurs across repeated iterator abandonment.

Add tests for:

- create iterator and drop before first chunk;
- consume one chunk and drop;
- producer blocked on full channel, then iterator dropped;
- body timeout while Python waits for next chunk;
- graceful shutdown mid-iteration;
- forced shutdown mid-iteration;
- client disconnect mid-iteration;
- repeated abandoned iterators with thread/task count checks;
- callback exception while iterator is live;
- second consumption attempt after iterator drop.

Installed-wheel tests must exercise at least:

- full iteration;
- partial iteration then drop;
- timeout;
- shutdown interaction;
- typed exception surface.

Acceptance:

- no blocked producer survives iterator abandonment;
- shutdown deadlines remain bounded;
- backpressure remains finite and observable.

## Track I — Strengthen cross-language conformance

Make `conformance/body_corpus.json` normative for semantics shared by Rust and Python.

Required fixture groups:

- strict TE+CL rejection;
- duplicate Content-Length policy;
- fixed-length exact/over limit;
- chunked exact/over limit;
- malformed chunk sizes and terminators;
- premature EOF;
- empty body;
- buffer pre-handler failures;
- stream handler-visible failures;
- one-shot consumption errors;
- partial stream `Close`;
- partial stream `Drain` success/failure;
- timeout;
- cancellation;
- TLS parity where represented by integration metadata.

Corpus requirements:

- expected handler invocation must be explicit;
- expected connection reuse/closure must be explicit;
- expected body/exception outcome must be explicit;
- fixtures must not encode impossible or misleading transport states;
- Rust and Python runners must report fixture IDs on failure;
- a schema/version field must prevent silent runner drift.

Add a corpus validator that checks:

- unique fixture IDs;
- known policies/actions/encodings;
- required expected fields;
- no buffer fixture claims unread transport bytes;
- no TE+CL fixture expects success;
- no repeated-consumption fixture expects silent empty data.

Acceptance:

- corpus semantics match the documented contract;
- both language runners consume the same fixtures without local reinterpretation.

## Track J — Fuzz and property closure

Extend fuzz/property coverage around the corrected invariants.

Fuzz targets or modes should cover:

- raw framing classification with duplicate CL and TE+CL;
- decimal length parsing and overflow;
- chunked decoded-byte limit accounting;
- consumption state transitions;
- drain state transitions;
- cancellation during each body state;
- repeated consumer acquisition;
- body adapter error mapping;
- Python-independent bounded channel model where feasible.

Properties:

- decoded bytes never exceed configured maximum without terminal error;
- TE+CL never reaches service;
- ambiguous CL never reaches service under the chosen policy;
- body state transitions are monotonic;
- at most one active consumer exists;
- successful drain implies body completion;
- failed drain implies connection non-reuse;
- cancellation eventually releases permits;
- exact-limit bodies succeed and one-over-limit bodies fail;
- error mapping is deterministic for equivalent inputs.

Add regression seeds for every defect found during this phase.

Acceptance:

- corpus replay includes all new seeds;
- fuzz smoke is represented by structured release evidence.

## Track K — Performance and resource verification

Measure the final body design without weakening correctness.

Benchmark or resource checks:

- empty/no-body request overhead versus pre-Milestone-4 baseline;
- fixed-length buffer at small/default/max representative sizes;
- chunked stream with large and many-small chunks;
- stream completion versus partial-close versus partial-drain;
- Python iteration channel overhead;
- cancellation cleanup latency;
- allocations for buffer mode;
- task/thread count under repeated timeout and iterator abandonment;
- keep-alive throughput after successful drain.

Requirements:

- built-in GET/HEAD static path should not pay material body-buffering cost;
- buffer allocation must remain bounded by configured maximum plus documented overhead;
- stream channel memory remains bounded;
- no benchmark is used as proof of correctness;
- performance regressions are documented before acceptance.

Acceptance:

- no unexplained material regression in static/bodyless serving;
- no resource count grows monotonically across stress cycles.

## Track L — Release criteria and CI evidence audit

Audit all Milestone 4 gates in `release/criteria.toml` and `.github/workflows/ci.yml`.

Required gate coverage should include, at minimum:

- `body.primitives`;
- `body.runtime-ingestion`;
- `body.raw-wire`;
- `body.framing-strictness`;
- `body.partial-consumption`;
- `body.cancellation`;
- `body.tls-parity`;
- `body.fuzz-smoke`;
- `python.body-primitives`;
- `python.body-parity`;
- `python.body-wire`;
- `python.body-timeout`;
- `python.body-wheel-linux`;
- `python.body-wheel-macos`;
- `python.body-wheel-windows`.

For each gate verify:

- unique gate ID and evidence filename;
- exact command;
- trigger applicability;
- platform mapping;
- required/advisory classification;
- invalidation paths;
- freshness;
- expected artifact;
- exact-SHA requirement;
- no broad job can accidentally satisfy multiple distinct gates without separate evidence records.

Add negative release-safety tests proving:

- missing framing-strictness evidence blocks release;
- missing platform wheel body evidence blocks release;
- wrong-SHA body evidence is stale;
- malformed or duplicate evidence fails closed;
- a broad workspace-test record cannot satisfy a dedicated body gate;
- skipped required body gates do not count as passed.

Acceptance:

- every required Milestone 4 guarantee has distinct structured evidence;
- contract-consistency tests detect workflow/criteria drift.

## Track M — Documentation and stability reconciliation

Update and reconcile:

- `docs/release-contract.md`;
- `docs/api-stability.md`;
- `docs/library-capability-matrix.md`;
- `docs/body-migration.md`;
- `docs/python-api.md`;
- `docs/security-policy.md`;
- `docs/threat-model.md`;
- `architecture/runtime.md`;
- `architecture/primitives-api.md`;
- `architecture/eggserve-python.md`;
- README and AGENTS references;
- Rust docs and Python `.pyi` files.

Required documentation truths:

- built-in static service still rejects request bodies;
- TE+CL is rejected;
- duplicate Content-Length policy is explicit;
- buffer mode completes ingestion before handler invocation;
- stream mode is one-shot and subject to incomplete-consumption policy;
- `Close` remains the safe default;
- `Drain` support and limitations are exact;
- second consumption raises a typed error;
- Python iteration remains Rust-backed and bounded;
- timeout/cancellation behavior is explicit;
- Hyper adapters are not stable public body APIs;
- body APIs remain experimental or stable exactly as classified.

Acceptance:

- no docs retain “TE wins” language;
- no docs imply partial buffer reads leave network bytes;
- public claims match conformance behavior.

## Track N — Final-SHA qualification

Select one clean main-branch commit after all corrective work.

Run the complete validation matrix on that exact SHA.

Required local/source validation:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p eggserve-core --test body_primitives
cargo test -p eggserve-core --test body_properties
cargo test -p eggserve-core --test body_conformance
cargo test -p eggserve-core --test request_body_integration
cargo test -p eggserve-core --test request_body_wire
cargo test -p eggserve-core --test request_body_cancellation
cargo test -p eggserve-core --test request_body_timeout_interaction
cargo test -p eggserve-core --features tls --test request_body_tls
cargo test -p eggserve-core --test no_hyper_in_public_api
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
python3 scripts/release_criteria.py generate-checklist --check
```

Required Python validation from an installed wheel with `PYTHONPATH` cleared:

- body primitives;
- shared body conformance;
- body wire tests;
- iterator drop/cancellation;
- timeout interactions;
- lifecycle/shutdown interactions;
- API snapshots and type stubs;
- no source-tree fallback.

Required CI/artifact inspection:

- Linux native wheel body evidence;
- macOS native wheel body evidence;
- Windows native wheel body evidence;
- raw-wire evidence;
- TLS parity evidence;
- framing strictness evidence;
- partial-consumption evidence;
- fuzz smoke evidence;
- Python parity and timeout evidence;
- aggregate manifest and generated checklist.

Verify:

- every evidence record names the candidate SHA;
- artifact digests match downloaded files;
- no required body gate is missing, stale, skipped, malformed, or conflicting;
- downloaded evidence regenerates the committed checklist deterministically;
- no credentials, local paths, or sensitive request data appear in evidence artifacts.

Record:

- candidate SHA;
- workflow run ID;
- job IDs;
- artifact IDs and digests;
- platform results;
- checklist digest;
- known non-blocking limitations;
- reviewer/operator approval.

Store the result in a dedicated Milestone 4 verification manifest and record under `release/`.

Acceptance:

- one exact SHA has complete, inspectable Milestone 4 evidence;
- local regeneration yields the same release state;
- no implementation claim rests only on commit-message test counts.

## Required completion criteria

Milestone 4 is closed only when:

- TE+CL is rejected before service invocation;
- duplicate Content-Length policy is strict, documented, and raw-wire tested;
- buffer and stream semantics are correctly separated;
- partial stream close/drain behavior is deterministic;
- drain ownership is single-reader, bounded, and cancellation-safe;
- repeated consumption raises a typed error;
- stable public APIs do not expose Hyper body types;
- body error mappings fit eggserve’s origin-server role;
- Python iterator abandonment and shutdown are leak-free and bounded;
- Rust/Python conformance corpus reflects the corrected contract;
- dedicated release gates emit distinct evidence;
- Linux, macOS, and Windows installed-wheel body tests pass on one SHA;
- a final evidence bundle and verification record are inspected and committed;
- built-in static serving remains GET/HEAD and body-rejecting;
- no Milestone 4 blocker remains before Milestone 5.

## Non-goals

- Multipart or form parsing.
- Upload storage or temporary-file management.
- Content decompression.
- Async Python handlers.
- ASGI/WSGI receive channels.
- Routing or middleware.
- HTTP/2 or HTTP/3 request bodies.
- Reverse proxy request forwarding.
- Stabilizing the entire experimental server module solely as part of this pass.
