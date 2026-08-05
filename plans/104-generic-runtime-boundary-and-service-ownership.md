# Plan 104 — Generic Runtime Boundary and Service Ownership Correction

## Status

**IMPLEMENTED — FINAL REVALIDATION REOPENED BY PLAN 108.**

This plan corrects the generic Rust/Python service runtime without expanding EggServe into an application server. It is intentionally the only API-breaking phase in the roadmap.

Prerequisites:

```text
Plan 102 roadmap present
Plan 103 implemented or implementation-compatible
```

Plan 103 and Plan 104 may overlap only where shared limit types must move to their final owner. Do not create temporary duplicate owners to avoid sequencing work.

## Goal

Make the reusable HTTP/1.1 runtime internally coherent and truthful:

1. custom services start without a static root or `ServeState`;
2. transport state and static filesystem state are separate;
3. one runtime file-stream semaphore governs every canonical file response;
4. service-declared body policy controls methods with service-defined content semantics;
5. transport-level framing and TRACE restrictions remain enforced;
6. request conversion never substitutes GET, HTTP/1.1, or empty header values after failure;
7. every retained `RuntimeConfig` field has a production effect;
8. static-serving behavior remains unchanged through `StaticService`;
9. the Python facade continues to use the same bounded native runtime.

## Non-goals

This plan does not add:

- routing;
- middleware;
- request extensions or arbitrary per-request type maps;
- ASGI/WSGI adapters;
- async Python handlers;
- response streaming from Python iterators;
- trailers support;
- multipart or decompression support;
- WebSockets or upgrades;
- HTTP/2 or HTTP/3;
- a generic listener registry;
- a dependency-injection container;
- a tower/axum/actix integration layer;
- a new public server framework.

The service boundary remains a small callback/trait around canonical request and response types.

## Architectural decision

### Runtime owns transport state

Introduce or clarify a runtime-owned state object whose contents are limited to transport concerns required after binding:

```text
RuntimeState
  RuntimeConfig
  connection semaphore
  file-stream semaphore
  lifecycle/task tracking references as needed
  logger/counters references only where already global or existing
```

The exact type name may differ, but the ownership rule must be explicit.

### Static service owns filesystem state

Retain or rename a static-only state object containing:

```text
StaticServiceState
  pinned root
  static policy
  directory listing limits
  index-page policy
```

It must not own the transport file-stream semaphore after this plan.

### File-stream admission belongs to transport conversion

A canonical `ResponseBody::File` can originate from:

- built-in static serving;
- a custom Rust service;
- a Python static responder;
- another bounded native responder.

Therefore the file-stream semaphore belongs to the runtime transport conversion path, not to a static resolver.

Required flow:

```text
Service -> canonical Response -> normalize -> acquire runtime file permit -> transport body
```

The owned permit remains inside the file body stream until completion or drop.

### Custom service construction does not require static configuration

A caller must be able to write conceptually:

```rust
let server = Server::builder()
    .runtime(runtime_config)
    .build_runtime()?;

let handle = server.start_with_service(service).await?;
```

The exact method names should minimize disruption to current API users. Acceptable alternatives include:

- `build()` constructs a runtime-only `Server`, while static startup requires `.static_service(...)` or `start_static(...)`;
- separate `build()` and `build_static()` methods;
- `ServerBuilder` stores an optional static service and `Server::start()` errors when absent, while `start_with_service()` does not inspect it.

Do not retain a fake `root: "."` or construct a pinned root for custom services.

## Track A — Inventory and define state ownership

### Required inventory

Before refactoring, identify all production references to:

- `ServeConfig`;
- `ServeState`;
- `RuntimeConfig`;
- file-stream semaphores;
- static root/policy;
- listener source;
- `ServerBuilder::build()`;
- `Server::start()`;
- `Server::start_with_service()`;
- Python native server construction;
- canonical-to-Hyper file conversion.

Classify each field as:

```text
transport-owned
static-service-owned
Python-facade-owned
obsolete/unused
```

Do not preserve a field solely because it appears in several conversion layers.

