# Plan 021: Correctness and policy-preservation pass

## Status

Planned. This is a focused corrective pass after the initial public primitive, response-planning, PyO3 binding, examples, and invariant-test implementation work.

## Objective

Tighten the primitive API so policy decisions and capability invariants survive across Rust and Python boundaries. The previous implementation pass moved the project in the right architectural direction, but several details need correction before the primitive track can be treated as closed.

This pass must fix:

- Rust capability forgery through public constructors.
- Python path-policy loss between `RequestTarget.parse()` and `SecureRoot.resolve()`.
- Python `resolve_path(..., path_policy=...)` accepting but ignoring its `path_policy` argument.
- Python directory methods losing the originating `StaticPolicy` and falling back to safe defaults.
- Python resolved-file objects discarding the resolver-opened file capability while docs/examples may imply safe streaming.
- Response-planning metadata gaps for range and unsatisfiable-range responses.
- Documentation overclaims or ambiguous wording around what the primitive layer can currently do.

## Non-goals

Do not add ASGI or WSGI adapters.

Do not add a router, middleware layer, template engine, or dynamic application framework.

Do not attempt a broad HTTP compliance rewrite beyond the specific planner correctness gaps below.

Do not replace the existing CLI/static service path unless needed for a localized correctness fix.

Do not implement Windows reparse-point hardening in this pass. Continue documenting Windows as weaker for untrusted mutable roots.

## Current risk summary

### 1. Rust `ResolvedFile::from_parts` can forge a capability

`ResolvedFile::from_parts(file, metadata, safe_relative_components)` is public. This undermines the capability-oriented public API because external Rust code can construct a `ResolvedFile` without going through `SecureRoot` resolution. The type then no longer proves that the file was resolved under root policy.

### 2. Python `RequestTarget` does not retain the validated confined path

The PyO3 `RequestTarget` stores `decoded` and `components`, but not the validated `ConfinedPath` or the effective path policy. `SecureRoot.resolve()` reparses the decoded string using `PathPolicy::default()`. A target parsed with `PathPolicy(allow_dotfiles=True)` can be rejected later even when the static policy permits dotfiles.

### 3. Python `SecureRoot.resolve_path(..., path_policy=...)` ignores `path_policy`

The method signature accepts `path_policy`, but the implementation delegates to `inner.resolve_uri(raw_path)`, which derives path behavior from `StaticPolicy` and ignores the provided Python path policy argument.

### 4. Python `ResolvedDirectory` loses static policy

`PyResolvedDirectory.list()` and `resolve_child()` reconstruct a new `RustSecureRoot` using `RustStaticPolicy::safe_default()` instead of preserving the original root policy. This causes directory methods to behave differently from the resolution that produced the directory.

### 5. Python `ResolvedFile` cannot stream the resolver-opened capability

The Python binding currently extracts metadata and drops the resolved file handle. That may be acceptable for metadata and response planning, but examples and docs must not imply that Python can safely stream the already-opened descriptor unless the implementation preserves that capability.

### 6. Range response planning is metadata-light

The range planner returns `206` with `Content-Length`, `Content-Range`, and `nosniff`, but omits useful validators and `Content-Type`. `416` returns `Content-Range` only. This is not catastrophic, but it is below the quality bar for an HTTP-correct static response planner.

## Implementation steps

### Step 1: Close Rust capability-forging surface

Inspect `crates/eggserve-core/src/primitives/secure_root.rs`.

Change `ResolvedFile::from_parts` from public API to one of:

- `pub(crate) fn from_parts(...)`, if only internal code needs it;
- `#[cfg(test)] fn from_parts_for_test(...)`, if only tests need it;
- remove it entirely if unused.

Do not leave a public constructor that allows arbitrary `std::fs::File` and caller-supplied `safe_relative_components` to become a `ResolvedFile`.

Acceptance criteria:

- External Rust callers cannot construct `ResolvedFile` directly from arbitrary parts.
- `ResolvedFile` is obtainable only from `SecureRoot`/resolver flow or internal trusted conversion.
- Rust tests still pass.
- Rustdoc no longer implies direct construction is supported.

Optional but recommended:

- Add a doc comment to `ResolvedFile` stating it is a resolver-created capability.
- Add a compile-fail style note in docs, even if not a trybuild test, showing that direct construction is intentionally unavailable.

### Step 2: Preserve Python request-target validation state

Refactor the PyO3 `PyRequestTarget` object in `crates/eggserve-python/src/lib.rs`.

Preferred design:

- Store the validated `ConfinedPath` directly in `PyRequestTarget` if the type is cloneable or can be made cloneable.
- If `ConfinedPath` is not cloneable enough for PyO3 ergonomics, store validated components and reconstruct only through a dedicated Rust primitive constructor that does not redo policy checks incorrectly.

