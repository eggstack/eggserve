# Plan 029: Corrective Contract and Security Pass

## Purpose

Correct the gaps introduced while implementing Plans 024–028 before any further feature expansion. The repository moved in the intended direction, but several implementations currently overstate their guarantees, weaken intended capability boundaries, or stop short of the behavior described by the plans.

This pass is not a new feature phase. It is a contract-fidelity, security-invariant, API-boundary, and test-closure pass.

The end state should be:

- The Python server primitive either genuinely supports Python-selected responses through a Rust-owned runtime, or its names/docs are narrowed to accurately describe a static-only server.
- The Python server path has the same fundamental connection, timeout, public-bind, and file-stream protections as the CLI path.
- Resolved-file capabilities cannot be forged through the public API.
- Body-source range APIs cannot read beyond their declared capability.
- HTTPS is never accepted unless real TLS is performed.
- Client behavior is documented and named according to its actual buffered/streaming behavior.
- Public Python exports and examples match the real API.
- End-to-end tests prove the supported paths.

## Scope

### In scope

- Python server primitive contract correction.
- Python server runtime hardening.
- Python callback/handler support if it can be implemented without introducing framework semantics.
- Public-bind acknowledgement.
- Connection and file-stream concurrency limits.
- Header and write timeout enforcement.
- Capability-boundary cleanup around `ResolvedFile`.
- Body-source range-bound validation.
- Client HTTPS/TLS correctness.
- Client buffered-versus-streaming contract cleanup.
- Python public export correction.
- Local end-to-end server and client tests.
- Documentation and README correction.

### Out of scope

- ASGI implementation.
- WSGI implementation.
- Routing framework.
- Middleware.
- Template rendering.
- HTTP/2 or HTTP/3.
- WebSocket.
- Proxy support.
- Cookie jar, auth helpers, redirects, retries, cache, or compression.
- Broad performance work unrelated to the corrective findings.

## Priority order

Implement this plan in the following order:

1. Correct false or unsafe protocol/security behavior.
2. Restore capability boundaries.
3. Harden the Python server runtime.
4. Reconcile API and documentation claims.
5. Add end-to-end regression coverage.
6. Only then consider callback support or client streaming expansion.

## Track A: Python server contract correction

### Problem

The current Python `Server` is a static server wrapper around `StaticResponder`. It does not invoke a Python handler and therefore cannot yet serve as the primitive substrate described by Plan 027 for downstream dynamic/app-server adapters.

The current docs and README describe Python server primitives in broader terms than the implementation supports.

### Required decision

Choose one of these paths explicitly:

#### Preferred path: add a handler-capable primitive

Add a Python handler argument or separate `HandlerServer`/`HttpServer` primitive with a framework-neutral callback contract.

The callback should receive an immutable `Request` object and return an explicit `Response` object.

Candidate shape:

```python
def handler(request: Request) -> Response:
    if request.path == "/health":
        return Response.text(200, "ok")
    return static.respond_request(request)

server = Server(handler=handler, bind="127.0.0.1", port=0)
```

Requirements:

- No ASGI scope.
- No WSGI environ.
- No routing abstraction.
- No middleware chain.
- No raw socket access.
- Rust owns request parsing and response serialization.
- Python returns only explicit response values.
- Handler exceptions map to generic 500 responses without traceback leakage.
- Callback concurrency is bounded.
- The GIL is not held during static file streaming.

#### Fallback path: narrow the API and claims

If callback support is not implemented in this pass:

- Rename or document `Server` as a static-serving server primitive.
- Remove claims that Python can build general HTTP servers with it.
- Remove or rewrite dynamic examples.
- State that dynamic callback support remains planned.

Do not leave the current ambiguous state.

### Request object corrections

If handler support is added, ensure `Request` exposes:

- Method.
- Raw target.
- Parsed path.
- Query string.
- Headers.
- Remote address.
- HTTP version.
- Body metadata status.

Do not infer `has_body` from method alone. Determine body presence from validated request framing metadata.

### Response object validation

Before serialization, validate:

- Status code is valid.
- Header names are valid HTTP field names.
- Header values reject CR/LF/NUL and invalid control bytes.
- User-supplied `Content-Length` cannot contradict the actual body.
- Rust generates or verifies content length.
- HEAD suppresses the body while preserving correct headers.
- Hop-by-hop headers are rejected or documented.