### Required design record

Update the existing runtime/configuration architecture document rather than adding a new ADR unless the repository convention requires one for public API changes.

The document must state:

- custom services have no implicit filesystem root;
- static root pinning begins when `StaticService` is constructed;
- transport file admission is shared by all file-backed responses;
- runtime and service configuration are intentionally separate.

### Acceptance criteria for Track A

- every state field has one named owner;
- the implementation sequence is clear before code movement;
- no new generic context bag is introduced.

## Track B — Separate runtime state from static state

### Required implementation

Refactor server startup so the accept loop and connection pipeline receive runtime state only.

The runtime path may require:

- bind/listener;
- connection semaphore;
- file-stream semaphore;
- timeout values;
- keep-alive behavior;
- request-body ceiling;
- TLS configuration;
- lifecycle/shutdown channels;
- counters/logger access.

It must not require:

- a root path;
- `PinnedRoot`;
- static dotfile/symlink/listing policy;
- index names;
- static MIME configuration.

### Static startup

`Server::start()` or its replacement should construct/use a `StaticService` explicitly and then call the same custom-service transport path.

Preferred convergence:

```text
static CLI/Python configuration
  -> StaticService
  -> Server::start_with_service(StaticService)
  -> shared connection pipeline
```

Avoid maintaining a separate static accept loop and generic accept loop with duplicated TLS, admission, shutdown, and error behavior.

If a direct static path currently preserves file streaming better than `StaticService::call()`, fix the canonical static response representation rather than retaining two long-term runtime paths.

### File-backed static response requirement

`StaticService::call()` must return a canonical file body without collecting file content into memory.

Remove or replace any conversion path that:

- constructs a Hyper response first;
- collects its body;
- converts it back to canonical bytes;
- relies on a comment that production does not use the path.

There must be one real service implementation path used by tests and production.

### Required tests

Add tests proving:

- runtime-only server construction succeeds without a root;
- custom service starts when current working directory is inaccessible or irrelevant where the platform allows a controlled test;
- no `PinnedRoot` is constructed for a custom service, using an internal test seam or invalid static path that would fail if touched;
- static startup still pins the configured root;
- CLI static serving uses the shared generic accept loop;
- TLS custom and static startup both use the same transport path;
- shutdown/lifecycle behavior remains unchanged.

### Acceptance criteria for Track B

- custom service startup has no filesystem dependency;
- one accept/connection implementation serves static and custom services;
- static responses remain file-backed and bounded;
- no duplicate compatibility accept loop remains unless narrowly required by Python callback scheduling and documented.

## Track C — Make runtime file-stream admission authoritative

### Required implementation

Construct the file-stream semaphore from `RuntimeConfig.max_file_streams` when the runtime starts.

Pass it to canonical response conversion for every service response.

Remove file-stream semaphore ownership from static `ServeState`/`StaticServiceState`.

Remove or change `StaticServiceBuilder::max_file_streams()` because a service should not configure a transport-global resource that it does not own.

Preferred API:

```rust
RuntimeConfig::builder().max_file_streams(n)
StaticService::builder(root).policy(...).build()
```

For compatibility constructors based on `ServeConfig`, translate `ServeConfig.limits.max_file_streams` into `RuntimeConfig` once, then do not retain another active copy.

### Multiple-service policy

EggServe currently starts one service per server. Do not design per-service weighted file admission or multiple service pools.

### Saturation behavior

Retain the documented bounded result, normally 503, when a file permit is unavailable.

Required behavior:

- full file responses acquire a permit;
- range responses acquire a permit;
- byte responses do not;
- empty responses do not;
- normalized HEAD responses do not;
- permit release occurs on completion, transport error, cancellation, or drop.

### Required tests

Use deterministic direct body ownership tests plus one runtime integration test:

- semaphore capacity one;
- first file body holds permit;
- second file response maps to 503/`FileStreamLimit`;
- dropping first body permits a later response;
- range shares the same pool;
- static and custom file responses contend for the same runtime pool in separate server instances/configurations as appropriate;
- no static-owned semaphore remains.

