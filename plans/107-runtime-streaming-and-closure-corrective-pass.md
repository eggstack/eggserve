# Plan 107 — Runtime Streaming, Ownership, and Closure Corrective Pass

## Status

**CORRECTIVE FOLLOW-UP REQUIRED — SEE PLAN 108.**

This is a bounded corrective pass against repository state:

```text
834caefa04e1261c58c35f2ff0ff51ed8a0e0335
```

It reopens the closure claims for Plans 102–106 only where the implementation does not satisfy their stated acceptance criteria. It does not reopen the broader product roadmap and does not authorize new features.

The implementation agent must treat the following as release-blocking until corrected:

1. static files are collected into memory through `StaticService::call()`;
2. file-stream admission is not one server-wide runtime resource;
3. static startup constructs duplicate `ServeState` instances;
4. the generic runtime still rejects GET/HEAD/DELETE request content before service policy;
5. Python custom-handler startup still attaches static filesystem configuration;
6. `RuntimeConfig.server_header` appears retained without authoritative runtime behavior;
7. incomplete streamed request bodies log closure intent without proving actual connection closure;
8. the manual release smoke test is not rooted in a controlled fixture and can fail with the secure default listing policy;
9. binary-size and closure records overstate or misdescribe the implemented result.

This plan is the sole corrective document for these findings. Do not create Plans 108+ for ordinary implementation details. If implementation uncovers a separate security vulnerability, record it explicitly and keep its correction in this plan where feasible.

## Goal

Restore the intended narrow architecture:

```text
TCP/TLS listener
    -> one runtime state per running server
    -> one connection pipeline
    -> service-declared request-body policy
    -> canonical response
    -> one runtime-owned file-stream admission point
    -> Hyper transport body
```

For static serving:

```text
StaticService
    -> confined handle-based resolution
    -> canonical metadata/body plan
    -> ResponseBody::File for file/range responses
    -> runtime file permit
    -> bounded streaming without collection
```

For custom services:

```text
RuntimeConfig + Service
    -> no root path
    -> no PinnedRoot
    -> no ServeConfig
    -> no static semaphore
```

## Non-goals

Do not add:

- ASGI or WSGI support;
- routing or middleware;
- upload, multipart, compression, WebSocket, HTTP/2, or HTTP/3 support;
- a generalized preflight framework;
- multiple service pools or weighted file admission;
- a new logging or metrics framework;
- a new release/evidence workflow;
- automated publication;
- another compatibility layer around removed alpha APIs;
- new Python handler classes;
- an HTTP client expansion;
- a broad rewrite of the filesystem resolver.

## Required implementation order

Implement in this sequence:

```text
A. Reopen inaccurate closure state
B. Make static responses canonical and non-collecting
C. Introduce one server-wide RuntimeState and file semaphore
D. Remove duplicate static state and duplicate static transport paths
E. Correct request-body policy layering
F. Remove static configuration from Python custom handlers
G. Make server_header and incomplete-body close behavior real
H. Repair release smoke and size evidence
I. Reconcile documentation, tests, and closure state
```

Tracks B–G should be reviewed as one runtime-correctness change set even if committed separately. Do not mark Plan 107 complete after only documentation or test changes.

---

# Track A — Reopen inaccurate closure state

## Required documentation state

At the beginning of implementation, change active status documents so they do not continue to assert completed acceptance criteria that are currently false.

Required updates:

- Plan 102 status: `REOPENED BY PLAN 107` or equivalent;
- Plan 104 status: `CORRECTIVE WORK REQUIRED — SEE PLAN 107`;
- Plan 105 status: retain completed build-profile work but state that final measurement/packaging evidence is pending Plan 107;
- Plan 106 status: retain CI/fuzz simplification as implemented but state closure validation is reopened;
- `release/plan-102-106-closure.md`: prepend a supersession notice pointing to Plan 107.

Do not delete the historical closure record. It remains useful as a record of what was claimed and tested at that point, but it must no longer be presented as the current final truth.

## Closure claims that must be withdrawn pending correction

Withdraw or qualify claims that:

- one runtime semaphore governs all file responses;
- static and custom request-body policy is correctly layered;
- every retained runtime field is effective;
- no relevant public configuration fields were removed;
- the size reductions are directly comparable end-to-end distribution reductions;
- Plans 102–106 are fully complete.

## Acceptance criteria