## Track B: Python server runtime hardening

### Problem

The Python server path currently accepts connections and spawns tasks without the CLI's core safeguards.

### Required implementation

Reuse or extract the CLI connection-serving machinery rather than maintaining a weaker parallel implementation.

Add:

- Connection semaphore.
- File-stream semaphore.
- Header read timeout.
- Write timeout.
- Hyper timer configuration required by header timeouts.
- Graceful shutdown behavior.
- Controlled accept-loop error handling.
- Explicit public-bind acknowledgement.
- Startup validation for limit values.

### Configuration

Expose a Python server config object or constructor parameters for:

- `bind`.
- `port`.
- `public` acknowledgement.
- `max_connections`.
- `max_file_streams`.
- `header_timeout`.
- `write_timeout`.
- Optional callback concurrency limit.

Defaults should match the CLI where practical.

### Public bind rule

Binding to an unspecified address such as `0.0.0.0` or `::` must fail unless `public=True` is explicitly supplied.

Add tests for:

- Loopback default succeeds.
- Unspecified bind without acknowledgement fails.
- Unspecified bind with acknowledgement succeeds.

### File stream permits

Every full-file or range-file response served by the Rust-owned Python server must acquire a file-stream permit.

Permit exhaustion should produce controlled 503 behavior or equivalent documented failure.

### Timeout tests

Add live tests for:

- Partial header slowloris connection is closed after header timeout.
- Slow reader triggers write timeout where practical.
- Connection limit prevents unbounded accepted work.
- Graceful stop returns even with idle connections.

## Track C: restore resolved-file capability integrity

### Problem

`ResolvedFile` is documented as constructible only through secure resolution, but the public `from_parts` function allows arbitrary callers to manufacture one.

### Required changes

- Make `ResolvedFile::from_parts` private or `pub(crate)`.
- If the Python bridge requires reconstruction, move the bridge into an internal module with crate-private access.
- Do not expose a general public unchecked constructor.
- Review `into_std_file` and `into_parts` for the same boundary problem.

Preferred public surface:

- Metadata inspection.
- Response planning.
- Consuming conversion into `BodySource`.

Raw handle extraction should be internal unless there is a compelling and documented low-level use case.

### Tests

Add compile-time/API-boundary coverage where feasible, or at minimum document and inspect public exports so external consumers cannot create a forged resolved capability.

Update architecture docs to state exactly which raw-handle methods are internal.

## Track D: body-source range correctness

### Problem

`BodySource::read_range` can read beyond the bounds of a `FileRange` body because relative subranges are not checked against the original planned range length.

### Required behavior

For `FileRange`:

- Treat `start` and `end_inclusive` as offsets relative to the body capability.
- Reject or clamp requests that exceed `range.len()`.
- Prefer returning `InvalidInput` or a dedicated `BodySourceError::InvalidRange` rather than silently reading beyond the capability.
- Prevent integer overflow in `range.start + start` and length calculations.

For `FileFull`:

- Validate against total file length.
- Reject reads past EOF rather than relying on `read_exact` after allocating an excessive buffer.

For `Bytes`:

- Use checked conversions from `u64` to `usize`.
- Reject values that cannot fit platform `usize`.

### Memory safety and resource limits

Add bounded read helpers:

- `read_all_bounded(max_bytes)`.
- `read_range_bounded(...)` if needed.

Python `BodySource.read_all()` should either require a bound or enforce a conservative default. It must not make unbounded production reads easy.

### Tests

Add tests for:

- File range subread exactly within bounds.
- File range subread beyond end rejected.
- File full read beyond EOF rejected.
- Inverted range rejected.
- Arithmetic overflow rejected.
- Very large requested length does not allocate before validation.

## Track E: HTTPS and TLS correctness

### Problem

The client parser accepts `https://`, but the active send path performs a plain TCP HTTP/1 handshake. `verify_tls` is exposed but not enforced by the shown connection path.

This must be treated as a release-blocking correctness/security issue.

### Required behavior

#### Without `client-tls`

- Reject `https://` before network I/O.
- Return a structured error such as `TlsUnavailable` or `UnsupportedScheme` with clear wording.

#### With `client-tls`

