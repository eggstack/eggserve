# Plan 109 — Final Admission Ownership and Wire Verification Corrective Pass

## Status

**COMPLETE — VERIFIED 2026-08-05.**

The functional candidate and final formatting correction are committed on
`main`; hosted CI run `31035414453` passed both `rust` and `python` jobs.

This is a narrow corrective pass against repository state:

```text
7615b19da0d3780359701b76e5b6a8ac67247dc4
```

Plan 108 fixed the primary static metadata regression, made Python callback startup rootless, removed the semaphore-free `StaticService::handle()` path, made streamed-body closure conditional in the runtime, and aligned the documented distribution profile. Its closure nevertheless remains too broad because several explicit acceptance criteria were relaxed after implementation rather than completed.

This plan covers only the remaining verified gaps:

1. `ServeState` still owns a transport file-stream semaphore;
2. the public pre-runtime compatibility adapter still performs transport conversion through that static-owned pool;
3. `serve_connection_with_service()` can still create `RuntimeState` inside a per-invocation wrapper;
4. `Server` still retains `ServeConfig` until static startup instead of consuming it into the static service at build time;
5. the Stream tests do not prove same-connection reuse after full consumption or pipelined suppression after incomplete consumption;
6. the bundled CLI size row is inconsistent with the actual non-TLS staging command;
7. Plan 108 claims a scheduler comparison that was not performed.

No new roadmap is authorized. This is deletion, ownership cleanup, focused wire verification, and documentation correction only.

---

## Goal

Finish the runtime ownership contract without broadening EggServe:

```text
Static configuration
    -> StaticService constructed once
    -> Server stores the constructed service, not ServeConfig
    -> one RuntimeState created once per running server
    -> all file-backed canonical responses use that RuntimeState admission pool
    -> Hyper transport
```

Custom service startup remains:

```text
RuntimeConfig
    + custom Service
    -> Server::start_with_service
    -> one RuntimeState
    -> no static configuration or root state
```

There must be no alternate public transport path that:

- creates a file semaphore outside `RuntimeState`;
- creates runtime state per connection or per call;
- converts a file-backed canonical response without a shared runtime admission source.

Stream behavior must be proven on the wire:

```text
fully consumed fixed-length Stream
    -> first response has no forced close
    -> second request succeeds on the same TCP connection

fully consumed chunked Stream
    -> first response has no forced close
    -> second request succeeds on the same TCP connection

incomplete Stream
    -> first response includes Connection: close
    -> pipelined second request is never invoked
    -> connection reaches EOF/reset promptly
```

---

## Non-goals

Do not add:

- ASGI, WSGI, routing, middleware, uploads, multipart parsing, WebSockets, HTTP/2, or HTTP/3;
- a new public runtime-state API for ordinary users;
- per-service, weighted, or hierarchical file-stream pools;
- background request-body draining;
- a second static server type unless unavoidable;
- a permanent benchmark framework;
- a second CLI runtime mode;
- new routine CI jobs;
- automatic release or publication;
- broad filesystem-confinement changes;
- new compatibility APIs to replace the compatibility APIs removed here;
- performance gates or binary-size gates.

Preserve the existing narrow product surface: a hardened HTTP/1.1 static server, reusable canonical HTTP primitives, and a Python API modeled where practical after `http.server`.

---

## Required implementation order

Implement in this order:

```text
A. Reopen the inaccurate Plan 108 closure state
B. Remove static-owned file admission
C. Remove per-invocation runtime-state construction
D. Consume static configuration before Server construction completes
E. Add exact Stream wire tests
F. Correct distribution measurements and scheduler claims
G. Run focused verification and close truthfully
```

Tracks B–E are the functional correction. Track F is evidence reconciliation. Do not close this plan with documentation changes alone.

---

# Track A — Reopen inaccurate closure state

## Required status changes

Before changing runtime code, update active status records so they no longer claim the remaining requirements are complete.

Required changes:

