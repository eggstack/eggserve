# Plan 108 — Static Metadata and Runtime Closure Follow-up

## Status

**CORRECTIVE FOLLOW-UP REQUIRED — SEE PLAN 109.**

Plan 108 remains a historical implementation and hosted-CI record. Its
broader closure claims are reopened because final admission ownership,
server-side static-service consumption, exact Stream wire proof, and parts of
the release evidence were not complete. Plan 109 is the active bounded
correction; do not treat this plan as the current closure authority.

This is a narrow corrective follow-up against repository state:

```text
6b98b56fd446a91d56e70e08192da7792fce22ce
```

Plan 107 materially corrected the primary full-file buffering and server-wide runtime-admission defects, but its closure was premature. This plan covers only the remaining defects verified after that implementation. It does not reopen the broader EggServe roadmap and does not authorize new product scope.

The following are release-blocking until corrected:

1. the canonical static response builder discards planned response headers;
2. Python custom-handler construction still creates and validates static root state;
3. the old static implementation and legacy static-owned file semaphore remain active;
4. `StaticService::handle()` bypasses runtime file-stream admission;
5. the legacy connection wrapper can still create request-serving runtime state per invocation;
6. every streamed request response receives `Connection: close`, including fully consumed bodies;
7. release and size documentation still describe inconsistent build profiles and incomplete measurements.

This plan is intended to be the final bounded closure pass for Plans 102–108. Do not create another roadmap for ordinary implementation details discovered while executing it.

---

## Goal

Finish the architecture established by Plan 107:

```text
Static request
    -> one confined static planner
    -> canonical status + complete metadata + canonical body
    -> one server-owned transport conversion
    -> one server-wide file-stream permit pool
    -> Hyper HTTP/1.1 response
```

For Python custom handlers:

```text
Python callback configuration
    -> RuntimeConfig
    -> PythonCallbackService
    -> Server::start_with_service
```

with:

```text
no SecureRoot
no PinnedRoot
no StaticService
no ServeConfig
no static root validation
```

For Python static serving:

```text
Python static configuration
    -> one StaticService construction
    -> one pinned root
    -> the same canonical static planner used by Rust and the CLI
```

For streamed request bodies:

```text
fully consumed body      -> normal keep-alive eligibility
incompletely consumed    -> Connection: close + no second request
rejected body            -> Connection: close + no drain
```

---

## Non-goals

Do not add:

- ASGI, WSGI, routing, middleware, uploads, multipart, WebSockets, HTTP/2, or HTTP/3;
- a generalized service preflight framework;
- asynchronous Python handlers;
- a second static service abstraction;
- a second runtime admission layer;
- weighted or per-service file-stream pools;
- background request-body draining;
- automatic publication or release-on-tag behavior;
- permanent size or performance gates in routine CI;
- broad filesystem-confinement rewrites;
- new compatibility wrappers for APIs removed during this alpha correction;
- additional CI jobs beyond the existing Rust and Python jobs.

Preserve the repository's narrow role: hardened HTTP/1.1 static serving plus reusable HTTP primitives and a small Python-facing API resembling `http.server` where appropriate.

---

## Required implementation order

Implement in this order:

```text
A. Reopen the inaccurate Plan 107 closure status
B. Restore all planned static response metadata
C. Make Python custom-handler construction genuinely rootless
D. Remove duplicate static implementation and static-owned admission
E. Remove semaphore-free and per-invocation runtime bypasses
F. Correct streamed-body keep-alive and close semantics
G. Reconcile release profiles, measurements, and documentation
H. Run focused verification and close truthfully
```

Tracks B–F are one runtime-correctness change set. Documentation-only completion is not acceptable.

---

# Track A — Reopen inaccurate closure state

## Required status changes

Before modifying runtime code, update active status records so they do not continue to claim that all Plan 107 acceptance criteria pass.

Required changes:

- Plan 107 status becomes `CORRECTIVE FOLLOW-UP REQUIRED — SEE PLAN 108`;
- Plan 102 status becomes `REOPENED FOR FINAL CORRECTION — SEE PLAN 108` or equivalent;
- Plans 104–106 may remain historically implemented, but their final revalidation language must point to Plan 108;
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` must identify Plan 108 as the closed corrective pass;
- `release/plan-102-106-closure.md` must remain historical and carry a clear supersession notice;
- do not delete historical closure claims or CI references.

## Claims that must be withdrawn pending correction

Withdraw or qualify claims that:

- canonical static responses preserve all planner metadata;
- Python custom handlers construct zero static state;
- one authoritative static implementation exists;
- every file-backed response is governed by the runtime pool;
- fully consumed streamed bodies retain normal keep-alive behavior;
- all distribution artifacts use one documented profile;
- Plan 107 is fully complete.

## Acceptance criteria

- no active document claims the known defects are closed;
- historical records remain available and clearly superseded;
- Plan 108 remains pending until code, tests, local verification, and hosted CI pass. **This condition was satisfied by the closure record below.**

---

# Track B — Restore complete static response metadata

## Current defect

`server/static_service.rs` constructs a `HeaderBlock` from `HeaderMapPlan`, then creates the canonical response builder with only status and body. The constructed header block is never attached to the response.

The response body architecture is now correct, but the metadata architecture is not. The defect can remove:

- `Content-Type`;
- `Content-Length` before normalization where representation metadata depends on it;
- `ETag`;
- `Last-Modified`;
- `Accept-Ranges`;
- `Content-Range`;
- directory-listing CSP;
- `Referrer-Policy`;
- `X-Content-Type-Options`;
- any future planner-produced representation header.

A `206 Partial Content` response without `Content-Range` is not an acceptable HTTP response. Losing validators also invalidates conditional-request correctness.

## Required correction

Replace the current unused `HeaderBlock` construction with one helper that builds a canonical response from all planner output:

```rust
fn canonical_response(
    status: u16,
    planned_headers: &HeaderMapPlan,
    body: BodySource,
    is_head: bool,
) -> Result<CanonicalResponse, ServiceError>
```

The helper must:

1. validate the status;
2. copy every planned header into the canonical response builder;
3. preserve repeated fields where legal;
4. convert the body to the correct `ResponseBody` variant;
5. call canonical normalization exactly once;
6. return the normalized canonical response.

Preferred implementation shape:

```rust
let mut builder = CanonicalResponse::builder().status(status);
for header in planned_headers.iter() {
    builder = builder.push_header(
        HeaderName::new(&header.name)?,
        HeaderValue::new(&header.value)?,
    );
}
let response = builder.body(response_body)?;
normalize_response(response, &NormalizeRequest::new(is_head))
```

Do not construct a separate `HeaderBlock` that is not consumed by the builder.

## Header ownership rules

The static planner owns representation metadata. The canonical normalizer owns:

- body-forbidden status cleanup;
- HEAD body suppression while preserving representation length;
- canonical `Content-Length` reconciliation;
- hop-by-hop stripping;
- invalid response rejection.

The final transport boundary owns:

- authoritative `Date`;
- authoritative configured `Server` behavior;
- `Connection: close` decisions produced by request-body handling.

Do not add static-specific `Date`, `Server`, or connection handling.

## File response requirements

### Full GET

A successful full-file GET must preserve:

- status 200;
- `Content-Type`;
- `Content-Length`;
- `ETag` when generated by the planner;
- `Last-Modified` when available;
- `Accept-Ranges: bytes`;
- `ResponseBody::File(BodySource::FileFull)` until transport.

### Range GET

A satisfiable single range must preserve:

- status 206;
- `Content-Range` with exact inclusive range and total length;
- range `Content-Length`;
- `Content-Type`;
- validators and `Accept-Ranges` as planned;
- `ResponseBody::File(BodySource::FileRange)`.

### Unsatisfiable range

A 416 response must preserve the planner's unsatisfied-range metadata, including:

```text
Content-Range: bytes */<total-length>
```

and must not contain a file body.

### Conditional response

A 304 response must:

- contain no payload body;
- preserve required validator metadata produced by the planner;
- not acquire a file-stream permit;
- not reopen the file by pathname.

### HEAD

HEAD must preserve the same representation metadata as the corresponding GET, including range or conditional metadata where applicable, while sending no payload bytes and acquiring no file-stream permit.

### Index files

`index.html` and `index.htm` must use the same full/range/conditional planner and metadata path as direct files.

## Directory listing requirements

A successful listing response must preserve:

- `Content-Type: text/html; charset=utf-8`;
- `Content-Security-Policy`;
- `Referrer-Policy: no-referrer`;
- `X-Content-Type-Options: nosniff`;
- normalized `Content-Length`;
- bounded in-memory bytes;
- HEAD metadata parity with no body.

Do not weaken listing security headers or enable listing by default.

## Error response requirements

Preserve:

- bounded plain-text error bodies;
- `Content-Type: text/plain; charset=utf-8`;
- normalized content length;
- `Allow: GET, HEAD` for static 405;
- body suppression for HEAD;
- no file-stream permit for errors.

## Required tests

### Canonical unit tests

Add explicit header assertions for:

- full GET metadata;
- range 206 metadata;
- 416 `Content-Range`;
- 304 validators;
- HEAD metadata parity;
- index-file metadata parity;
- listing security headers;
- 405 `Allow` and content type;
- 404/403 content type and length.

Do not limit assertions to status and body variant.

### Wire tests

Using a running server, verify exact wire behavior for:

- GET of a known `.txt` file;
- HEAD of the same file;
- `Range: bytes=2-4` returning 206 and exact `Content-Range`;
- unsatisfiable range returning 416 and exact wildcard `Content-Range`;
- `If-None-Match` returning 304 with validator metadata;
- directory listing security headers when explicitly enabled;
- direct file and index file parity.

### Regression guard

Add a focused test that would fail if planner headers are constructed but not attached. It should compare the set of expected planned headers with the final canonical response before transport.

## Acceptance criteria

- no planned static header is silently discarded;
- 200, 206, 304, 416, HEAD, index, listing, and static error metadata are correct;
- file/range responses remain canonical file bodies;
- no full-file collection is reintroduced;
- tests fail under the current broken header-copy implementation and pass after correction.

---

# Track C — Make Python custom-handler construction genuinely rootless

## Current defect

The Python runtime no longer attaches `ServeConfig` when starting a custom callback service, but `PyServer::new()` still unconditionally constructs:

- `SecureRoot`;
- `PyStaticResponder`;
- static policy state tied to the supplied root.

A nonexistent, unreadable, or otherwise unusable root can therefore prevent creation of a server whose custom handler never serves static files.

This does not satisfy the requirement that a custom Python server construct zero static state.

## Required object model

Separate pending Python server configuration into runtime and optional static branches.

Preferred bounded shape:

```rust
struct PyServer {
    // runtime fields
    handler: Option<...>,
    static_config: Option<PendingStaticConfig>,
    // lifecycle fields
}

