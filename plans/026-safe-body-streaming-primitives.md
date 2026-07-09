# Plan 026: Safe Body and File Streaming Primitives

## Purpose

Expose safe response body primitives so downstream Rust and Python consumers can send empty bodies, byte bodies, full static files, and byte ranges without reconstructing filesystem paths or manually implementing HTTP framing. This is the critical bridge between the current response-planning layer and future Python server primitives.

The highest priority is preserving the core filesystem guarantee: if a file was resolved under `SecureRoot`, downstream consumers should be able to stream that already-opened file capability. They should not need to rebuild a path from safe components and call `open()` again.

## Current context

Current useful pieces:

- `crates/eggserve-core/src/fs/mod.rs` resolves files and stores an opened `std::fs::File` in `ResolvedFile`.
- `crates/eggserve-core/src/service.rs` converts the resolver-opened file into `tokio::fs::File::from_std(file.file)` and streams it.
- `crates/eggserve-core/src/response.rs` builds concrete Hyper responses.
- `crates/eggserve-core/src/primitives/response.rs` defines response plan types.
- `docs/python-api.md` correctly warns that Python `ResolvedFile` does not yet expose the resolver-opened file handle and that callers must not reconstruct and reopen paths.

This pass should close that gap.

## Goals

- Introduce a public body-source abstraction suitable for Rust consumers.
- Expose safe static file and range streaming without path reopening.
- Make the CLI use the same body-source machinery where practical.
- Expose Python-safe file/range streaming primitives backed by resolver-opened handles.
- Preserve stream permit accounting and timeout compatibility.
- Prepare for future Python server callbacks returning body sources.

## Non-goals

- Do not add ASGI/WSGI adapters.
- Do not add a routing system.
- Do not add request-body streaming into Python yet.
- Do not add compression.
- Do not add multipart range responses.
- Do not add chunked encoding as a public abstraction unless it naturally falls out of Hyper response construction.
- Do not expose raw file descriptors to Python unless there is a documented ownership and safety model.

## Design constraints

### Capability preservation

A `ResolvedFile` should remain the authoritative capability. Any streaming API must consume, borrow, or clone/duplicate that capability in a way that does not reopen by path. On Unix, if duplication is needed, prefer descriptor duplication over path reopening. On Windows, if implemented, use handle duplication rather than path reopening.

### Single ownership model

The current internal `ResolvedFile` owns `std::fs::File`. Public APIs must avoid accidental double-use or unsound sharing. Choose one of these approaches deliberately:

- Consuming body source: converting a `ResolvedFile` into a `BodySource::File` consumes the file capability.
- Cloneable duplicated handle: `ResolvedFile` can create a new handle through OS handle duplication, not path reopening.
- Borrowed streaming: response construction borrows the file capability for a bounded operation.

The first approach is simplest and should be preferred unless it creates unusable ergonomics.

### Rust owns HTTP body mechanics

Python should not manually chunk, range-seek, or serialize HTTP responses. Python may return or inspect body-source objects, but Rust should implement the actual I/O.

## Implementation tasks

### 1. Design `BodySource` in Rust primitives

Add a primitive body abstraction under `crates/eggserve-core/src/primitives/response.rs` or a new `primitives/body.rs` module.

Candidate variants:

```rust
pub enum BodySource {
    Empty,
    Bytes(bytes::Bytes),
    StaticFile(ResolvedFileBody),
    StaticRange(ResolvedFileRangeBody),
}
```

Use different names if clearer. The important properties are:

- Empty body has explicit content length 0 where appropriate.
- Bytes body owns immutable bytes.
- Static file body owns or safely duplicates a resolver-opened file.
- Static range body owns or safely duplicates a resolver-opened file plus start/end.
- Body source can be converted into the concrete Hyper body used by the CLI/server layer.

Avoid exposing `crate::fs::ResolvedFile` directly if it remains internal. If needed, add a public primitive wrapper that preserves invariants.

### 2. Add a public resolved-file body conversion

In `primitives/secure_root.rs`, add a method or conversion that turns a resolved file into a body source.

Candidate API shapes:

```rust
impl ResolvedFile {
    pub fn into_body(self) -> BodySource;
    pub fn into_range_body(self, start: u64, end_inclusive: u64) -> Result<BodySource, RangeError>;
}
```

or:

```rust
pub fn body_for_plan(self, plan: &StaticResponsePlan) -> Result<BodySource, BodySourceError>;
```