- active docs no longer advertise known-false runtime properties;
- historical records remain intact and clearly superseded;
- no implementation completion is claimed before Tracks B–I pass.

---

# Track B — Make `StaticService` canonical and non-collecting

## Current failure mode

The current static service constructs a Hyper response, then `StaticService::call()` collects the entire Hyper body and converts it back into a canonical byte response. Under the unified `Server::start() -> start_with_service(StaticService)` path, this can buffer complete files in memory before transport.

This violates:

- streaming semantics;
- file-stream admission;
- bounded memory behavior;
- backpressure;
- the canonical response ownership design;
- Plan 104's explicit prohibition on Hyper-to-canonical body collection.

## Required architecture

`StaticService::call()` must directly return a canonical response.

Required result classes:

```text
normal file             -> ResponseBody::File(BodySource::FileFull)
range                    -> ResponseBody::File(BodySource::FileRange)
HEAD                     -> ResponseBody::EmptyWithLength or equivalent canonical metadata
304 / 416 / errors       -> ResponseBody::Empty or bounded ResponseBody::Bytes
listing                  -> bounded ResponseBody::Bytes
```

No production static path may construct a Hyper response before the runtime conversion boundary.

## Required refactor

Create or consolidate one canonical static planning path that accepts:

- canonical `RequestHead`;
- static-only state containing pinned root, policy, listing limits, chunk-size metadata if still service-owned, and index behavior;
- the already-opened file/directory handles returned by confinement;
- conditional and range request fields.

It must return:

```rust
Result<canonical::Response, ServiceError>
```

or a similarly bounded internal error type converted once to the public service error.

Preferred internal shape:

```text
resolve request
  -> StaticResolvedResponse
  -> canonical Response
```

Do not retain a Hyper response as the internal static response type.

## File body requirements

For direct and index files:

- preserve the opened handle/capability from `RootGuard` resolution;
- never reopen by pathname after authorization;
- preserve full/range metadata;
- preserve ETag, Last-Modified, Content-Type, Accept-Ranges, and Content-Range behavior;
- preserve HEAD representation length without constructing a stream;
- do not acquire a file permit inside `StaticService`;
- do not convert the file to `Vec<u8>`;
- do not use `.collect()` on a file-backed body.

## Directory listing requirements

Directory listing may remain an in-memory body because it is bounded by `max_listing_response_bytes`.

Retain:

- entry count enforcement;
- checked response-size growth;
- escaping and percent encoding;
- CSP, nosniff, and referrer policy;
- no partial listing on overflow;
- HEAD metadata parity.

Convert listing output directly to canonical bytes rather than constructing and re-parsing a Hyper response.

## Error response requirements

Add a small canonical error constructor if needed. It should produce validated:

- status;
- Content-Type;
- Content-Length through normalization;
- Allow for static 405;
- bounded text body;
- HEAD suppression.

Do not preserve two separate error-construction systems merely because one currently returns Hyper responses.

## Remove misleading paths and comments

Delete comments claiming that large static files do not use `StaticService::call()` if production now routes through it.

Delete or rewrite any helper whose only purpose is:

```text
canonical -> Hyper -> collect -> canonical
```

## Required tests

Add tests that prove behavior rather than only type shape:

### Canonical static unit tests

- direct file returns `ResponseBody::File`;
- range returns `ResponseBody::File` with correct range;
- HEAD returns no file body while retaining GET representation length;
- 304 contains no file body;
- directory listing returns bounded bytes;
- index.html and index.htm return file bodies;
- errors remain bounded bytes/empty bodies.

### No-collection regression test

Create a large sparse or generated file substantially larger than the configured listing/body memory bounds. Call `StaticService::call()` directly and assert:

- returned body is `ResponseBody::File`;
- no body bytes have been read;
- no complete in-memory allocation is produced;
- the file handle remains available to transport conversion.

Do not rely solely on process RSS timing. Inspect the canonical body variant directly.

### Wire tests

- large file starts transmitting without prior full-file read;
- slow client receives bounded streaming;
- range transmits only requested bytes;
- HEAD sends no payload;
- client cancellation releases transport resources.

Use deterministic seams where possible. Avoid flaky memory assertions.

## Acceptance criteria

- `StaticService::call()` never calls `BodyExt::collect()`;
- production CLI static serving returns canonical file bodies;
- no file-backed response becomes `ResponseBody::Bytes`;
- direct and index routes use the same canonical file planner;
- static correctness, range, conditional, and confinement tests remain green.

