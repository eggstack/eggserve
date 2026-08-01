# Plan 099 — Python `http.server` Compatibility Corrective Closure

## Status

Corrective closure plan for Plans 094–098.

Plans 094–098 established the intended product direction and implemented most of the required work: RFC 9110 response corrections, the Python `http.server`-shaped facade, secure static integration, TLS classes, namespace cleanup, and test consolidation.

This plan addresses only the remaining defects and policy contradictions identified in the post-implementation review. It is not a new feature phase and must not be used to reopen the broader roadmap.

## Goal

Close the `http.server` compatibility workstream by correcting the remaining gaps in:

1. file-stream admission control for Python-originated static responses;
2. standard constructor address handling and request peer metadata;
3. MIME customization behavior;
4. fail-closed handler response validation;
5. deterministic Python TLS verification;
6. active documentation consistency;
7. manual-release policy enforcement;
8. same-commit local and hosted verification.

After this plan, Plans 094–099 should be considered complete unless implementation uncovers a directly related correctness defect that prevents an acceptance criterion below from being satisfied.

## Governing constraints

All work under this plan must preserve the following boundaries.

- EggServe remains a hardened static file server and low-level HTTP server primitive.
- Remain HTTP/1.1 only.
- No ASGI or WSGI adapter.
- No routing framework, middleware stack, application framework, CGI, upload API, proxy, WebSocket, HTTP/2, HTTP/3, compression, or multipart-range support.
- Do not add a second HTTP parser, accept loop, file-serving implementation, or TLS stack.
- Do not expose raw accepted sockets to Python.
- Do not weaken SecureRoot confinement, dotfile denial, symlink denial, directory-listing defaults, body ceilings, timeouts, or canonical response normalization.
- Do not reopen the experimental Python HTTP client.
- Do not broaden Python-version support in this plan.
- Do not add a new test runner, compatibility framework, CI matrix, evidence registry, qualification system, scheduled workflow, or automatic release gate.
- Routine CI remains the existing `rust` and `python` jobs in one workflow.
- Release remains a manual maintainer decision.
- Prefer correcting shared transport and adapter seams over adding Python-only patches.
- Delete or consolidate obsolete tests and documentation where the final contract already has authoritative coverage.

## Verified baseline

The implementation agent must re-check the current tree before editing, but the post-Plan-098 review found the following concrete gaps.

### A. Python static file responses bypass file-stream admission control

`SimpleHTTPRequestHandler` produces native file-backed response bodies through `StaticResponder`. Those bodies are converted into canonical `ResponseBody::File` and streamed by the generic canonical transport path.

The built-in static service carries a `max_file_streams` semaphore permit into the stream lifetime. The custom-service/canonical file-body path used by the Python facade does not currently acquire the same permit.

This means filesystem confinement and one-shot file ownership are preserved, but the configured file-stream concurrency ceiling is not demonstrably enforced for Python-originated static file responses.

### B. Standard server address forms are not handled correctly

The facade currently validates wildcard intent through `ipaddress.ip_address()` and passes host/port to the native server in a way that does not cleanly support all common standard-library forms.

The following require correction or explicit supported handling:

- `("", 8000)`;
- `("localhost", 8000)`;
- IPv4 literals;
- IPv6 literals such as `("::1", 8000)`;
- wildcard IPv6 `("::", 8000)`;
- port `0` with accurate bound-address publication.

`server_bind()` and `server_activate()` also currently diverge from the standard lifecycle in ways that should be made internally consistent and documented without implementing all `socketserver` internals.

### C. `client_address` is not a proper `(host, port)` tuple

The native request exposes `remote_addr` as one formatted string. The Python facade wraps that string with a second synthetic zero port, producing a malformed compatibility value.

Peer and local socket addresses should cross the native boundary as structured host/port values or be parsed once using a robust shared representation.

### D. `guess_type()` does not affect actual static responses

The facade exposes `SimpleHTTPRequestHandler.guess_type()`, but actual static responses use a startup-captured `extensions_map` passed directly into Rust. Overriding `guess_type()` in a subclass does not currently alter the MIME type served.

