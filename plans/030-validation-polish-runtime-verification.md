# Plan 030: Validation, Polish, and Runtime Verification Pass

## Purpose

Close the remaining verification and API-polish gaps after Plan 029 without expanding product scope. This pass should prove that the newly added Python callback server, runtime limits, timeout behavior, body-source capability model, client TLS path, Python exports, and wheel packaging behave as documented under real end-to-end conditions.

The goal is not to add another feature layer. The goal is to make the current surface trustworthy enough to approach API stabilization and production-readiness review.

## Scope

This pass covers:

- Runtime verification of Python server limits and timeout semantics.
- Callback error, header, status, body, and shutdown behavior.
- File-stream permit acquisition and lifetime verification.
- Capability-boundary tightening around `ResolvedFile` extraction and reconstruction APIs.
- Client feature-matrix verification for `client` and `client-tls`.
- HTTPS/TLS correctness tests using local fixtures.
- Public Python import/export verification.
- Installed-wheel tests rather than source-tree-only tests.
- Documentation and example cleanup for any behavior found to differ from implementation.

## Non-goals

- Do not add ASGI or WSGI adapters.
- Do not add routing, middleware, templates, sessions, cookies, authentication, or framework behavior.
- Do not add HTTP/2, HTTP/3, WebSocket, proxying, caching, redirects, retries, or compression.
- Do not add streaming client responses in this pass.
- Do not add request-body streaming into Python callbacks.
- Do not redesign the full public API unless a current invariant is impossible to preserve.

## Current state to validate

Plan 029 introduced or corrected:

- Python handler callbacks returning explicit `Response` objects.
- Static fallback mode when no handler is supplied.
- Public-bind acknowledgement.
- Connection semaphore.
- Header and write timeouts.
- Graceful shutdown.
- File-stream limit configuration.
- Checked body-source range reads.
- TLS client transport behind `client-tls`.
- HTTPS rejection without TLS support.
- Buffered-only client documentation.
- Public Python exports for client primitives.
- URL parser hardening.

This plan should treat these as claims to verify, not assumptions.

## Track A: Python server runtime-limit verification

### A1. Connection-limit saturation

Add a live integration test that starts the Python `Server` with a very small `max_connections`, such as 1 or 2, and opens enough concurrent connections to saturate it.

Verify:

- The configured number of active connections is respected.
- Additional connections do not create unbounded tasks.
- Saturation behavior is deterministic and documented: queued, refused, closed, or otherwise controlled.
- Releasing an active connection permits a subsequent connection.
- The server remains responsive after saturation clears.

Prefer socket-level tests rather than only unit tests around the semaphore.

### A2. Header timeout

Add a slowloris-style test:

- Connect to the server.
- Send an incomplete request line or partial headers.
- Wait longer than `header_timeout_secs`.
- Verify the connection is closed or otherwise terminated.
- Verify the server continues accepting subsequent normal requests.

Ensure the HTTP/1 builder is configured with the required Tokio timer so timeout configuration cannot panic.

### A3. Write timeout

Add a stalled-reader test:

- Serve a sufficiently large file or large byte response.
- Connect and send a valid request.
- Stop reading or read extremely slowly.
- Verify `write_timeout_secs` bounds the stalled response lifecycle.
- Verify the stream permit and connection permit are released afterward.

Confirm that the timeout wraps actual response transmission, not only response construction.

### A4. File-stream semaphore

Add tests with `max_file_streams=1`:

- Start one large file response and keep the client from draining it.
- Request a second file concurrently.
- Verify the second response follows documented exhaustion behavior.
- Release the first stream and verify a later request succeeds.

Check permit lifetime carefully. The permit must remain held until the body stream completes or is dropped after disconnect, not merely until the response object is built.

### A5. Public bind guard

Add tests for:

- `127.0.0.1` allowed without `public=True`.
- `::1` allowed without `public=True` if supported.
- `0.0.0.0` rejected without `public=True`.
- `[::]` rejected without `public=True`.
- Unspecified or wildcard forms cannot bypass the guard through alternate formatting.
- Public bind succeeds with explicit acknowledgement.

Document platform-specific differences if necessary.

## Track B: Python callback contract hardening