- Plan 108 status becomes `CORRECTIVE FOLLOW-UP REQUIRED — SEE PLAN 109`;
- Plan 107 remains historically implemented but points to Plan 109 for final runtime-admission verification;
- Plan 102 closure language is qualified until Plan 109 passes;
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` identify Plan 109 as the active bounded correction;
- `release/plan-102-106-closure.md` remains historical and gains a concise supersession note if it still implies the broader admission contract is fully closed;
- retain the Plan 108 implementation and hosted-CI record as historical evidence;
- do not delete or rewrite prior commit identifiers.

## Claims that must be withdrawn pending correction

Withdraw or qualify claims that:

- static state owns no file-stream semaphore;
- no public pre-runtime path can use an independent file pool;
- no per-call or per-connection wrapper can create runtime admission state;
- the generic `Server` object contains no retained static configuration;
- fully consumed Stream requests have same-connection keep-alive wire proof;
- incomplete Stream requests have pipelined-second-request suppression proof;
- the bundled CLI size row is known to match the staged non-TLS binary;
- a current-thread versus multithread scheduler comparison was completed.

## Acceptance criteria

- active documentation accurately describes Plan 109 as pending;
- prior implementation records remain available;
- no active checklist marks an unverified requirement complete;
- Plan 109 is not marked complete before code, focused tests, local verification, and hosted CI pass.

---

# Track B — Remove static-owned file admission

## Current defect

`ServeState` still contains:

```rust
compatibility_file_stream_semaphore: Arc<Semaphore>
```

and exposes it through:

```rust
compatibility_file_stream_semaphore()
file_stream_semaphore()
```

The pre-runtime `eggserve_core::service` adapter uses that semaphore when converting canonical file responses to Hyper responses.

Renaming the field from `legacy` to `compatibility` did not change its ownership. Static state still owns transport admission, and the public adapter still provides an alternate file-serving transport path outside a running `Server` and its single `RuntimeState`.

## Required final state

`ServeState` must contain only static-serving state:

```text
ServeConfig or narrowed immutable static configuration
PinnedRoot
```

Remove from `ServeState`:

- `compatibility_file_stream_semaphore`;
- `compatibility_file_stream_semaphore()`;
- `file_stream_semaphore()`;
- Tokio semaphore imports used only by that state;
- semaphore-limit validation that exists only to construct that state;
- comments describing a compatibility admission pool.

`Limits.max_file_streams` remains valid configuration. It must be translated into `RuntimeConfig.max_file_streams` and consumed only when `RuntimeState` is created for a running server.

## Required compatibility decision

The preferred correction is to remove the public Hyper compatibility adapter:

```text
eggserve_core::service::handle_request
eggserve_core::service::handle_request_with_metadata
```

The module is experimental, pre-1.0, hidden from normal documentation, and has no justification for retaining a second transport entry point that violates the final ownership model.

Before removal:

1. search the repository for all in-tree callers;
2. migrate tests and examples to `StaticService::call()` or `Server::start()`;
3. update documentation to use `server::{StaticService, Server}`;
4. remove response helpers used only by the old adapter;
5. remove the public module export if nothing else remains.

## Allowed fallback only if an in-repository consumer cannot be migrated

A compatibility helper may remain only if all of the following hold:

- it accepts canonical requests or converts the Hyper request without independent static planning;
- it delegates static resolution and planning to `StaticService`;
- it requires an explicit caller-supplied shared `Arc<RuntimeState>` or equivalent admission capability;
- it never creates a semaphore;
- it never creates `RuntimeState`;
- it cannot convert a `ResponseBody::File` without the supplied admission source;
- it is marked deprecated and experimental;
- tests prove two calls sharing one runtime state contend on the same permit pool.

Do not retain a `ServeState`-owned pool as the compatibility mechanism.

## Avoid exposing raw semaphore ownership

Do not make `Arc<Semaphore>` the preferred public compatibility parameter. If a fallback is unavoidable, prefer a narrowly scoped internal transport context:

```rust
pub(crate) struct RuntimeState {
    file_stream_semaphore: Arc<Semaphore>,
}
```

or a hidden experimental reference accepted only by the compatibility adapter.

The ordinary public API remains `Server`.

## Required tests

### Static-state shape

Add a compile-time or ordinary unit assertion through construction and accessible fields that `ServeState` contains no transport admission object.

A repository search should find no:

```text
compatibility_file_stream_semaphore
legacy_file_stream_semaphore
ServeState::file_stream_semaphore
```

### Production admission

Retain and strengthen server-wide tests for:

- static full file;
- static range;
- custom full file;
- custom range;
- Python custom file response where supported.

Each must use a running `Server` with `max_file_streams = 1` and prove:

1. the first response holds the permit;
2. a concurrent second file response receives 503;
3. a non-file response remains available if the connection limit permits;
4. dropping or completing the first body releases the permit;
5. a later file response succeeds.

### Compatibility removal or fallback

If removed:

- repository search finds no public `handle_request` static transport adapter;
- all prior tests compile through the canonical service or server path.

If retained under the strict fallback:

- the adapter cannot compile or run without an explicit shared runtime context;
- two adapter invocations sharing that context contend on one pool;
- no static-owned pool exists.

## Acceptance criteria

- `ServeState` owns no semaphore;
- static state performs no transport admission;
- no public static adapter creates or hides a separate file pool;
- every supported file transport path uses one server/runtime-owned admission source;
- no file is collected into memory to simplify the change.

---

# Track C — Remove per-invocation runtime-state construction

## Current defect

The public hidden wrapper:

```rust
serve_connection_with_service(...)
```

accepts an optional `ServeState` and otherwise constructs:

```rust
Arc::new(RuntimeState::new(config))
```

inside the function.

A caller can invoke that wrapper once per connection, producing one file-stream pool per connection. Marking the function `#[doc(hidden)]` does not prevent external use or correct its ownership semantics.

