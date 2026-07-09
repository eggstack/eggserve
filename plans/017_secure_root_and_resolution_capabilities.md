# Plan 017: Secure root and resolution capabilities

## Status

Planned. This plan builds on Plan 016 by exposing secure filesystem resolution as public capability-oriented primitives.

## Objective

Introduce a public `SecureRoot` API and resolved-resource capability objects that let Rust callers, and later Python callers, resolve request-derived paths under eggserve's audited confinement and symlink policy. The API must preserve the existing Unix safe-default descriptor-relative traversal guarantee and must not encourage callers to reopen validated absolute paths manually.

This plan should not introduce dynamic request callbacks, ASGI/WSGI adapters, routing, middleware, or framework behavior.

## Current state

The current resolver is internal. `RootGuard::new()` canonicalizes the configured root, opens a root directory descriptor on Unix, and uses `resolve()` / `resolve_child()` / `list_directory()` to produce internal `ResolvedResource` values. Under safe defaults on Unix, resolution uses `statat(AT_SYMLINK_NOFOLLOW)` and `openat(O_NOFOLLOW)` per component. Files are opened during resolution and passed upward as already-open `std::fs::File` handles. This is the right security model.

The public API should preserve this implementation but hide the concrete internal types behind a stable facade.

## Design constraints

The public API must make the safe path the ergonomic path:

- Construct `SecureRoot` from a root path and `StaticPolicy`.
- Resolve a parsed/validated `RequestTarget` or `ConfinedPath`.
- Receive a `ResolvedResource` result.
- Use methods on `ResolvedFile` / `ResolvedDirectory` for safe metadata, listing, child resolution, and later response planning.

The API must not require callers to receive an absolute path and perform filesystem I/O themselves. Diagnostic path access, if exposed, must be clearly named and documented as non-serving/debug use.

## Implementation steps

### 1. Introduce `SecureRoot`

Create a public wrapper, likely in `eggserve_core::primitives::root`:

```rust
pub struct SecureRoot { /* private fields */ }
```

Candidate constructors:

```rust
impl SecureRoot {
    pub fn new(root: impl AsRef<Path>, policy: StaticPolicy) -> Result<Self, SecureRootError>;
    pub fn with_policy(root: impl AsRef<Path>, policy: StaticPolicy) -> Result<Self, SecureRootError>;
    pub fn resolve(&self, target: &RequestTarget) -> ResolvedResource;
    pub fn resolve_confined(&self, path: &ConfinedPath) -> ResolvedResource;
}
```

Decide whether `SecureRoot` should hold an internal `RootGuard` persistently or recreate it per call. Current behavior constructs `RootGuard` per request. For this plan, prioritize correctness and invariant preservation over caching. Persistent root descriptors can be added later if carefully tested. If `SecureRoot` caches a root guard, document root-renaming/replacement semantics.

Recommended first pass: `SecureRoot` stores the root path and policy, and internally constructs `RootGuard` during `resolve()`, matching current request behavior. Add a TODO for optional cached descriptor optimization.

### 2. Define public resolved-resource types

Add a public result shape:

```rust
pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(ResourceDeniedReason),
}
```

`ResourceDeniedReason` should be structured enough for callers to distinguish symlink denial, dotfile denial, root escape, traversal/policy denial, and platform denial.

`ResolvedFile` should expose safe metadata:

- `len()`
- `is_empty()`
- `modified()`
- `content_type()` or safe relative path for MIME planning
- `safe_relative_components()`
- later: `plan_response()` in Plan 018

For actual file access, prefer one of:

- an internal method used by response planner;
- `into_std_file()` if Rust callers truly need the open handle;
- `try_clone_file()` if repeat planning/streaming needs it.

If exposing the file handle, document that it is already policy-resolved and should be preferred over reopening paths. Do not expose a constructor that accepts `std::fs::File` from callers.

`ResolvedDirectory` should expose:

- safe relative components;
- `resolve_child("index.html")` through the same policy;
- `list()` returning policy-filtered entries;
- later: response listing planning in Plan 018.

### 3. Preserve Unix descriptor-relative semantics