### B1. Handler return-type validation

Test handlers returning:

- A valid `Response`.
- `None`.
- A string.
- Bytes directly.
- An arbitrary object.

Invalid types must produce a generic 500 response or a clearly documented server-side failure without panicking Rust or terminating the accept loop.

### B2. Handler exception handling

Test handlers raising:

- `ValueError`.
- A custom exception.
- An exception containing a local filesystem path or secret-looking text.

Verify:

- Client receives a generic 500 body.
- Python traceback, exception message, and local paths are not sent to the client.
- The error remains observable to the embedding process through logging or a documented hook if available.
- The server handles subsequent requests normally.

### B3. Status validation

Test invalid status values:

- 0.
- 99.
- 1000.
- Negative values if Python conversion permits them.

Reject invalid values before writing a response. Valid unusual statuses should follow the HTTP library's accepted range.

### B4. Header validation

Test handler responses containing:

- Empty header names.
- Spaces or delimiters in header names.
- CR, LF, NUL, or other forbidden control characters in values.
- Duplicate headers where duplication matters.
- A false `Content-Length`.
- Conflicting `Transfer-Encoding`.
- Hop-by-hop headers.

Required behavior:

- Rust validates names and values before serialization.
- Invalid responses fail safely as generic 500 or a documented construction error.
- Rust-generated body length cannot be contradicted by Python-provided `Content-Length`.
- File and range responses retain authoritative planner-generated headers.

### B5. HEAD semantics for callback responses

Verify HEAD requests to callback-generated byte/text responses:

- Return the same relevant headers as GET.
- Return no body.
- Generate or preserve correct content length.

If callback dispatch currently treats HEAD as an ordinary response body, correct it in the Rust serialization path rather than relying on Python handlers.

### B6. Handler concurrency and blocking behavior

Document and test the callback execution model:

- Maximum concurrent Python callbacks.
- Whether callbacks use `spawn_blocking` or another bounded executor.
- Whether the GIL is held only during callback execution.
- Whether a blocked handler can starve unrelated connections.

If there is no explicit callback limit, add a bounded `max_python_callbacks` setting or document and reuse a conservative existing limit. Avoid unbounded blocking tasks.

### B7. Shutdown with active work

Test shutdown while:

- A Python handler is running.
- A static file is streaming.
- A client is stalled in headers.
- A client is stalled reading a response.

Define and test whether shutdown is immediate, graceful within a timeout, or waits for active work. Avoid indefinite joins.

## Track C: `ResolvedFile` capability-boundary polish

### C1. Remove normal public construction where possible

Revisit public methods:

- `ResolvedFile::from_parts`.
- `ResolvedFile::into_parts`.
- `ResolvedFile::into_std_file`.

Preferred order:

1. Move PyO3 bridge logic into a crate/module arrangement that permits `pub(crate)` access.
2. Add a narrow internal feature such as `python-bindings-internal`, not enabled for ordinary consumers.
3. Introduce an opaque transfer wrapper consumed only by the Python crate.
4. If none are feasible in this pass, rename reconstruction to an explicitly unsafe-looking API such as `from_unchecked_parts` and remove it from the normal primitive facade documentation.

Do not use Rust `unsafe` merely for naming. The concern is semantic trust, not memory unsafety.

### C2. Public documentation invariants

Ensure docs distinguish:

- A file capability produced by `SecureRoot`.
- A raw file handle extracted by a consumer.
- A reconstructed wrapper that no longer carries a verified provenance guarantee.

No public docs should state that all `ResolvedFile` values necessarily came from confinement if public unchecked construction remains possible.

### C3. Tests

Add compile-time or API-level tests where practical:

- Normal downstream code cannot call an internal reconstruction method.
- Python bindings still work without path reopening.
- Safe body-source conversion remains available.

## Track D: Body-source polish

### D1. Bounded read helpers

`read_all()` is useful for tests but can allocate an entire file. Add or verify an explicit bounded variant:

```rust
read_all_bounded(max_bytes)
```

Python should expose an equivalent bound or require a bound for file-backed bodies. Keep an unbounded helper only if clearly marked test/debug-only.

### D2. Consistent range behavior