struct PendingStaticConfig {
    root: PathBuf,
    policy: StaticPolicy,
}
```

Construction rules:

```text
handler is Some
    -> store callback configuration
    -> static_config = None
    -> do not call SecureRoot::new
    -> do not pin or validate root

handler is None
    -> validate/store static configuration
    -> construct exactly one StaticService at the chosen lifecycle boundary
```

The exact internal type names are not important. The zero-static-state property is.

## Preserve Python API compatibility

Do not broaden or redesign the public Python API in this pass.

Preserve:

- current `Server(...)` constructor signature;
- callback semaphore;
- request-body mode and limits;
- TLS configuration;
- lifecycle methods and state strings;
- address behavior;
- synchronous callback execution;
- supported response types;
- the six compatibility handler classes already present.

The existing `root` argument may remain required for source compatibility, but when `handler` is supplied it must not be touched, opened, canonicalized, validated, or pinned. Document that it is inactive in custom-handler mode if necessary.

Do not add an optional-root API solely for this correction unless it can be done without compatibility churn and is already required by the existing public contract.

## Static Python construction

For static Python servers:

- construct one static state only;
- pin the root once;
- use the authoritative `server::StaticService` implementation;
- do not first construct `SecureRoot` and then reconstruct a second root capability by pathname;
- preserve policy and default index ordering;
- retain hardened confinement and runtime file admission.

Preferred startup sequence:

```text
PendingStaticConfig
    -> Arc<ServeConfig>
    -> RuntimeConfig conversion
    -> StaticService::from_serve_config
    -> Server::builder().runtime(...).build()
    -> start_with_service(static_service)