---

# Track C — Introduce one server-wide runtime state and file semaphore

## Current failure mode

The current connection pipeline chooses the static `ServeState` semaphore when static state exists and otherwise creates a semaphore inside `serve_connection_with_service()`. Because that function runs once per accepted connection, custom runtime-only servers can receive one file-stream pool per connection rather than one per server.

`StaticServiceBuilder` also retains a `max_file_streams` setting and builds a service-owned semaphore.

## Required ownership

Create one runtime-owned state per running server.

Minimum contents:

```rust
struct RuntimeState {
    file_stream_semaphore: Arc<Semaphore>,
    // add only other transport-global state that is already required
}
```

The connection semaphore may remain owned by the accept loop if that is simpler, but the file-stream semaphore must be created once alongside it, before accepting connections.

Required construction:

```text
Server::start_with_service
  -> validate RuntimeConfig
  -> create Arc<Semaphore>(max_connections)
  -> create Arc<Semaphore>(max_file_streams)
  -> pass Arc<RuntimeState> into accept loop
  -> clone Arc into each connection task
  -> clone Arc into canonical response conversion
```

## Required signature changes

Remove `Option<&ServeState>` from the generic connection execution API.

The generic connection path should receive only:

- runtime config;
- runtime state;
- service;
- shutdown/lifecycle data;
- connection metadata.

Static filesystem state must be captured by `StaticService` itself and must not be passed through the generic runtime.

## Canonical transport conversion

All canonical responses must pass through one conversion helper that receives the runtime file semaphore:

```rust
canonical::to_hyper_response_with_file_stream_semaphore(
    response,
    &runtime_state.file_stream_semaphore,
)
```

Required behavior:

- `ResponseBody::FileFull` acquires one permit;
- `ResponseBody::FileRange` acquires one permit from the same pool;
- `Bytes`, `Empty`, and normalized HEAD do not acquire permits;
- failed acquisition maps to the existing bounded 503 result;
- permit lives inside the stream until EOF, error, cancellation, or drop;
- no static path acquires a separate permit first.

## Remove service-owned file admission

Remove:

- file semaphore from `ServeState` or its successor static state;
- `StaticServiceBuilder::max_file_streams()`;
- any static response helper that calls `state.file_stream_semaphore()`;
- tests that saturate file admission by directly manipulating static state.

If `Limits.max_file_streams` remains in `ServeConfig` for CLI/Python compatibility, translate it exactly once into `RuntimeConfig.max_file_streams` during startup. It must not remain active in static state.

## Required tests

### Server-wide saturation

Start one server with `max_file_streams = 1` and a custom service returning file bodies.

- first connection holds the stream permit;
- second connection receives 503 while first is active;
- after first body is dropped/completed, a later request succeeds;
- test uses two distinct TCP connections to prove the limit is server-wide.

### Static/custom parity

Run equivalent saturation tests for:

- `StaticService` full file;
- `StaticService` range;
- custom service full file;
- custom service range.

All must use the runtime-owned pool.

### HEAD/bytes behavior

- HEAD does not consume a permit;
- bytes responses remain available while a file permit is saturated;
- error responses remain available while saturated.

### Permit release

Prove release on:

- EOF;
- client disconnect;
- stream I/O error through an existing deterministic fault seam;
- dropped response before first poll where practical.

## Acceptance criteria

- exactly one file-stream semaphore exists per running server;
- no semaphore is created per connection;
- no static state owns file admission;
- `RuntimeConfig.max_file_streams` is authoritative;
- static and custom file responses share identical admission behavior.

---

# Track D — Remove duplicate static state and duplicate static transport paths

## Current failure mode

Static startup currently builds static state inside `StaticService::from_state_config()` and then may build another `ServeState` inside `Server::start_with_service()` because `serve_config` remains attached to the server.

The repository also retains substantial static-serving logic in both:

- `crates/eggserve-core/src/service.rs`;
- `crates/eggserve-core/src/server/static_service.rs`.

This duplication is already producing drift and obscuring which path is authoritative.

## Required startup ownership

`StaticService` must construct and own static state exactly once.

Preferred sequence:

```text
ServeConfig
  -> RuntimeConfig conversion
  -> StaticService::from_serve_config(ServeConfig)
  -> Server built with RuntimeConfig only
  -> start_with_service(StaticService)
```

After the service is constructed, the `Server` object must not retain `ServeConfig`.