The second form may be safer because it couples range bounds to a previously computed response plan.

Whichever API is chosen must prevent invalid ranges from being streamed silently.

### 3. Refactor CLI response construction to consume body sources where feasible

`service.rs` currently matches `BodyPlan` and then calls `file_response` or `file_response_range`. Keep behavior but route through the new body-source abstraction if this can be done without broad churn.

The desired architecture is:

1. Resolve resource.
2. Plan response.
3. Convert resolved resource + plan into body source.
4. Convert status + headers + body source into Hyper response.

If a full refactor is too large, add the abstraction in parallel and leave CLI behavior unchanged, but add tests proving the new body source produces equivalent output.

### 4. Preserve file-stream semaphore behavior

Static file and range streaming must remain subject to `max_file_streams`. Decide where permits live:

- In service response construction.
- In body-source-to-Hyper conversion.
- In a server context passed to conversion.

Do not let downstream users bypass stream limits accidentally when using the Rust-owned server loop. For pure primitive consumers, document whether they are responsible for applying their own limit or whether a `StreamLimiter` helper exists.

### 5. Python binding design

Expose Python-safe body primitives without exposing raw file paths.

Candidate Python API:

```python
resource = root.resolve_path('/asset.bin')
file = resource.file
plan = file.plan_response('GET', headers={'range': 'bytes=0-99'})
body = file.body_for_plan(plan)
```

The resulting `body` should be an opaque Rust-backed object. It may expose metadata such as:

- `kind`: `empty`, `bytes`, `file_full`, `file_range`.
- `length`.
- `range`.

For initial Python support, provide controlled read methods suitable for tests and downstream integration:

- `read_all(max_bytes=None)` for small files/tests with size guard.
- `read_range(start, end_inclusive)` if range-specific object is not enough.

Avoid creating APIs that encourage production servers to pull large files fully into Python memory. For production server use, the later Python server primitive should pass the body object back to Rust for streaming.

### 6. Python tests

Add tests for:

- Resolved file can create a full body source.
- Resolved file can create a range body source from a valid range response plan.
- Invalid range body creation fails.
- Body source does not expose an absolute filesystem path.
- Reading small full body returns expected bytes.
- Reading small range body returns expected bytes.
- Dotfile/symlink denied resources cannot produce body sources.
- HEAD response plan produces empty body behavior.

### 7. Rust tests

Add tests for:

- Full file body source streams exact bytes.
- Range body source streams exact bytes.
- Empty body source produces zero bytes.
- Bytes body source produces exact bytes.
- Range bounds are checked.
- Consuming conversion prevents accidental double-use if using consuming API.
- No path reopening is used in safe-default static path. This can be a structural/code-review invariant if hard to test directly.

## Error handling

Add a structured error type if needed:

- `InvalidRange`.
- `PlanBodyMismatch`.
- `BodyAlreadyConsumed` if applicable.
- `Io`.
- `UnsupportedBodyKind`.

Python should map these to an eggserve-specific exception, not generic `RuntimeError` where avoidable.

## Documentation changes

Update:

- `docs/python-api.md` to replace the current limitation with the new safe body-source API.
- `docs/secure-root.md` to explain file capabilities and streaming.
- `architecture/response-planning.md` to include body-source conversion.
- `docs/extension-contract.md` to instruct downstream adapters to return/pass body sources rather than reopen paths.
- `docs/invariants.md` to add body-source invariants.

## Acceptance criteria

- Rust exposes a body-source abstraction for empty, bytes, full file, and file range bodies.
- A resolved file can be converted to a body source without path reopening.
- Python can obtain an opaque body source from a resolved file and response plan.
- Python tests can read small body sources for verification without making full-memory reads the production path.
- CLI behavior remains unchanged or is refactored through the new abstraction with equivalent tests.
- File stream limits remain enforceable in Rust-owned server paths.
- Documentation no longer says Python lacks any safe way to use resolver-opened file content; it instead explains the body-source model.
- No ASGI/WSGI/framework code is added.

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

Packaging smoke:

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

## Handoff notes

The main failure mode to avoid is accidentally weakening the filesystem guarantee for ergonomic reasons. If an API needs bytes from a resolved file, carry the opened file capability forward. Do not reconstruct paths. Do not expose a convenience that tells users to call Python `open()` on `safe_relative_components`.