## Required correction

Remove `serve_connection_with_service()` from production code.

Migrate all callers to one of:

```rust
Server::start_with_service(service)
```

or, for internal connection tests:

```rust
serve_connection_with_runtime_state(
    io,
    service,
    config,
    shared_runtime_state,
    ...
)
```

The shared runtime state must be created once outside the per-connection function.

## Test helper rules

If tests need a lower-level helper, place it under `#[cfg(test)]` or in test support code and require:

```rust
Arc<RuntimeState>
```

as an explicit argument.

The helper must not:

- accept `Option<ServeState>`;
- create `RuntimeState`;
- create a semaphore;
- infer static configuration;
- use a different file-body conversion path.

## RuntimeState visibility

Prefer keeping `RuntimeState::new()` private or crate-visible. Ordinary embedders should use `Server`, not construct transport internals.

If integration tests outside the crate require a state constructor, prefer a small `#[doc(hidden)]` constructor that is explicit and cannot be mistaken for per-connection use. Do not expose mutable semaphore replacement.

## Required repository searches

The final tree should contain no production occurrence of:

```text
serve_connection_with_service
RuntimeState::new(config)
```

inside a connection-serving function.

`RuntimeState::new()` should occur at the server startup boundary only, plus narrowly scoped test setup if unavoidable.

## Required tests

### One state per server

Add or retain a test seam that counts `RuntimeState` construction and proves:

- one static server creates one runtime state;
- one custom server creates one runtime state;
- multiple accepted connections do not increase the count.

A construction counter may be test-only. Do not add production metrics solely for this test.

### Cross-connection contention

Use two independent TCP connections to one server and prove the same `max_file_streams = 1` pool governs both.

### No per-connection fallback

A code-level regression test or repository check should fail if a connection helper begins constructing runtime state again.

## Acceptance criteria

- no production connection wrapper creates `RuntimeState`;
- no production connection wrapper creates a file semaphore;
- one runtime state is constructed once per running server;
- all connection tasks receive a clone of the same `Arc<RuntimeState>`;
- low-level tests model the same ownership as production.

---

# Track D — Consume static configuration before Server construction completes

## Current defect

`Server` still stores:

```rust
serve_config: Option<Arc<ServeConfig>>
```

and `Server::start()` later constructs `StaticService` from it.

This is not a duplicate root open in the current implementation, but it leaves filesystem configuration inside the generic server object and allows custom startup to be built from a `Server` that still retains an unrelated root configuration.

## Required bounded design

Do not introduce a new server hierarchy. Keep the current `Server` and builder, but consume static configuration into a constructed service during `build()`.

Preferred shape:

```rust
pub struct Server {
    config: RuntimeConfig,
    builtin_static_service: Option<StaticService>,
    lifecycle: Arc<Lifecycle>,
    listener_source: Option<ListenerSource>,
}
```

Builder behavior:

```text
ServerBuilder::serve_config(config)
    -> builder temporarily stores configuration

ServerBuilder::build()
    -> derives RuntimeConfig if needed
    -> constructs StaticService exactly once
    -> returns Server containing the constructed service
    -> does not retain ServeConfig separately
```

`Server::start()`:

```text
consume builtin_static_service
-> start_with_service(service)
```

`Server::start_with_service(custom)`:

```text
ignore or reject an attached builtin static service deterministically
```

Preferred behavior is for custom builders not to attach static configuration at all. Internal tests currently doing so should be simplified.

## Builder compatibility

The existing `serve_config()` method may remain for source compatibility. Its configuration must be consumed during `build()`.

Do not make `serve_config()` open the root immediately if that would make builder chaining unexpectedly fallible. `build()` is already fallible and is the correct boundary for constructing `StaticService`.

`static_service(root)` may remain as convenience if it returns a `Server` containing a constructed `StaticService`, not retained raw configuration.

## Custom startup cleanup

Update Rust tests and examples that currently create a temporary `ServeConfig` before `start_with_service()`.

Custom startup should use:

```rust
Server::builder()
    .runtime(runtime_config)
    .build()?
    .start_with_service(custom_service)
```

This proves the custom server path is filesystem-agnostic.

## Python static startup

