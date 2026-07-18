# Plan 081 — Unified Static-File and Directory-Index Response Path

## Goal

Eliminate semantic drift between direct file requests and directory-index requests by routing both through one representation planner and one body-construction path.

After this plan, `/directory/index.html` and `/directory/` resolving to that same opened file must share metadata, validators, conditional handling, range handling, content headers, GET/HEAD planning, and streaming behavior.

## Preconditions

- Release A is closed.
- Plans 078–080 have established stable service, body, and configuration contracts.
- Existing filesystem resolution returns an opened resource or equivalent stable representation suitable for planning without path-based reopen.
- Plan 075 finding/evidence tracking remains active.

## Non-goals

Do not:

- redesign directory listing HTML;
- complete Windows handle-relative child lookup or enumeration assigned to Release D;
- add content negotiation, compression, caching proxy behavior, or multipart range responses unless already supported;
- add virtual hosting or routing;
- optimize buffer allocation;
- broaden supported HTTP methods.

## Defect statement

The direct-file path applies conditional and range request headers, while the directory-index path can bypass those inputs and call a file response helper with `None` values. Equivalent resources therefore produce different status codes, headers, and bodies depending on URL form.

Duplicated planning logic also increases the chance of future divergence across GET, HEAD, range, validators, and errors.

## Track A — Map all static representation entry points

Inventory every path that can serve file bytes:

- direct regular file;
- directory index file;
- root index file;
- range response;
- full response;
- HEAD response;
- Python or library static-serving adapter if it bypasses the main service;
- tests and helper APIs that construct responses independently.

For each entry point record:

- resolver output type;
- metadata source;
- request headers supplied to planning;
- validator generation;
- status selection;
- body creation;
- semaphore/permit ownership;
- error mapping;
- whether any pathname is reopened after validation.

Remove or explicitly deprecate alternate serving paths that cannot preserve the same invariant.

## Track B — Define an opened representation input

Create an internal representation input that contains the stable information required by the HTTP planner without reopening by pathname.

It should include as applicable:

- owned/opened file handle or stream factory bound to the opened object;
- file length;
- modification timestamp at available precision;
- platform file identity where available;
- content type source/name;
- safe display or relative path for diagnostics only;
- range capability;
- relevant filesystem/profile metadata.

The planner must not receive a raw untrusted absolute path as authority for reopening the file.

On platforms where directory-index lookup still falls back to a path-based operation, preserve current support classification and make that limitation visible. Do not claim Release D completion.

## Track C — Canonical request representation inputs

Define one request input structure for static-file planning containing:

- method;
- `Range`;
- `If-Range`;
- `If-None-Match`;
- `If-Modified-Since`;
- any existing supported precondition headers;
- HTTP version if it affects response behavior;
- connection/body-forbidden context only where needed.

Both direct-file and index-file routes must construct this input identically from the canonical request.

Avoid passing multiple optional header arguments through long helper signatures. A typed planner input should make omitted semantics visible in review and tests.

## Track D — One representation planner

Implement one pure or mostly pure planner that decides:

- 200 versus 206 versus 304 versus 416 and other existing outcomes;
- full versus partial byte interval;
- representation length;
- response `Content-Length`;
- `Content-Range`;
- `Accept-Ranges`;
- ETag and Last-Modified headers;
- content type;
- whether a body stream is required for the method/status;
- file offset and byte count for streaming.

The planner should not perform network I/O. It should be testable using metadata and request headers alone.

Preserve RFC precedence already adopted by the project. Where current behavior is ambiguous, align it with the repository's canonical conformance policy and document the decision.

## Track E — One body-construction path

Use the planner output to create the response body for both direct and index resources.

Required behavior:

- full response streams exactly the planned representation length;
- range response seeks/reads exactly the planned interval;
- HEAD and 304 do not acquire unnecessary file-stream permits or create body streams;
- body creation consumes or duplicates the opened handle according to explicit ownership rules;
- cancellation and shutdown release file handles and permits;
- streaming errors use the Plan 077 operational/lifecycle path;
- no helper recalculates status or headers after planning.

