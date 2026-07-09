# Plan 016: Public API boundary and core primitives

## Status

Planned. This plan is the first implementation handoff for the Python-accessible hardened HTTP primitives track.

## Objective

Define and implement the first deliberate public primitive boundary for `eggserve-core` without exposing internal modules wholesale. The goal is to create a small, typed, capability-oriented API surface that downstream Rust and Python bindings can rely on while preserving freedom to change internal filesystem traversal, Hyper integration, and response body implementation details.

This plan should not add ASGI, WSGI, routing, middleware, template rendering, or a Python callback server.

## Current state

`eggserve-core/src/lib.rs` currently marks `config`, `limits`, and `policy` as stable-ish, `service` as experimental, and `fs`, `path`, `response`, `telemetry`, MIME detection, and error taxonomy as internal. That was correct for the initial static-server phase. The next track needs a higher-level public primitive facade so callers can use safe path parsing, policy decisions, resolution, and response planning without depending on private module layout.

The implementation already contains useful internal pieces:

- `path::ConfinedPath`
- `path::PathPolicy`
- `path::PathRejection`
- `fs::RootGuard`
- `fs::ResolvedResource`
- `fs::ResolvedFile`
- `fs::ResolvedDirectory`
- `policy::StaticPolicy`
- `policy::DirectoryListingPolicy`
- `policy::SymlinkPolicy`
- `policy::DotfilePolicy`
- `service::validate_no_request_body`
- response helpers and MIME detection

Do not simply make all of these public. Create a public facade with explicit invariants and minimal dependencies.

## Design constraints

The public API must be capability-oriented. A user should not be guided toward this pattern:

1. Validate URL path.
2. Receive absolute filesystem path.
3. Reopen that path manually.

The preferred pattern is:

1. Parse and validate request target.
2. Resolve under a secure root.
3. Receive a resolved capability object.
4. Ask the capability object for metadata, response planning, or a stream/open handle through controlled APIs.

Raw Hyper body types, raw `std::fs::File` handles, raw fd-relative traversal, `openat`, `statat`, and concrete response body internals should remain internal until there is a specific need to expose them.

## Implementation steps

### 1. Add a public `primitives` module

Create `crates/eggserve-core/src/primitives/mod.rs` and re-export it from `lib.rs` as `pub mod primitives;`.

The initial module should be mostly wrapper types and re-exports, not a large refactor. Use submodules only where helpful:

- `primitives::policy`
- `primitives::target`
- `primitives::root`
- `primitives::resource`
- `primitives::errors`
- later: `primitives::response`

If adding all submodules at once creates churn, start with a flat `primitives/mod.rs` and split later.

### 2. Define the public names and invariants

Introduce or re-export public wrapper names that can remain stable:

- `PathPolicy`
- `StaticPolicy`
- `DirectoryListingPolicy`
- `SymlinkPolicy`
- `DotfilePolicy`
- `RequestTarget`
- `ConfinedPath`
- `ParseTargetError` or `RequestTargetError`
- `ResolveError` or `ResourceDeniedReason`
- placeholders or stubs for `SecureRoot`, `ResolvedResource`, `ResolvedFile`, and `ResolvedDirectory` if they are completed in Plan 017

For each public type, include rustdoc that states:

- how the object may be constructed;
- what security property construction proves;
- what it does not prove;
- whether it is stable-ish, experimental, or internal-adjacent.

Example invariant wording:

`RequestTarget` represents an HTTP origin-form request target that has been syntax-checked and normalized into path-only form. It does not prove the target exists on disk and does not grant filesystem access.

`ConfinedPath` represents validated relative path components derived from a request target under a `PathPolicy`. It rejects traversal and platform-ambiguous components but does not prove the resource exists.

### 3. Avoid leaking private implementation types directly

If existing internal types are used internally, wrap them:

- Public `RequestTarget` may contain an internal `crate::path::ConfinedPath`.
- Public `ConfinedPath` may be a newtype over internal `crate::path::ConfinedPath` or may replace it after refactor.
- Public errors should avoid exposing every private enum variant until naming is stable.

Do not expose `crate::fs::RootGuard` directly under that name. Plan 017 should introduce `SecureRoot` as the public wrapper.

### 4. Make policy types stable enough for both Rust and Python

Audit `policy.rs` and decide whether the existing enums can remain directly public. They are simple and already public, so the likely action is to preserve them while documenting their primitive role.

Add `Default` where appropriate:

- `StaticPolicy::default()` should match `StaticPolicy::safe_default()` if not already present.
- Consider `Default` for the individual policy enums if ergonomic and unambiguous.

Do not add compatibility-mode policy variants in this plan unless already implemented elsewhere. Keep the safe surface small.

### 5. Add structured error mapping

Create a public error taxonomy that is stable enough for callers:

- malformed request target;
- unsupported URI form;
- invalid percent encoding;
- invalid UTF-8;
- NUL byte;
- traversal component;
- dotfile denied;
- separator ambiguity;
- Windows prefix denied;
- Windows alternate stream denied;
- Windows reserved name denied;
- path too long;
- root escape denied;
- symlink denied;
- not found should remain a resolution result, not a parse error.

The exact enum can wrap internal `PathRejection`, but the public names should be clear and documented.

### 6. Update rustdoc and docs

Add `docs/public-api-boundary.md` describing:

- Reference CLI API.
- Rust primitive API.
- Python subprocess lifecycle API.
- Future Python native primitive API.
- Experimental service API.
- Internal implementation modules.

Include a table with columns: Surface, Stability, Intended audience, Allowed dependency direction, Notes.

Update `README.md` only if needed to mention that a primitive roadmap exists. Avoid overselling unfinished Python bindings.

## Tests

Add Rust tests for the public primitive facade, even if equivalent internal tests already exist. These tests should call only public APIs.

Required tests:

- Parse `/foo/bar` into safe components.
- Parse `/foo?x=1` as path-only target.
- Reject absolute-form target.
- Reject authority-form target.
- Reject asterisk-form target.
- Reject `.` and `..` components.
- Reject percent-encoded traversal.
- Treat double-encoded traversal consistently with documented behavior.
- Reject NUL.
- Reject backslash under default policy.
- Reject Windows drive prefixes and ADS syntax.
- Reject dotfiles under default policy.
- Allow dotfiles only when the policy permits.

These tests should be in a public-facing test module or integration test where possible, so they catch accidental privacy regressions.

## Documentation acceptance criteria

`docs/public-api-boundary.md` must explicitly state that eggserve does not provide in-tree ASGI/WSGI adapters and that such adapters should be built out of tree against the primitive API once stable.

It must also state that raw internal modules are not part of the public contract and may change.

## Validation

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
```

If CI has `cargo audit` and `cargo deny`, also run:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This plan is complete when:

- A `primitives` public module exists.
- Public primitive types have invariant-focused rustdoc.
- Path and policy primitives can be used without importing private modules.
- Internal filesystem traversal and response body implementation details remain hidden.
- Public-facing tests cover the primitive facade.
- `docs/public-api-boundary.md` explains the stability model and non-goals.
