# Phase 68 — Reverse-Proxy Desynchronization Qualification

## Goal

Prove that qualified reverse proxies and eggserve cannot be driven into HTTP/1 request-boundary disagreement by malformed, ambiguous, pipelined, or partially delivered input. Extend the Plan 066 harness into a protocol-security qualification suite and close any frontend/backend desynchronization defects.

## Preconditions

- Plan 066 provides reproducible Caddy and nginx integration harnesses.
- Plan 067 provides explicit connection, header, body, keep-alive, and shutdown bounds.
- Milestone 4 strict TE+CL and duplicate Content-Length behavior remains intact.

## Non-goals

Do not add:

- reverse-proxy behavior to eggserve;
- tolerant parsing for compatibility with a proxy;
- HTTP/2 or HTTP/3 origin support;
- proxy-specific application routing;
- forwarding-header trust;
- automatic remediation of unsafe third-party proxy configurations;
- a generalized external vulnerability scanner.

## Security invariant

> For every request sequence accepted by a qualified edge, eggserve and the edge agree on message boundaries. Any ambiguous or malformed sequence is rejected without allowing hidden bytes to become a second origin request.

## Track A — Harness instrumentation

Extend the proxy harness to record:

- exact client-to-edge bytes;
- edge response bytes;
- origin request count;
- origin handler invocation IDs;
- origin connection open/close/reuse events;
- edge upstream connection reuse where observable;
- timestamps for timeout classification;
- sanitized edge and origin logs;
- proxy and eggserve versions/commit SHA.

Add a deterministic test endpoint or internal observer only within test builds if needed to count service invocation. Do not add a production admin endpoint.

The harness must distinguish:

- edge rejected before origin;
- edge normalized and forwarded one request;
- origin rejected one request;
- edge/origin closed;
- an unintended second request reached origin.

## Track B — Content-Length corpus

Test raw requests containing:

- one valid Content-Length;
- duplicate identical fields;
- duplicate conflicting fields;
- comma-combined identical values;
- comma-combined conflicting values;
- leading zeros;
- empty value;
- signed/negative/nondecimal values;
- overflow;
- mixed header casing;
- whitespace before/after value;
- whitespace before colon;
- embedded control bytes;
- Content-Length on GET/HEAD under static policy;
- declared length shorter or longer than delivered bytes;
- pipelined request bytes immediately after the declared body.

Expected outcome must be explicit per proxy. Qualification requires no hidden origin request, even if the edge itself rejects before forwarding.

## Track C — Transfer-Encoding corpus

Test:

- `chunked`;
- mixed casing;
- repeated Transfer-Encoding fields;
- comma-separated coding lists;
- unsupported codings;
- `chunked` not final;
- empty value;
- whitespace anomalies;
- malformed chunk size;
- extension syntax boundaries;
- missing chunk terminators;
- premature EOF;
- malformed trailers where the parser sees them;
- extra bytes after terminal chunk;
- pipelined request after valid terminal chunk.

Eggserve’s static service should reject body-signaling GET/HEAD requests according to its contract. Generic body-enabled service tests may be used only to exercise framing, not to expand static product behavior.

## Track D — TE plus CL conflicts

Cover all combinations:

- chunked plus one Content-Length;
- chunked plus duplicate identical lengths;
- chunked plus conflicting lengths;
- unsupported transfer coding plus Content-Length;
- mixed casing and whitespace;
- body bytes crafted for frontend-first and backend-first interpretations;
- hidden second request after either interpretation.

Required eggserve behavior:

- service not invoked for the ambiguous request;
- 400 or transport-level rejection according to parser stage;
- origin connection closed;
- no next request parsed on that origin connection.

## Track E — Request-line and header grammar corpus

Test:

- bare LF;
- bare CR;
- malformed CRLF;
- obsolete line folding;
- leading whitespace before header names;
- invalid field-name bytes;
- invalid field-value controls;
- NUL;
- oversized request line;
- oversized target;
- too many headers;
- aggregate header limit;
- absolute-form, authority-form, and asterisk-form targets;
- multiple Host fields;
- missing Host in HTTP/1.1;
- invalid Host values where Hyper/proxy behavior differs;
- HTTP/0.9-like requests;
- invalid version tokens.