Keep the existing internal `fs::unix` implementation private. Public `SecureRoot` should call into it through `RootGuard`, not duplicate it.

Add public-facing tests that assert behavior through `SecureRoot`, not through `RootGuard`:

- final symlink denied under safe defaults;
- intermediate symlink denied under safe defaults;
- symlink to outside root denied when follow-symlinks is enabled;
- normal file resolves;
- normal directory resolves;
- root path resolves as directory;
- directory index can be resolved by child lookup;
- directory listing hides symlink entries under safe defaults.

Where Unix-specific, gate tests with `#[cfg(unix)]`.

### 4. Document weaker modes precisely

Update or add `docs/secure-root.md` with these guarantees:

- Unix + symlink denied: descriptor-relative traversal, `statat(..., AT_SYMLINK_NOFOLLOW)`, `openat(..., O_NOFOLLOW)`, files opened during resolution, no absolute-path reopen in serving flow.
- Unix + follow symlinks: canonicalize fallback, root escape check, weaker TOCTOU posture.
- Non-Unix: parser and canonicalization checks, not equivalent to Unix descriptor-relative hardening.
- Windows: functional but not hardened against all reparse-point/junction attacks; do not use for untrusted mutable public roots until a later Windows-specific hardening plan lands.

Do not overstate guarantees.

### 5. Keep service behavior unchanged

The existing CLI/static server should continue to work. It may keep using internal `RootGuard` directly in this plan or be refactored to use `SecureRoot` if the refactor is straightforward. If refactoring the service risks regressions, leave it alone and add a follow-up note to migrate service usage after public primitives settle.

Recommended approach: implement `SecureRoot` as a facade over the existing internals, add tests, and only migrate service code if it simplifies rather than complicates the patch.

### 6. Prepare for Python binding

Design the public wrappers so PyO3 binding is feasible:

- Avoid lifetimes in public capability objects exposed to Python.
- Prefer owned objects or cloneable handles where necessary.
- Keep errors convertible to strings and structured codes.
- Avoid exposing generic parameters in the primitive facade.

If some Rust API needs lifetimes for efficiency, hide that behind an owned Python-specific wrapper later rather than complicating the main public API.

## Tests

Required Rust public API tests:

- `SecureRoot::new` accepts an existing directory.
- `SecureRoot::new` rejects missing or non-directory roots with structured error.
- resolving `/hello.txt` returns `ResolvedResource::File` with correct length and MIME-relevant components.
- resolving `/subdir` returns `ResolvedResource::Directory`.
- resolving missing path returns `NotFound`.
- resolving dotfile returns `Denied(DotfileDenied)` under defaults.
- resolving dotfile succeeds when policy permits.
- resolving final symlink denied under defaults on Unix.
- resolving intermediate symlink denied under defaults on Unix.
- follow-symlinks allows internal symlink but denies outside-root symlink on Unix.
- directory listing skips dotfiles and symlinks under default policy.
- directory child lookup for `index.html` uses the same policy and does not reopen by absolute path.

If any public type exposes diagnostic paths, test that docs mark them diagnostic and that serving code does not use them.

## Documentation acceptance criteria

Add `docs/secure-root.md` or a section under `docs/public-api-boundary.md` that explains:

- what `SecureRoot` proves;
- what `ResolvedFile` proves;
- what `ResolvedDirectory` proves;
- why callers should not reopen paths;
- how follow-symlinks and Windows differ from Unix safe defaults.

Include a small Rust example:

```rust
use eggserve_core::primitives::{RequestTarget, SecureRoot, StaticPolicy};

let root = SecureRoot::new("public", StaticPolicy::safe_default())?;
let target = RequestTarget::parse("/assets/app.css", Default::default())?;
let resource = root.resolve(&target);
```

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

If available:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This plan is complete when:

- `SecureRoot` is public through the primitive facade.
- Callers can resolve validated targets without private modules.
- Resolved resources are capability objects with no unsafe direct constructors.
- Unix safe-default symlink denial remains descriptor-relative and tested through the public API.
- Weaker follow-symlinks and non-Unix modes are documented accurately.
- Existing CLI/static server behavior does not regress.