- Wrap the TCP stream using `tokio-rustls`.
- Validate the server name.
- Verify certificates by default using the configured root store.
- Use TLS before the Hyper HTTP/1 handshake.
- Map handshake and verification failures into structured TLS errors.

### `verify_tls` policy

Preferred option:

- Remove `verify_tls=False` from the initial stable API unless there is a strong use case.

If retained:

- Name it loudly, such as `danger_accept_invalid_certs`.
- Keep default false for the dangerous option.
- Document it as outside production-safe defaults.
- Add startup/runtime warnings where practical.

Do not retain a benign-looking `verify_tls` field that has no effect.

### Tests

Add local tests for:

- HTTPS rejected when TLS feature is absent.
- Trusted local TLS server succeeds with configured trust material, if custom roots are supported.
- Certificate mismatch fails.
- Untrusted certificate fails.
- Plain HTTP remains unaffected.

Avoid external-network CI dependencies.

## Track F: client buffered/streaming contract correction

### Problem

The current client buffers the complete response body but documents lower-level streaming APIs that do not yet exist.

### Required decision

Choose one:

#### Preferred near-term correction

Document and name the current API as buffered-only.

- `ClientResponse.body` remains bytes.
- Enforce `max_response_body_bytes`.
- Remove references to nonexistent streaming APIs.
- State that streaming is planned.

#### Optional implementation

Add a real response-stream type only if it can be done cleanly in this pass.

A real streaming response must:

- Keep the runtime/connection alive for the stream lifetime.
- Expose bounded chunk reads or iteration.
- Enforce read timeouts.
- Define cancellation/drop behavior.
- Avoid requiring a new Tokio runtime for each chunk.

Do not implement a superficial iterator over an already-buffered body and call it streaming.

### Runtime reuse

Review the current model of creating a new Tokio runtime per `send()` call.

For the synchronous Python API, prefer:

- A runtime owned by `HttpClient`, or
- A dedicated worker thread/runtime per client.

At minimum document the current cost and ensure concurrent calls are safe.

## Track G: client URL/parser correctness

### Required review

The hand-written parser must be treated as security-sensitive.

Add or verify handling for:

- URL fragments: reject or strip before request target construction.
- Empty port.
- Bracketed IPv6 with trailing junk.
- Unbracketed IPv6 rejection.
- Query without explicit `/` path.
- Control characters.
- Spaces.
- Percent-encoded host ambiguities.
- Internationalized domain names: explicitly unsupported unless normalized safely.
- Host header formatting for IPv6 literals.
- Absolute URI versus origin-form request target.

The client should send origin-form to origin servers, not an absolute URI unless proxy mode is explicitly implemented later.

Consider using a narrow established URL parser dependency if the hand-written implementation becomes complex. Any dependency addition must be justified under `docs/dependency-policy.md`.

## Track H: Python API/export cleanup

### Required changes

- Export client primitives from `eggserve.__init__` when available:
  - `HttpClient`.
  - `ClientConfig`.
  - `ClientRequest`.
  - `ClientResponse`.
  - `ClientError`.
  - Client method enum, with naming reviewed to avoid conflict with server method types.
- Export body-source errors consistently.
- Remove duplicate or confusing server-side wrapper types where possible.
- Review names such as `StaticPolicy`, `StaticPolicyWrapper`, `SecureRoot`, and `ServerSecureRoot` for unnecessary duplication.

Do not break the existing primitive API casually. If renaming is necessary, add aliases and document provisional status.

### Exception taxonomy

Do not map every client failure to `ValueError`.

Provide Python exception classes or mappings that distinguish:

- Invalid input/configuration.
- Timeout.
- Connection error.
- TLS error.
- Protocol error.
- Body limit exceeded.

## Track I: documentation and example reconciliation

### Required corrections

Audit and correct:

- `README.md`.
- `docs/python-api.md`.
- `docs/http-client-primitives.md`.
- `docs/extension-contract.md`.
- `architecture/eggserve-python.md`.
- `architecture/client.md`.
- `architecture/primitives-api.md`.
- `docs/invariants.md`.
- `AGENTS.md`.

Fix specifically:

- README `Server` example must compile and match the constructor.
- Do not claim dynamic server support unless a handler callback exists.
- Do not claim timeout enforcement in the Python server until implemented.
- Do not claim client streaming until implemented.
- Do not claim HTTPS support without actual TLS wrapping.
- Mark client and Python server APIs provisional until corrected and reviewed.

Ensure `examples/python_dynamic_static.py` actually uses a supported dynamic handler API. If no callback API exists, remove or rename it.

## Track J: test closure

### Rust tests

Add or expand:

- `BodySource` range-bound tests.
- Public capability-boundary tests where possible.
- Python server-equivalent connection limit/timeout tests at Rust integration level.
- HTTPS feature/no-feature tests.
- URL parser adversarial corpus.
- Client origin-form request-target assertion.
- Response body size-limit tests.
- Runtime reuse/concurrency tests if runtime ownership changes.

### Python tests

Add local end-to-end tests for:

- Public top-level imports.
- Basic local HTTP GET through `HttpClient`.
- HEAD through `HttpClient`.
- Buffered response-size limit.
- Timeout error type.
- Unsupported HTTPS without TLS feature.
- Dynamic handler endpoint if implemented.
- Static fallback through handler if implemented.
- Handler exception maps to 500 without traceback leakage.
- Public-bind acknowledgement.
- Connection/file-stream limit behavior where practical.

Do not rely on the public internet.

### CI

Update CI to run:

- Core default features.
- Core `client` feature.
- Core `client-tls` feature.
- Python native tests from an installed wheel.
- Python client end-to-end local tests.
- Python server primitive tests.

Ensure feature-specific code is not only compile-checked but exercised.

## Implementation sequencing

### Step 1: immediate security corrections

- Reject HTTPS without TLS.
- Make forged `ResolvedFile` construction internal.
- Fix body-source range bounds.
- Correct false documentation claims.

### Step 2: runtime hardening

- Extract/reuse connection-serving configuration.
- Add semaphores and timeouts to Python server.
- Add public-bind acknowledgement.

### Step 3: server API contract

- Implement handler callback or narrow the API and docs.
- Add response validation.
- Add callback error and concurrency policy.

### Step 4: client contract

- Decide buffered-only versus real streaming.
- Fix URL target construction.
- Implement TLS feature path.
- Improve Python exceptions and exports.

### Step 5: test and documentation closure

- Add local end-to-end tests.
- Run full feature matrix.
- Update all docs/examples to exact behavior.

## Acceptance criteria

This plan is complete only when all of the following are true:

- `https://` never uses plaintext transport.
- `verify_tls` or its replacement has real enforced semantics.
- External Rust callers cannot forge a `ResolvedFile` capability through public constructors.
- `BodySource::FileRange` cannot read outside its declared range.
- Python server public binding requires explicit acknowledgement.
- Python server connections and file streams are bounded.
- Python server header/write timeouts are enforced.
- Documentation accurately describes whether Python handler callbacks exist.
- If callback support exists, Python can return explicit dynamic responses while Rust owns I/O.
- If callback support does not exist, all dynamic/app-server claims are removed or marked planned.
- Client buffered/streaming behavior is accurately named and documented.
- Client classes are available from the public Python package namespace.
- Python client tests perform successful local end-to-end requests.
- No ASGI, WSGI, routing, middleware, or framework code is added.
- All feature combinations compile and test.

## Validation commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features client --all-targets -- -D warnings
cargo test -p eggserve-core --features client
cargo clippy -p eggserve-core --features client-tls --all-targets -- -D warnings
cargo test -p eggserve-core --features client-tls
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python source/native tests:

```sh
cd crates/eggserve-python
maturin develop
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_client_primitives -v
```

Installed-wheel validation:

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python -m unittest eggserve.test_primitives -v
python -m unittest eggserve.test_server_primitives -v
python -m unittest eggserve.test_client_primitives -v
```

## Handoff notes

Treat this as a corrective gate. Do not add convenience features while these issues remain. The most serious defects are plaintext handling of accepted HTTPS URLs, public construction of a supposedly resolver-only capability, and documentation that claims runtime guarantees the Python server path does not yet enforce.

The intended architecture remains valid: eggserve should expose correct, low-level HTTP primitives that downstream projects can build ASGI/WSGI/app servers and higher-level clients on top of. The correction is to make the actual API and implementation live up to that contract without bringing those higher-level protocols into this repository.