Acceptable builder API:

```rust
let runtime = try_from_serve_config(&serve_config)?;
let service = StaticService::from_serve_config(serve_config)?;
let server = Server::builder().runtime(runtime).build()?;
server.start_with_service(service).await
```

A convenience `ServerBuilder::static_service(...)` may remain only if it constructs this shape internally and does not retain duplicate state.

## Remove generic runtime filesystem awareness

After this track:

- `Server` contains no `serve_config` field;
- `ServerBuilder` does not need `serve_config` for custom startup;
- generic accept loop contains no `Option<ServeState>`;
- generic connection function imports no `ServeState`;
- root initialization logging occurs when `StaticService` successfully constructs its pinned root, not in the generic runtime.

## Consolidate static logic

Choose one canonical static implementation.

Preferred direction:

- `server/static_service.rs` becomes the authoritative `Service` implementation;
- old `service.rs` public helpers either delegate to `StaticService`/shared canonical planner or are removed if internal and unused;
- direct legacy tests migrate to the canonical service/planner;
- no second file/conditional/index/listing implementation remains.

If a compatibility helper must remain, it must be a thin adapter with no independent resolution or response logic.

## Required tests

- static root is pinned exactly once, using a test counter/seam or construction behavior that fails on a second attempt;
- custom service startup constructs no static state;
- static startup emits one root-initialized event;
- no duplicate semaphore/state exists;
- direct helper and `StaticService` behavior cannot drift because they share one implementation;
- CLI, Rust embedded static, and Python static paths all reach the same canonical static service.

## Acceptance criteria

- one static state instance per static server;
- zero static state instances per custom server;
- one authoritative static planner/handler;
- generic runtime has no filesystem imports or optional static state.

---

# Track E — Correct request-body policy layering

## Current failure mode

The generic runtime currently rejects content for GET, HEAD, DELETE, and TRACE before consulting service policy. This incorrectly treats service-defined GET/HEAD/DELETE semantics as transport-invalid.

The runtime also synthesizes a GET request and compares body policies to infer whether another method is supported. This is static-service-specific reasoning inside the generic transport layer.

## Required transport policy

The runtime may globally enforce only:

- valid HTTP/1.0 and HTTP/1.1 version;
- request-target validity;
- header conversion validity;
- duplicate/conflicting Content-Length rules visible at the Hyper boundary;
- transfer-framing rules EggServe supports;
- runtime hard request-body ceiling;
- `Expect: 100-continue` behavior;
- TRACE content prohibition;
- connection closure for rejected/incomplete content.

Remove global request-content prohibition for:

- GET;
- HEAD;
- DELETE;
- OPTIONS;
- extension methods.

A custom service may define content semantics for those methods by returning `Buffer` or `Stream`.

## Remove synthetic method inference

Delete the logic that:

- constructs a synthetic GET `RequestHead`;
- compares GET policy with the current method policy;
- returns 405 based on that comparison.

`request_body_policy()` answers one question only: how the body for this request is handled.

Method support remains a service concern.

For `StaticService`:

- bodyless unsupported methods reach the service and return 405;
- body-bearing requests for a service-declared Reject policy may return the existing body-policy rejection before service invocation;
- do not add a new preflight framework merely to force 405 before 413 for every unsupported body-bearing method.

Document this precedence if externally observable.

## Static body policy

`StaticService::request_body_policy()` must return `Reject` for GET and HEAD because static serving does not consume request content.

It may return `Reject` for all methods. The service's `call()` remains responsible for method-not-allowed behavior on bodyless unsupported methods.

Do not use `Buffer { max_bytes: 0 }` as an indirect encoding of rejection.

## Runtime ceiling semantics

`RuntimeConfig.max_request_body_bytes` is a hard upper bound, not a global on/off body policy.

Required effective policy:

```text
Reject                   -> Reject
Buffer(service_limit)    -> Buffer(min(service_limit, runtime_limit))
Stream(service_limit)    -> Stream(min(service_limit, runtime_limit))
```

If the runtime limit is zero, Buffer/Stream becomes Reject.

## TRACE

TRACE content remains rejected before service invocation.

Do not implement TRACE reflection. Bodyless TRACE may either reach the service or be globally rejected according to the existing documented method scope, but content must never be accepted.

## Required tests

### Custom service acceptance

