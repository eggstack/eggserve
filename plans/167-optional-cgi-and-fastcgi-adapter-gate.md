# Plan 167 — Optional CGI and FastCGI Adapter Gate

## Status

**CLOSED — NO-GO, no in-tree CGI/FastCGI adapters.**

This is intentionally optional work. Do not implement either adapter merely to increase feature count or claim `http.server` parity.

## Goal

Determine whether EggServe should ship narrowly scoped CGI and/or FastCGI service adapters after the generic HTTP substrate is complete, and if approved, implement them outside the core runtime with strict process/protocol/resource boundaries.

CGI and FastCGI are separate questions:

- CGI is historical Python `http.server` compatibility;
- FastCGI is a useful downstream application gateway protocol but was never a `http.server` feature.

Neither belongs in `eggserve-core`'s canonical HTTP machinery.

## Upstream reality

CPython deprecated `CGIHTTPRequestHandler` and `python -m http.server --cgi` in 3.13 because CGI is obsolete/unmaintained and raises security concerns. Python 3.15 removes both.

References:

- https://docs.python.org/3.14/library/http.server.html
- https://docs.python.org/3.15/whatsnew/3.15.html

Therefore “maintain Python stdlib semantics” must not be interpreted as permanently adopting every removed stdlib feature. If EggServe implements CGI, document it as a legacy compatibility adapter targeting the historical Python <=3.14 behavior subset, not as the modern default product surface.

FastCGI remains a distinct protocol with Responder/Authorizer/Filter roles and record streams (`BEGIN_REQUEST`, `PARAMS`, `STDIN`, `STDOUT`, `STDERR`, `END_REQUEST`, etc.).

Reference: https://fastcgi-archives.github.io/FastCGI_Specification.html

## Mandatory go/no-go gate

Before implementation, record answers in this plan or a follow-up implementation plan:

1. Is there a concrete EggServe/upstream consumer for CGI, FastCGI, or both?
2. Are Plans 162–166 stable enough that the adapter can be a plain canonical `Service` with no core exceptions?
3. Does adding the adapter require a material new dependency or platform-specific process-management burden?
4. Can all subprocess/backend input/output be bounded and cancelled without weakening the core runtime?
5. Is the packaging/maintenance burden justified compared with leaving an example/downstream crate?

Possible outcomes are valid:

- implement FastCGI only;
- implement legacy CGI only;
- implement both as separate optional adapters;
- implement neither and document the extension seam.

Do not force a yes result.

## Packaging boundary

Preferred ownership if implemented:

```text
eggserve-core          canonical HTTP runtime only
     ^
     |
eggserve-cgi           optional/default-off Service adapter

eggserve-fastcgi       optional/default-off Service adapter
```

A single `eggserve-adapters` crate is acceptable only if it does not couple CGI subprocess dependencies to FastCGI-only users.

The workspace/default binary and Python wheels must not gain subprocess/FastCGI dependencies unless the feature is explicitly enabled and packaging policy approves it.

The anonymity-sensitive profile must never enable CGI/FastCGI by default.

# Track A — Legacy CGI adapter, if approved

## Compatibility target

Target the useful behavioral subset of historical `CGIHTTPRequestHandler` rather than reproducing socketserver internals or known awkward behavior.

Potential compatibility surface:

- configurable CGI directories, historically `/cgi-bin` and `/htbin`;
- GET/HEAD to CGI resources;
- POST only where configured for CGI execution;
- CGI environment mapping from canonical request metadata;
- parsed CGI response headers/body mapped back into canonical `Response`.

Do not reproduce unsafe quirks solely for fidelity. Document intentional differences.

## Process security

CGI execution must be explicit opt-in and treated as local code execution, not a safe untrusted-content feature.

Requirements:

- resolve executable/script through EggServe confinement policy before spawn;
- no shell interpolation (`sh -c`, `cmd /c`) for request-derived data;
- explicit executable/argv construction;
- bounded environment key/value count and bytes;
- allowlist or precisely documented CGI variables;
- do not inherit arbitrary sensitive parent environment by default;
- bounded stdin/body bytes inherited from canonical request policy;
- bounded stdout header bytes and response bytes/streaming;
- bounded stderr capture or direct sanitized logging with a hard cap;
- process concurrency semaphore independent of HTTP connections;
- startup/execution/idle/output deadlines;
- child termination/reaping on timeout, client disconnect, shutdown, and adapter drop;
- no zombie processes;
- platform-specific process semantics tested on supported targets or unsupported targets explicitly excluded.

Do not promise UID dropping/sandboxing portability equivalent to historical CPython behavior. If privilege dropping is desired, require an external process/container boundary unless a separate platform-security plan is written.

## CGI response parsing

Treat CGI stdout as hostile adapter output:

- bounded header scan;
- reject NUL/CRLF injection/malformed field names;
- parse `Status` deliberately;
- feed all headers through `HeaderBlock` validation;
- strip/reject CGI-supplied `Content-Length`, `Transfer-Encoding`, `Connection`, and other runtime-owned framing/hop-by-hop fields according to canonical policy;
- apply Plan 165 final privacy policy after adaptation;
- stream body through Plan 162 rather than buffering unbounded output.

Do not forward script stderr to clients.

## Compatibility tests