The final contract must either make `guess_type()` functional or stop presenting it as a supported customization hook. The preferred outcome is to make common subclass overrides work without moving path resolution or file I/O into Python.

### E. Invalid handler response headers can be silently dropped

The Python-to-Rust response adapter currently attempts to validate each returned header and pushes only successfully validated fields. An invalid field can disappear while the remainder of the response is sent.

The documented contract is fail-closed: invalid handler status, headers, framing, or body state must produce a controlled generic 500 response and a sanitized operational diagnostic.

### F. Python TLS tests can skip entirely

The installed-wheel HTTPS tests generate a certificate with the system `openssl` binary and skip when it is unavailable.

This allows a full verification run to succeed without testing the Python HTTPS facade. TLS fixtures should be deterministic and repository-owned for tests, without adding a certificate-generation dependency or exposing production key material.

### G. Active documentation contains stale statements

Current active guidance contains contradictions around:

- plan completion ranges;
- historical status validation through 999;
- Python client exposure;
- duplicate legacy Python type notes;
- old callback-server primary API wording;
- binary/subprocess ownership wording in the primary `server.py` module;
- historical verification/profile language that is no longer current.

The active docs should describe the final six-class facade and advanced/internal boundaries consistently.

### H. Release workflow contradicts the manual-release policy

Active project guidance states that GitHub Actions never publishes and that release is manual. `.github/workflows/release.yml` still contains a workflow-dispatch path capable of publishing to PyPI.

This plan must reconcile code and policy in favor of the established manual release decision.

## Execution order

Implement this plan in the following order:

```text
A. shared file-stream admission control
B. address and socket metadata correction
C. MIME customization contract
D. fail-closed response validation
E. deterministic TLS verification
F. documentation and release-policy reconciliation
G. final test consolidation and closure verification
```

Do not start documentation closure before the implementation contract is final.

## Track A — Enforce file-stream limits for every file-backed response

### Objective

Ensure every file-backed HTTP response, including responses produced by a custom `Service` or the Python `SimpleHTTPRequestHandler`, participates in the same bounded file-stream admission policy.

### Architectural requirement

File-stream limiting belongs at the shared runtime/transport boundary, not only inside the built-in static service.

The final path should resemble:

```text
resolver-opened BodySource::File*
  -> canonical ResponseBody::File
  -> runtime acquires file-stream permit
  -> transport body owns file + permit
  -> permit released on completion, error, disconnect, or cancellation
```

The built-in static service and custom-service paths must not maintain independent implementations of the same invariant.

### Required investigation

Inspect at least:

- `crates/eggserve-core/src/server/mod.rs`
- `crates/eggserve-core/src/server/connection.rs`
- `crates/eggserve-core/src/server/config.rs`
- `crates/eggserve-core/src/server/static_service.rs`
- `crates/eggserve-core/src/primitives/canonical.rs`
- `crates/eggserve-core/src/primitives/body.rs`
- `crates/eggserve-core/src/response.rs`
- `crates/eggserve-python/src/server.rs`
- file-stream semaphore tests in `service.rs`, `integration.rs`, and `streaming_buffer_qualification.rs`

Search:

```sh
rg -n "max_file_streams|file_stream|Semaphore|OwnedSemaphorePermit" crates/eggserve-core
rg -n "ResponseBody::File|BodySource::FileFull|BodySource::FileRange|file_body" crates/eggserve-core crates/eggserve-python
```

### Preferred implementation

1. Give the custom-service connection execution path access to a shared file-stream semaphore created from `RuntimeConfig.max_file_streams`.
2. Before converting a canonical file body to the Hyper transport body, acquire one permit.
3. Move the owned permit into the resulting stream state.
4. Release the permit automatically when:
   - the full body completes;
   - a range body completes;
   - file seek/read fails;
   - the client disconnects;
   - the response future/body is dropped;
   - shutdown aborts the connection task.