Recommended core change if needed:

- Add a safe Rust constructor such as `ConfinedPath::from_validated_components_for_internal(...)` only if it remains crate-private and cannot be called by external users.
- Avoid public unchecked constructors.

Python behavior target:

```python
pp = PathPolicy(allow_dotfiles=True)
sp = StaticPolicy(allow_dotfiles=True)
target = RequestTarget.parse("/.env", pp)
root = SecureRoot(tmpdir, sp)
res = root.resolve(target)
assert res.is_file
```

This should pass because the path policy and static policy both permit the dotfile.

Acceptance criteria:

- `SecureRoot.resolve(target)` does not reparse with `PathPolicy::default()`.
- A `RequestTarget` parsed under a custom `PathPolicy` carries the effective validated target into resolution.
- The binding does not introduce an unchecked public Python constructor for `RequestTarget`.
- Tests cover dotfile allow/deny behavior through both `RequestTarget.parse()` + `SecureRoot.resolve()` and `SecureRoot.resolve_path()`.

### Step 3: Make `resolve_path(..., path_policy=...)` honest

Fix `PySecureRoot.resolve_path(raw_path, path_policy=None)`.

Two acceptable options:

Option A, preferred: honor the provided `path_policy`.

- If `path_policy` is `None`, derive the path policy from the root's `StaticPolicy` exactly as the Rust `resolve_uri()` convenience method currently does.
- If `path_policy` is provided, parse with that policy and resolve the resulting confined path.

Option B: remove the `path_policy` parameter from Python if the API should intentionally derive path parsing from static policy.

Option A is better because it matches the plan and gives callers explicit control.

Acceptance criteria:

- Passing `PathPolicy(allow_dotfiles=True)` to `resolve_path()` changes parsing behavior as expected.
- Passing `PathPolicy(reject_backslash=False)` is honored or explicitly rejected/documented; do not silently ignore it.
- Tests fail against the old implementation and pass after the fix.

### Step 4: Preserve `StaticPolicy` in Python resolved directories

Refactor `PyResolvedDirectory` so that `list()` and `resolve_child()` use the same static policy as the `SecureRoot` that produced the directory.

Recommended design:

- Store `root_path` and `static_policy` inside `PyResolvedDirectory`.
- Populate both from `PySecureRoot` in `PyResolvedResource::from_rust(...)`.
- Reconstruct `RustSecureRoot::new(&root_path, static_policy.clone())` inside directory methods.

If `StaticPolicy` clone is not already ergonomic, make it cloneable through existing Rust type behavior.

Tests to add:

- Directory listing under `StaticPolicy(allow_dotfiles=True)` includes dotfiles when policy permits.
- Directory listing under defaults still hides dotfiles.
- `resolve_child(".env")` under `StaticPolicy(allow_dotfiles=True)` and path policy allowing dotfiles can resolve the file.
- `follow_symlinks=True` behavior is preserved for directory child resolution on Unix.

Be careful: directory listing may be controlled separately by `DirectoryListingPolicy`. The primitive `ResolvedDirectory.list()` is a low-level capability and currently lists policy-filtered entries; it is not the same as allowing an HTTP directory listing response. Document this distinction if needed.

### Step 5: Clarify or preserve Python file streaming capability

Decide whether Python `ResolvedFile` should preserve the resolver-opened file capability now.

Option A: Preserve the file handle in Python.

- Store an owned file handle or a safe duplicate in `PyResolvedFile`.
- Expose a controlled method such as `read_bytes(max_bytes=None)` or `open_file()` only if it does not undermine safety.
- Ensure large-file streaming semantics are not implemented poorly in this pass.

Option B, preferred for this corrective pass: keep Python as metadata/response-planning only and document it honestly.

- Do not expose streaming until a separate plan designs streaming semantics, backpressure, file descriptor lifetime, and chunking.
- Update docs/examples to say Python primitives currently produce safe resolution outcomes and response plans, but they do not yet stream the resolver-opened file descriptor.
- Ensure examples do not read files by raw path derived from the request. If an example needs to send bytes, make it explicitly illustrative and state that production streaming should wait for the file-streaming primitive.

Acceptance criteria:

- Docs and examples do not claim Python can serve file bytes via resolver-opened handle if it cannot.
- Any example that maps response plans to Python stdlib server responses avoids raw request-derived path joins.
- If a file path is used in an example for demonstration, it is not derived from untrusted request text outside `SecureRoot`.

### Step 6: Harden response planner headers for range and 416 responses

Update `crates/eggserve-core/src/primitives/planner.rs`.