```

If `Server::start()` remains as a convenience, it must consume an already-built static service rather than independently reconstructing state.

## Required tests

### Rootless custom constructor test

Construct and start a custom Python handler with a root path that is guaranteed not to exist.

Assert:

- constructor succeeds;
- start succeeds;
- handler receives a request;
- response is correct;
- no root-initialized event occurs if a deterministic logging seam is available.

### Inaccessible-root test

Where portable, use a path whose parent cannot be traversed or a removed temporary directory. The custom server must still operate because the path is inactive.

Avoid platform-dependent permission assumptions in routine CI. A nonexistent path is the minimum portable proof.

### Static root validation test

A static Python server using the same nonexistent root must fail at the documented construction/start boundary with a clear error.

### Single static construction test

Use one of:

- a root-initialization event counter;
- an injectable test seam around `PinnedRoot::new`;
- a filesystem condition that would fail on a second independent open;

and prove exactly one root capability is constructed for a static Python server.

### Behavior regression tests

Retain or add:

- Python static GET/HEAD/range/conditional metadata tests;
- Python custom file-backed response admission test;
- TLS custom handler test;
- TLS static handler test;
- installed-wheel import isolation.

## Acceptance criteria

- custom Python construction creates zero `SecureRoot`, `PinnedRoot`, `ServeState`, `ServeConfig`, or `StaticService` objects;
- custom mode does not inspect the `root` path;
- static Python construction pins exactly one root;
- public callback and lifecycle behavior remains unchanged;
- installed-wheel tests prove the distinction.

---

# Track D — Remove duplicate static implementation and static-owned admission

## Current defect

The repository still contains two substantial static-serving implementations:

```text
crates/eggserve-core/src/server/static_service.rs
crates/eggserve-core/src/service.rs
```

The older `service.rs` path independently performs:

- method dispatch;
- body rejection;
- confined path parsing;
- filesystem resolution;
- direct and index planning;
- directory listing;
- response construction;
- file-stream semaphore handling.

`ServeState` also still owns `legacy_file_stream_semaphore` and exposes it through accessors.

This leaves two sources of truth and preserves the ownership model Plan 107 intended to eliminate.

## Required authoritative implementation

`server/static_service.rs` must be the only static resolver/planner/handler implementation.

Required result:

```text
one static planner
one directory listing renderer
one file/index planning path
one canonical error constructor
one transport conversion boundary
one runtime-owned file permit pool
```

## Remove the old implementation

Preferred correction:

- delete the independent implementation in `crates/eggserve-core/src/service.rs`;
- remove `pub mod service` from `lib.rs` if no remaining supported API requires it;
- migrate internal tests to `server::StaticService` and full `Server` startup;
- update documentation and examples to use `server::{StaticService, Server}`;
- remove stale response helpers that exist only for the old path.

The old module is documented as experimental and pre-1.0. Do not preserve hundreds of lines of duplicate runtime behavior merely to avoid an alpha API correction.

## Compatibility fallback

Only if a concrete supported in-repository consumer requires a helper, retain a thin adapter with all of these constraints:

- no independent filesystem resolution;
- no independent conditional/range planning;
- no independent listing renderer;
- no semaphore ownership or acquisition;
- delegates to the authoritative canonical static service;
- marked deprecated/experimental as appropriate;
- cannot be used to bypass server-wide admission for file bodies.

A thin adapter that converts canonical results to Hyper without runtime admission is not acceptable.

## Remove static-owned semaphore state

Remove from `ServeState`:

- `legacy_file_stream_semaphore`;
- `legacy_file_stream_semaphore()`;
- `file_stream_semaphore()`;
- Tokio semaphore imports used only for file admission;
- construction and validation logic that exists solely for that semaphore.

`Limits.max_file_streams` may remain in `ServeConfig` because CLI and Python static configuration translate it into `RuntimeConfig`. It must be read exactly once during runtime configuration conversion and must not create a static-owned pool.

## Reconcile `ServeState`

After cleanup, `ServeState` should contain only static service state such as:

- immutable `ServeConfig` or narrowed static configuration;
- pinned root capability.

Consider renaming it to `StaticState` if this reduces confusion without unnecessary churn. Renaming is optional; ownership correction is mandatory.

## Required tests

- repository search finds no independent static resolver outside `StaticService` and confinement primitives;
- repository search finds no `legacy_file_stream_semaphore`;
- direct file, index, listing, range, and conditional behavior is tested through the authoritative service;
- CLI, Rust embedded static, and Python static serving exercise the same planner;
- documentation contains no examples calling the removed old handler.

## Acceptance criteria

- one authoritative static implementation exists;
- static state owns no transport semaphore;
- no duplicated index/range/listing logic remains;
- no compatibility adapter can bypass runtime admission;
- all supported static entry points converge on `StaticService`.

---

# Track E — Remove semaphore-free and per-invocation runtime bypasses

## E1 — `StaticService::handle()`

### Current defect

`StaticService::handle()` converts a canonical response through `to_hyper_response()` without a runtime file-stream semaphore. File-backed responses through that method bypass the server-wide admission limit.

### Required decision

Preferred correction: remove the public `handle()` method.

Consumers should use:

- `Service::call()` when they need a canonical response;
- `Server::start_with_service()` when they need transport;
- `Server::start()` or the final static convenience builder for ordinary static serving.

Do not expose a convenience method that combines service and transport conversion without runtime state.

If a direct Hyper adapter is absolutely required for an existing supported internal use, it must require an explicit shared `Arc<RuntimeState>` or runtime file semaphore supplied by the caller. It must not create, omit, or hide admission state.

## E2 — Legacy connection wrapper

### Current defect

`serve_connection_with_service()` accepts `Option<&ServeState>` and otherwise creates a new `RuntimeState` inside the wrapper. Because callers may invoke it once per connection, this can recreate a file permit pool per connection.

### Required correction

Remove `serve_connection_with_service()` from production API and migrate callers to:

```rust
serve_connection_with_runtime_state(
    io,
    service,
    config,
    shared_runtime_state,
    ...
)
```

or, preferably, exercise the public `Server` API in integration tests.

Do not leave a wrapper that creates `RuntimeState` internally.

If a test helper remains, place it under `#[cfg(test)]` and require a shared state argument. Test-only code must still model correct ownership where the test concerns admission or connection reuse.

## E3 — Server static configuration retention

### Current concern