The Python static branch may continue constructing a `ServeConfig` and passing it to the builder. The builder must consume it into one `StaticService` at `build()`.

The Python custom branch continues to use runtime-only construction.

## Required tests

### Server object ownership

Prove after `build()`:

- a static server owns one constructed `StaticService`;
- no raw `ServeConfig` field remains in `Server`;
- a custom server owns no static service or static configuration;
- root initialization occurs exactly once for static construction.

### Invalid root boundary

A static server with an invalid root must fail during `build()` rather than later after a listener has been prepared or startup has begun.

Document this boundary if it changes observable timing.

### Custom root independence

Rust custom-service tests must not create temporary directories or static configuration unless the test is explicitly about static serving.

### Lifecycle regression

Retain:

- start/ready/shutdown/wait behavior;
- listener injection;
- TLS startup;
- bind override behavior;
- static CLI startup;
- Python static and custom startup.

## Acceptance criteria

- `Server` contains no `ServeConfig` field;
- static configuration is consumed once during `build()`;
- static root pinning occurs once;
- custom `Server` construction retains no root or filesystem policy;
- `start_with_service()` remains filesystem-agnostic;
- no new public server abstraction is added.

---

# Track E — Add exact Stream wire verification

## Current test deficiencies

The runtime code now conditionally inserts `Connection: close` based on the Stream consumption flag, but the current tests do not prove the required behavior.

Observed gaps include:

- tests named for keep-alive use `RequestBodyPolicy::Buffer`, not `Stream`;
- some tests open a new connection for the second request;
- some tests treat connection closure as acceptable when the requirement is reuse;
- incomplete-body tests do not pipeline a second request behind unread body bytes;
- incomplete-body tests do not assert service invocation count;
- tests that send `Connection: close` cannot prove the server made the close decision;
- several tests use `read_to_end()` where the expected connection should remain alive.

## Add a deterministic HTTP/1 test response reader

Create a small test-only helper that reads one response without waiting for connection EOF.

Required behavior:

1. read until `\r\n\r\n`;
2. parse status and headers case-insensitively;
3. if `Content-Length` is present, read exactly that many body bytes;
4. support empty-body statuses and HEAD expectations;
5. support chunked response framing if any tested response uses it;
6. preserve bytes after the first response if the socket read crosses into the next response;
7. use bounded timeouts;
8. return a structured test value:

```rust
struct WireResponse {
    status: u16,
    headers: HashMap<String, Vec<String>>,
    body: Vec<u8>,
}
```

Do not add a production HTTP client or parser for this purpose.

## E1 — Fully consumed fixed-length Stream body

Create a service with:

```rust
RequestBodyPolicy::Stream { max_bytes: ... }
```

The handler must:

- increment an atomic invocation counter;
- call `read_all()` or consume chunks until EOF;
- return a deterministic 200 body containing the invocation number or request path.

On one TCP connection:

1. send a POST with `Content-Length` and no `Connection: close`;
2. read exactly the first response;
3. assert status 200;
4. assert the response does not contain `Connection: close` unless separately required by the client request;
5. send a second bodyless GET on the same connection with `Connection: close` only on the second request;
6. read the second response;
7. assert status 200;
8. assert invocation count is two;
9. assert EOF follows the second response.

This test must fail if the runtime closes every Stream response.

## E2 — Fully consumed chunked Stream body

Repeat E1 with:

```text
Transfer-Encoding: chunked
```

Use multiple chunks and a terminating zero-size chunk.

The handler must consume to EOF. The second request must succeed on the same socket.

Assert invocation count is two.

## E3 — Empty Stream request

Use a service whose policy is Stream but send a request with no body framing.

Assert:

- handler receives an empty body;
- first response does not force close;
- a second request succeeds on the same connection;
- invocation count is two.

## E4 — Incomplete fixed-length Stream with pipelined request

Use a handler that:

- increments an atomic invocation counter;
- reads exactly one chunk or fewer bytes than the declared body length;
- returns a deterministic 200 response immediately.

Send in one write where practical:

```text
POST /first HTTP/1.1
Host: localhost
Content-Length: <large enough for multiple reads>

<body bytes>
GET /second HTTP/1.1
Host: localhost

```

The second request bytes must appear after the complete declared first body, not inside it. Ensure the first body is large enough that the service returns before all body bytes are consumed by its wrapper.

Assert:

- first response status is 200;
- first response contains `Connection: close`;
- invocation counter remains one;
- no second HTTP response is received;
- the connection reaches EOF/reset within a bounded timeout;
- no background drain is observed or required.

Do not put `Connection: close` on the first request.

## E5 — Incomplete chunked Stream with pipelined request