If CGI is approved, build a small fixture corpus comparing documented historical Python behavior for normal cases, while explicitly recording security-driven incompatibilities.

# Track B — FastCGI adapter, if approved

## Initial protocol scope

Implement **FastCGI v1 Responder role only** first.

Support:

- `FCGI_BEGIN_REQUEST`;
- encoded `FCGI_PARAMS`;
- `FCGI_STDIN` streaming;
- `FCGI_STDOUT` streaming;
- bounded `FCGI_STDERR` diagnostics;
- `FCGI_END_REQUEST`;
- `FCGI_ABORT_REQUEST` where needed for cancellation;
- Unix-domain and TCP backend connections if they can use existing async I/O without large dependency expansion.

Initially exclude:

- Authorizer role;
- Filter role / `FCGI_DATA`;
- multiplexing multiple request IDs over one backend connection;
- backend process spawning/management;
- automatic FastCGI pool supervision;
- dynamic backend discovery/load balancing.

A downstream application server/process manager owns backend lifecycle.

## FastCGI request mapping

Build PARAMS only from canonical validated request/context data. Define and test mappings for standard CGI/FastCGI variables such as:

- request method;
- request URI/path/query;
- protocol version;
- content type/length where valid;
- server name/port only when semantically available;
- remote address only when `ConnectionInfo` has socket endpoints;
- HTTP request headers using documented CGI mapping.

For non-socket transports, do not fabricate `REMOTE_ADDR`. Either omit it or define a documented empty/unspecified representation according to backend compatibility testing.

Never pass I2P identities through generic CGI variables automatically.

## Backend resource controls

Add explicit bounds for:

- backend concurrent requests/connections;
- connection establishment timeout;
- PARAMS bytes/count;
- STDIN bytes (already constrained by request policy but enforce protocol accounting too);
- STDOUT response-header bytes;
- response body streaming under Plan 162 backpressure;
- STDERR bytes retained/logged;
- total backend request duration and no-progress timeout.

Client disconnect/shutdown must send `FCGI_ABORT_REQUEST` when safe/useful, then close/drop backend state deterministically if the backend does not complete.

## Backend response conversion

Parse FastCGI `STDOUT` as CGI-style headers + body:

- no raw backend framing reaches the HTTP client;
- canonical status/header validation applies;
- runtime owns HTTP framing;
- Plan 165 response privacy policy applies after conversion;
- backend protocol errors become generic 502/500-class adapter errors according to a documented mapping, never raw protocol diagnostics.

Consider adding `BAD_GATEWAY` (502) and `GATEWAY_TIMEOUT` (504) canonical helpers if missing; do not overload internal 500 for every upstream failure when a gateway status is semantically correct.

## Connection reuse

Start with simple non-multiplexed backend connection semantics. `FCGI_KEEP_CONN`/connection pooling may be added only after correctness tests prove request IDs, cancellation, unread output, and error recovery cannot cross-contaminate requests.

Do not add pooling as a performance assumption in the first implementation.

## Python exposure

Do not add `CGIHTTPRequestHandler` to `eggserve.server` unless the go/no-go specifically approves historical compatibility and the support matrix clearly labels it legacy/default-off.

Prefer adapters as Rust services. If Python needs them, expose a small constructor/service wrapper through `eggserve.lowlevel`; do not add subprocess/backend protocol logic in Python.

## Qualification

For either implemented adapter add:

- malformed/oversized output tests;
- timeout/disconnect/shutdown tests;
- concurrency saturation/permit recovery;
- response-smuggling/framing adversarial cases;
- no version/path/env leakage to client errors;
- cross-platform process tests for CGI where supported;
- FastCGI fake-backend protocol corpus with fragmented records, malformed lengths, STDERR, early EOF, abort, and delayed output;
- soak for child/backend resource leakage.

## Non-goals

Do not use adapter work to add:

- framework routing;
- worker process supervision;
- automatic app reload;
- ASGI/WSGI;
- reverse proxying to arbitrary HTTP origins;
- WAF/rate limiting;
- shell command execution configured from request data.

## Acceptance criteria

This plan can close in one of two ways.

### No-go closure

- [ ] concrete consumer/maintenance analysis is recorded;
- [ ] documentation points downstream authors to the canonical `Service` extension seam;
- [ ] no core complexity is added for speculative compatibility.

### Implementation closure

- [ ] each approved adapter is a separate/default-off service layer with no HTTP parser/framing ownership;
- [ ] all process/protocol I/O is bounded and cancellation-safe;
- [ ] CGI is clearly labeled legacy and not recommended for untrusted/public/I2P serving;
- [ ] FastCGI initial scope is Responder-only and non-multiplexed unless separately qualified;
- [ ] adapter responses pass through canonical normalization and Plan 165 privacy policy;
- [ ] Plan 168 includes adapter-specific tests/benchmarks only if the adapter actually ships.

## Handoff

Do not block Plan 168 core qualification on a no-go decision. If an adapter is implemented, add its evidence to Plan 168 or a narrowly scoped follow-up qualification file.

## Closure record

Closed as no-go in commit `5083252cc9b4b6bf65f23caaa361c910912f8d87`.

No in-tree CGI or FastCGI adapter was added. Downstream gateways use the
canonical `Service` boundary and own their subprocess/protocol limits.