- GET body accepted when service returns Buffer;
- DELETE body accepted when service returns Buffer;
- OPTIONS body accepted when service returns Buffer;
- extension-method body accepted when service returns Stream/Buffer;
- each is rejected when service returns Reject;
- runtime ceiling overrides a larger service limit;
- service limit overrides a larger runtime ceiling.

### Static behavior

- static GET body rejected;
- static HEAD body rejected;
- bodyless unsupported method returns 405 with correct Allow;
- `Expect: 100-continue` is not sent for rejected static content.

### TRACE and framing

- TRACE content rejected before service call;
- duplicate Content-Length rejected;
- visible TE+CL conflict rejected;
- invalid Content-Length rejected rather than treated as no body;
- body rejection closes connection and suppresses a pipelined second request.

## Acceptance criteria

- generic runtime no longer classifies GET/HEAD/DELETE as universally content-forbidden;
- no synthetic GET policy probe remains;
- service policy determines body handling within runtime limits;
- static service explicitly rejects request bodies;
- TRACE and framing remain hardened.

---

# Track F — Remove static configuration from Python custom handlers

## Current failure mode

The Python native server path constructs a `ServeConfig` from the responder root and attaches it to `ServerBuilder` even when starting a custom handler service. This causes custom-handler startup to create/pin static filesystem state.

## Required branch separation

The Python facade should have two native startup shapes.

### Custom handler

```text
Python callback service
  + RuntimeConfig
  -> Server::builder().runtime(...).build()
  -> start_with_service(callback_service)
```

No `ServeConfig`, static responder root, or pinned filesystem state is created.

### SimpleHTTPRequestHandler/static responder

```text
Python static configuration
  -> ServeConfig / static policy
  -> StaticService constructed once
  + RuntimeConfig translated once
  -> start_with_service(StaticService)
```

Retain Python-compatible default index ordering and MIME hooks through the canonical static service/responder boundary.

## Rootless custom-handler test

Add an installed-wheel regression test that starts a custom handler when the nominal working/static directory is unusable.

Possible deterministic forms:

- start from a temporary working directory, then remove it before server construction where supported;
- provide a deliberately nonexistent static path through an internal test seam that must not be touched;
- use a custom handler configuration with no responder root object at all.

The test must fail under any accidental static-root construction and pass under runtime-only startup.

## Preserve Python limits

Retain:

- callback semaphore;
- handler response-size bound;
- request-body mode and limit;
- synchronous callback execution model;
- TLS server classes;
- address tuples and lifecycle semantics;
- installed-wheel import isolation.

Do not add async callbacks or raw socket exposure.

## Required tests

- custom handler starts without static root;
- custom handler GET/POST/OPTIONS body behavior follows its declared policy;
- SimpleHTTPRequestHandler still uses hardened root confinement;
- Python static full/range responses remain streamed through runtime admission;
- Python custom file responses use the same runtime file semaphore;
- TLS custom and static servers preserve behavior;
- six-class compatibility suite remains green.

## Acceptance criteria

- custom Python servers construct zero static state;
- static Python servers construct one static state;
- Python uses the same runtime-owned file admission as Rust;
- no compatibility class or supported lifecycle behavior is removed.

---

# Track G — Make retained runtime fields and close behavior effective

## G1 — `server_header`

### Required decision

Either implement `RuntimeConfig.server_header` authoritatively or remove it.

Preferred implementation because it is already documented:

- validate configured value during `RuntimeConfigBuilder::build()` using HTTP header-value rules;
- store a validated form internally if practical;
- at the final runtime response boundary, remove every service-provided `Server` field;
- if configured, insert exactly one authoritative `Server` header;
- if `None`, emit no `Server` header;
- apply to success, static, custom, error, HEAD, and file responses.

Do not allow handler spoofing or duplication.

### Single finalization point

Restructure the per-request closure so every response, including early conversion/body-policy errors, passes through one final runtime response function:

```rust
fn finalize_runtime_response(
    response: HyperResponse,
    runtime: &RuntimeConfig,
) -> HyperResponse
```

That boundary should own:

- authoritative `Server` behavior;
- authoritative `Date` behavior if not already guaranteed lower down;
- any final `Connection: close` decision produced by request-body handling.

Avoid manually applying `server_header` in every return branch.

### Tests

- configured header appears exactly once;
- service-provided Server is replaced;
- None removes/spares no Server header according to documented policy;
- static/custom/error/HEAD/file responses are consistent;
- invalid configured value fails builder construction.

## G2 — Incomplete streamed body closure