5. Do not acquire a permit for:
   - empty bodies;
   - buffered byte bodies;
   - HEAD responses;
   - 1xx, 204, 205, or 304 responses;
   - conditional responses with no file body.
6. Preserve one-shot `BodySource` ownership.
7. Avoid reopening a path or cloning file handles merely to acquire a permit.

### Integration options

The implementation agent should choose the smallest shared seam. Acceptable designs include:

- an async canonical-to-Hyper conversion function that accepts the file-stream semaphore;
- a runtime response finalizer that inspects `ResponseBody` before conversion;
- a transport context passed into canonical file-body conversion.

Avoid:

- putting a semaphore into `BodySource` constructors globally;
- Python-managed permits;
- a new middleware abstraction;
- separate semaphores for each handler class;
- permit acquisition during path resolution rather than stream lifetime.

### Required tests

Add deterministic Rust tests that exercise the custom-service/canonical file path, not only the built-in static service.

Required cases:

- with `max_file_streams = 1`, one held custom-service file response consumes the permit;
- a second custom-service file response does not begin file streaming until the first permit is released, or receives the repository's documented bounded-admission result if the runtime uses nonblocking admission;
- permit releases after full completion;
- permit releases after range completion;
- permit releases after peer disconnect;
- permit releases after read/seek error where feasible with an existing test seam;
- permit releases after connection cancellation or shutdown;
- HEAD file response does not consume a permit;
- byte responses bypass the file-stream semaphore;
- built-in static and Python static behavior continue to pass existing tests.

Use deterministic gates, mock/test body state, or controlled socket barriers. Do not reintroduce unreliable kernel-buffer timing tests.

### Acceptance criteria for Track A

- There is one runtime-enforced `max_file_streams` invariant covering built-in and custom-service file bodies.
- Python `SimpleHTTPRequestHandler` cannot exceed the configured file-stream ceiling.
- Permits are owned for the actual body lifetime.
- HEAD and in-memory responses do not consume permits.
- No path reopening or Python file buffering was introduced.

## Track B — Correct address handling and socket metadata

### Objective

Support common documented `HTTPServer` construction forms without weakening CLI public-bind safeguards or implementing private `socketserver` behavior.

### Host normalization

Implement one internal normalization function for the Python facade.

Required inputs:

- empty host string;
- `localhost`;
- IPv4 literals;
- IPv6 literals;
- wildcard IPv4 and IPv6;
- valid resolvable hostnames where native binding supports them.

Required outputs:

- a concrete bind address or a controlled `OSError`/`ValueError` before serving;
- explicit determination of whether the resolved bind is unspecified/wildcard;
- correct formatting for native IPv4 and IPv6 socket addresses;
- no string concatenation that produces ambiguous IPv6 `host:port` forms.

### Wildcard policy

For the Python standard constructor, explicit wildcard intent is already represented by the caller passing `""`, `0.0.0.0`, or `::`.

Required behavior:

- accept explicit wildcard addresses through `HTTPServer`/`HTTPSServer` without a second `public=True` argument;
- continue requiring CLI `--public` for CLI wildcard binds;
- do not change subprocess convenience defaults.

### Binding lifecycle

Preserve a bounded compatibility model:

- `bind_and_activate=True` should leave the server genuinely bound/activated by the end of construction if this can be achieved cleanly through a pre-bound listener or native runtime support;
- port `0` should publish the actual selected port as early as the supported lifecycle permits;
- if exact constructor-time bind would require invasive runtime changes, document the deliberate divergence and ensure `server_activate()` performs the real bind predictably;
- `server_bind()` must not silently claim success while doing nothing unless documentation explicitly states the narrowed behavior;
- double activation and activation after close must fail clearly;
- `server_close()` must release a bound but not-yet-serving listener if such a state is supported.

Prefer using the existing Rust `ServerBuilder::from_listener()` support if it provides a clean way to model bind/activate without creating a second accept loop.

### Peer and local address metadata

Change the native request boundary so Python receives structured socket addresses.

Preferred representation:

```python
request.remote_address == (host, port)
request.local_address == (host, port)
```

or equivalent internal names.

Required handler values:

- `self.client_address` is a proper `(host, port)` tuple;
- `self.server.server_address` is the actual bound `(host, port)` tuple;
- IPv6 host strings contain no brackets in the tuple value;
- TLS and plaintext requests report the same address shape;
- no reverse DNS lookup is required;
- `address_string()` remains numeric by default.

Legacy string fields may remain private/internal if needed, but the compatibility facade must use structured values.

### Required tests

Installed-wheel tests must cover:

- `HTTPServer(("", 0), Handler)`;
- `HTTPServer(("localhost", 0), Handler)`;
- IPv4 literal port 0;
- IPv6 loopback where the test host supports it;
- explicit wildcard bind accepted;
- actual port populated after bind/activation;
- correct `client_address` tuple;
- correct `server_address` tuple;
- TLS has the same tuple behavior;
- invalid hostname/address fails clearly;
- no regression to CLI wildcard guard.

Skip IPv6 only when the operating system genuinely lacks IPv6 loopback support; do not skip due to parser limitations.

### Acceptance criteria for Track B

- Common stdlib constructor address forms work.
- IPv6 formatting is correct.
- `client_address` and `server_address` have the expected tuple structure.
- CLI public-bind policy is unchanged.
- No Python accept loop or raw-socket handler access was added.

## Track C — Make the MIME customization contract truthful

### Objective

Ensure the advertised `SimpleHTTPRequestHandler` MIME customization mechanism affects real static responses.

### Decision

Support both of these common forms:

```python
class Handler(SimpleHTTPRequestHandler):
    extensions_map = {".wasm": "application/wasm"}
```

and:

```python
class Handler(SimpleHTTPRequestHandler):
    def guess_type(self, path):
        if path.endswith(".custom"):
            return "application/x-custom"
        return super().guess_type(path)
```

### Security and architecture requirements

- Rust still resolves and opens the file.
- Python receives only a safe display/relative path or filename needed for MIME selection.
- `guess_type()` performs no authoritative path translation.
- Invalid MIME/header values fail closed through the normal header-validation path.
- Unknown types still default to `application/octet-stream`.
- `X-Content-Type-Options: nosniff` remains present.
- No OS MIME registry or broad dependency is introduced.

### Preferred implementation

1. Keep the native MIME map as the default.
2. Capture `extensions_map` at server startup for the simple fast path.
3. Detect whether the handler class overrides `guess_type()` from the base implementation.
4. For overridden behavior, invoke the method with the safe request-relative display path or resolved filename before final response serialization.
5. Apply only the resulting content type to the already-resolved native response.
6. Do not read or reopen the file in Python.

If invoking an instance override before/within normal handler dispatch would substantially complicate the fast path, an alternative small class-level MIME callback adapter is acceptable. It must preserve subclass semantics and remain synchronous/bounded.

### Required tests

- class `extensions_map` override changes actual response Content-Type;
- subclass `guess_type()` override changes actual response Content-Type;
- `super().guess_type()` retains native/default mapping;
- unknown suffix remains octet-stream;
- invalid returned MIME value produces generic 500 rather than silent removal;
- ranges and HEAD retain the selected MIME type;
- MIME customization does not cause Python file reads or path reopening.

### Acceptance criteria for Track C

- Every documented MIME customization hook is functional.
- Documentation does not promise unsupported stdlib hooks.
- Native confinement and streaming remain unchanged.

## Track D — Fail closed on invalid handler responses

### Objective

Make handler status/header/body validation atomic: either the complete response is valid, or the runtime returns a controlled 500.

### Required response validation

Validate before constructing the canonical response:

- status is an integer in 100–599;
- headers are an ordered sequence of two-string fields;
- header names and values pass canonical validation;
- CR, LF, NUL, invalid names, and invalid values are rejected;
- runtime-owned connection and framing fields are rejected according to the final documented facade policy;
- duplicate ordinary headers are preserved;
- supplied `Content-Length` cannot disagree with the staged body;
- one-shot native file body ownership is valid;
- the handler did not attempt to combine staged bytes and a native static body;
- body ceilings are enforced;
- HEAD and body-forbidden status normalization remains central.