Repeat E4 using multiple chunked body chunks followed by a terminating chunk and a pipelined second request.

The handler reads only the first chunk and returns.

Assert the same closure and invocation properties.

## E6 — Rejected body suppression

Retain a test where service policy is Reject and a body-bearing request is followed by a pipelined request.

Assert:

- service invocation count remains zero for the rejected first request;
- response has the expected rejection status;
- response includes `Connection: close`;
- the second request is never invoked;
- no `100 Continue` is sent when `Expect: 100-continue` accompanies a rejected body.

## E7 — Buffer behavior

Keep one concise same-connection test proving a successfully buffered body permits a second request.

Do not label Buffer tests as Stream tests.

## E8 — Error and timeout behavior

Retain bounded tests for:

- Stream read timeout;
- malformed chunking;
- body limit exceeded mid-stream;
- declared length over limit;
- TRACE with content;
- duplicate/conflicting framing.

These should assert close behavior where required, but they do not replace the complete/incomplete Stream tests.

## Test quality requirements

Remove or rewrite tests that contain comments such as:

```text
connection closed — acceptable
keep-alive may or may not work
reconnect for the second request
```

when the test name or acceptance criterion requires same-connection reuse.

Avoid tests that pass without asserting the intended branch.

Use atomic invocation counters for pipelining tests. Response-header assertions alone are insufficient.

## Acceptance criteria

- fixed-length Stream reuse is proven on one TCP connection;
- chunked Stream reuse is proven on one TCP connection;
- empty Stream reuse is proven on one TCP connection;
- incomplete fixed-length Stream suppresses a pipelined second invocation;
- incomplete chunked Stream suppresses a pipelined second invocation;
- rejected bodies suppress pipelined reuse;
- tests do not treat the opposite behavior as acceptable;
- tests fail against the pre-Plan-108 unconditional-close implementation.

---

# Track F — Correct distribution measurements and scheduler claims

## F1 — Correct the bundled CLI size row

### Current inconsistency

The measurement record lists the bundled CLI as the same size as the TLS dist CLI, while the staging commands build:

```sh
cargo build --profile dist --locked -p eggserve-bin
```

without `--features tls`.

The binary crate has no default TLS feature. The staged wheel binary should therefore match the default dist CLI, subject only to platform-specific copying and packaging.

### Required measurement procedure

Use a clean candidate tree and remove stale artifacts before measurement:

```sh
cargo clean -p eggserve-bin
rm -rf crates/eggserve-python/target
rm -rf crates/eggserve-python/python/eggserve/bin
rm -rf dist
```

Then build and record exact paths:

```sh
cargo build --release --locked -p eggserve-bin
cargo build --profile dist --locked -p eggserve-bin
cargo build --release --locked -p eggserve-bin --features tls
cargo build --profile dist --locked -p eggserve-bin --features tls
```

Because the final TLS build can overwrite the same target filename, measure or copy each artifact immediately after its build into a uniquely named temporary path:

```text
eggserve-default-release
eggserve-default-dist
eggserve-tls-release
eggserve-tls-dist
```

Then run the wheel build script from a clean state and measure:

- staged bundled CLI path before wheel construction;
- bundled CLI member extracted from the wheel;
- native extension member;
- final wheel file.

Verify by hash, not only size, that the staged and packaged bundled CLI match the intended default dist artifact:

```sh
sha256sum <default-dist> <staged-cli> <wheel-extracted-cli>
```

Use the platform-equivalent hash command where needed.

### Required record

`benchmarks/binary-size.md` must include:

- full candidate SHA;
- toolchain and Maturin versions;
- exact feature set for every row;
- exact artifact path or clear artifact identity;
- size in bytes;
- whether stripped;
- bundled CLI hash relationship;
- native extension and wheel size;
- explicit note that default and TLS artifacts share a target filename and must be captured immediately to avoid stale/overwritten measurements.

## F2 — Correct scheduler evidence claims

### Current issue

Plan 108 marked complete a current-thread versus multithread comparison across small, large, range, and TLS workloads, but the record contains only the earlier small-file current-thread smoke and states that no multithread CLI variant exists.

### Narrow decision

Do not add a permanent multithread CLI mode or benchmark framework solely to satisfy a historical checkbox.

The preferred correction is documentation honesty:

- remove the claim that a scheduler comparison was completed;
- state that the standalone CLI intentionally uses current-thread Tokio;
- retain the bounded 1 KiB smoke measurement as suitability evidence only;
- state that large-file, range, TLS, and cancellation behavior are covered functionally, not as comparative performance evidence;
- record scheduler comparison as not performed because there is no supported multithread CLI variant;
- do not imply lifecycle tests are throughput comparisons.