### Current concern

The Stream path logs `IncompleteBodyClose` when the service returns before consuming the body, but logging alone does not prove the connection is closed or that a pipelined second request cannot execute.

### Required behavior

When a streamed body is not fully consumed:

- mark the response `Connection: close`;
- drop the remaining request body;
- do not process another request on that HTTP/1 connection;
- preserve the service response status/body if safe;
- emit the existing event;
- do not drain in the background.

If Hyper requires a stronger connection-level signal than the response header, introduce the smallest internal close flag returned from the service closure to the connection executor. Do not add a public policy enum.

### Required wire tests

- send a streamed body followed by a pipelined second request;
- service consumes only the first chunk and returns;
- first response is received with `Connection: close`;
- second request is never invoked;
- connection reaches EOF/reset promptly;
- fully consumed Stream body permits keep-alive where otherwise allowed.

### Acceptance criteria

- close behavior is observable on the wire, not only in logs;
- no background drain exists;
- connection reuse is impossible after incomplete consumption.

---

# Track H — Repair release smoke and binary-size evidence

## H1 — Manual release smoke fixture

### Current failure mode

The release workflow starts the bundled CLI in the repository directory, requests `/`, and expects 200. Secure defaults disable directory listings, and the repository root is not a controlled static fixture.

### Required smoke design

For Linux, macOS, and Windows:

1. create a temporary directory;
2. write a deterministic file such as `smoke.txt` containing known bytes;
3. allocate or discover a loopback port;
4. start the bundled CLI with:
   - explicit `--directory <temp-root>`;
   - explicit loopback bind/port;
   - `--log-format none` where useful;
5. poll readiness with bounded retries rather than fixed one-second sleep;
6. GET `/smoke.txt`;
7. assert status 200 and exact body;
8. terminate and confirm clean exit;
9. fail the workflow on any error.

Do not enable directory listing merely to make `/` return 200.

### Race reduction

The current pattern of binding a socket to find a free port and closing it before server startup has a small race. Prefer:

- a supported port-0/ready-address mechanism if the CLI exposes one;
- or a retry loop around ephemeral port selection;
- or launch through the Python native facade when testing that artifact.

Do not expand the CLI solely for this smoke test unless port 0 already works and the listening address can be obtained reliably from logs.

## H2 — Use the intended distribution profile for artifacts

The manual release workflow currently builds the bundled CLI with `--release`, not `--profile dist`.

Required correction:

- build the bundled standalone CLI with the selected dist profile;
- ensure the wheel stages that dist artifact;
- configure Maturin to build the extension with the intended size profile where technically supported and tested;
- if the excluded Python crate cannot consume the workspace dist profile directly, use a documented equivalent profile/environment configuration rather than silently falling back;
- verify symbols are stripped as intended on each platform without breaking wheel repair/signing expectations.

Do not claim wheel-size reduction until the actual wheel uses the measured profile.

## H3 — Correct measurement methodology

Update `benchmarks/binary-size.md` to separate:

### Comparable compiler/code changes

Compare like-for-like artifacts:

```text
release before, stripped consistently
release after, stripped consistently
```

or:

```text
dist before runtime/feature change
dist after runtime/feature change
```

### Packaging/profile effect

Report separately:

```text
unstripped release -> stripped dist
```

Label this as a distribution-profile effect, not pure code reduction.

### Required artifacts

Record at least on the primary measurement platform:

- default CLI release and dist;
- TLS CLI release and dist;
- Python extension artifact;
- bundled CLI staged in package;
- final wheel;
- feature graph summary;
- commit and toolchain.

### Runtime scheduler evidence

Record a small representative comparison for current-thread versus multithread standalone CLI:

- small file at moderate concurrency;
- large file at moderate concurrency;
- range requests;
- TLS if available.

The benchmark need not be elaborate, but “no behavioral regression” is not a throughput/latency measurement.

If current-thread regresses representative workloads materially, revert it or document why the measured size benefit justifies the tradeoff under the repository's local/LAN scope.

## Acceptance criteria

- release smoke uses a controlled file fixture on every platform;
- release workflow stages the intended dist artifact;
- wheel and extension sizes are recorded;
- stripped/unstripped effects are not conflated with code reduction;
- current-thread acceptance has actual representative data;
- no permanent size CI gate is added.

---

# Track I — Tests, documentation, and truthful closure

## Required focused test inventory

Add or update tests for every corrected boundary.