Define and test behavior for all body kinds when requested subranges are:

- Inverted.
- Empty.
- Exactly at the end.
- Beyond the end.
- Overflowing `u64` arithmetic.
- Too large for `usize` on the current platform.

Prefer consistent structured errors for file-backed and byte-backed sources. Avoid silently returning empty data for some invalid ranges while erroring for others unless documented.

### D3. Stream error propagation

Current file streaming must not silently convert read errors into a clean EOF. Verify the body stream propagates I/O failures to Hyper so the connection terminates as an error rather than presenting a truncated successful response.

If the stream error type is currently `Infallible`, replace it with an appropriate boxed I/O error type.

## Track E: HTTP client verification

### E1. Feature matrix

Run and add CI coverage for:

```sh
cargo test -p eggserve-core
cargo test -p eggserve-core --features client
cargo test -p eggserve-core --features client-tls
cargo clippy -p eggserve-core --all-targets --features client -- -D warnings
cargo clippy -p eggserve-core --all-targets --features client-tls -- -D warnings
```

Ensure no client code accidentally compiles into default static-server builds.

### E2. HTTP local integration

Add local end-to-end tests for:

- GET.
- HEAD.
- POST/PUT bytes bodies.
- Custom request headers.
- Response headers.
- Response body-size cap.
- Request timeout.
- Connect timeout where deterministic.
- Server disconnect during body.
- Invalid response framing if feasible.

No CI test should require public internet access.

### E3. HTTPS local integration

Use a local TLS server and test certificate fixture.

Verify:

- `https://` is rejected when built with `client` but not `client-tls`.
- Trusted local certificate succeeds with verification enabled when configured through the supported trust mechanism.
- Untrusted certificate fails with `TlsError` when verification is enabled.
- `verify_tls=False`, if retained, succeeds only through the explicitly insecure code path.
- Hostname mismatch fails with verification enabled.
- TLS handshake is covered by the configured request or connection timeout.

Avoid external endpoints.

### E4. Buffered-only contract

Verify the public Rust and Python APIs consistently describe buffered responses:

- No docs say response streaming is available.
- `max_response_body_bytes` is always enforced.
- A misleading `read()` or iterator API is not exposed.
- Architecture docs identify streaming as deferred.

### E5. URL parser regression cases

Add cases for:

- Query without explicit slash where relevant.
- Fragments stripped before transmission.
- IPv6 authority formatting with brackets in `Host` and URI.
- Empty port.
- Extra characters after IPv6 closing bracket.
- Control characters.
- Spaces.
- Percent-encoded path/query preservation.
- Hostname case behavior.
- Internationalized domain names explicitly rejected or documented as unsupported.

Do not expand into a general URL library. Document the supported subset.

## Track F: Python public API and exception polish

### F1. Public imports

From an installed wheel, verify these imports:

```python
from eggserve import (
    HttpClient,
    ClientConfig,
    ClientRequest,
    ClientResponse,
    ClientError,
    Method,
    Server,
    ServerSecureRoot,
    Request,
    Response,
    StaticResponder,
)
```

Ensure `__all__` contains the same public names when native bindings are available.

### F2. Exception hierarchy

Current client failures should not all surface as generic `ValueError` if a meaningful hierarchy is already intended.

Prefer:

- `EggserveError` base.
- `ClientError` subclass or dedicated native exception type.
- Structured attributes or stable error code where practical.

At minimum, distinguish programmer validation errors from network/TLS/timeout failures.

Do not break existing consumers unnecessarily; add compatibility tests if changing exception classes.

### F3. Constructor validation

Reject at construction time:

- Negative timeout values.
- NaN/infinite timeout values.
- Zero limits where zero is nonsensical.
- Invalid callback objects.
- Invalid bind addresses.

Avoid panics from `Duration::from_secs_f64` on invalid Python floats.

## Track G: Installed-wheel and packaging verification

### G1. Build wheel in clean environment

Build and install the wheel into a fresh virtual environment. Do not rely on `PYTHONPATH` pointing at the source tree.

Verify:

- Native module imports.
- Public exports.
- Python server starts and serves static content.
- Callback handler works.
- HTTP client performs a local request.
- Packaged CLI runs with `python -m eggserve --help`.
- Binary lookup works.

