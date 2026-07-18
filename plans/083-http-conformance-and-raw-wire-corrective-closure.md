# Plan 083 — HTTP Conformance and Raw-Wire Corrective Closure

## Goal

Independently verify and close Corrective Release C by exercising the final runtime, body-policy, configuration, static-response, HEAD, conditional, range, and validator behavior through canonical planner tests, raw TCP tests, production server paths, installed binaries, and installed Python wheels.

This is a release-closure and corrective-audit plan. It must not become a feature phase.

## Preconditions

- Plans 075–082 are implemented.
- Release A and Release B findings are either closed or explicitly remain blocking.
- Plan 081 has one static representation planner and body path.
- Plan 082 has one method-aware response normalizer and final validator design.
- Exact-SHA evidence collection and release containment from Plan 075 are operational.

## Non-goals

Do not:

- add new protocol features to make tests pass;
- add HTTP/2, HTTP/3, multipart ranges, compression, or caching;
- broaden support-profile claims;
- waive canonical/raw-wire disagreement;
- optimize performance except to correct a demonstrated release-blocking regression;
- duplicate expected behavior separately in each test harness without one canonical source.

## Closure principles

The closure pass must prove all of the following:

1. the canonical planner expresses intended behavior;
2. production response construction follows the planner;
3. actual bytes on the socket match the normalized response;
4. keep-alive framing remains synchronized after every no-body, error, range, rejection, and drain outcome;
5. installed artifacts behave like the reviewed source;
6. fixes for Release C have not regressed Release A lifecycle or Release B body/configuration contracts.

A green unit suite alone is insufficient.

## Track A — Independent implementation review

Use a reviewer or agent that did not author the Release C implementation.

Review:

- every static file/index entry point;
- request-header propagation into the planner;
- planner status/header precedence;
- opened-file identity and metadata source;
- body construction and permit ownership;
- HEAD/body-forbidden normalization;
- error response routing;
- validator construction;
- direct/index parity tests;
- raw-wire test oracles;
- installed-artifact test paths.

Search specifically for:

- legacy helpers still reachable from production routes;
- `None` or default header values replacing real request headers;
- pathname reopen after validation;
- status/header recalculation outside the planner;
- body creation before HEAD suppression;
- generic error bodies bypassing method context;
- platform-specific validator fallback not represented in docs;
- tests that exercise a mock path instead of production code.

Record findings in the Plan 075 registry. Critical/high findings block closure.

## Track B — Canonical conformance matrix

Create a single machine-readable or table-driven matrix covering supported semantics.

Dimensions should include:

- resource form: direct file, nested index, root index, directory listing, missing, denied;
- method: GET, HEAD, unsupported method;
- body policy state where relevant;
- conditionals: none, matching/non-matching ETag, matching/non-matching date, both validators;
- range: none, exact, open-ended, suffix, unsatisfiable, malformed, `If-Range` match/mismatch;
- file state: empty, small, large, rapidly replaced, same-size replacement;
- HTTP version: 1.0 and 1.1 where supported;
- connection intent: close and keep-alive.

For each case define:

- expected status;
- expected mandatory headers;
- forbidden headers;
- expected representation length;
- expected payload length;
- whether service/body/file/listing factories may run;
- whether connection reuse is permitted.

Use this matrix to drive planner tests and raw-wire assertions where practical.

## Track C — Direct versus index equivalence

For the same opened file, compare `/x/index.html` and `/x/` across:

- ordinary GET and HEAD;
- matching and non-matching validators;
- range forms;
- `If-Range`;
- empty files;
- MIME type;
- ETag and Last-Modified;
- content length and range length;
- 304 and 416;
- keep-alive reuse;
- cancellation during streaming.

Run the same comparison for `/index.html` and `/`.

Only intentional URL-specific behavior may differ, and every allowed difference must be documented in the test matrix.

## Track D — HEAD and body-forbidden raw-wire suite

Use raw TCP and parse bytes independently of the server's response objects.

Cover:

- HEAD 200 file;
- HEAD 206 range;
- HEAD 304;
- HEAD 416;
- HEAD directory listing;
- HEAD redirects where supported;
- HEAD 400, 403, 404, 405, 413, 500, and 503;
- body-forbidden statuses emitted by supported code paths;
- a second request immediately following each response on the same connection.

Assert:

- zero payload bytes for HEAD/body-forbidden responses;
- correct `Content-Length` policy;
- no conflicting transfer framing;
- next response begins at the exact expected boundary;
- no file/listing body factory invocation for HEAD;
- no file-stream permit retained after the response.

## Track E — Range and conditional precedence

Verify canonical and production behavior for:

- `If-None-Match` precedence over `If-Modified-Since`;
- matching/non-matching weak ETags;
- valid and invalid date values;
- range ignored or applied according to `If-Range`;
- suffix/open-ended ranges;
- zero-length resource ranges;
- unsatisfiable ranges and `Content-Range` syntax;
- conditional responses on direct and index resources;
- HEAD with ranges and conditionals;
- file replacement between requests.

Where the repository intentionally supports a subset of RFC behavior, ensure the subset is explicit and internally consistent.

## Track F — Request-body and connection synchronization regression

Rerun Plan 079 raw-wire cases after response-planner changes:

- rejected body does not invoke service;
- `Expect: 100-continue` rejection;
- TE+CL and duplicate Content-Length rejection;
- close-only incomplete body;
- successful bounded drain, if supported;
- failed/limited/timed-out drain;
- hidden second-request attempts;
- keep-alive after fully consumed allowed body.

Verify error normalization from Plan 082 does not accidentally send a body on HEAD variants or create two terminal responses.

## Track G — Lifecycle and shutdown regression

Rerun Plan 077 cases with active static responses:

- large progressing download longer than write inactivity setting;
- stalled client timeout;
- shutdown during full-file stream;
- shutdown during range stream;
- shutdown during directory listing;
- forced deadline with multiple active connections;
- all tasks aborted and joined before `Stopped`;
- file handles and permits return to baseline;
- direct and index streams behave identically.

Response refactoring must not reintroduce detached body tasks.

## Track H — Validator qualification

On Unix and Windows dedicated runners where supported, verify:

- unchanged file produces stable validator;
- rapid same-size replacement changes validator when platform identity/high-resolution metadata distinguishes it;
- replacement with a new inode/file ID changes validator;
- direct and index paths share validator;
- validator syntax is valid and safely quoted;
- validators reveal no absolute path;
- conditional matching operates on the final emitted value;
- fallback behavior is documented for filesystems with insufficient metadata.

Do not promote a filesystem class beyond available evidence.

## Track I — Installed binary and Python wheel parity

Build release-like artifacts from the exact candidate SHA.

Test the installed binary for:

- direct/index GET and HEAD;
- range/conditional matrix subset;
- errors and keep-alive synchronization;
- configured non-default limits;
- shutdown behavior;
- platform-specific validators.

Build, install, and test Python wheels for supported platforms:

- static server direct/index parity;
- real connection metadata from Plan 078;
- body rejection/callback suppression from Plan 079;
- non-default file-stream/configuration behavior from Plan 080;
- HEAD and error normalization;
- shutdown and repeated lifecycle.

Record artifact hashes and prove bundled/native components derive from the reviewed source SHA.

## Track J — Differential and property testing

Where useful, add property tests for planner invariants:

- HEAD plan equals GET plan except body requirement and method-specific allowed headers;
- direct and index adapters produce equal planner inputs for equivalent resources;
- body-forbidden statuses never require a body;
- range interval never exceeds representation length;
- 206 lengths match interval length;
- 416 never constructs a file body;
- 304 never constructs a file body;
- normalized response never contains conflicting framing.

Differential testing against another server may be informative but cannot replace eggserve's documented canonical policy.

## Track K — Resource and stability checks

Run repeated cycles covering:

- direct/index request matrix;
- HEAD/error paths;
- invalid ranges and body framing;
- client disconnects;
- start/stop;
- forced shutdown;
- Windows handle counts and Unix descriptor counts;
- task and semaphore instrumentation;
- memory trend smoke checks.

No path may show monotonic unbounded growth.

## Track L — Documentation consistency audit

Compare implementation and evidence against:

- README;
- support profiles;
- security policy;
- threat model;
- deployment guide;
- Rust API docs;
- CLI help;
- Python docs;
- timeout/body-policy/configuration references;
- HTTP semantics reference;
- known limitations;
- release notes.

Correct or narrow any statement unsupported by the final candidate.

Do not promote Windows hardened profiles; Release D remains outstanding.

## Track M — Release C closure report

Produce a closure report containing:

- candidate SHA;
- plan implementation commits;
- closed finding identifiers;
- remaining findings and severity;
- platform/feature/artifact matrix;
- canonical/raw-wire/install test results;
- independent reviewer result;
- support-profile impact;
- documentation changes;
- invalidated evidence rerun on final SHA;
- recommendation: release, narrow release, or block.

The report must fail closed if any required evidence is missing, skipped, stale, or tied to another SHA.

## Required verification matrix

At minimum run:

- `cargo fmt --check`;
- Clippy with warnings denied;
- workspace tests and doctests;
- relevant feature combinations;
- canonical planner/conformance tests;
- raw-wire HTTP/1.0 and HTTP/1.1 tests;
- production-path tests;
- Unix filesystem identity/race regression tests;
- dedicated Windows Unicode/handle/validator/streaming tests;
- body-policy/desynchronization corpus;
- lifecycle/shutdown suite;
- CLI installed binary suite;
- Python unit and installed-wheel suite;
- dependency/security/license checks;
- short resource-stability soak;
- release evidence aggregation.

## Acceptance criteria

- Independent review finds no unresolved critical/high Release A–C defect.
- Direct file and directory index forms are equivalent across the documented conditional/range matrix.
- HEAD and body-forbidden responses transmit no payload bytes and preserve unambiguous keep-alive framing.
- Canonical planner outputs match production socket bytes.
- Error responses use the same method-aware normalization.
- Request-body rejection still suppresses user service invocation and failed drain paths cannot smuggle a next request.
- Response timeout and shutdown behavior from Release A remain correct under full/range/index streams.
- Validators are stable for unchanged objects and change for distinguishable rapid replacements.
- Rust, CLI, and Python non-default configuration reaches actual enforcement.
- Installed binaries and wheels match the reviewed source SHA and pass the required matrix.
- No repeated test/soak path shows unbounded task, descriptor, handle, permit, or memory growth.
- Documentation and machine-readable support metadata match final behavior.
- Release C closure report is complete and evidence-backed.

## Stop conditions

Block Release C if:

- canonical and raw-wire behavior disagree;
- direct and index resources diverge without an intentional documented reason;
- any HEAD/error path emits payload bytes or corrupts the next response boundary;
- body-policy regressions invoke user code unexpectedly;
- shutdown reports `Stopped` before response tasks are joined;
- installed artifacts cannot be traced to source;
- dedicated Windows evidence is absent for Windows functional claims;
- any critical/high finding remains open;
- release aggregation accepts missing, stale, or skipped evidence.

## Final handoff

When this plan closes, Corrective Releases A–C are complete. The next planning batch should cover Release D: Windows directory-handle retention, handle-relative child/index resolution, handle-relative directory enumeration, and dedicated adversarial filesystem qualification. Release E operational/performance work remains blocked until Release D establishes the final Windows confinement path.