For `206 Partial Content`, include at least:

- `content-length`
- `content-range`
- `content-type`
- `x-content-type-options: nosniff`
- `etag` when available
- `last-modified` when available
- `accept-ranges: bytes`

This likely requires passing `content_type`, `etag`, and `last_modified` into `build_range_response()`.

For normal `200 OK` file responses, consider adding:

- `accept-ranges: bytes`

For `416 Range Not Satisfiable`, include at least:

- `content-range: bytes */<len>`
- `content-length: 0`
- optionally `accept-ranges: bytes`

Be conservative and test exact behavior.

Acceptance criteria:

- `206` plans include content type and validators.
- `200` plans document whether `accept-ranges` is emitted.
- `416` plans include deterministic zero-body metadata.
- Python response-plan tests mirror the Rust planner expectations for range cases.

### Step 7: Strengthen conditional/range Python tests

Add Python tests for:

- `Range: bytes=0-0` returns status `206`, body kind `file_range`, range `(0, 0)`.
- `Range: bytes=0-` returns `206` and expected range.
- `Range: bytes=-N` returns suffix range.
- Unsatisfiable range returns `416`.
- `If-Range` match returns range.
- `If-Range` mismatch returns full `200`.
- `HEAD` with range returns `206` and `body_kind == "empty"`.
- `206` includes expected metadata headers.

Add tests that specifically protect the policy-preservation fixes:

- `RequestTarget.parse("/.env", PathPolicy(allow_dotfiles=True))` then `SecureRoot(..., StaticPolicy(allow_dotfiles=True)).resolve(target)` resolves file.
- `SecureRoot(..., StaticPolicy(allow_dotfiles=True)).resolve_path("/.env", PathPolicy(allow_dotfiles=True))` resolves file.
- `resolve_path("/.env")` without allowing dotfiles still rejects or denies according to current documented behavior.
- `ResolvedDirectory` methods preserve static policy.

### Step 8: Update docs and examples

Update the following docs as needed:

- `docs/python-api.md`
- `docs/public-api-boundary.md`
- `docs/secure-root.md`
- `docs/http-response-planning.md`
- `docs/extension-contract.md`
- `docs/invariants.md`
- `README.md` if it currently overstates primitive capability.

Required wording corrections:

- `ResolvedFile` is a resolver-created capability in Rust; no public forging constructor.
- Python `ResolvedFile` currently supports metadata and response planning. If no file-handle-preserving streaming is implemented, say so explicitly.
- `validate_request_target()` is a coarse origin-form check, not a replacement for `RequestTarget.parse()` / `ConfinedPath` validation.
- Directory primitive listing is policy-filtered filesystem listing; HTTP directory-listing exposure remains separately controlled by static server policy.
- Follow-symlinks remains weaker than Unix safe-default descriptor-relative symlink-denied mode.

Update examples:

- Ensure no example joins raw request path strings into filesystem paths.
- Ensure examples either do not stream bytes or label byte-serving portions as illustrative until streaming primitive lands.
- Ensure examples use `PathPolicy` explicitly when demonstrating dotfile or unusual path behavior.

## Suggested implementation order

1. Fix Rust `ResolvedFile::from_parts` exposure.
2. Add failing Rust/Python tests for policy preservation.
3. Refactor `PyRequestTarget` and `PySecureRoot.resolve()`.
4. Fix `resolve_path(..., path_policy=...)`.
5. Carry static policy through `PyResolvedDirectory`.
6. Harden planner headers.
7. Add range/conditional Python tests.
8. Update docs/examples.
9. Run full validation.

## Validation commands

Run the standard Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
```

Run Python tests from source:

```sh
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_primitives -v
```

Run packaging smoke if maturin is available:

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import NATIVE_AVAILABLE
assert NATIVE_AVAILABLE
from eggserve import SecureRoot, RequestTarget, PathPolicy, StaticPolicy
print(SecureRoot, RequestTarget, PathPolicy, StaticPolicy)
PY
```

Run supply-chain checks if available:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This pass is complete when:

- Rust `ResolvedFile` cannot be forged through a public constructor.
- Python `RequestTarget` resolution preserves the path-policy decision that produced it.
- Python `resolve_path(..., path_policy=...)` either honors the argument or no longer exposes it.
- Python `ResolvedDirectory` methods preserve the originating `StaticPolicy`.
- Python docs/examples accurately state whether file streaming preserves the resolver-opened capability.
- Range responses include content type, validators where available, and deterministic `416` metadata.
- Rust and Python tests cover the policy-preservation regressions directly.
- The existing CLI/static server behavior remains unchanged except for intentional planner/header improvements if integrated.
