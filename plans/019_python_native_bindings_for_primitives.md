# Plan 019: Python native bindings for hardened primitives

## Status

Complete.

## Objective

Extend the Python package beyond subprocess lifecycle management. Add a PyO3-backed primitive module that lets Python callers parse request targets, apply path policy, resolve resources under a secure root, inspect safe metadata, and consume static response plans without launching the eggserve binary.

This plan should not replace the existing `serve_directory()` or `ServerProcess` APIs. It should add native primitives alongside them.

## Scope

In scope:

- PyO3 bindings for path/policy primitives.
- PyO3 bindings for `SecureRoot` and resolved resources.
- PyO3 bindings for request validation and response planning.
- Python exception taxonomy mirroring structured Rust errors.
- Python docs and tests for native primitives.
- Packaging changes required to build the native extension reliably.

Out of scope:

- ASGI adapter.
- WSGI adapter.
- Python callback server.
- Router abstraction.
- Middleware system.
- Dynamic application lifecycle runtime.
- Replacing Rust core logic with Python implementations.

## Current state

The current Python package is pure Python and wraps the Rust binary. `ServeConfig`, `StaticPolicy`, `ServerProcess`, and `serve_directory()` translate config into CLI arguments and manage a subprocess. This should remain available and stable.

The package already has a maturin configuration and Python tests. The workspace currently excludes `crates/eggserve-python`, so binding and packaging validation must account for that layout.

## Design constraints

The Python API must not expose unsafe direct constructors for capability objects. Python code should not be able to create `ResolvedFile` from an arbitrary path or file descriptor. It should receive `ResolvedFile` only from `SecureRoot.resolve()`.

The Python API should feel Pythonic, but not at the cost of hiding security-sensitive states. Denied, malformed, and not-found outcomes should be distinguishable.

Avoid inheritance-heavy designs. Prefer composition and value objects.

## Proposed Python surface

Final naming should be checked against the Rust primitive names from Plans 016-018, but a target shape is:

```python
from eggserve import (
    PathPolicy,
    StaticPolicy,
    RequestTarget,
    SecureRoot,
    ResolvedResource,
    RequestValidation,
)

root = SecureRoot("public", policy=StaticPolicy())
target = RequestTarget.parse("/assets/app.css", policy=PathPolicy())
resource = root.resolve(target)

if resource.kind == "file":
    plan = resource.file.plan_response(method="GET", headers={})
    print(plan.status)
    print(plan.headers)
```

Alternative convenience shape:

```python
resource = root.resolve_path("/assets/app.css")
```

This convenience method is acceptable only if it internally uses the same `RequestTarget`/`PathPolicy` pipeline and does not bypass validation.

## Implementation steps

### 1. Introduce native module layout

Add a PyO3 extension module, likely `_native`, under `crates/eggserve-python`.

Recommended layout:

- keep `python/eggserve/server.py` for subprocess lifecycle;
- add `python/eggserve/primitives.py` as a Python-facing wrapper/re-export layer if useful;
- expose Rust module as `eggserve._native`;
- re-export public classes from `eggserve.__init__` only after stable enough.

Avoid making application code import PyO3 objects from deeply internal paths.

### 2. Bind policy types

Expose:

- `StaticPolicy(directory_listing=False, follow_symlinks=False, allow_dotfiles=False)`
- `PathPolicy(allow_dotfiles=False, reject_backslash=True)` or equivalent

Keep defaults safe and identical to Rust/CLI defaults.

Validate constructor arguments strictly. Reject non-bool values if ambiguity could hide bugs.

### 3. Bind request-target parsing

Expose `RequestTarget.parse(raw: str, policy: PathPolicy | None = None) -> RequestTarget`.

`RequestTarget` should expose safe inspection:

- `raw_path` or `decoded_path` if useful;
- `components` as a tuple of strings;
- `__repr__` that does not include local filesystem paths.

Errors should be structured Python exceptions:

- `EggserveError` base class.
- `RequestTargetError` for malformed/unsupported targets.
- optionally subclasses or `.code` fields: `unsupported_uri_form`, `traversal_denied`, `dotfile_denied`, `separator_ambiguity`, etc.

Favor `.code` plus clear messages over too many exception subclasses if the taxonomy is large.

### 4. Bind `SecureRoot`

Expose:

```python
root = SecureRoot(path: str | os.PathLike, policy: StaticPolicy | None = None)
```

Methods:

- `resolve(target: RequestTarget) -> ResolvedResource`
- `resolve_path(raw_path: str, path_policy: PathPolicy | None = None) -> ResolvedResource`
- maybe `policy` read-only property

Constructor should raise a structured exception for missing/non-directory root or permission errors.

Do not expose internal root file descriptors.