Do not use kernel-buffer saturation as the primary proof.

### Acceptance criteria for Track C

- `RuntimeConfig.max_file_streams` directly controls the production semaphore;
- no second active file-stream count exists;
- canonical file bodies never bypass transport admission.

## Track D — Correct request-body policy layering

### Transport rules to retain

The runtime must continue to reject malformed framing, including:

- conflicting transfer framing visible at the Hyper boundary;
- duplicate invalid `Content-Length` forms;
- declared length above the effective service/runtime ceiling;
- unsupported body framing that EggServe cannot consume safely;
- invalid `Expect` behavior when the request will be rejected;
- TRACE request content, which HTTP semantics prohibit.

### Static policy

`StaticService` declares `RequestBodyPolicy::Reject` for its supported methods.

The runtime then enforces that declaration before invoking the service.

### Custom service policy

For custom services, call `service.request_body_policy(&head)` before deciding whether a body is accepted.

Do not globally reject request content merely because the method is:

- GET;
- HEAD;
- OPTIONS;
- DELETE.

A service may choose `Reject`, `Buffer`, or `Stream`, subject to the runtime hard ceiling.

HEAD response normalization remains transport behavior; request content policy remains service-declared.

### TRACE rule

Reject TRACE content before service invocation regardless of service preference.

Do not implement TRACE reflection. A custom service may handle bodyless TRACE only if the existing method surface permits it and security documentation is explicit. Rejecting TRACE entirely is also acceptable if that is the existing bounded policy, but do not claim all safe/idempotent methods forbid content.

### Rejected-body connection behavior

When policy rejects a body:

- do not invoke the service;
- do not send `100 Continue`;
- return the controlled error;
- ensure the connection closes rather than attempting a fixed-duration drain;
- do not add a drain mode.

### Buffer policy

Retain:

- runtime hard ceiling;
- service-requested lower ceiling;
- total body-read deadline;
- one complete buffered body for the service;
- fail-closed disconnect/incomplete-body errors.

### Stream policy

Retain bounded streaming only as already implemented. Do not add:

- parallel body readers;
- rewind;
- trailers;
- arbitrary cloneability;
- background draining;
- Python async iteration.

Ensure incomplete consumption forces connection closure in actual transport behavior, not only an event log.

### Required tests

Add service-level wire tests for:

- custom OPTIONS body accepted under Buffer policy;
- custom DELETE body accepted under Buffer policy;
- custom extension method body accepted when policy allows;
- custom GET body accepted only when the service explicitly allows it;
- the same methods are rejected when service policy is Reject;
- static GET/HEAD bodies remain rejected;
- TRACE content is rejected before invocation;
- `Expect: 100-continue` is not emitted for rejected bodies;
- unread Stream body closes the connection;
- runtime ceiling overrides a larger service request;
- service lower limit overrides a larger runtime ceiling.

Do not add exhaustive method matrices. One representative test per semantic class is sufficient.

### Acceptance criteria for Track D

- runtime framing policy and service content policy are distinct;
- static behavior remains hardened;
- custom service semantics are no longer preempted by static assumptions;
- no rejected-body drain remains.

## Track E — Make request conversion fail closed

### Method conversion

Replace any fallback of the form:

```rust
Method::new(other).unwrap_or_else(|_| Method::get())
```

with explicit error propagation.

Because Hyper normally validates method tokens, this path is defensive. It must still never alter semantics.

Required result:

- recognized standard methods use canonical constructors if desired;
- valid extension tokens preserve exact bytes/case representable by the canonical type;
- invalid conversion returns a controlled 400/internal boundary error as appropriate;
- no invalid method becomes GET.

### HTTP version conversion

Do not map an unsupported version to HTTP/1.1.

For the HTTP/1-only server:

- preserve HTTP/1.0;
- preserve HTTP/1.1;
- reject any unexpected version with a controlled error/505 path;
- do not add HTTP/2 support.

### Header conversion