### G2. Platform matrix

At minimum, cover supported CI platforms:

- Linux.
- macOS.
- Windows for functional tests, respecting the documented reparse-point limitation.

If a full multi-Python cibuildwheel matrix is not yet present, add the smallest practical matrix that catches packaging breakage.

### G3. Python versions

Test the supported Python version range declared in `pyproject.toml`. If Python 3.14 requires an environment workaround, either automate it in CI or narrow declared support until the toolchain supports it cleanly.

Do not claim support that is only manually achievable through undocumented environment variables.

## Track H: CI and release-gate polish

Update `.github/workflows/ci.yml` to include:

- Default Rust workspace tests.
- `client` feature tests.
- `client-tls` feature tests.
- Python native primitive tests from an installed wheel.
- Python server callback integration tests.
- Python client local integration tests.
- TLS local integration tests where feasible.
- `cargo audit`.
- `cargo deny check`.

Keep jobs reasonably separated so failures identify the affected surface.

Do not mark Plan 030 complete solely because local tests pass. CI coverage should enforce the new contracts.

## Track I: Documentation and examples

Audit and reconcile:

- `README.md`.
- `AGENTS.md`.
- `.opencode/skills/eggserve-dev/SKILL.md`.
- `docs/python-api.md`.
- `docs/http-client-primitives.md`.
- `docs/extension-contract.md`.
- `docs/threat-model.md`.
- `docs/invariants.md`.
- `architecture/client.md`.
- `architecture/eggserve-python.md`.
- `architecture/primitives-api.md`.
- Python and Rust examples.

Required documentation points:

- Python callback server is low-level and not ASGI/WSGI.
- Request bodies remain unsupported or limited according to actual implementation.
- Client responses are buffered-only.
- TLS support is feature-gated.
- Windows reparse-point hardening remains deferred.
- Capability extraction ends eggserve's confinement provenance guarantee.
- Timeout and limit semantics are precise.

Run all documented examples or convert them into tested examples where practical.

## Acceptance criteria

Plan 030 is complete only when all of the following are true:

- Live tests prove connection, header-timeout, write-timeout, and file-stream limits on the Python server path.
- Handler exceptions and invalid return values cannot panic Rust or leak traceback/path details to clients.
- Response status and headers are validated before serialization.
- HEAD semantics are correct for callback-generated responses.
- Shutdown behavior with active callbacks and streams is documented and tested.
- The `ResolvedFile` reconstruction/extraction boundary is narrowed or explicitly represented as unchecked provenance.
- Body-source reads are bounded, range-consistent, and overflow-safe.
- File streaming propagates I/O errors rather than silently truncating.
- `client` and `client-tls` feature matrices are covered in CI.
- HTTPS behavior is verified against a local TLS fixture.
- Python public imports work from an installed wheel.
- Python timeout/config constructors reject invalid float and limit values safely.
- Documentation and examples match actual behavior.
- No ASGI/WSGI/framework features are introduced.

## Validation commands

### Rust default and server

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
```

### Client feature matrix

```sh
cargo clippy -p eggserve-core --all-targets --features client -- -D warnings
cargo test -p eggserve-core --features client
cargo clippy -p eggserve-core --all-targets --features client-tls -- -D warnings
cargo test -p eggserve-core --features client-tls
```

### Dependency checks

```sh
cargo audit
cargo deny check
```

### Python source-tree tests

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_client_primitives -v
```

### Installed-wheel verification

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m venv .venv-wheel-test
. .venv-wheel-test/bin/activate
python -m pip install --upgrade pip
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python -m unittest eggserve.test_primitives -v
python -m unittest eggserve.test_server -v
python -m unittest eggserve.test_server_primitives -v
python -m unittest eggserve.test_client_primitives -v
```

Adapt virtual-environment activation for Windows CI.

## Handoff guidance

Treat this as a verification and contract-polish pass. Fix defects discovered by the tests, but do not use them as justification for adding unrelated features. The highest-risk areas are timeout coverage around actual I/O, permit lifetime across streaming bodies, Python callback validation, TLS correctness, and capability provenance. Those should receive priority over cosmetic cleanup.