### Atomic conversion

Do not push fields opportunistically while ignoring errors.

Preferred approach:

1. Extract and validate all fields into temporary canonical values.
2. Build the complete canonical response only after validation succeeds.
3. On any failure:
   - emit a sanitized service error event naming the failure category, not untrusted values;
   - return generic 500;
   - do not expose exception text or header content to the client;
   - release any consumed body/file capability safely.

### Runtime-owned fields

The Python facade already rejects common hop-by-hop fields in `send_header()`. The Rust boundary must independently enforce the same policy for defense in depth.

At minimum reject:

- `Connection`;
- `Keep-Alive`;
- `Proxy-Connection`;
- `TE`;
- `Trailer`;
- `Transfer-Encoding`;
- `Upgrade`.

For `Content-Length`, choose one consistent policy:

- preferred: accept it only when it exactly matches the computed representation length, then normalize to one runtime-owned field;
- acceptable: reject all handler-supplied Content-Length and require the runtime to compute it.

Document the selected behavior and test it consistently.

### Required tests

- invalid header name -> 500;
- CR/LF/NUL header value -> 500;
- hop-by-hop field -> 500;
- mismatched Content-Length -> 500;
- duplicate safe fields survive;
- invalid status -> 500;
- malformed response object -> 500;
- file-body conversion error -> 500 and no leaked permit/handle;
- no partial valid headers from an invalid response reach the wire;
- logs contain no untrusted raw header value or traceback.

### Acceptance criteria for Track D

- Invalid handler responses are rejected atomically.
- No invalid field is silently dropped while the rest of the response is sent.
- Duplicate valid fields remain intact.
- Canonical normalization remains the only final framing authority.

## Track E — Make Python TLS tests deterministic

### Objective

Ensure the installed-wheel test suite always exercises the Python HTTPS facade on supported CI hosts.

### Required implementation

1. Add a fixed test-only certificate and private key fixture under an existing test-fixture location.
2. The fixture must:
   - be clearly labeled as test-only;
   - contain no production secret;
   - use a localhost identity suitable for unverified/local test clients;
   - have a validity window chosen to avoid routine expiry churn where the test client does not require validation;
   - be excluded from packages if test files are not meant to ship.
3. Remove runtime dependency on the `openssl` executable from Python tests.
4. Remove `skipTest("openssl is unavailable")`.
5. Continue using an unverified client context for local self-signed test transport unless certificate validation itself is the test.
6. Do not add `cryptography`, `pyOpenSSL`, `rcgen` to the Python wheel, or another certificate-generation dependency.

### Required TLS tests

Retain or add focused cases:

- HTTPS custom handler response;
- `request.scheme == "https"`;
- HTTPS `SimpleHTTPRequestHandler` GET and HEAD;
- range over TLS;
- invalid/missing certificate fails before readiness;
- invalid/missing key fails before readiness;
- unsupported ALPN rejected;
- password argument rejected if unsupported;
- shutdown/context management over TLS;
- no plaintext fallback.

Avoid duplicating Rust TLS parser tests at the Python level.

### Acceptance criteria for Track E

- Python HTTPS tests cannot pass by skipping due to missing `openssl`.
- The wheel verification path always exercises at least one successful HTTPS request.
- No new runtime dependency is introduced.

## Track F — Reconcile active documentation and release policy

### Objective

Make active repository guidance accurately describe the final implementation and enforce the established manual-release policy.

### Documentation audit

Review at least:

- `README.md`
- `AGENTS.md`
- `.opencode/skills/eggserve-dev/SKILL.md`
- `architecture/overview.md`
- `architecture/eggserve-python.md`
- `architecture/primitives-api.md`
- `architecture/testing-and-conformance.md`
- `docs/api-stability.md`
- `docs/python-api.md`
- `docs/python-http-server-compatibility.md`
- `docs/compatibility.md`
- `docs/http-primitives.md`
- `docs/http-response-planning.md`
- `docs/library-capability-matrix.md`
- `docs/non-goals.md`
- `docs/release-contract.md`
- `docs/release-process.md`
- `docs/security-policy.md`
- `docs/tls.md`
- package/module docstrings in `eggserve/__init__.py`, `eggserve/server.py`, `eggserve/lowlevel.py`, and `eggserve/subprocess.py`

### Required corrections

- Plan status consistently states Plans 000–099 only after this plan is implemented and verified.
- Status validation consistently says 100–599.
- 205 is documented as body-forbidden where relevant.
- Weak ETags are documented correctly for `If-None-Match` versus `If-Range`.
- The six-class `eggserve.server` facade is the primary Python API.
- Native callback classes are internal implementation details, not the recommended API.
- The Python client is not shipped.
- `eggserve.lowlevel` is advanced and experimental, not a second primary surface.
- `eggserve.subprocess` is optional CLI process management.
- The `server.py` module docstring describes the native Rust runtime, not only subprocess CLI translation.
- Address compatibility and deliberate `socketserver` divergences are accurate.
- `guess_type()` behavior matches implementation.
- File-stream limits are documented as applying to built-in and compatibility static responses.
- TLS fixture/test behavior is not presented as a production certificate pattern.
- Historical plans remain historical; do not rewrite all plan files.

### Release-policy reconciliation

The established repository policy is:

- routine CI performs regression verification only;
- GitHub Actions does not publish;
- crates.io and PyPI publication are manual maintainer actions;
- release cadence is not automated.

Required change:

- remove the PyPI publication job and publish-capable `dry_run=false` path from `.github/workflows/release.yml`;
- retain a manually dispatched wheel-build workflow only if maintainers still find cross-platform artifact building useful;
- rename it to make its non-publishing purpose explicit if necessary;
- remove `id-token: write` and release environment requirements when no publication occurs;
- ensure no workflow can publish to crates.io or PyPI;
- update release docs to describe artifact building separately from manual publication.

Do not expand this correction into a new release system. The simplest compliant result is preferred.

### Acceptance criteria for Track F

- Active docs contain no known contradictions from the baseline list.
- The release workflow cannot publish.
- Routine CI remains unchanged in shape.
- Manual publication instructions remain clear and concise.

## Track G — Test consolidation and final closure

### Objective

Prove the corrected contract without recreating the prior over-engineered verification surface.

### Required retained coverage

#### Rust

- canonical status/body/header normalization;
- strong `If-Range` semantics;
- Date finalization;
- built-in and custom-service file-stream permits;
- filesystem confinement;
- static full/range/HEAD behavior;
- lifecycle and timeouts;
- TLS loader/runtime behavior.

#### Python installed wheel

- six-class imports and namespace boundaries;
- standard address forms;
- accurate peer/server addresses;
- base handler dispatch and body bounds;
- duplicate headers and fail-closed invalid responses;
- simple static GET/HEAD/range/conditional/index/listing behavior;
- MIME overrides;
- deterministic HTTPS behavior;
- subprocess convenience smoke.

### Tests to remove or update

- remove tests that assert obsolete malformed address behavior;
- remove TLS skip paths based on external tools;
- replace comments claiming custom-service file limits are covered when they are not directly exercised;
- keep native internal-server tests only where they prove runtime invariants still required by the facade;
- do not restore removed API snapshot or Python client tests;
- use explicit small namespace assertions rather than broad snapshots.

### Verification commands

Run focused commands while implementing, adapting exact names to the final test locations:

```sh
cargo test -p eggserve-core canonical
cargo test -p eggserve-core planner
cargo test -p eggserve-core file_stream
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test streaming_buffer_qualification
cargo test -p eggserve-core --features tls
cargo test -p eggserve-bin --features tls
bash scripts/test-python-wheel.sh
```

Then run the existing standard verification:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Do not add another verification mode.