## Track F — Directory index route refactor

Refactor directory handling into two distinct outcomes:

1. index file resolved: produce the same opened representation and invoke the canonical file planner;
2. no index file: apply directory listing/deny policy.

Forward all relevant request headers to the planner. Do not use `None` placeholders for headers that were present on the request.

Tests must demonstrate that URL form alone does not change representation semantics.

## Track G — Direct file route refactor

Migrate direct file serving to the same types and planner.

Delete old helpers only after all call sites and tests move. Do not keep a legacy code path hidden behind convenience APIs.

Use compile-time visibility or module boundaries to prevent future static routes from bypassing the planner.

## Track H — Resolver and identity interaction

Ensure metadata and body refer to the same opened object.

Required checks:

- metadata used for length/validators is obtained from the opened handle;
- range offset/length is applied to that handle;
- pathname replacement after resolution does not switch the served object;
- index-file replacement races have the same semantics as direct-file replacement;
- Unix descriptor and Windows handle ownership remain correct;
- no canonicalization/reopen is introduced for convenience.

Future Release D may change how Windows index handles are obtained, but it should not need to change the planner contract.

## Track I — Error boundaries

Separate resolver errors from representation-planning errors and streaming errors.

Examples:

- not found or denied before planning;
- invalid/unsatisfiable range during planning;
- metadata/identity failure;
- handle duplication/seek failure during body construction;
- read failure after response start.

Only one layer should choose each status. Avoid converting a planner outcome into a generic 500 in an outer route.

## Required planner tests

For one metadata fixture, run identical cases through direct and index route adapters:

- ordinary GET;
- ordinary HEAD input, while Plan 082 owns final normalization;
- matching/non-matching `If-None-Match`;
- matching/non-matching `If-Modified-Since`;
- valid range;
- suffix range;
- open-ended range;
- unsatisfiable range;
- `If-Range` match and mismatch;
- conditional plus range precedence;
- zero-length file;
- file changed between pathname lookup and opened-handle metadata observation where testable.

Assert planner outputs, not just final high-level client behavior.

## Required production-path tests

Use raw TCP or the production server path to compare:

- `/x/index.html` and `/x/`;
- root `/index.html` and `/`;
- full response bytes and headers;
- range response bytes and headers;
- 304 behavior;
- 416 behavior;
- keep-alive reuse after each body/no-body outcome;
- slow-reader cancellation and file permit release;
- installed binary and Python static server where they share the static service.

Comparison should ignore only intentionally URL-specific headers, if any, and document them.

## Documentation changes

Update:

- static response architecture;
- directory index semantics;
- conditional/range documentation;
- opened-resource ownership notes;
- release criteria and invalidation mappings;
- finding registry and corrective status.

Do not claim Windows handle-relative index lookup beyond current evidence.

## Acceptance criteria

- Direct files and directory index files use one typed request input and one representation planner.
- The index route no longer drops conditional or range headers.
- Planner metadata and response bodies refer to the same opened object.
- Full and range bodies are constructed only from planner output.
- HEAD/304 planner outcomes do not create unnecessary body streams or consume file-stream permits.
- `/x/` and `/x/index.html` produce equivalent conditional and range behavior for the same file.
- Root index and nested index cases are covered.
- Old bypass helpers are removed or inaccessible to production routes.
- Raw-wire and installed-artifact evidence is tied to the exact implementation SHA.

## Stop conditions

Stop and add a blocking finding if:

- an index route cannot provide an opened object without a path-based reopen on a profile currently claimed hardened;
- metadata and body cannot be tied to the same file identity;
- a compatibility API requires retaining a second planner with different semantics;
- range construction cannot operate on the validated/opened handle;
- a planner refactor changes unrelated directory-listing policy.

## Handoff

Plan 082 applies complete GET/HEAD normalization and strengthens validators on top of this single planner. Plan 083 independently verifies direct/index parity through canonical and raw-wire suites.