### Static streaming

- canonical file variant;
- no collection;
- full/range/HEAD/index behavior;
- client cancellation;
- listing bounds.

### Runtime ownership

- one server-wide file semaphore;
- static/custom parity;
- no per-connection semaphore;
- one static state;
- rootless custom service.

### Body policy

- service-defined GET/DELETE/OPTIONS/extension bodies;
- static rejection;
- TRACE restriction;
- framing failures;
- reject/incomplete close semantics.

### Python

- rootless custom handler;
- static handler confinement;
- custom/static file admission;
- TLS and lifecycle compatibility.

### Runtime fields

- authoritative Server header;
- invalid Server value rejection;
- all retained fields have a direct behavior test or are removed.

### Release/size

- workflow fixture logic reviewed on all shell variants;
- bundled CLI `--version` and exact-file GET;
- final artifact profile/size record.

## Existing tests that must be replaced

Replace tests that encode the incorrect architecture, including tests that:

- pass `ServeState` into a custom-service connection solely to obtain file admission;
- saturate a static-owned semaphore to prove runtime behavior;
- accept full-file collection as a valid `StaticService::call()` path;
- assert global GET/HEAD/DELETE body prohibition for custom services;
- claim incomplete body closure based only on an event counter.

Do not simply delete coverage. Rewrite it against the corrected owner and wire behavior.

## Required local verification

Run on the final implementation candidate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --features client-tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
bash scripts/verify.sh fast
PYTHON=python3.14 bash scripts/verify.sh full
```

Also run the relevant manual deep suites because the request-body and file-stream execution paths change:

```sh
cargo test -p eggserve-core --test request_body_wire
cargo test -p eggserve-core --test request_body_cancellation
cargo test -p eggserve-core --test fault_injection
cargo test -p eggserve-core --test stateful_fuzz_replay
cargo test -p eggserve-core --test filesystem_race_qualification
cargo test -p eggserve-bin --test tls_abuse --features tls
```

Run proxy/desync tests only when available and relevant to modified framing behavior. Do not make external proxy tools a universal blocker.

## Documentation reconciliation

Update active documents, including as applicable:

- `README.md`;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`;
- `architecture/runtime.md`;
- `architecture/configuration.md`;
- `architecture/eggserve-core.md`;
- `architecture/eggserve-python.md`;
- `architecture/testing-and-conformance.md`;
- `docs/http-primitives.md`;
- `docs/python-api.md`;
- `docs/python-http-server-compatibility.md`;
- `docs/security-policy.md`;
- `docs/threat-model.md`;
- `docs/release-process.md`;
- `docs/dependency-policy.md`;
- `benchmarks/binary-size.md`;
- Plans 102, 104, 105, 106, and 107;
- `release/plan-102-106-closure.md` supersession notice;
- a new concise Plan 107 closure section or record only after implementation.

Active documentation must state accurately:

- static responses remain canonical and file-backed until transport;
- runtime owns one file-stream pool per server;
- static state owns no transport semaphore;
- custom Rust and Python services have no root dependency;
- service policy controls GET/HEAD/DELETE/OPTIONS/extension request bodies within runtime limits;
- TRACE content is rejected;
- incomplete streamed bodies close the connection;
- `server_header` is effective or absent;
- release smoke uses a controlled fixture;
- size tables distinguish profile stripping from code/runtime changes.

## Closure protocol without recursive SHA claims

Do not repeat the previous impossible/ambiguous pattern of editing a closure file to contain the SHA of the commit that contains that same edit.

Use this procedure:

1. implement code, tests, workflow, and documentation;
2. mark Plan 107 `IMPLEMENTATION COMPLETE — HOSTED CI PENDING` in the candidate commit;
3. push that candidate commit;
4. require the existing `rust` and `python` jobs to pass on that exact candidate SHA;
5. do not make another repository commit merely to copy CI results into a file;
6. treat GitHub's checks attached to the candidate SHA as the authoritative hosted result;
7. in the handoff/final review response, cite the candidate SHA and checks;
8. only then may the roadmap be described externally as closed.

A closure record may include:

- implementation candidate SHA once known in a later non-self-referential record;
- local commands/results;
- remaining limitations;
- measurement table.

If such a later docs-only commit is created, it becomes a new candidate and must itself pass CI. Avoid it unless necessary.

## Final acceptance criteria

Plan 107 is complete only when all conditions below are true.

### Static serving