Current canonical headers are string-backed. Until/unless a byte-backed header representation is intentionally designed, reject values that cannot be represented rather than replacing them with an empty string.

Required behavior:

- valid field names and values are preserved;
- duplicate ordering is preserved;
- invalid UTF-8/opaque values that cannot enter the current canonical type cause a controlled request rejection;
- no value becomes `""` because `to_str()` failed;
- no invalid field is silently skipped.

Do not redesign all canonical headers to byte storage in this plan. That would broaden the API and FFI work considerably. Record byte-preserving headers as a potential future pre-1.0 decision only if needed.

### Request-target conversion

Retain exact origin-form parsing and query separation. Do not normalize an invalid target to `/`.

### Required tests

Add direct conversion tests for:

- extension method preservation;
- method case preservation;
- invalid method conversion returns error and never GET;
- HTTP/1.0 and HTTP/1.1 preservation;
- unexpected version rejection at the internal seam;
- duplicate header order preservation;
- non-representable header value rejection;
- no partial header block returned after a later field fails;
- invalid request target rejection without fallback.

### Acceptance criteria for Track E

- request conversion is atomic and fail-closed;
- no semantic fallback remains;
- static and Python handlers see the same canonical method/header data.

## Track F — Reconcile `RuntimeConfig`

### Field-by-field decision

#### Retain and implement

`bind`
: authoritative listener address when no pre-bound listener is supplied.

`max_connections`
: authoritative connection semaphore count.

`max_file_streams`
: authoritative transport file-response semaphore count.

`header_read_timeout`
: passed to Hyper HTTP/1 builder.

`connection_total_timeout`
: retains bounded connection lifetime semantics.

`handler_timeout`
: wraps one service invocation.

`body_read_timeout`
: bounds buffer/stream body consumption according to existing contract.

`graceful_shutdown_timeout`
: used by lifecycle shutdown orchestration.

`keep_alive`
: must be passed to the Hyper HTTP/1 builder using its supported configuration.

`server_header`
: if retained, must be added at the final runtime response boundary and cannot be spoofed/duplicated by a handler.

`tls_config`
: remains feature-gated and effective.

`max_request_body_bytes`
: remains the runtime hard ceiling.

`request_body_policy`
: reconsider ownership. If the service always declares policy, remove the global policy field or define it strictly as a runtime maximum/default with explicit precedence. Do not retain two equal policy sources.

#### Remove

`max_in_flight_requests`
: remove unless a real production enforcement point exists. Hyper HTTP/1 request processing should not be wrapped in a fictitious pipelining concurrency setting.

`incomplete_body_policy`
: remove from `RuntimeConfig` and the public API if `Close` is the only supported behavior. Keep close behavior as an invariant, not a configurable one-variant enum.

### `request_body_policy` final choice

Preferred model:

- service declares `RequestBodyPolicy` per request;
- runtime supplies only `max_request_body_bytes` as a hard ceiling;
- static service always returns Reject;
- Python facade service returns Buffer or Reject according to its bounded constructor mode.

Remove `RuntimeConfig.request_body_policy` if this model already covers all use cases.

Do not create a policy-merging matrix beyond `effective_limit = min(service_limit, runtime_limit)`.

### Server header normalization

If `server_header` is `Some`:

- validate it at configuration build time as a legal field value;
- strip/replace any service-provided `Server` header at finalization;
- emit exactly one authoritative header;
- HEAD/error/file responses behave consistently.

If `None`:

- remove any service-provided `Server` header if the current security contract reserves it;
- or preserve a valid service header only if documentation already permits it.

Choose one rule and test it. Prefer runtime authority because the field exists for that purpose.

### Keep-alive implementation

Pass `keep_alive` to the HTTP/1 connection builder. Tests must prove:

- default true permits a second request where protocol/framing allow;
- false closes after one response;
- rejection/incomplete-body paths still close regardless of true;
- shutdown behavior remains correct.

### Migration handling

This is an alpha crate. Update compile samples and migration notes, but do not add deprecated no-op setters for removed fields.

### Acceptance criteria for Track F