If maintainers choose to perform a one-off comparison, it must use a temporary local harness or temporary uncommitted runtime variant and report:

- current-thread and multithread builds from the same candidate;
- small file;
- large file;
- range request;
- TLS where available;
- concurrency and request counts;
- machine and toolchain;
- at least three samples;
- no permanent CI gate.

A one-off comparison is optional for Plan 109. Honest correction of the false closure claim is mandatory.

## F3 — Reconcile release documentation

Ensure agreement among:

- `.github/workflows/release.yml`;
- `scripts/test-python-wheel.sh`;
- `docs/release-process.md`;
- `docs/python-packaging.md`;
- `benchmarks/binary-size.md`;
- Plan 108 closure notes;
- Plan 109 closure notes.

All must state:

- standalone and bundled CLI use `dist`;
- native extension uses the explicit equivalent `dist` profile in the excluded Python crate;
- the bundled CLI is the default non-TLS binary unless the packaging policy is explicitly changed;
- GitHub Actions builds artifacts but does not publish;
- publication remains manual;
- size comparisons distinguish profile/stripping effects from code changes;
- no scheduler comparison is claimed unless actually measured.

## Acceptance criteria

- bundled CLI size and hash match the actual staged default dist binary;
- stale TLS artifacts cannot contaminate the measurement procedure;
- the record contains a full candidate SHA;
- native extension and wheel measurements are reproducible;
- no active document claims an unperformed scheduler comparison;
- no new permanent benchmark or CI gate is added.

---

# Track G — Verification and truthful closure

## Focused verification matrix

Run focused tests first so failures are attributable.

### Static and admission ownership

```sh
cargo test -p eggserve-core static_service
cargo test -p eggserve-core server_integration
cargo test -p eggserve-core file_stream
```

Use the actual final test filters present in the repository. Add narrow names if needed so the suites can be run directly.

Required focused coverage:

- no semaphore in `ServeState`;
- one `RuntimeState` per server;
- cross-connection static full/range admission;
- cross-connection custom full/range admission;
- permit release on EOF, cancellation, error, and body drop;
- no public compatibility bypass.

### Stream wire behavior

```sh
cargo test -p eggserve-core --test request_body_wire stream -- --nocapture
```

Required tests:

- complete fixed-length Stream same-connection reuse;
- complete chunked Stream same-connection reuse;
- empty Stream same-connection reuse;
- incomplete fixed-length Stream pipelined suppression;
- incomplete chunked Stream pipelined suppression;
- rejected body pipelined suppression;
- Buffer same-connection reuse.

### Python

Run the installed-wheel suite and confirm:

- custom handler still ignores a nonexistent root;
- static root validation still occurs;
- static and custom file responses still use production server admission;
- lifecycle and TLS behavior remain intact.

### Release measurement

Run the clean artifact procedure and record exact bytes and hashes before editing the measurement document.

## Routine verification

After focused tests pass:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace --locked
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
bash scripts/verify-cargo-packages.sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Run the Python wheel script with the repository-supported Python version:

```sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

If local CPython 3.14 is unavailable, do not claim the full installed-wheel suite passed locally. Record the exact local substitute and rely on the hosted Python job for the supported interpreter.

## Hosted CI

The implementation candidate must pass the existing routine Rust and Python jobs on the exact functional candidate tree.

Do not add new CI jobs. Do not add publication.

The closure process is:

1. implement functional changes and focused tests;
2. run local verification;
3. commit and push the implementation candidate;
4. wait for the existing hosted Rust and Python jobs on that exact candidate;
5. record the candidate SHA and CI run identifier in a documentation-only closure commit;
6. ensure the closure commit changes no runtime, tests, build profiles, workflows, or release scripts;
7. if functional files change afterward, repeat verification on the new candidate.

## Closure record requirements

Append a concise closure record to this plan containing:

- implementation candidate full SHA;
- hosted CI run identifier;
- local commands actually run;
- explicit confirmation that `ServeState` owns no semaphore;
- explicit confirmation that no production/per-call wrapper constructs runtime state;
- explicit confirmation that `Server` no longer stores `ServeConfig`;
- exact Stream wire tests executed;
- bundled CLI size and SHA-256 relationship;
- explicit scheduler-evidence statement: either measured comparison details or clear statement that no comparison is claimed;
- accepted residual limitations, if any.

Do not redefine an unmet acceptance criterion as “non-production” after implementation. Change the code or amend the plan transparently before closure.

---

## File-by-file implementation map

Expected primary files:

```text
crates/eggserve-core/src/config.rs
crates/eggserve-core/src/lib.rs
crates/eggserve-core/src/service.rs
crates/eggserve-core/src/server/mod.rs
crates/eggserve-core/src/server/connection.rs
crates/eggserve-core/src/server/static_service.rs
crates/eggserve-core/tests/server_integration.rs
crates/eggserve-core/tests/request_body_wire.rs
crates/eggserve-python/src/server.rs
crates/eggserve-python/tests/test_server_primitives.py
benchmarks/binary-size.md
docs/release-process.md
docs/python-packaging.md
architecture/runtime.md
architecture/overview.md
AGENTS.md
.opencode/skills/eggserve-dev/SKILL.md
plans/102-runtime-correctness-scope-and-size-roadmap.md
plans/107-runtime-streaming-and-closure-corrective-pass.md
plans/108-static-metadata-and-runtime-closure-follow-up.md
plans/109-final-admission-and-wire-verification-corrective-pass.md
release/plan-102-106-closure.md
```

Modify only files required by the final implementation. Do not churn unrelated documentation.

---

## Suggested implementation details

### Simplest removal path for `service.rs`

1. find all in-repository uses of `eggserve_core::service`;
2. migrate tests to canonical `StaticService` or running `Server`;
3. remove the module export;
4. delete the adapter;
5. remove adapter-only error helpers and imports;
6. run package dry-runs to catch accidental public packaging references.

This is preferred over retaining an alternate transport context API.

### Simplest `Server` ownership correction

Inside `ServerBuilder::build()`:

```rust
let builtin_static_service = match self.serve_config {
    Some(config) => Some(StaticService::from_serve_config(config)?),
    None => None,
};