Connection disposition is as important as status code.

## Track F — Multi-request sequence corpus

Use one client connection where the edge allows it:

- valid then valid;
- malformed then valid;
- valid then malformed then valid;
- partial body then hidden request;
- timeout then valid bytes;
- limit rejection then valid request;
- maximum requests per connection then extra request;
- successful body drain then next request;
- failed body drain then hidden request;
- connection-close response followed by extra bytes.

Assert exact origin invocation count and order.

## Track G — Proxy normalization audit

For each qualified proxy/version, document:

- whether duplicate headers are preserved, joined, or rejected;
- how TE+CL is handled;
- whether unsupported transfer codings are forwarded;
- how request targets are normalized;
- header whitespace behavior;
- upstream connection reuse after 4xx;
- timeout behavior;
- whether request buffering changes delivery.

Do not depend on undocumented permissive behavior. Pin qualification versions and rerun on proxy upgrades.

## Track H — Differential direct-origin testing

Run the same corpus directly against eggserve and through each proxy.

Classify differences as:

- safe edge rejection;
- safe edge normalization;
- safe origin rejection;
- unsafe disagreement;
- test harness ambiguity.

Any unsafe disagreement is a release blocker. Prefer stricter origin rejection and connection close rather than adding tolerance.

## Track I — TLS edge path

Run the client-to-edge leg over TLS for a representative subset and all previously discovered discrepancy cases.

The origin may remain plaintext HTTP/1.1 in the preferred profile. Confirm TLS termination does not alter request-boundary results due to buffering or connection reuse.

## Track J — Corpus and reproducibility

Store test vectors in a versioned machine-readable corpus with fields for:

- ID;
- raw bytes or construction recipe;
- expected edge outcome;
- expected origin invocation count;
- expected origin status if invoked;
- expected edge/origin connection closure;
- proxy-specific notes;
- security rationale;
- applicable profiles.

Add schema validation, unique IDs, and readable failure output.

Seed fuzzing in Plan 070 from every fixed corpus case.

## Required tests

- Caddy and nginx full fixed corpus;
- direct origin full corpus;
- proxy version matrix defined by project policy;
- installed eggserve binary, not only `cargo run`;
- TLS client-to-edge representative matrix;
- repeated execution to detect connection-pool-dependent flakes;
- IPv4 and IPv6 loopback where supported;
- graceful origin shutdown during a malformed sequence;
- edge restart/connection pool reset.

## Release criteria

Add non-waivable security gates for:

- fixed desynchronization corpus on Caddy;
- fixed desynchronization corpus on nginx;
- direct-origin framing corpus;
- zero hidden origin requests;
- expected connection closure;
- proxy-version evidence;
- corpus schema validation.

Invalidate on:

- Hyper or HTTP dependencies;
- connection pipeline;
- header/body framing;
- keep-alive/drain logic;
- proxy configuration or image versions;
- harness/corpus changes.

## Corrective behavior

When a discrepancy is found:

1. Reproduce directly at edge and origin.
2. Determine exact byte interpretation.
3. Prefer strict pre-service rejection.
4. Close origin connection after uncertain framing.
5. Add the case to fixed corpus.
6. Add a stateful fuzz seed.
7. Rerun all proxy variants.

Do not “fix” a discrepancy by accepting more malformed syntax.

## Acceptance criteria

- No corpus case causes an unintended second origin request.
- TE+CL and duplicate length ambiguity never reaches a service.
- Failed drain or uncertain framing prevents origin connection reuse.
- Proxy normalization behavior is documented for pinned versions.
- Direct and proxied tests produce only classified safe differences.
- Corpus and evidence are reproducible and tied to the final SHA.

## Stop conditions

Do not claim the reverse-proxy production profile if:

- origin invocation cannot be observed reliably;
- a proxy/version produces unresolved boundary disagreement;
- malformed framing permits origin connection reuse against policy;
- the harness hides upstream pooling behavior;
- required proxy evidence is skipped or stale.

## Handoff

Plan 070 incorporates this fixed corpus into stateful live-socket fuzzing. Proxy upgrades must invalidate this qualification even after the production release.