### Hosted verification

The two existing routine CI jobs must pass on the same final implementation commit:

- `rust`
- `python`

Do not claim closure based on local verification alone.

## Suggested commit sequence

Keep the corrective pass reviewable:

1. `fix: enforce file-stream limits for canonical file responses`
2. `fix: normalize Python server addresses and peer metadata`
3. `fix: honor SimpleHTTPRequestHandler MIME overrides`
4. `fix: reject invalid handler responses atomically`
5. `test: make Python HTTPS verification deterministic`
6. `docs: reconcile final Python server compatibility contract`
7. `ci: remove publish capability from release workflow`
8. `test: close compatibility corrective coverage`

Combining adjacent commits is acceptable where separation would be artificial. Do not combine all implementation, workflow, tests, and documentation into one opaque commit.

## Stop conditions

Stop implementation and write a narrowly scoped follow-up plan only if one of these becomes necessary:

- a second HTTP parser or accept loop;
- raw Python socket ownership;
- path reopening after SecureRoot resolution;
- a new TLS implementation;
- a general response middleware framework;
- expansion into HTTP/2, ASGI/WSGI, proxying, routing, or uploads;
- a new CI matrix or evidence system;
- weakening an existing filesystem or framing invariant.

Ordinary implementation difficulty is not a reason to broaden scope.

## Final acceptance criteria

Plan 099 and the Plans 094–099 workstream are complete only when all of the following are true on one final commit.

### File-stream hardening

- Every `ResponseBody::File` sent by the runtime is governed by `max_file_streams`.
- Built-in static, custom Rust service, and Python `SimpleHTTPRequestHandler` file responses share the invariant.
- Full and range streams retain permits for their actual lifetime.
- Permits release on completion, failure, disconnect, cancellation, and shutdown.
- HEAD and byte responses do not consume file permits.

### Compatibility addresses

- Empty-host, localhost, IPv4, and supported IPv6 constructors work.
- Explicit wildcard constructors work without an extra public flag.
- CLI wildcard behavior remains guarded by `--public`.
- Port 0 resolves to the actual bound port at the documented lifecycle point.
- `client_address` and `server_address` are proper host/port tuples.

### Static handler behavior

- `extensions_map` affects actual responses.
- A subclass `guess_type()` override affects actual responses.
- MIME customization does not reopen or read files in Python.
- Unknown MIME types remain `application/octet-stream` with `nosniff`.

### Handler response safety

- Invalid headers, status, framing, or body state produce generic 500.
- Invalid fields are not silently discarded.
- Duplicate valid headers remain preserved.
- No traceback, secret value, or invalid header content reaches the client.

### TLS verification

- Python HTTPS tests use deterministic repository-owned test fixtures.
- No successful full verification can omit HTTPS coverage due to missing `openssl`.
- HTTPS custom and static handlers pass installed-wheel tests.

### Documentation and policy

- Active docs accurately describe Plans 094–099 and the final API.
- No active statement still permits status 600–999.
- No active docs present the Python client or native callback server as the primary facade.
- The release workflow cannot publish to PyPI or crates.io.
- Manual release policy is consistent across workflow and documentation.

### Verification

- Focused corrective tests pass.
- `./scripts/verify.sh fast` passes.
- `./scripts/verify.sh full` passes.
- Both existing routine CI jobs pass on the final commit.
- No new routine workflow, test framework, evidence system, release automation, or scope-expanding dependency was introduced.

## Handoff requirements

The implementation handoff must include:

- final commit SHA;
- exact shared file-stream permit ownership path;
- deterministic tests proving custom-service and Python static limits;
- supported address forms and deliberate lifecycle divergences;
- final peer/local address representation;
- MIME override behavior;
- atomic response-validation behavior;
- TLS fixture location and test coverage;
- release workflow disposition;
- tests removed or consolidated;
- `verify.sh fast` and `verify.sh full` results;
- same-commit `rust` and `python` CI results.

Do not mark this plan complete while any acceptance criterion is only documented but not implemented and verified.