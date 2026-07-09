# Plan 025: HTTP Correctness Primitives

## Purpose

Turn eggserve's current HTTP behavior into a documented, reusable HTTP/1.1 primitive contract that downstream projects can depend on. This pass should make the static server more correct and make the primitive layer useful for external app-server or adapter projects without adding ASGI/WSGI/framework code to this repository.

The output should be a sharper Rust primitive boundary around request validation, headers, body metadata policy, response planning, conditional requests, range requests, and status/header/body contracts. Python bindings should expose the same concepts where practical.

## Current context

The repo already has several relevant pieces:

- `crates/eggserve-core/src/primitives/http.rs` for request validation.
- `crates/eggserve-core/src/primitives/planner.rs` for ETag, conditional, range, and response planning.
- `crates/eggserve-core/src/primitives/response.rs` for response-plan types.
- `crates/eggserve-core/src/service.rs` for actual HTTP handler behavior.
- `crates/eggserve-core/src/response.rs` for concrete response construction.
- Python bindings in `crates/eggserve-python/src/lib.rs` and Python tests in `crates/eggserve-python/python/eggserve/test_primitives.py`.

This plan should avoid broad rewrites. Prefer extracting clean primitives from the behavior that already exists.

## Goals

- Define eggserve's supported HTTP/1.1 subset for server-side primitives.
- Expose reusable request metadata validation primitives.
- Expose a stable response-planning contract for static and custom responses.
- Make HEAD/GET parity mechanically testable.
- Make byte-range and conditional behavior explicit and regression-tested.
- Make malformed and unsupported input fail predictably.
- Prepare the primitive layer for future Rust-owned Python server callbacks.
- Preserve the current CLI behavior unless a bug is found.

## Non-goals

- Do not add ASGI/WSGI support.
- Do not add routing or middleware.
- Do not add request body streaming for dynamic applications yet.
- Do not add HTTP/2 or HTTP/3.
- Do not add WebSocket or CONNECT.
- Do not add proxy behavior.
- Do not add response compression.
- Do not add cookie/session helpers.

## Supported server-side HTTP subset

Document and test this subset:

- HTTP/1.1 server behavior through Hyper.
- GET and HEAD for the static CLI path.
- Explicit method validation primitive for downstream code.
- Origin-form request targets for static path parsing.
- No request bodies for the static CLI path.
- Configurable body metadata validation primitive for downstream code.
- Static full-file responses.
- Static range responses.
- Empty responses.
- Byte responses for future dynamic primitive work.
- Conditional GET/HEAD via `If-None-Match` and `If-Modified-Since`.
- Range requests via `Range` and `If-Range`.
- Generic 400/403/404/405/413/416/500/503 behavior for the CLI path.

Unsupported in this contract:

- Request body streaming into Python callbacks.
- Multipart range responses.
- Chunked response construction as a public primitive.
- HTTP trailers.
- Upgrade semantics.
- Absolute-form proxy requests.
- Authority-form CONNECT requests.
- Asterisk-form OPTIONS requests.

## Implementation tasks

### 1. Add `docs/http-primitives.md`

Create a new document that describes the HTTP primitive contract. Include sections for:

- Supported protocol subset.
- Request method validation.
- Request target validation.
- Header handling rules.
- Request body metadata policy.
- Static response planning.
- Conditional request behavior.
- Range request behavior.
- HEAD/GET parity.
- Error mapping.
- Downstream use by app-server/adapter projects.

The doc must state that downstream projects may build ASGI/WSGI/app servers externally, but eggserve does not implement those protocols in-tree.

### 2. Review `primitives/http.rs`

Ensure the HTTP validation primitive layer exposes enough to downstream callers:

- `ReadOnlyMethod` or equivalent for GET/HEAD.
- Method validation returning structured errors.
- Request-target validation that distinguishes coarse URI-form checks from full path confinement.
- Body metadata validation that can reject invalid `Content-Length`, conflicting `Content-Length`/`Transfer-Encoding`, unsupported transfer encoding, and body length above configured policy.

If current function names are unclear, add new clearer wrappers while preserving existing APIs for compatibility where feasible.

Do not force downstream projects into the static CLI's zero-body policy. The static CLI should use zero-body policy, but future dynamic servers need a primitive that can validate configurable body limits.

### 3. Review `primitives/response.rs`

Ensure response planning types are explicit enough for downstream consumers:

- Status code abstraction.
- Header map plan or ordered header list.
- Body plan variants.
- File range representation.
- Conditional request outcome.
- Range request outcome.

If a type is currently static-specific, document that. If it is general enough for downstream use, name and document it that way.

### 4. Review `primitives/planner.rs`

Keep the pure-function planner model. Expand tests for:

- `If-None-Match` exact strong tag.
- `If-None-Match` weak comparison.
- `If-None-Match: *`.
- Multiple ETags with whitespace.
- Malformed `If-Modified-Since` ignored or mapped according to current documented behavior.
- `If-Modified-Since` older/equal/newer behavior.
- `Range: bytes=0-0`.
- `Range: bytes=0-`.
- `Range: bytes=-1`.
- `Range` suffix larger than file.
- `Range` start beyond EOF.
- `Range` start greater than end.
- Multiple ranges explicitly unsupported.
- Unsupported range unit.
- Zero-length file range behavior.
- `If-Range` matching ETag.
- `If-Range` non-matching ETag.
- `If-Range` matching date.
- `If-Range` stale date.
- HEAD with range returns headers but no body.

If any behavior is ambiguous, document the chosen behavior in `docs/http-primitives.md` and test it.

### 5. Add live HTTP integration tests

Add integration tests that exercise the actual connection path, not only pure planners or handler functions. These may live under an integration-test module or a crate-level test file depending on current layout.

Test at minimum:

- GET existing file returns 200 and body.
- HEAD existing file returns 200, same relevant headers, no body.
- GET missing returns 404.
- Dotfile returns 403 under safe defaults.
- Directory without index returns 403 under safe defaults.
- Directory with index returns 200.
- POST returns 405 with `Allow: GET, HEAD`.
- Malformed percent path returns 400.
- Traversal attempt returns 403 or 400 according to documented mapping.
- GET with positive `Content-Length` returns 413.
- GET with invalid `Content-Length` returns 400.
- GET with `Transfer-Encoding` returns 400.
- Range request returns 206 and exact body bytes.
- Unsatisfiable range returns 416 and `Content-Range: bytes */N`.
- Conditional matching ETag returns 304.
- HEAD range returns 206 with no body.

Prefer starting the server on `127.0.0.1:0` or using the existing `serve_connection` test pattern. Avoid adding an external HTTP client dependency if raw `TcpStream` assertions are sufficient.

### 6. Python primitive bindings

Expose or verify Python equivalents for:

- Method validation.
- Request target validation.
- Body metadata validation.
- Response plan inspection.
- Range outcome inspection if already feasible.
- Conditional response planning through `ResolvedFile.plan_conditional_response`.

Add Python tests for:

- GET/HEAD method validation.
- Unsupported method rejection.
- Invalid content length rejection.
- Transfer-Encoding rejection.
- Range response planning.
- Conditional 304 planning.
- HEAD response planning body kind.

Do not expose incomplete socket/server behavior in this pass.

## Error taxonomy expectations

Use structured errors internally and across Python bindings. The exact enum/class names can remain current, but consumers should be able to distinguish:

- Unsupported method.
- Unsupported request target form.
- Malformed request target.
- Path policy denial.
- Invalid content length.
- Body too large.
- Unsupported transfer encoding.
- Conflicting body headers.
- Malformed range.
- Unsatisfiable range.

Do not expose local filesystem paths in error strings.

## Documentation expectations

Update these docs as needed:

- `docs/http-primitives.md` new.
- `docs/python-api.md` for Python HTTP validation and response-planning primitives.
- `architecture/response-planning.md` for expanded test matrix and downstream use.
- `docs/invariants.md` for HTTP primitive invariants.

## Acceptance criteria

- `docs/http-primitives.md` exists and clearly defines the supported HTTP subset.
- Request validation primitives support static-server policy and future configurable body policies.
- Response planner behavior is tested for conditionals, ranges, HEAD, and malformed inputs.
- Live HTTP integration tests cover the actual server connection path.
- Python primitive tests cover request validation and response planning.
- No ASGI, WSGI, routing, middleware, framework, or proxy code is added.
- Existing CLI behavior remains compatible unless a documented bug is corrected.
- All validation commands pass.

## Validation commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
```

If native tests require a built extension:

```sh
cd crates/eggserve-python
maturin develop
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
```

## Handoff notes

Keep this pass focused on contracts and tests around the HTTP/1.1 subset. Resist adding convenience APIs that look like a framework. Downstream adapter authors need precise primitives more than they need a large surface.
