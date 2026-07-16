# Phase 70 — HTTP/1 Conformance and Stateful Live-Socket Fuzzing

## Goal

Close remaining HTTP/1.0 and HTTP/1.1 correctness gaps and add stateful live-socket fuzzing over request sequences, body lifecycle, connection reuse, TLS truncation, timeout, and shutdown. Move beyond isolated parser fuzzing to the actual running server state machine.

## Preconditions

- Plan 067 bounds connection phases and keep-alive behavior.
- Plan 068 provides fixed direct/proxy desynchronization corpora.
- Plan 069 provides qualified TLS handshake and connection behavior.
- Existing canonical, raw-wire, body, property, and corpus tests pass.

## Non-goals

Do not add:

- HTTP/2, HTTP/3, WebSocket, CONNECT, or upgrades;
- tolerant legacy grammar beyond the documented HTTP/1 contract;
- HTTP trailers unless separately accepted in scope;
- application routing or framework semantics;
- a general network fuzzer product;
- proxying or cache semantics.

## Track A — Normative HTTP behavior inventory

Create a matrix of implemented behavior for:

- HTTP/1.0 and HTTP/1.1 versions;
- Host requirements;
- connection persistence;
- request-target forms;
- method token validation;
- header field grammar;
- Content-Length and Transfer-Encoding;
- body-forbidden static methods;
- response body prohibition for HEAD, 1xx, 204, and 304;
- Content-Length normalization;
- Date, Last-Modified, ETag, and conditional precedence;
- range parsing and response status;
- error status and connection disposition.

For each row identify:

- canonical primitive behavior;
- Hyper adapter behavior;
- production connection behavior;
- Python parity;
- raw-wire test;
- corpus fixture;
- fuzz seed.

Any undocumented dependency on Hyper normalization must become an explicit adapter contract or a production-path test.

## Track B — HTTP/1.0 and persistence closure

Test and correct:

- HTTP/1.0 default close;
- HTTP/1.0 keep-alive only where explicitly supported;
- HTTP/1.1 default persistence;
- `Connection: close`;
- request maximum per connection;
- idle timeout;
- error-induced close;
- response version selection;
- HEAD behavior under both versions;
- pipelined requests if supported by Hyper’s connection path;
- no parsing of a next request after uncertain framing.

Do not implement pipelining-specific optimization. Correct sequencing is sufficient.

## Track C — Request grammar corpus expansion

Expand fixed cases for:

- missing/duplicate/invalid Host;
- method token boundaries and extension methods;
- invalid version tokens;
- origin-form validation;
- absolute-, authority-, and asterisk-form rejection;
- percent-encoding and target-size boundaries;
- invalid header names;
- control characters and NUL;
- whitespace before colon;
- obsolete folding;
- bare CR/LF;
- header count/bytes boundaries;
- TE/CL cases from Plan 068;
- premature EOF at every request phase.

Expected service invocation and connection closure must be explicit.

## Track D — Response normalization closure

Test canonical and raw-wire behavior for:

- HEAD suppression while preserving correct representation headers;
- 1xx, 204, and 304 no-body behavior;
- 200 empty file;
- 206 ranges;
- 416 unsatisfiable range;
- conditional 304 precedence;
- ETag quoting/comparison;
- Last-Modified precision;
- conflicting user-supplied response headers from generic service primitives;
- duplicate response headers;
- invalid response header values;
- Content-Length generation/removal;
- connection-close signaling;
- service errors before and after response start.

Static-service output must remain canonical even if generic downstream services can construct broader responses.

## Track E — Stateful fuzz model

Model a connection as states such as:

- TCP connected;
- TLS handshaking;
- request line partial/complete;
- headers partial/complete;
- body fixed/chunked partial/complete;
- service active;
- body draining;
- response headers/body active;
- keep-alive idle;
- graceful shutdown;
- forced shutdown;
- closed.

Generate action sequences including:

- send arbitrary/request-derived bytes;
- split bytes at arbitrary boundaries;
- delay/timeout;
- half-close;
- reset;
- consume or abandon body in generic test service;
- initiate shutdown;
- begin another request;
- stop reading response;
- complete TLS or truncate records.

The fuzzer should drive a real loopback server, not only pure parsers.

## Track F — Fuzz oracles

Every run must assert:

- no panic, abort, deadlock, or task leak;
- no service invocation after a pre-service rejection;
- no unintended extra service invocation;
- no response splitting;
- no cross-request body contamination;
- no connection reuse after uncertain framing;
- no two concurrent readers of one request body;
- no permit count below zero or above configured capacity;
- bounded completion after shutdown;
- bounded memory/output for generated input;
- response bytes remain valid enough for the test parser or are terminated by connection close.

Use test instrumentation for invocation IDs, permits, and task baselines. Keep instrumentation out of production APIs.

## Track G — Seed strategy

Seed stateful fuzzing with:

- every Plan 068 desynchronization case;
- every body corpus case;
- canonical request/response corpus;
- path and percent-decoding fuzz corpus;
- previous crash/regression inputs;
- TLS truncation cases from Plan 069;
- shutdown/cancellation cases from Milestone 4;
- multi-request keep-alive cases from Plan 067.

Minimized failures must be checked into a deterministic replay corpus with issue/commit references.

## Track H — Plaintext and TLS modes

Run:

- high-budget plaintext fuzzing;
- smaller scheduled TLS stateful fuzzing;
- deterministic TLS corpus replay in ordinary CI;
- generic service body buffer/stream modes for lifecycle testing;
- static service mode for product contract testing.

Do not let generic body-enabled fuzz services change the static server’s GET/HEAD/bodyless default.

## Track I — Python parity subset

Python does not need to participate in high-throughput fuzz loops, but installed-wheel deterministic replay must cover:

- request framing rejection;
- body one-shot semantics;
- callback exception;
- partial stream drop;
- timeout;
- shutdown;
- invalid response construction;
- no hidden second invocation.

Use the actual Rust runtime behind Python bindings.

## Track J — Fuzz execution policy

Define:

- short deterministic corpus replay on every PR;
- bounded smoke fuzz on relevant CI where stable;
- scheduled longer plaintext fuzz;
- scheduled TLS fuzz;
- pre-release minimum execution budget;
- artifact retention for crashes;
- toolchain pinning;
- sanitizers/platforms where practical;
- maximum input and per-case duration.

A fuzz run is evidence only when its source SHA, target, budget, seed corpus hash, and outcome are recorded.

## Track K — Corrective workflow

For every failure:

1. Preserve raw input/action sequence.
2. Minimize reproducibly.
3. Add deterministic regression test.
4. Classify parser, framing, lifecycle, cancellation, TLS, or instrumentation defect.
5. Correct without broadening accepted malformed syntax.
6. Rerun direct, proxy, plaintext, and TLS affected gates.
7. Add release evidence.

## Required tests

- full fixed HTTP/1 conformance matrix;
- raw-wire direct server;
- canonical Rust/Python parity;
- HTTP/1.0/1.1 multi-request connections;
- body buffer/stream sequencing;
- failed/successful drain sequencing;
- TLS deterministic replay;
- stateful corpus replay;
- scheduled fuzz targets;
- installed Python replay subset;
- permit/task baseline after fuzz corpus.

## Release criteria

Add non-waivable security gates for:

- HTTP/1 conformance matrix;
- stateful corpus replay;
- pre-release plaintext fuzz budget;
- pre-release TLS fuzz budget for direct-TLS profiles;
- zero hidden service invocations;
- zero leak/panic findings;
- corpus hash and source SHA evidence.

Invalidate on HTTP dependencies, canonical types, connection/body lifecycle, TLS adapter, service invocation, response normalization, and corpus changes.

## Acceptance criteria

- HTTP/1.0 and HTTP/1.1 behavior is explicit and tested.
- Response normalization is correct for HEAD and body-forbidden statuses.
- Fixed desynchronization cases are stateful fuzz seeds.
- Real-server fuzzing covers multi-request, timeout, shutdown, body, and TLS states.
- No unintended second request or cross-request contamination occurs.
- Every discovered failure becomes deterministic corpus coverage.
- Static service scope remains unchanged.

## Stop conditions

Do not qualify HTTP runtime if:

- service invocation count cannot be observed;
- stateful failures are dismissed as flaky without minimization;
- uncertain framing permits connection reuse;
- fuzzing requires exposing internal transport types as stable API;
- fixed corpus and production path disagree without explanation.

## Handoff

Plan 071 combines the qualified network state machine with live filesystem mutation and fault injection. Plan 072 uses deterministic corpora and fuzz evidence in release aggregation.
