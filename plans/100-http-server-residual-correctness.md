# Plan 100 — HTTP Server Residual Correctness Closure

## Status

Corrective implementation follow-up to Plan 099.

Baseline reviewed commit:

```text
03ba7851f1ac8f1b79552144c4406fbfaf075ac1
```

Plan 099 substantially corrected the Python `http.server` compatibility workstream, but post-implementation review found a small number of remaining runtime and API-contract defects. This plan is limited to those defects. It must not reopen the broader roadmap or introduce a new server architecture.

Plan 101 owns the focused verification, documentation cleanup, and final closure evidence after this implementation plan lands.

## Goal

Correct the remaining implementation-level gaps in four areas:

1. normalize Python compatibility-server bind addresses, especially the empty-host form;
2. make Python response conversion strictly fail closed for malformed body objects and conversion failures;
3. sanitize handler-failure diagnostics so logs do not contain untrusted exception text;
4. finish the MIME customization contract without reopening or reading files in Python.

## Governing constraints

The following constraints are mandatory.

- Keep the supported Python API limited to the existing six `eggserve.server` classes.
- Remain HTTP/1.1 only.
- Do not add ASGI, WSGI, CGI, routing, middleware, proxying, uploads, WebSockets, HTTP/2, or HTTP/3.
- Do not add a Python accept loop or expose raw accepted sockets.
- Do not create a second path resolver, file opener, file streamer, TLS stack, or response serializer.
- Rust retains socket ownership, request parsing, path confinement, file opening, streaming, framing, and final response construction.
- Preserve the CLI requirement that wildcard binds require explicit `--public` intent.
- Do not add a new dependency for address parsing, MIME handling, logging, or testing.
- Do not add a new CI workflow, matrix, evidence framework, or test runner.
- Prefer small internal helpers over new public abstractions.
- Keep changes reviewable; do not combine all implementation, tests, documentation, and workflow changes into one opaque commit.

## Confirmed residual defects

### A. Empty-host construction is not normalized

The compatibility facade accepts:

```python
HTTPServer(("", 0), Handler)
```

but currently passes the empty string through to the native server while `_is_wildcard_host("")` returns false. The native binding layer then resolves the value independently and may reject it as an unresolved or unauthorized unspecified bind.

For the Python compatibility constructor, an empty host is explicit wildcard intent and should behave predictably without weakening CLI safeguards.

### B. Structural response conversion still has silent empty-body fallbacks

The Rust Python callback bridge validates status and headers more strictly than before, but malformed structural body objects can still be converted into empty bodies through fallback behavior such as:

- an unknown body `kind` becoming `ResponseBody::Empty`;
- `read_all()` extraction failure becoming `Vec::new()` through `unwrap_or_default()`;
- a missing or malformed body attribute being interpreted as an empty response;
- unsupported body objects being accepted without a controlled error.

This violates the documented atomic fail-closed boundary.

### C. Handler exceptions can leak untrusted text into operational logs

The callback bridge currently formats the Python exception into an operational event. The client receives a generic 500, but exception text can contain secrets, filesystem paths, tokens, request data, or application-specific values.

Diagnostics must identify the failure category without interpolating untrusted exception or header content.

### D. MIME hooks require a final bounded contract

Direct-file requests now honor both `extensions_map` and subclass `guess_type()`. Remaining edge cases need to be made explicit and tested:

- invalid MIME values must fail closed;
- GET, HEAD, and range responses must retain the selected MIME value;
- default and `super().guess_type()` behavior must remain stable;
- directory-index behavior must be truthful without Python path resolution or a second filesystem lookup.

## Execution order

Implement in this order:

```text
A. compatibility bind normalization
B. strict response-body conversion
C. sanitized operational diagnostics
D. bounded MIME-contract cleanup
E. focused implementation tests
```

Do not mark Plan 100 complete until all implementation acceptance criteria are met locally. Plan 101 performs final repository-level verification and hosted closure.

---

## Track A — Normalize compatibility-server bind addresses

### Objective

Make the Python `HTTPServer` and `HTTPSServer` constructor family handle common stdlib-shaped address forms predictably while keeping the CLI wildcard guard unchanged.

### Required behavior

Support these constructor forms:

```python
HTTPServer(("", 0), Handler)
HTTPServer(("localhost", 0), Handler)
HTTPServer(("127.0.0.1", 0), Handler)
HTTPServer(("0.0.0.0", 0), Handler)
HTTPServer(("::1", 0), Handler)
HTTPServer(("::", 0), Handler)
```

IPv6 cases may be skipped only when the host operating system does not provide the requested address family or loopback capability.

### Empty-host decision

For the compatibility facade, normalize:

```text
"" -> "0.0.0.0"
```

This mirrors the common IPv4 `HTTPServer` interpretation and avoids depending on platform-specific empty-host resolver behavior.