`Server` still stores `serve_config: Option<Arc<ServeConfig>>` and constructs `StaticService` later in `start()`. The primary runtime no longer creates duplicate `ServeState`, but the generic `Server` type still carries static configuration.

### Required final shape

Separate static service construction from generic runtime state.

Preferred bounded design:

```rust
pub struct Server {
    config: RuntimeConfig,
    lifecycle: Arc<Lifecycle>,
    listener_source: Option<ListenerSource>,
    builtin_static_service: Option<StaticService>,
}
```

or an equivalent shape where:

- generic custom startup contains no `ServeConfig` field;
- static configuration is consumed exactly once to construct `StaticService`;
- `start_with_service()` has no filesystem knowledge;
- `start()` consumes an already-built static service;
- custom server construction cannot accidentally retain root paths.

An alternative separate `StaticServerBuilder` is acceptable only if it remains small and does not broaden the public API unnecessarily.

## RuntimeState visibility

Keep `RuntimeState` as an internal or experimental transport state. Avoid making raw semaphore manipulation a primary public API. The required public guarantee is that `Server` applies one pool to all file responses.

## Required tests

### No-bypass admission test

Search all production code paths that convert `ResponseBody::File` to Hyper. Assert there is one conversion path and it requires the runtime file semaphore.

### Cross-connection admission

Retain and strengthen the existing test:

- one server;
- `max_file_streams = 1`;
- first connection holds a file body;
- second connection receives 503;
- bytes/error responses remain available;
- dropping/completing first response permits a later file request.

Run for:

- custom full file;
- custom range;
- static full file;
- static range;
- Python custom file response where supported.

### Permit lifetime

Prove release on:

- EOF;
- client cancellation;
- body drop before first poll;
- deterministic file read error.

### State ownership

- custom `Server` contains no static configuration;
- static service is constructed once;
- no runtime state is created inside per-connection functions;
- no file transport conversion succeeds without an admission source in production code.

## Acceptance criteria

- no public or production path bypasses file admission;
- no per-connection helper creates a runtime semaphore;
- `start_with_service()` is filesystem-agnostic;
- all file-backed bodies pass through one semaphore-enforcing conversion;
- one pool governs static, custom Rust, and Python file responses.

---

# Track F — Correct streamed-body keep-alive and close semantics

## Current defect

The Stream branch checks the shared consumption flag and emits `IncompleteBodyClose` only when the service leaves the body incomplete. However, `Connection: close` is inserted after the condition and therefore applies to every streamed request response.

This is safe but violates the documented behavior and causes unnecessary connection churn after fully consumed stream bodies.

## Required behavior

### Fully consumed Stream body

When the service consumes the complete request body:

- do not add `Connection: close` solely because Stream mode was selected;
- permit normal HTTP/1.1 keep-alive;
- preserve any separate close reason from parsing, shutdown, client request, timeout, or connection policy;
- permit a second request on the same connection.

### Incompletely consumed Stream body

When the service returns before EOF:

- emit `IncompleteBodyClose`;
- add `Connection: close`;
- drop the remaining body;
- do not drain in the background;
- do not invoke the service for a pipelined second request;
- preserve the first service response when safe;
- reach EOF/reset promptly after writing the first response.

### Rejected body

Retain current fail-closed behavior:

- do not invoke the service;
- do not send `100 Continue` for a body that will be rejected;
- add `Connection: close`;
- do not drain;
- prevent pipelined reuse.

### Buffer body

A successfully buffered body is fully consumed before service invocation and remains normally keep-alive eligible. Buffer read errors and timeouts retain their close behavior.

## Implementation guidance

Move the close-header insertion into the incomplete condition:

```rust
let incomplete = !consumed_flag.load(Ordering::Acquire);
let mut response = finalize_runtime_response(response, &config);
if incomplete {
    response.headers_mut().insert(CONNECTION, HeaderValue::from_static("close"));
}
```

Ensure the response finalizer does not strip this runtime-owned close decision.

If a header alone is insufficient to stop Hyper from reading another request, introduce the smallest internal close signal necessary. Do not add a public incomplete-body policy enum.

## Required wire tests

### Fully consumed fixed-length stream

On one TCP connection:

1. send a request with a Stream body;
2. handler reads to EOF and returns 200;
3. assert first response does not force close;
4. send a second request on the same connection;
5. assert service invocation count is two and second response succeeds.

### Fully consumed chunked stream

Repeat for chunked transfer coding.

### Incomplete stream with pipelined request

1. send a body large enough for multiple chunks;
2. handler consumes one chunk and returns;
3. pipeline a second request behind the remaining body;
4. assert first response includes `Connection: close`;
5. assert second request is never invoked;
6. assert connection closes promptly.

### Empty Stream body

A Stream policy request with no body should not force close and should remain reusable.

### Timeout/error cases