- every retained field has a direct production behavior test;
- every removed field disappears from public docs and bindings;
- there is one request-body policy authority plus the runtime ceiling;
- runtime configuration contains no static filesystem fields.

## Track G — Python facade integration

### Required preservation

The six supported classes remain:

```text
HTTPServer
ThreadingHTTPServer
HTTPSServer
ThreadingHTTPSServer
BaseHTTPRequestHandler
SimpleHTTPRequestHandler
```

The facade remains synchronous and bounded.

### Native server translation

Update Python native server construction to the corrected runtime model:

- custom handlers create runtime state without a static root;
- `SimpleHTTPRequestHandler` constructs a static service explicitly;
- callback-worker limit remains Python-facade-owned;
- handler response-size limit remains Python-facade-owned;
- request-body mode translates into the service policy, not a global static assumption;
- file-backed native responses use runtime transport admission.

### No fake root

Remove `root = "."` or equivalent placeholder behavior for custom handlers.

A custom handler must not fail because the process current directory was deleted, inaccessible, or not intended as a static root.

### Required installed-wheel tests

Add or update tests for:

- custom handler server works without static root initialization;
- static handler retains confinement and range/conditional behavior;
- body-bearing custom POST/OPTIONS follows configured buffer limit;
- static body rejection remains unchanged;
- file response admission remains shared;
- lifecycle and tuple metadata remain unchanged;
- TLS custom/static parity remains intact.

Do not add async handler support or raw socket exposure.

## Track H — Documentation and migration

Update active references, including at minimum:

- `README.md` Rust/Python API descriptions;
- `architecture/runtime.md`;
- `architecture/configuration.md`;
- `architecture/eggserve-core.md`;
- `architecture/eggserve-python.md`;
- `architecture/overview.md` only where diagrams are materially wrong;
- `docs/http-primitives.md`;
- `docs/python-api.md`;
- `docs/python-http-server-compatibility.md`;
- `docs/api-stability.md`;
- `docs/security-policy.md`;
- `docs/threat-model.md`;
- `docs/non-goals.md`;
- compile-sample tests.

Required documentation statements:

- custom services do not create a static root;
- runtime owns file-stream admission;
- service owns method/content semantics within transport bounds;
- static service rejects bodies and supports only GET/HEAD;
- close is the invariant for incomplete/rejected bodies;
- removed configuration fields are listed in a concise pre-1.0 migration note.

Do not produce another broad architecture rewrite.

## Required verification

At minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features client-tls
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Run focused deep tests only where affected:

- canonical file permit ownership;
- raw-wire body policy/framing;
- lifecycle/shutdown;
- Python installed-wheel compatibility;
- TLS static/custom parity.

Do not run filesystem adversarial suites unless path/root code changes beyond state ownership.

## Completion criteria

Plan 104 is complete when:

- custom service startup has no root dependency;
- static and custom services share one transport path;
- static service returns canonical file bodies without buffering;
- runtime file-stream admission is authoritative;
- service body policy is respected;
- TRACE/framing restrictions remain transport-enforced;
- no request data is silently substituted;
- every retained `RuntimeConfig` field is effective;
- inert fields are removed without no-op compatibility shims;
- Python facade behavior remains bounded and compatible;
- focused and full verification pass.

## Explicit rejection criteria

Reject the implementation if it:

- retains two accept loops with duplicated correctness logic without a documented unavoidable reason;
- constructs any pinned root for a custom service;
- leaves file-stream admission inside static state;
- buffers static files in `StaticService::call()`;
- globally forbids content on OPTIONS or DELETE;
- maps invalid methods to GET or unsupported versions to HTTP/1.1;
- drops or empties invalid headers silently;
- keeps one-variant configuration enums;
- adds a generic framework abstraction;
- adds ASGI/WSGI, routing, middleware, or async Python handlers;
- adds new CI workflows or dependencies unrelated to the correction.

## Handoff note

Plan 105 begins only after the final runtime ownership model is stable. Binary measurements taken before Plan 104 are useful as historical context but must not be treated as the final optimization baseline.
