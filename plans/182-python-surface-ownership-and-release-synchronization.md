# Plan 182 — Python Surface Ownership and Release Synchronization

## Status

**PLANNED — Python maintenance consolidation; no Python API expansion.**

Prerequisites: Plan 179 closed before changing the low-level runtime configuration projection. This plan may otherwise proceed independently of Plans 180–181 provided it does not duplicate their runtime-internal work.

## Purpose

Reduce unnecessary coupling inside the Python distribution while preserving the documented six-class `http.server` compatibility facade, the separate `eggserve.lowlevel` embedding surface, and existing subprocess convenience imports.

The current Python package has three maintenance issues:

1. `eggserve.subprocess` is nominally the subprocess convenience module but currently only re-exports implementation owned by the already-large `eggserve.server` compatibility module;
2. `eggserve.lowlevel.RuntimeConfig` manually repeats many runtime defaults and `lowlevel.Server` manually projects each field into `_NativeServer`, making every runtime configuration change a multi-site edit;
3. the Maturin Rust crate is intentionally excluded from the root workspace, so its package version and distribution profile are separately declared and can drift from the root workspace/release configuration.

This plan improves ownership and synchronization only. It must not add async Python handlers, raw socket APIs, socketserver implementation identity, ASGI/WSGI behavior, routing, middleware, or new client features.

## Current-state findings

### 1. Subprocess implementation lives in the wrong module

`crates/eggserve-python/python/eggserve/subprocess.py` currently imports and re-exports `ServeConfig`, `ServerProcess`, `StaticPolicy`, and `serve_directory` from `eggserve.server`.

The actual implementation of those types/functions, along with `_parse_bind` and `_config_to_argv`, lives in `server.py`. That module also owns the six compatibility classes and their request-handler machinery.

The result is inverted ownership: the specialized subprocess module depends on the broad compatibility module rather than owning the functionality named by its module.

### 2. Low-level runtime projection is hand-maintained

`eggserve.lowlevel.RuntimeConfig` is a frozen Python dataclass that intentionally exposes only operator-meaningful controls. `lowlevel.Server.__init__()` then spells out the complete mapping from those dataclass fields into the native server constructor.

The boundary itself is appropriate; the repeated projection is the problem. It increases the chance that a new/renamed runtime field gets a Python default but is not forwarded, or is forwarded under inconsistent conversion semantics.

Plan 179 establishes the Rust-side source of truth. This plan should make Python projection similarly singular without trying to make Python import Rust configuration types directly.

### 3. Excluded Python crate has duplicate release metadata

The root workspace owns the EggServe version and distribution profile. `crates/eggserve-python/Cargo.toml` is intentionally excluded so Maturin can build it independently, and therefore declares its own package version plus an explicit `[profile.dist]` equivalent to the workspace profile.

CI checks the excluded crate with its own manifest, but there is no cheap invariant ensuring these release-critical declarations remain synchronized.

Do not solve this by forcing the Maturin crate into the workspace if that complicates packaging. A narrow synchronization check is preferable.

## Design constraints

- Preserve the public six compatibility classes in `eggserve.server`.
- Preserve public `serve_directory` behavior and currently documented import paths.
- Preserve `eggserve.subprocess` as the explicit home for subprocess convenience APIs.
- Preserve `eggserve.lowlevel` as the handler/runtime embedding surface.
- Preserve synchronous bounded callback semantics; do not add coroutine handlers.
- Preserve the excluded-crate Maturin build architecture unless independent evidence shows it is no longer required.
- Do not introduce a Python configuration framework, code generator, Pydantic dependency, or dynamic schema layer.
- Do not add another release system; add only a small conformance assertion to existing verification.

## Track A — Move subprocess ownership into `eggserve.subprocess`

### A1. Move implementation, not behavior

Move the following implementation responsibilities from `server.py` into `subprocess.py`:

- subprocess/static convenience `StaticPolicy` if it is not the compatibility handler's authoritative policy type;
- `ServeConfig`;
- `_parse_bind`;
- `_config_to_argv`;
- `ServerProcess`;
- `serve_directory`.

During Phase 0, verify whether `server.py`'s compatibility classes directly depend on the same `StaticPolicy` definition. If they do, choose one neutral/shared small module rather than creating a circular dependency. Do not duplicate the class.

### A2. Preserve compatibility aliases

Existing user-visible import paths must continue to work where documented or already exported:

- `eggserve.serve_directory`;
- `eggserve.subprocess.serve_directory`;
- any supported `eggserve.server.serve_directory` import retained by compatibility policy.

Prefer re-exporting from the true owner rather than maintaining two implementations.

Update `eggserve.__init__.py` to import the function from its canonical module while preserving the top-level name.

### A3. Avoid circular imports

`subprocess.py` must not require importing the whole compatibility `server.py` merely to build its configuration/process objects after the move.

If `server.py` needs compatibility aliases, import/re-export them in the direction `server -> subprocess`, preferably near the module boundary so ownership is obvious.

## Track B — Centralize low-level Python runtime projection

### B1. One projection helper

Give `lowlevel.RuntimeConfig` or a nearby private bridge helper one canonical method that produces the validated keyword arguments expected by `_NativeServer`, for example an internal `_native_kwargs()`/conversion function.

`lowlevel.Server.__init__()` should consume that projection instead of listing every field independently.