### 5. Bind resolved resources

Expose `ResolvedResource` as a Python object with:

- `kind`: `'file'`, `'directory'`, `'not_found'`, `'denied'`
- `is_file`, `is_directory`, `is_not_found`, `is_denied`
- `file`, `directory`, or `denied_reason` accessors that raise if the kind does not match

Expose `ResolvedFile` with:

- `length`
- `modified` as a Python datetime or timestamp; choose one and document it
- `content_type`
- `safe_relative_components` as tuple
- `plan_response(method='GET', headers=None)` after Plan 018 lands

Expose `ResolvedDirectory` with:

- `safe_relative_components`
- `list()` returning entries after policy filtering
- `resolve_child(name: str)` if safely supported by the Rust API
- `plan_listing_response(method='GET')` after Plan 018 lands

No public Python constructor should exist for `ResolvedFile` or `ResolvedDirectory`.

### 6. Bind request validation and response plans

Expose pure helpers:

- `validate_static_request(method: str, target: str, headers: Mapping[str, str] | Sequence[tuple[str, str]], ...)`
- or lower-level `RequestValidation.validate(...)`

Expose `StaticResponsePlan` with:

- `status` integer;
- `headers` as tuple/list of `(name, value)` pairs preserving deterministic order;
- `body_kind`: `'empty'`, `'file_full'`, `'file_range'`, `'bytes'`;
- range metadata when applicable.

Do not expose Hyper types. Do not attempt to make the plan itself write to a Python socket in this plan unless it is trivial and safe. Let examples show how a caller maps the plan into a server implementation.

### 7. Keep subprocess API compatible

`serve_directory`, `ServeConfig`, `ServerProcess`, and current `StaticPolicy` naming must not break unexpectedly.

There may be naming conflict between current pure-Python `StaticPolicy` and new native `StaticPolicy`. Resolve carefully:

Option A: keep pure-Python `StaticPolicy` as config-only and introduce `PrimitiveStaticPolicy`.

Option B: replace pure-Python `StaticPolicy` with a wrapper that can also translate to CLI args.

Recommended first pass: avoid breaking imports. Keep existing `StaticPolicy` behavior or create a compatibility wrapper. Native internals can live in `_native.NativeStaticPolicy` until the public Python shape is finalized.

### 8. Packaging and build updates

Update `pyproject.toml`, maturin configuration, and package data as needed.

Ensure the wheel includes:

- the native extension;
- the eggserve binary if that is still the package model;
- Python wrapper modules.

If including both binary and extension complicates the build, document the chosen packaging model in `docs/python-api.md` and add a follow-up note. Do not silently drop the subprocess CLI wrapper.

### 9. Documentation

Update `docs/python-api.md` with separate sections:

- Blocking static server API.
- Subprocess lifecycle API.
- Native primitive API.
- Non-goals.
- Error handling.
- Packaging notes.

Add examples but avoid framework language.

## Tests

Required Python tests:

- `RequestTarget.parse('/foo')` succeeds.
- absolute-form target rejected.
- traversal rejected.
- percent-encoded traversal rejected.
- NUL rejected.
- backslash rejected by default.
- dotfile rejected by default.
- dotfile allowed only when policy permits.
- `SecureRoot` resolves normal file.
- `SecureRoot` resolves directory.
- `SecureRoot` returns not-found distinctly.
- `SecureRoot` returns denied distinctly for dotfile.
- Unix symlink denial through Python binding.
- follow-symlinks inside-root allowed and outside-root denied on Unix.
- `ResolvedFile` cannot be directly constructed from Python.
- `ResolvedDirectory` cannot be directly constructed from Python.
- response plan returns expected status and headers for GET.
- response plan returns empty body plan for HEAD.
- conditional and range tests mirror Rust tests from Plan 018.
- existing `ServerProcess` tests still pass.

Required Rust/PyO3 tests:

- conversion of Rust errors to Python exceptions preserves `.code` or equivalent.
- Python wrapper cannot mutate policy objects after construction if immutability is intended.
- reference cycles or leaked process handles are not introduced.

## Validation

Run Rust validation:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
```

Run Python validation:

```sh
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import ServeConfig, ServerProcess, serve_directory
print(ServeConfig())
PY
```

After native tests are added, include:

```sh
python -m unittest eggserve.test_primitives -v
```

If available:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This plan is complete when:

- The Python package exposes native hardened primitives without launching the server binary.
- Existing subprocess APIs remain compatible.
- Python capability objects cannot be directly constructed unsafely.
- Python tests mirror Rust security invariant tests.
- Response plans are consumable from Python without Hyper types.
- Documentation clearly separates subprocess server API from primitive API and repeats non-goals.