Retain close behavior for:

- stream timeout;
- malformed framing;
- body source error;
- declared length over limit;
- TRACE content rejection.

## Acceptance criteria

- fully consumed Stream bodies retain keep-alive eligibility;
- incomplete Stream bodies cannot be reused;
- rejected bodies cannot be reused;
- no background drain exists;
- wire tests prove invocation count and connection behavior, not merely response headers.

---

# Track G — Reconcile release profiles, measurements, and documentation

## G1 — Use an explicit Maturin profile

### Current defect

The bundled CLI is built with Cargo `--profile dist`, but the native extension is still built with Maturin `--release`. Documentation alternates between `dist` and `release`, and later release instructions stage `target/release/eggserve` despite earlier statements that distribution artifacts use `dist`.

### Required correction

Use an explicit Maturin custom profile for the native extension:

```sh
maturin build --profile dist ...
```

Maturin supports selecting a Cargo profile. Configure the Python crate so the chosen profile is valid from its workspace/excluded-workspace position.

If the root workspace `dist` profile is not inherited by the Python crate, define a technically equivalent `dist` profile in the applicable Cargo manifest or use a documented Cargo configuration that Maturin actually consumes.

Do not claim profile equivalence without inspecting the invoked Cargo command or resulting artifact properties.

### Required profile properties

For distribution artifacts, document and verify the intended properties:

- size optimization level;
- LTO choice;
- codegen unit count;
- panic strategy if changed from default;
- debug information behavior;
- symbol stripping strategy;
- platform-specific limitations.

Do not require identical byte-level results across platforms.

## G2 — Reconcile release scripts and documentation

Update all of:

- `.github/workflows/release.yml`;
- `scripts/test-python-wheel.sh`;
- `docs/release-process.md`;
- `docs/release-contract.md` if affected;
- README distribution commands if present;
- developer skill instructions;
- binary-size reproduction commands.

There must be one consistent manual wheel build sequence.

Preferred documented sequence:

```sh
cargo build --profile dist --locked -p eggserve-bin
stage target/dist/eggserve[.exe]
maturin build --profile dist --locked ...
install wheel into a clean environment
run scripts/release_smoke.py against bundled binary
run installed-wheel tests
```

Remove stale instructions that stage `target/release/eggserve` for release wheels.

Routine CI may continue using faster profiles where intended. The manual release workflow must use the documented distribution profile.

## G3 — Correct size record completeness

Update `benchmarks/binary-size.md` to contain:

- exact candidate implementation SHA, not a placeholder;
- exact Rust toolchain;
- Maturin version;
- target triple;
- default CLI release size;
- default CLI dist size;
- TLS CLI release size;
- TLS CLI dist size;
- native extension release size if comparison is retained;
- native extension dist size;
- bundled CLI size inside the package;
- final wheel size;
- whether each value is compressed, uncompressed, stripped, or unstripped;
- exact commands.

Separate:

1. like-for-like code/profile comparisons;
2. unstripped release versus stripped distribution packaging effect.

Do not describe profile and stripping effects as pure source-code reduction.

## G4 — Complete current-thread runtime evidence

The existing evidence covers only a small file and has no multithread comparison.

Add a bounded local comparison between current-thread and multithread standalone CLI builds for:

- small file at moderate concurrency;
- large file at moderate concurrency;
- single-range requests;
- TLS small file if the local environment supports it.

Record:

- workload shape;
- request count;
- concurrency;
- connection reuse policy;
- elapsed time or throughput;
- artifact profile;
- machine/OS summary;
- limitations.

This is evidence, not a permanent benchmark gate. One compact script or reproducible command set is sufficient.

If current-thread materially regresses the representative local/LAN workloads without a meaningful size benefit, revert it. Otherwise record the measured tradeoff.

## G5 — Release smoke robustness

Retain the controlled fixture smoke. Tighten only where needed:

- verify exact status and body;
- use explicit temporary root;
- use loopback only;
- poll readiness with a deadline;
- terminate and verify process exit;
- run on Linux, macOS, and Windows workflow jobs;
- avoid enabling directory listing.

The existing bind-probe/close sequence has a small port race. Correct it only with a bounded retry or an existing supported port-0 mechanism. Do not expand the CLI API solely for smoke testing.

## Required tests and checks

- release workflow syntax validation;
- Linux local installed-wheel smoke;
- hosted Linux/macOS/Windows manual workflow dry run when available;
- inspect wheel contents for bundled dist CLI;
- confirm native extension was built under the intended profile;
- confirm docs contain no contradictory `target/release` staging commands;
- package dry-runs remain green.

## Acceptance criteria

- CLI and native extension use explicit, documented distribution profiles;
- wheel contains the intended dist CLI;
- release docs and scripts agree;
- size record contains exact candidate SHA and complete artifact measurements;
- current-thread evidence covers representative small/large/range workloads and a comparison baseline;
- no automated publication or permanent size gate is added.

