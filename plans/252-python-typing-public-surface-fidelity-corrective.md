# Plan 252 — Python typing and public-surface fidelity corrective

## Purpose

Correct concrete drift between EggServe's shipped Python type information and
the already-implemented Python runtime surface without changing runtime
behavior, import paths, signatures, compatibility semantics, or capability.

Planning baseline:

```text
0ee02acd69f1c63d32134f8265283fff04e4630c
```

The baseline is green in CI run `35620987177`. The defect is therefore in the
developer-facing type description and its qualification coverage, not in the
known-good runtime contract.

This plan is the first implementation step of Plans 251–256.

## Constraints

- Runtime behavior is authoritative where a stub disagrees with the existing
  documented/tested implementation.
- Do not change a runtime property type merely to make an existing stub true.
- Do not remove, rename, or re-home public Python classes, methods, properties,
  or imports.
- Do not make `eggserve._native` a supported public import.
- Preserve `py.typed`, abi3-py311 packaging, CPython 3.11+ support, and the
  existing wheel layout.
- Python remains H1-only.
- No new production dependency is authorized.
- Internal annotation cleanups are allowed when they do not change behavior.

## Confirmed baseline mismatches

The current review identified at least the following mismatches in
`eggserve.lowlevel.AsyncRequest`.

### Header view

Runtime:

```python
@property
def headers(self):
    return self._req.headers
```

The native `Request.headers` getter is the compatibility first-wins
`dict[str, str]` view. Duplicate-preserving access remains available through
`header_items` / `header_items_bytes`.

Current `lowlevel.pyi` incorrectly declares `headers: HeaderBlock`.

The stub must describe the existing dictionary view. Do not change runtime
headers to return a `HeaderBlock`.

### String address versus parsed tuple address

Runtime intentionally exposes both forms:

- `remote_addr` / `local_addr` / `effective_addr`: compatibility text
  representation or `None`;
- `remote_address` / `local_address` / `effective_address`: parsed
  `tuple[str, int] | None`.

The current async stub conflates the `*_addr` properties with the tuple form.

### Proxy source/destination

The native request stores and exposes `proxy_source` and
`proxy_destination` as optional text values. The current async stub declares
tuple addresses.

Match the runtime representation.

### Query representation

`AsyncRequest.query` forwards the native compatibility getter. The native
getter canonicalizes an absent query to the existing text representation.
Reconcile the stub with the actual supported runtime behavior and existing
tests; do not introduce a new optional runtime result in this plan.

### `http.server` compatibility hooks

The implementation exposes public compatibility hooks not fully represented in
`server.pyi`, including at least:

- `BaseHTTPRequestHandler.log_request`;
- `BaseHTTPRequestHandler.log_error`;
- `BaseHTTPRequestHandler.log_message`;
- `HTTPServer.server_bind`;
- `HTTPServer.server_activate`.

These are ordinary subclass/compatibility hooks and should be typed if they are
part of the implemented supported facade.

The implementation pass must inventory the whole supported surface rather than
stopping after these known examples.

## Track A — build a runtime/stub surface inventory

Before editing stubs, generate or maintain a reviewable inventory for:

- `eggserve.__init__` exports;
- `eggserve.server` public classes and their public methods/properties;
- `eggserve.lowlevel` `__all__` plus public classes/functions;
- `eggserve.subprocess` public helpers;
- supported native-backed object attributes reachable through those modules.

Compare:

1. Python implementation definitions;
2. native PyO3 getters/methods where wrappers forward directly;
3. `.pyi` declarations;
4. current public API tests/documentation.

Private underscore-prefixed helpers do not need to appear in public stubs
unless required to describe an existing supported protocol.

The inventory may be test code rather than a generated repository artifact if
that keeps maintenance smaller.

## Track B — correct low-level stub types

Update `crates/eggserve-python/python/eggserve/lowlevel.pyi` to describe the
existing runtime faithfully.

At minimum cover:

- `AsyncRequest.headers`;
- all `*_addr` versus `*_address` pairs;
- `proxy_source` / `proxy_destination`;
- query representation;
- lifecycle methods;
- tunnel capability/return types;
- async response construction return types;
- `AsyncBody.trailers` representation;
- `AsyncServer.track` task typing.

Review the synchronous `Request` re-export from `_native.pyi` at the same
time so low-level aliases do not accidentally contradict the native stub.

Do not broaden return types to `Any` merely to make mypy pass. Use the most
specific truthful type that matches the runtime.