Do not serialize through dictionaries/JSON across the FFI boundary if ordinary keyword construction is sufficient; this is source consolidation, not a new configuration protocol.

### B2. Preserve Python-specific validation

Retain early Python errors for Python-domain constraints such as enum-like strings and `None`/positive max-request semantics. Rust remains the final authority for runtime limits after Plan 179.

Avoid copying detailed Rust range validation into Python solely to fail earlier. Duplicated validation is exactly the maintenance pattern Plan 179 removes on the Rust side.

### B3. Projection completeness test

Add a test that constructs a non-default low-level config across the supported fields and proves each field reaches the native configuration path correctly.

Where direct introspection of native config is unavailable, use a narrow test hook or observable behavior only if it is deterministic and does not enlarge the public API. Do not test by parsing implementation `repr` strings.

## Track C — Add release metadata synchronization checks

### C1. Inventory release-critical duplicated metadata

At minimum compare:

- root `[workspace.package].version`;
- `crates/eggserve-python/Cargo.toml` package version;
- root `[profile.dist]` values;
- excluded Python crate `[profile.dist]` values.

Also inspect the active `pyproject.toml`/Maturin metadata and release scripts for any independently declared version or build-profile invariant. Add checks only for values that are actually required to remain equal.

### C2. Small deterministic verification script

Add or extend a repository verification script that reads TOML using Python's standard-library `tomllib` and fails with a concise mismatch message.

Do not add a TOML parser dependency or release orchestration framework.

A suitable scope is a script such as `scripts/verify-release-metadata.py`, or integration into an existing conformance verifier if that keeps responsibilities clearer.

### C3. Wire into existing CI/release checks

Run the synchronization assertion in the existing ordinary Rust/Python CI path or an existing release validation script. Do not create a separate workflow/job solely for it.

The check should execute before expensive wheel builds so drift fails cheaply.

## Track D — Import/API compatibility tests

Add tests covering the supported import graph after ownership changes:

- top-level `eggserve` exports the documented six classes and `serve_directory`;
- `eggserve.subprocess` exports its convenience API;
- `eggserve.server` compatibility imports remain available according to the documented contract;
- importing `eggserve.subprocess` does not require circular initialization through `eggserve.server`;
- `eggserve.lowlevel` remains independently importable.

Do not expand `__all__` merely to expose private migration helpers.

## Track E — Python/wheel behavior regression

Exercise:

- subprocess configuration validation;
- public-bind acknowledgement;
- subprocess startup/shutdown smoke path;
- compatibility `HTTPServer`/`ThreadingHTTPServer` classes;
- TLS compatibility classes where already covered;
- low-level handler server;
- request body modes and streaming response bridge;
- wheel import and bundled CLI smoke tests.

The source move must not change the native server used by these surfaces.

## Track F — Documentation cleanup

Update current Python API documentation to make ownership explicit:

- `eggserve.server` = six-class stdlib-shaped compatibility facade;
- `eggserve.lowlevel` = bounded low-level handler/runtime embedding surface;
- `eggserve.subprocess` = optional subprocess/CLI convenience API;
- top-level `serve_directory` remains a convenience re-export.

Avoid presenting subprocess helpers as the preferred in-process embedding API.

If the metadata synchronization script changes release instructions, update `docs/release-process.md` or the current equivalent with the new cheap preflight check.

## Verification

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
python3 scripts/verify-conformance-matrix.py
```

Run the new release-metadata synchronization verifier explicitly and include it in existing CI/release verification.

If the Python test suite has a direct invocation separate from the wheel script, run it as well.

## Acceptance criteria

- [ ] subprocess convenience implementation is owned by `eggserve.subprocess` or a narrowly justified neutral helper module rather than `server.py`.
- [ ] `server.py` is focused on the six-class compatibility facade and only retains compatibility re-exports of subprocess helpers where required.
- [ ] top-level and documented subprocess import paths remain compatible.
- [ ] there is no circular import between `server` and `subprocess`.
- [ ] low-level runtime configuration has one Python-to-native projection path.
- [ ] Python does not duplicate detailed Rust runtime-limit validation introduced by Plan 179.
- [ ] non-default low-level configuration projection is covered by tests.
- [ ] workspace and excluded Python crate versions cannot silently drift.
- [ ] required distribution-profile invariants cannot silently drift.
- [ ] metadata verification uses existing/standard tooling and does not create a new release framework or CI job.
- [ ] wheel, CLI, compatibility server, low-level server, and TLS tests remain green.
- [ ] no async handler, raw socket, ASGI/WSGI, routing, middleware, client, or other Python API expansion is introduced.

## Suggested implementation order

1. Inventory Python import dependencies and duplicated release metadata.
2. Move subprocess implementation ownership and preserve aliases/import tests.
3. Introduce one low-level config projection helper after Plan 179's Rust authority is available.
4. Add the TOML metadata synchronization verifier and wire it into existing cheap CI/release validation.
5. Run direct Python and wheel smoke/regression suites.
6. Update Python/release documentation and add a closure record stating the canonical owner of each surface.

## Handoff

After closure, Python maintenance should have three clearly owned surfaces—compatibility, low-level embedding, and subprocess convenience—with release metadata guarded against drift. Further Python capability expansion remains governed by `docs/non-goals.md` and requires a separate product decision.