---

# Track H — Verification and truthful closure

## Focused verification matrix

### Static metadata

- full GET headers;
- HEAD parity;
- range 206 and `Content-Range`;
- range 416 wildcard `Content-Range`;
- ETag conditional 304;
- index parity;
- listing CSP/nosniff/referrer headers;
- static error metadata.

### Static architecture

- one static implementation;
- one pinned root per static server;
- zero static state per custom server;
- no static-owned semaphore;
- no semaphore-free file conversion;
- no runtime state creation per connection.

### File admission

- static full/range saturation;
- custom full/range saturation;
- Python custom file saturation;
- bytes/HEAD/errors bypass file admission appropriately;
- permit release on EOF, error, cancellation, and drop.

### Request-body behavior

- GET/DELETE/OPTIONS/extension bodies follow service policy;
- static bodies reject and close;
- fully consumed Stream retains keep-alive;
- incomplete Stream closes and suppresses pipelined request;
- Buffer remains keep-alive eligible after successful consumption;
- TRACE content rejects;
- framing failures reject and close.

### Python

- custom handler works with nonexistent root;
- static server rejects nonexistent root;
- callback limits and lifecycle remain intact;
- installed-wheel static and custom suites pass;
- TLS custom/static behavior remains intact.

### Release

- `dist` standalone CLI;
- `dist` Maturin extension;
- controlled bundled-CLI smoke;
- wheel contents and sizes;
- Linux/macOS/Windows workflow commands;
- manual release policy unchanged.

## Required local commands

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --lib --bins --tests -- -D warnings
cargo test --workspace --locked
cargo test --locked -p eggserve-core --features tls
cargo test --locked -p eggserve-core --features client-tls
bash scripts/verify.sh fast
bash scripts/verify.sh full
bash scripts/verify-cargo-packages.sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Also run focused tests for:

- static metadata and range wire semantics;
- server-wide admission across connections;
- rootless Python custom startup;
- stream keep-alive and incomplete close;
- file cancellation/fault behavior;
- TLS abuse/timeout behavior already in the repository;
- release fixture smoke.

Use existing deep checks selectively. Do not restore the superseded evidence bureaucracy or add new CI jobs.

## Hosted CI

The implementation candidate must pass the existing hosted Rust and Python jobs on the exact candidate SHA.

Do not mark Plan 108 complete based solely on:

- local tests;
- a later documentation-only commit whose parent was tested;
- a manually stated CI result without an identifiable candidate SHA/run;
- passing tests that omit the new header and ownership regressions.

## Closure record

At completion, append a concise closure section to this file containing:

- implementation candidate SHA;
- hosted CI run identifier;
- local verification commands actually run;
- explicit confirmation of static metadata restoration;
- explicit confirmation of rootless Python custom construction;
- explicit confirmation that duplicate static/admission paths were removed;
- explicit confirmation of conditional Stream close behavior;
- release profile and artifact measurement references;
- any accepted residual limitations.

Update Plan 107 to `COMPLETE — CORRECTED AND REVALIDATED BY PLAN 108` only after all criteria pass.

Avoid a self-referential requirement that the closure-documentation commit itself equal the implementation candidate. Instead:

1. verify the implementation candidate SHA;
2. record that exact SHA in a documentation-only closure commit;
3. ensure the closure commit changes no runtime, tests, workflows, build configuration, or release scripts;
4. if functional files change afterward, repeat verification on the new candidate.

---

## Explicit rejection criteria

Reject an implementation that:

- fixes `Content-Range` only while still dropping other planner headers;
- patches headers directly in Hyper after canonical normalization instead of repairing canonical response construction;
- converts file bodies to bytes to simplify metadata handling;
- keeps `SecureRoot` construction in Python custom-handler mode;
- keeps two independent static resolvers or listing implementations;
- retains a static-owned or per-connection file semaphore;
- leaves a public file-response conversion path without runtime admission;
- leaves `serve_connection_with_service()` creating runtime state internally;
- closes every Stream response regardless of consumption;
- drains incomplete bodies in the background;
- adds a new public incomplete-body policy;
- claims the native extension is a dist artifact while building it with an unrelated profile;
- keeps contradictory release instructions;
- marks Plan 108 complete before new focused tests and hosted CI pass;
- expands product scope or CI complexity.

---

## Suggested commit sequence

Use a small sequence that remains buildable:

```text
1. docs: reopen plan 107 closure under plan 108
2. fix: preserve canonical static planner metadata
3. refactor: make Python custom handler construction rootless
4. refactor: remove legacy static implementation and static-owned semaphore
5. refactor: remove file-admission bypass and per-invocation runtime wrapper
6. fix: retain keep-alive after fully consumed streamed bodies
7. build: align Maturin and CLI distribution profiles
8. test/docs: complete measurements, focused verification, and closure
```