The empty string is explicit caller intent to bind a wildcard address. Therefore the compatibility facade must pass the native equivalent of `public=True` for this normalized bind.

Do not change CLI behavior. `eggserve --bind 0.0.0.0` and `eggserve --bind ::` must continue requiring the existing explicit public-bind opt-in.

### Preferred implementation

Add one small internal normalization helper in the Python facade, for example:

```python
def _normalize_compat_server_address(host: str, port: int) -> tuple[str, int, bool]:
    ...
```

The helper should return:

- the host value passed to the native binding layer;
- the validated port;
- whether the caller explicitly requested an unspecified/wildcard bind.

Required rules:

1. Validate the original `(host, port)` tuple before creating native state.
2. Normalize only the empty string; do not perform a second independent hostname resolution in Python.
3. Detect literal unspecified IPv4 and IPv6 addresses with `ipaddress.ip_address()`.
4. Leave `localhost`, resolvable hostnames, IPv4 literals, and IPv6 literals for the native Rust resolver.
5. Pass explicit wildcard intent to the native server through its existing `public` argument.
6. Preserve the original requested address only where useful for error reporting; publish the actual native bound address through `server_address` after activation.
7. Keep IPv6 tuple values unbracketed in Python: `("::1", port)`, not `(“[::1]”, port)`.
8. Continue using the native bound address for port `0` publication.

### Lifecycle behavior

Preserve the current bounded lifecycle contract:

- `bind_and_activate=True` performs real native activation during construction;
- `server_address` and `server_port` contain the actual bound port before construction returns;
- `bind_and_activate=False` does not bind until `server_activate()` or `_start()`;
- double activation must not create a second native listener;
- activation after `server_close()` must fail clearly;
- `server_close()` releases any activated native listener;
- no raw listener descriptor is exposed.

Do not attempt exact `socketserver.TCPServer` internal parity.

### Error behavior

- Invalid tuple shape: controlled `OSError` or `TypeError`, consistent with the existing facade.
- Port outside `0..65535`: controlled `OSError`.
- Unresolvable hostname: controlled `OSError` before serving.
- Unsupported address family: controlled `OSError`, not a panic.
- Explicit wildcard: accepted by the Python compatibility facade.
- CLI wildcard without `--public`: still rejected.

### Required tests

Add focused installed-wheel tests for:

- empty host with port `0` activates successfully;
- empty host publishes a nonzero actual port;
- explicit `0.0.0.0` activates successfully;
- `localhost` activates successfully;
- IPv4 loopback port `0` remains correct;
- IPv6 loopback works where supported;
- IPv6 wildcard works where supported;
- `client_address` remains a `(host, port)` tuple;
- `server_address` remains a `(host, port)` tuple;
- TLS uses the same tuple shape;
- invalid hostname fails clearly;
- CLI wildcard protection is unchanged through an existing CLI-level test or one small focused assertion.

### Acceptance criteria for Track A

- `HTTPServer(("", 0), Handler)` works deterministically.
- Explicit Python wildcard intent is accepted without adding a new public API argument.
- CLI wildcard behavior remains unchanged.
- Hostnames and IPv4/IPv6 forms remain resolved by the native layer once.
- Published addresses are proper Python tuples with actual ports.
- No Python socket ownership or second resolver path is introduced.

---

## Track B — Make response conversion strictly fail closed

### Objective

Ensure the Python callback boundary either constructs one completely valid canonical response or returns a sanitized generic 500. No malformed body representation may silently become an empty successful response.

### Supported response forms

Preserve the currently supported response producers:

- native `eggserve._native.Response` / `PyResponse` objects;
- the internal `_HandlerResponse` produced by `BaseHTTPRequestHandler`;
- any deliberately supported structural response form already documented by the low-level API.

Do not broaden the accepted response protocol. It is acceptable to narrow undocumented duck-typed behavior when necessary to enforce deterministic validation.

### Required extraction order

Refactor response conversion into explicit stages:

```text
1. identify supported response representation
2. extract and validate status
3. extract headers as an ordered sequence
4. validate every header atomically
5. extract body using the representation-specific path
6. validate one-shot body ownership and body length
7. validate supplied Content-Length against staged body length
8. build the canonical response
9. normalize framing once
```

Do not mutate a canonical response while validation is still in progress.

### Header requirements

Retain the Plan 099 behavior:

- status type and range validation;
- ordered duplicate-preserving headers;
- invalid names rejected;
- CR, LF, and NUL rejected;
- all hop-by-hop fields rejected case-insensitively;
- supplied `Content-Length` accepted only when exactly equal to the representation length;
- no partial valid headers reach the wire after a later validation failure.

### Body requirements

The following must produce a controlled service error and generic 500:

- missing required body field for a structural response form;
- unknown body `kind`;
- unsupported body object;
- failure to call `read_all()`;
- failure to extract returned bytes;
- already-consumed native body source;
- poisoned native body lock;
- file body conversion failure;
- a body length that cannot be determined where the response contract requires it;
- structural inconsistency between headers and body.

Remove all `unwrap_or_default()` and silent `ResponseBody::Empty` fallbacks from error paths.

A deliberate empty response must be represented explicitly by the supported response type, not inferred from malformed state.

### One-shot ownership

When a native file or byte body is taken from a response:

- consume it exactly once;
- on subsequent reuse, fail closed;
- if later validation fails, drop the taken capability safely;
- do not reopen the path;
- do not clone file handles to recover from validation failure;
- ensure any file-stream permit is not acquired until transport conversion.

### Body-forbidden and informational statuses

Keep canonical normalization authoritative for HEAD and body-forbidden statuses.

Clarify the distinction between:

- canonical status type acceptance of `100..599`;
- final handler-response transport policy for informational responses.

If the runtime does not support a standalone handler-produced 1xx response as a final response, retain the current generic-500 behavior and document it explicitly. Do not add interim-response machinery under this plan.

### Error mapping

All conversion failures must map to the existing sanitized service-error path and produce a generic 500 response.

Do not expose:

- exception messages;
- raw invalid header names or values;
- filesystem paths;
- token-like values;
- Python repr output;
- tracebacks.

### Required tests

Add focused low-level Python tests for:

- unknown body kind -> 500;
- `read_all()` raises -> 500;
- `read_all()` returns a non-bytes value -> 500;
- missing structural body -> 500 where body is required;
- malformed structural response object -> 500;
- already-consumed native body -> 500;
- mismatched `Content-Length` -> 500;
- invalid header after a valid header -> 500 with no partial valid header on the wire;
- duplicate safe fields remain preserved;
- explicit empty native response remains valid;
- normal bytes and file responses remain valid.

Use the smallest test response doubles necessary. Do not add a generalized mock-response framework.

### Acceptance criteria for Track B

- There are no silent empty-body fallbacks in error paths.
- Every supported response representation has one explicit extraction path.
- Malformed or unsupported body state produces generic 500.
- Header and body validation are atomic.
- One-shot native body ownership remains intact.
- Canonical normalization remains the sole final framing authority.

---

## Track C — Sanitize handler-failure diagnostics

### Objective

Prevent untrusted Python exception text and invalid response data from entering operational logs while retaining useful failure categories.

### Required logging behavior

Replace diagnostics that interpolate Python exception text, response reprs, or invalid values with fixed-category messages.

Examples of acceptable event messages:

```text
Python handler raised an exception
Python handler returned an unsupported response type
Python handler response header validation failed
Python handler response body conversion failed
Python handler response length validation failed
```

Do not include:

- `{e}` from a Python exception;
- `repr(result)`;
- raw header names or values supplied by the handler;
- body data;
- filesystem paths;
- request authorization or cookie values.

The internal event kind, severity, connection identifier, and a bounded static category are sufficient.

### Error-category design

Use the existing `ServiceError` and operational event model. Do not introduce a new logging framework or public exception hierarchy.

If useful, add one private conversion-error category enum with a small fixed set of variants, but only if it reduces repeated string logic. Do not expose it publicly.

### Required tests

Add focused tests proving:

- a handler exception containing a sentinel secret returns a generic 500;
- the sentinel secret does not appear in captured operational output;
- an invalid header value containing a sentinel secret does not appear in captured operational output;
- a malformed body conversion does not log the Python object repr;
- valid handler failures still emit one useful service-error category where the existing test seam supports it.

Prefer an existing in-memory log sink or existing stderr-capture helper. Do not create a second logging test framework solely for these assertions.

### Acceptance criteria for Track C

- Client responses remain sanitized.
- Operational logs contain fixed categories rather than untrusted values.
- Existing event kinds and severity semantics remain intact.
- No new public logging API is introduced.

---

## Track D — Finish the bounded MIME customization contract

### Objective

Make MIME override behavior deterministic, fail closed, and accurately documented without introducing Python filesystem access or a second native response pass.

### Supported contract

Keep these supported behaviors:

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

### Direct-file and index behavior

Use the following bounded contract unless implementation can support more through an already-available native metadata field without another resolver or filesystem pass:

- `extensions_map` is passed to the native responder and applies to direct files and native-selected index files;
- subclass `guess_type()` is invoked for direct request targets that name a file suffix;
- directory requests resolved to an index page use the captured `extensions_map` and native defaults;
- `guess_type()` is not promised for an index filename that Python never authoritatively resolves.

This limitation must be stated in compatibility documentation. Do not add a two-phase resolver, Python `stat()`, Python directory probing, or path reopening solely to invoke `guess_type()` on an index filename.