- `StaticService::call()` returns canonical responses directly;
- file/range bodies remain handle-backed;
- no file response is collected into memory;
- CLI static serving uses this path;
- conditional, range, HEAD, MIME, index, listing, and confinement behavior remains correct.

### Runtime ownership

- one runtime state exists per server;
- one runtime file semaphore exists per server;
- no file semaphore exists in static state;
- no file semaphore is created per connection;
- static state is constructed once;
- custom service constructs no static state.

### Request bodies

- custom services may accept GET/HEAD/DELETE/OPTIONS/extension content when declared;
- static service rejects request content;
- TRACE content remains rejected;
- no synthetic GET method-policy probe remains;
- rejected and incomplete content prevents connection reuse.

### Python

- custom handler startup has no root dependency;
- static handler retains hardened root behavior;
- both use runtime-owned file admission;
- installed-wheel compatibility remains green.

### Runtime configuration

- `server_header` is authoritative and tested or removed;
- every retained field has production behavior;
- no documentation lists inert fields.

### Release and size

- each manual wheel smoke uses a controlled file fixture;
- release artifacts use the documented distribution profile;
- extension, bundled CLI, and wheel sizes are recorded;
- size comparisons are methodologically comparable;
- current-thread runtime has representative performance evidence.

### Verification and closure

- routine CI remains exactly two jobs;
- no automated publication is added;
- fast/full and selected deep checks pass;
- hosted Rust and Python jobs pass on the implementation candidate SHA;
- active docs no longer contain false closure claims;
- Plans 102–106 are not reclosed until this plan's criteria pass.

## Explicit rejection criteria

Reject an implementation that:

- leaves `.collect()` in a production file-backed static response path;
- introduces another adapter that buffers files to preserve the current API;
- creates a file semaphore inside a per-connection function;
- retains `StaticServiceBuilder::max_file_streams()` as an active transport control;
- keeps `ServeState` in generic connection signatures;
- pins a root for custom Rust or Python services;
- globally rejects GET/HEAD/DELETE content regardless of service policy;
- retains the synthetic GET policy heuristic;
- logs incomplete-body closure without enforcing it on the wire;
- leaves `server_header` inert;
- changes the release smoke to enable directory listing instead of using a fixture;
- reports unstripped-to-stripped reduction as pure code-size reduction;
- adds new CI jobs, scheduled workflows, publication, evidence aggregation, or scope expansion;
- marks Plan 107 complete before hosted checks pass on the implementation candidate.

## Suggested commit sequence

Use a small, reviewable sequence:

```text
1. docs: reopen runtime closure under plan 107
2. refactor: return canonical static file responses without collection
3. refactor: add one server-wide runtime file admission state
4. refactor: remove duplicate ServeState and legacy static path
5. fix: honor service request-body policy and enforce connection close
6. fix: make Python custom handlers rootless and server header authoritative
7. build: repair release smoke and dist artifact packaging
8. docs/test: correct size evidence and reconcile active contracts
```

Commits may be combined where necessary to keep the tree buildable. Do not combine the entire corrective pass into one opaque commit.

## Handoff note

The primary implementation risk is not filesystem confinement itself; it is preserving handle-backed static responses while moving all transport concerns out of static state. Keep the resolver and planner behavior stable, change the response ownership boundary, and prove the result through canonical-body and cross-connection admission tests.

After Plan 107 passes, return to ordinary issue-scale maintenance. Do not create another broad closure roadmap for residual documentation polish.

## Candidate closure record

Implementation satisfies the corrective requirements locally:

- static file and range responses remain canonical file-backed bodies until transport;
- one server-wide runtime file-admission pool is shared by static and custom services;
- custom Rust and Python services start without an implicit static root;
- body policy is selected for the actual method, with global TRACE-content rejection and wire-enforced incomplete-stream closure;
- `server_header` is runtime-authoritative;
- release smoke uses a controlled temporary fixture and `dist` artifacts;
- current artifact sizes and a bounded current-thread workload measurement are recorded in `benchmarks/binary-size.md`.

Local `verify.sh fast`, `verify.sh full`, workspace/TLS/client-TLS tests, selected
body/cancellation/fault/replay/race/TLS-abuse suites, package dry-runs, and the
installed CPython 3.14 wheel suite pass. Hosted Rust and Python jobs pass on
candidate commit `b20b089f66a3c007e941bc9a53c5d74e4ceb9eea` in CI run
`31012371257`.