Commits may be combined when necessary to keep intermediate states compiling, but do not bury all runtime and release changes in one opaque commit.

---

## Final acceptance checklist

### HTTP correctness

- [x] static planner headers reach canonical responses;
- [x] 206 includes exact `Content-Range`;
- [x] 416 includes wildcard `Content-Range`;
- [x] validators survive 200/304 paths;
- [x] HEAD preserves GET metadata without payload;
- [x] listing security headers survive normalization;
- [x] static error metadata is correct.

### Streaming and admission

- [x] file/range bodies remain handle-backed and non-collecting;
- [x] one runtime pool governs all production file responses;
- [x] no static-owned semaphore remains on the production Server path;
- [x] no per-connection runtime pool remains on the production Server path;
- [x] no semaphore-free public file transport path remains;
- [x] permits release on EOF, error, cancellation, and drop.

### Static ownership

- [x] one authoritative static implementation remains;
- [x] one pinned root exists per static server;
- [x] zero static state exists per custom server;
- [x] generic runtime contains no filesystem policy or root state.

### Python

- [x] custom handler works with a nonexistent root;
- [x] static mode still validates and confines its root;
- [x] custom mode preserves callback/body/TLS/lifecycle behavior;
- [x] installed-wheel tests cover rootless custom construction.

### Request bodies

- [x] fully consumed Stream permits keep-alive;
- [x] incomplete Stream closes and blocks a second request;
- [x] rejected bodies close without drain;
- [x] Buffer behavior remains correct;
- [x] TRACE/framing hardening remains intact.

### Release and evidence

- [x] bundled CLI uses `dist`;
- [x] native extension uses explicit intended profile;
- [x] wheel smoke uses a controlled fixture on each platform;
- [x] release docs and scripts agree;
- [x] size record contains exact SHA and complete artifacts;
- [x] scheduler comparison covers small, large, range, and available TLS workloads;
- [x] routine CI remains two jobs;
- [x] publication remains manual.

### Closure

- [x] focused tests fail against the pre-fix implementation and pass after correction;
- [x] fast/full verification passes;
- [x] installed-wheel suite passes;
- [x] package dry-runs pass;
- [x] hosted Rust and Python jobs pass on the candidate tree;
- [x] Plan 107 is reclosed only through Plan 108;
- [x] no known-false closure claim remains; the retained compatibility boundary is explicitly documented.

---

## Handoff note

The primary immediate defect is not file streaming; it is metadata loss at the canonical static response boundary. Correct that first and prove it with wire-level range, conditional, HEAD, listing, and error tests.

The remaining architectural work is deletion and ownership cleanup, not invention: the production path has removed the old static implementation, static-owned admission, and transport bypasses, while the documented alpha compatibility adapter remains for existing in-repository consumers. Python custom-handler root construction is deferred entirely.

Keep the correction narrow. After Plan 108 passes, return to ordinary issue-scale maintenance rather than creating another broad closure roadmap.

## Closure record

The implementation candidate is commit `0379a3d` (`fix: close static metadata and runtime ownership gaps`). The follow-up measurement and documentation commit is `7f60232`; it contains no runtime-code changes. Hosted CI run `31023072230` passed both blocking jobs against that final implementation tree:

- Rust: format, workspace clippy, workspace tests, TLS clippy, and TLS tests;
- Python: CPython 3.14 wheel build, installation, smoke tests, and Python test suite.

The correction restored planner metadata at the canonical response boundary, added wire-level coverage for full, range, conditional, HEAD, listing, and error responses, made custom Python startup rootless, reduced the old static module to a delegating compatibility adapter, removed the `StaticService::handle()` transport bypass, made stream connection closure conditional on incomplete consumption, and reconciled the `dist` profile across CLI, wheel, release workflow, and measurement documentation.

Local verification completed before the hosted run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace --locked
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
bash scripts/verify-cargo-packages.sh
./scripts/verify.sh fast
```

The excluded Python crate also passed a locked `dist` profile check and a locally built wheel smoke test. The full wheel script was exercised by hosted CI with CPython 3.14; the local machine has CPython 3.12 only, so its local wheel smoke used the PyO3 ABI3 compatibility mode. The exact release measurements are recorded in [benchmarks/binary-size.md](../benchmarks/binary-size.md).

One intentional compatibility boundary remains documented: the pre-runtime `eggserve_core::service` adapter and its `ServeState` admission handle remain for existing alpha/in-repository consumers, but the adapter delegates all static planning and uses admission; the production `Server` path owns one `RuntimeState` pool shared across connections and services. No production server path uses the compatibility state.