## Track C — complete supported `server.pyi` compatibility hooks

Audit `server.py` against `server.pyi` for public names.

Add declarations for supported public subclass/override points that are
implemented and intentionally part of the `http.server`-shaped facade.

At minimum qualify:

- logging hooks listed above;
- bind/activate hooks listed above;
- inherited public lifecycle methods;
- public handler attributes used by subclasses;
- `SimpleHTTPRequestHandler` public customization attributes and methods.

Do not type private helper methods such as `_dispatch`,
`_static_response`, or `_publish_native_address` merely because they exist.

The goal is supported-surface fidelity, not a line-for-line stub of every
implementation helper.

## Track D — strengthen strict typing fixtures

The current `tests/typing_smoke.py` mostly constructs representative objects.
Expand it so the known drift classes become compile-time regressions.

Use representative code that:

- accesses each `AsyncRequest` address form and assigns it to the expected
  text/tuple type;
- consumes `headers` as a dictionary and `header_items` as the
  duplicate-preserving sequence;
- accesses trusted-proxy metadata;
- checks query/body/lifecycle/tunnel types;
- subclasses `BaseHTTPRequestHandler` and overrides
  `log_request`/`log_error`/`log_message`;
- subclasses `HTTPServer` or a small test subclass and overrides
  `server_bind`/`server_activate`;
- exercises sync/async response constructors and stream return values.

Prefer `typing.assert_type` / ordinary typed assignments over checker-specific
comments where supported by the repository's Python baseline.

The fixture must run against the built and installed wheel, not only the source
tree.

## Track E — add runtime shape assertions where static typing cannot prove truth

Add narrow runtime tests for properties whose shape is central to the stub:

- no-query and query request values;
- string versus tuple address forms;
- accepted PROXY metadata representations;
- duplicate header dictionary versus ordered header-item views.

Use existing request fixtures/server harnesses. Do not add another server
implementation to test the bridge.

The point is to make the runtime shape and the static declaration fail
together if either later drifts.

## Track F — package composition

Verify the wheel includes exactly the intended typed artifacts:

- `eggserve/py.typed`;
- `eggserve/__init__.pyi`;
- `eggserve/_native.pyi`;
- `eggserve/lowlevel.pyi`;
- `eggserve/server.pyi`;
- `eggserve/subprocess.pyi`.

Retain the existing private status of `_native`.

If reasonable, strengthen `scripts/check-python-release-metadata.py` or the
wheel smoke test so a missing typed artifact fails before release.

## Track G — documentation reconciliation

Update only docs that describe types or supported subclass hooks incorrectly.

Likely surfaces:

- `docs/python-api.md`;
- `docs/python-http-server-compatibility.md`;
- examples if a typed example currently contradicts the corrected stub.

Do not turn typing corrections into a support-tier promotion. The async Python
surface remains experimental/H1-only as currently documented.

## Required qualification

At minimum run:

```sh
python3 scripts/check-python-release-metadata.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
```

Also run the repository's strict mypy/type fixture against the installed wheel
and the focused Python public-API/low-level/async suites.

If production Rust/Python runtime code is touched for an annotation-only
cleanup, run the normal Rust format/check/clippy/tests appropriate to that
change.

## Acceptance criteria

- [ ] `AsyncRequest.headers` stub matches the runtime dictionary view.
- [ ] `*_addr` text properties and `*_address` tuple properties are typed
      distinctly and correctly.
- [ ] proxy source/destination and query types match runtime behavior.
- [ ] supported `BaseHTTPRequestHandler` logging hooks are present in
      `server.pyi`.
- [ ] supported `HTTPServer.server_bind` / `server_activate` hooks are
      present in `server.pyi`.
- [ ] a whole-surface review finds no other material supported-public
      implementation/stub mismatch.
- [ ] strict installed-wheel typing exercises property access and subclass
      overrides, not only construction.
- [ ] runtime shape tests pin the important representations.
- [ ] typed wheel composition remains correct.
- [ ] no runtime API, behavior, capability, or support tier changes.
- [ ] focused Python tests and routine structural checks are green.

## Rollback / stop conditions

If a stub appears to disagree with ambiguous runtime behavior, stop and resolve
the existing documented/tested contract before changing either side.

If correcting a type would require changing a public runtime representation,
that is outside this plan. Record the mismatch and open a separate API-change
plan rather than changing behavior here.