Ok(Server {
    config,
    builtin_static_service,
    lifecycle: Arc::new(Lifecycle::new()),
    listener_source: self.listener_source,
})
```

Inside `Server::start()`:

```rust
let service = self
    .builtin_static_service
    .ok_or_else(|| ServerError::Config("static service required".into()))?;
self.start_with_service(service).await
```

Adapt ownership details to Rust move semantics without cloning or reopening the root.

### Simplest connection-helper correction

Keep only:

```rust
serve_connection_with_runtime_state(..., Arc<RuntimeState>, ...)
```

for internal production use.

Tests should use a test helper that requires the shared state explicitly, or use the public `Server` API.

### Stream test reliability

Prefer a single test helper module for:

- writing requests;
- reading exactly one response;
- parsing `Content-Length`;
- checking connection closure with timeout;
- retaining unread bytes between responses.

Do not duplicate fragile byte-at-a-time parsing across many tests.

Keep the helper test-only and small.

---

## Explicit rejection criteria

Reject an implementation that:

- merely renames the `ServeState` semaphore again;
- retains `ServeState::file_stream_semaphore()`;
- leaves a public static Hyper adapter with its own admission pool;
- leaves `serve_connection_with_service()` creating `RuntimeState` internally;
- moves per-connection semaphore construction to another helper;
- exposes a semaphore-free file-body conversion path;
- stores raw `ServeConfig` in `Server` after `build()`;
- opens or pins the static root more than once;
- converts files to bytes to avoid admission ownership work;
- labels Buffer tests as Stream verification;
- opens a new TCP connection in a test intended to prove keep-alive;
- treats connection closure as acceptable in a keep-alive acceptance test;
- sends `Connection: close` in a request intended to prove server-enforced incomplete-body closure;
- omits a service invocation counter from pipelined suppression tests;
- relies only on response headers without proving the second request was not invoked;
- records bundled CLI size from an artifact that may have been overwritten by a TLS build;
- marks scheduler comparison complete without comparative measurements;
- adds a permanent multithread CLI mode solely for evidence;
- adds CI jobs, scheduled checks, publication, or release automation;
- broadens the application-server scope;
- marks Plan 109 complete before hosted checks pass on the functional candidate.

---

## Suggested commit sequence

Use small, reviewable commits:

```text
1. docs: reopen plan 108 closure under plan 109
2. refactor: remove static-owned compatibility admission
3. refactor: remove per-invocation runtime-state wrapper
4. refactor: consume static configuration into StaticService at build
5. test: prove Stream reuse and incomplete pipelined closure
6. docs: correct artifact measurements and scheduler claims
7. docs: record verified candidate and close plan 109
```

Commits 2–4 may be combined if required to keep the tree compiling. Do not combine all functional and evidence work into one opaque commit.

---

## Final acceptance checklist

### Admission ownership

- [x] `ServeState` contains no semaphore;
- [x] `ServeState` exposes no file-admission accessor;
- [x] no public static adapter owns or creates a file pool;
- [x] no connection helper creates `RuntimeState`;
- [x] no connection helper creates a semaphore;
- [x] one `RuntimeState` is created per running server;
- [x] all connection tasks share the same runtime state;
- [x] all file-backed transport uses runtime admission;
- [x] static, custom Rust, and Python file responses share the production pool.

### Server/static ownership

- [x] `Server` contains no `ServeConfig` field;
- [x] static configuration is consumed during `build()`;
- [x] one static root is pinned per static server;
- [x] custom server construction retains no static state;
- [x] `start_with_service()` is filesystem-agnostic;
- [x] no second server abstraction was added.

### Stream wire behavior

- [x] fully consumed fixed-length Stream reuses one connection;
- [x] fully consumed chunked Stream reuses one connection;
- [x] empty Stream reuses one connection;
- [x] incomplete fixed-length Stream closes and suppresses a pipelined second invocation;
- [x] incomplete chunked Stream closes and suppresses a pipelined second invocation;
- [x] rejected body closes and suppresses pipelined reuse;
- [x] Buffer reuse remains correct;
- [x] invocation counters prove service-call behavior;
- [x] no test accepts the opposite behavior.

### Release evidence

- [x] default release and dist CLI sizes are measured from unique captured artifacts;
- [x] TLS release and dist CLI sizes are measured from unique captured artifacts;
- [x] staged bundled CLI matches default dist by SHA-256;
- [x] wheel-extracted bundled CLI matches staged CLI by SHA-256;
- [x] native extension and wheel sizes are recorded;
- [x] full candidate SHA is recorded;
- [x] no stale artifact can contaminate measurements;
- [x] no unperformed scheduler comparison is claimed;
- [x] no permanent benchmark gate was added.

### Verification and closure

- [x] focused admission tests pass;
- [x] focused Stream wire tests pass;
- [x] workspace tests pass;
- [x] TLS tests pass;
- [x] package dry-runs pass;
- [x] installed-wheel suite passes on supported CPython;
- [x] routine CI remains two jobs;
- [x] publication remains manual;
- [x] hosted Rust and Python jobs pass on the functional candidate;
- [x] Plans 102, 107, and 108 are reclosed only after Plan 109 passes;
- [x] active documentation contains no known-false closure claim.

---

## Verified closure — 2026-08-05

Implementation completed in:

- `cea39f779b4f6b828c92ff8bd9332bd0d2d1d99d` — functional implementation
  candidate (final admission ownership, server/static construction, exact wire
  tests, and documentation correction);
- `d273134aa7eb1583106afc00f4e24dc09e0aeb91` — artifact measurements and
  evidence correction;
- `49ecb712be1677a027891ad373b6951d7b916182` — final verified implementation
  tree tested by hosted CI (rustfmt correction for the compatibility benchmark
  call sites);
- `3b75bd621a90a94fc5d732a1afb4f36e03b255dd` — documentation-only Plan 109
  closure commit.

The final verified implementation tree is
`49ecb712be1677a027891ad373b6951d7b916182`. Hosted CI run `31035414453` checked
out and tested that exact tree. The later Plan 109 closure commit
`3b75bd621a90a94fc5d732a1afb4f36e03b255dd` changed documentation only.

Verified locally:

- `cargo fmt --all -- --check` and `git diff --check`;
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`;
- `cargo test --workspace` — 1,366 passed, 9 ignored;
- TLS clippy and `cargo test -p eggserve-bin --features tls` — 88 passed;
- exact Stream wire tests for fixed-length, chunked, empty, incomplete
  fixed/chunked pipelining, rejected-body closure, and Buffer reuse;
- `PYTHON=python3.14 bash scripts/test-python-wheel.sh`;
- `bash scripts/verify-cargo-packages.sh`.

Artifact evidence is recorded in `benchmarks/binary-size.md`. The staged
bundled CLI, wheel-extracted bundled CLI, and default `dist` CLI all have SHA
`f7b69951e629796672073bc110f7f968d8479d482b3a578bac2f69a1eeb669b9`. No
current-thread versus multi-thread performance comparison was performed or
claimed.

Hosted verification: [CI run 31035414453](https://github.com/eggstack/eggserve/actions/runs/31035414453)
passed `rust` (format, clippy, workspace tests, TLS lint/tests) and `python`
(wheel build, installation, smoke checks, and test suite). The only retry was
for a rustfmt-only benchmark correction; the preceding run's Python job also
passed.

Plans 102, 107, and 108 are historical records reclosed by this verified Plan
109 pass. Routine CI remains the existing two-job workflow, publication remains
manual, and no permanent scheduler benchmark or new CI job was added.

## Archival note

The implementation handoff that originally followed this section is superseded by the
verified closure above. No runtime work remains under Plan 109. Subsequent documentation
terminology and reproducibility polish is tracked by Plan 110.