If the native response already carries a safe selected display filename and a one-line internal hook can apply `guess_type()` without another filesystem operation, using that metadata is acceptable. Do not expand the implementation to manufacture such a framework.

### MIME validation

Treat the result of `guess_type()` and every `extensions_map` value as untrusted response metadata.

Required behavior:

- value must be a string;
- value must pass the existing canonical header-value validation;
- CR, LF, and NUL are rejected;
- invalid values produce generic 500;
- values are not silently dropped;
- unknown suffixes remain `application/octet-stream`;
- `X-Content-Type-Options: nosniff` remains present.

Do not add OS MIME-database integration or a broad MIME dependency.

### GET, HEAD, and range behavior

For the same direct file and handler class:

- GET returns the selected MIME value;
- HEAD returns the same selected MIME value;
- range responses return the same selected MIME value;
- conditional responses preserve the same metadata where applicable;
- no Python file read, `stat`, path translation, or reopen occurs.

### Required tests

Add focused compatibility tests for:

- `extensions_map` direct-file GET;
- `extensions_map` direct-file HEAD;
- `extensions_map` direct-file range;
- subclass `guess_type()` GET;
- subclass `guess_type()` HEAD;
- subclass `guess_type()` range;
- `super().guess_type()` retains the base default;
- unknown suffix remains octet-stream plus `nosniff`;
- invalid `guess_type()` value -> generic 500;
- invalid `extensions_map` value -> generic 500;
- index-page behavior matches the documented bounded contract;
- no Python file-opening helper is invoked by MIME selection, using a small monkeypatch or existing seam if practical.

### Acceptance criteria for Track D

- Supported MIME hooks affect the wire response consistently.
- Invalid MIME values fail closed.
- HEAD and range metadata match GET.
- Directory-index behavior is truthful and bounded.
- Rust retains exclusive path resolution and file ownership.

---

## Track E — Focused implementation verification

### Objective

Verify only the implementation changes introduced by Plan 100 before handing off to Plan 101.

### Required local commands

Run the smallest relevant tests during implementation, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features client-tls
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
bash scripts/test-python-wheel.sh
```

The final repository-wide `verify.sh fast`, `verify.sh full`, hosted CI confirmation, documentation audit, and closure record belong to Plan 101.

### Test-size control

- Extend existing test files where the behavior naturally belongs.
- Add no new test framework.
- Add no new corpus or matrix.
- Avoid timing-sensitive socket saturation tests.
- Prefer direct deterministic conversion and lifecycle assertions.
- Do not duplicate lower-level Rust parser or TLS-loader tests at the Python level.

## Suggested commit sequence

Keep the implementation reviewable:

1. `fix: normalize Python compatibility bind addresses`
2. `fix: reject malformed Python response bodies atomically`
3. `fix: sanitize Python handler diagnostics`
4. `fix: close MIME override edge behavior`
5. `test: cover residual HTTP server correctness`

Combining adjacent implementation and its directly associated tests is acceptable. Do not combine all tracks with final documentation and closure evidence in one commit.

## Plan 100 final acceptance criteria

Plan 100 is implementation-complete only when all of the following are true:

### Address handling

- Empty-host Python server construction works.
- Explicit wildcard IPv4 and supported IPv6 construction work.
- CLI wildcard safeguards remain unchanged.
- Port `0` publishes the actual bound port.
- Plaintext and TLS peer/server addresses remain structured tuples.
- Invalid addresses fail without panic.

### Response conversion

- Malformed body objects cannot silently become empty successful responses.
- Unknown body kinds fail closed.
- Body extraction failures fail closed.
- Header/body validation is atomic.
- One-shot file and byte ownership remains correct.
- Invalid responses return generic 500 with no partial headers.

### Diagnostics

- Handler exception text is not interpolated into operational logs.
- Invalid response values are not logged verbatim.
- Static bounded failure categories remain observable.

### MIME behavior

- Supported overrides affect GET, HEAD, and range responses.
- Invalid MIME values return generic 500.
- Unknown types remain octet-stream with `nosniff`.
- Directory-index behavior matches active documentation.
- Python performs no authoritative path or file operation.

### Scope and quality

- No new dependency was added.
- No new public server abstraction was added.
- No new CI workflow or test framework was added.
- Rust formatting, lint, workspace tests, TLS tests, and installed-wheel tests pass.

## Handoff to Plan 101

The Plan 100 handoff must include:

- final implementation commit SHA;
- exact empty-host normalization behavior;
- supported IPv4/IPv6/wildcard forms;
- final supported response representations;
- list of removed silent body fallbacks;
- sanitized diagnostic categories;
- MIME behavior for direct files and directory indexes;
- focused tests added or consolidated;
- local command results;
- any deliberate compatibility divergence that Plan 101 must document.

Do not mark Plans 094–100 or the overall compatibility workstream closed under this plan. Final closure is owned by Plan 101.