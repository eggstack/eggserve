# Phase 33 — HTTP Wire-Correctness Closure

## Goal

Validate EggServe’s HTTP/1.1 behavior at the raw socket boundary and close ambiguity classes relevant to request smuggling, malformed framing, and incorrect response serialization.

## Scope

The supported surface remains intentionally narrow: HTTP/1.1 origin-form requests, static serving, callback responses, and the low-level client. Unsupported protocol forms must fail consistently.

## Workstream A — Request-line and target forms

Add raw TCP tests for:

- valid origin-form targets;
- absolute-form rejection;
- authority-form rejection;
- asterisk-form rejection unless explicitly supported;
- malformed methods;
- invalid HTTP versions;
- embedded controls, spaces, NULs, and oversized targets;
- query/fragment edge cases;
- percent-encoded separator and traversal variants.

Document exactly which parser decisions are EggServe policy versus Hyper behavior.

## Workstream B — Header grammar and limits

Test:

- invalid field names;
- obsolete folding;
- leading whitespace;
- bare LF and mixed line endings;
- CR/LF injection attempts;
- duplicate fields;
- oversized single headers;
- oversized aggregate headers;
- incomplete headers;
- invalid byte sequences.

Ensure malformed inputs fail without traceback, path, or internal error leakage.

## Workstream C — Message framing ambiguity

Add regression tests for:

- duplicate identical `Content-Length`;
- duplicate conflicting `Content-Length`;
- comma-joined lengths;
- `Transfer-Encoding` plus `Content-Length`;
- unsupported transfer codings;
- malformed chunked encoding where applicable;
- request bodies on GET/HEAD according to the declared policy;
- body bytes without framing;
- premature EOF.

Choose conservative rejection for ambiguous framing. Record the rule in `docs/release-contract.md`.

## Workstream D — Response validation

Create one Rust-controlled validation path for static and callback responses.

Validate:

- status range;
- invalid or informational statuses where unsupported;
- header names and values;
- duplicate-header preservation;
- forbidden hop-by-hop headers;
- explicit `Content-Length` consistency;
- `Transfer-Encoding` injection;
- HEAD body suppression;
- 204/304 body suppression;
- file-backed and bytes-backed consistency.

Invalid callback responses must produce a generic 500 or a pre-dispatch Python exception according to the release contract; they must never emit malformed wire data.

## Workstream E — Conditional and range semantics

Socket-test:

- `If-None-Match` precedence;
- `If-Modified-Since` behavior;
- `If-Range` behavior;
- valid, suffix, open-ended, unsatisfiable, malformed, and multi-range requests;
- 206 and 416 headers;
- HEAD with range;
- body length matching `Content-Range`;
- mutation/truncation behavior after resolution.

Document any deliberate deviation from common server behavior.

## Workstream F — Connection lifecycle

Verify:

- HTTP/1.1 keep-alive behavior;
- explicit `Connection: close`;
- malformed-request closure;
- timeout closure;
- no reuse after truncated file-stream errors;
- pipelining behavior, either supported and tested or explicitly rejected/unsupported.

## Test harness

Build a small reusable raw-socket harness under Rust integration tests or Python integration tests that can:

- send exact byte sequences;
- half-close or stall connections;
- parse status line/headers without normalizing malformed responses;
- assert connection reuse/closure;
- run consistently in CI.

## Likely files

- `crates/eggserve-core/src/service.rs`
- request-validation and response-planning modules
- `crates/eggserve-python/src/server.rs`
- Rust and Python integration test directories
- HTTP correctness and release-contract documentation

## Acceptance criteria

- Supported request-target and framing rules are normative.
- Smuggling-relevant ambiguity cases are rejected and regression-tested.
- Static and callback responses share validation semantics.
- HEAD, 204, 304, 206, and 416 behavior is wire-tested.
- Body length and response headers cannot disagree.
- Truncated/error responses cannot leave a reusable ambiguous connection.
- No unsupported protocol capability is implied by docs.

## Non-goals

- No HTTP/2 or HTTP/3.
- No multipart range implementation unless already committed to the contract.
- No proxy-mode absolute-form support.
