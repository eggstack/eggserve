# Plan 012: release-candidate hardening, CI, and API-boundary pass

## Goal

Close the remaining gaps before treating eggserve as a release-candidate alpha/beta. The repo now has a coherent static-serving core, safe defaults, component-wise symlink policy, bounded file streaming, optional TLS, a thin Python API, release documentation, and initial dependency guardrails. The remaining work is not product expansion. It is validation and boundary hardening:

```text
CI must exercise all newly added surfaces.
TLS handshakes must be bounded by timeout/connection-limit policy.
Filesystem denial taxonomy should either be meaningful or removed.
Python API config validation should fail before spawning the binary.
Rust public API exposure should be explicit and documented.
Release docs should match the final alpha/beta support story.
```

Do not add Range, HTTP/2, compression, CORS, authentication, upload/write support, ASGI/WSGI, request callbacks, ACME, hot TLS reload, config files, dashboards, or broader Python embedding in this pass.

## Current repo state

Since the previous planning set, the repo has implemented:

```text
Plan 008 polish and validation pass
Plan 009 optional TLS support and deployment guidance
Plan 010 minimal Python API
Plan 011 library stabilization and release hardening docs
```

Observed remaining issues:

```text
1. CI does not appear to run TLS feature checks/tests.
2. CI does not appear to run Python API unit tests from the installed wheel/source tree.
3. `deny.toml` exists, but CI does not run `cargo deny check`.
4. TLS handshake is not separately timed out, so a client can occupy a connection permit during handshake until lower-layer behavior closes it.
5. `PathRejection::SymlinkDenied` and `PathRejection::RootEscapeDenied` exist but filesystem denials currently use `ResolvedResource::Denied(())`, making the variants effectively unused.
6. Python `ServeConfig` relies mostly on type hints; runtime validation for `log_format`, port range, and bind/public combinations is incomplete.
7. Rust public API boundaries are not fully settled: `service::handle_request` is public, `fs` module is public but its useful types are crate-private, and `BoxBodyInner` is re-exported from an internal response module.
8. TLS-enabled builds appear to support plaintext mode when no cert/key is passed; that may be acceptable, but it should be explicit in docs and tests.
```

## Workstream A: CI coverage for all current surfaces

### Problem

The workflow currently runs formatting, OS-matrix clippy/tests, a Python wheel `--help` smoke, and `cargo audit`. It does not obviously validate TLS feature builds, Python API tests, or `cargo deny` despite these now being part of the repo surface.

### Target CI shape

Keep the CI simple but complete:

```text
fmt:
  ubuntu only
  cargo fmt --all -- --check

rust:
  ubuntu-latest, macos-latest, windows-latest
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace

tls:
  ubuntu-latest
  cargo check -p eggserve-bin --features tls
  cargo test -p eggserve-bin --features tls
  optionally cargo test --workspace --features tls if feature unification is clean

python-wheel:
  ubuntu-latest
  maturin build --release -o dist
  pip install dist/*.whl
  python -m eggserve --help
  python -m unittest discover -v eggserve or equivalent test invocation

dependency:
  cargo audit
  cargo deny check
```

Do not add an overly complex release matrix yet. The goal is to validate current surfaces, not to build all release wheels.

### Python API test invocation

Because Python tests live under `crates/eggserve-python/python/eggserve/test_server.py`, choose one reliable path:

Option A: run tests from source before wheel build:

```bash
cd crates/eggserve-python/python
python -m unittest eggserve.test_server -v
```

Option B: include tests in wheel and run after install:

```bash
python -m unittest eggserve.test_server -v
```

Option C: keep tests outside the wheel and run them using `PYTHONPATH`:

```bash
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
```

Preferred: Option C for CI simplicity, plus a separate installed-wheel import smoke:

```bash
python - <<'PY'
from eggserve import ServeConfig, StaticPolicy, ServerProcess, serve_directory
print(ServeConfig())
PY
```

### Dependency tooling

If `cargo deny` is added to CI, avoid installing it from scratch on every run if runtime becomes excessive. Options:

```text
cargo install cargo-deny --locked
or use a maintained cargo-deny action if acceptable
```

Keep `deny.toml` practical. The first release-candidate gate should catch advisories, license issues, duplicate critical dependencies, and unknown registries/sources without creating excessive false positives.

### Acceptance criteria

```text
CI checks TLS feature builds/tests.
CI runs Python API unit tests.
CI runs cargo deny check.
CI still runs on Linux/macOS/Windows for core Rust tests.
README or release checklist documents the CI matrix.
```

## Workstream B: TLS handshake timeout and TLS-mode semantics

### Problem

The TLS accept path currently acquires a connection permit, then awaits `tls_accept.accept(stream)`. A client that connects and does not complete TLS can hold a permit. The global semaphore bounds this, but a handshake timeout should exist because eggserve advertises resource-bound behavior.

### Target behavior

TLS handshake must be bounded. Use an existing limit if semantically acceptable, or add a dedicated limit.

Preferred minimal approach:

```text
Use `header_read_timeout` as the TLS handshake timeout in TLS mode.
Document that this timeout covers initial TLS handshake and HTTP header read.
```

Alternative:

```rust
pub struct Limits {
    pub tls_handshake_timeout: Duration,
    ...
}
```

Only add a new CLI flag if it will be enforced and tested. Otherwise keep it internal/defaulted for now.

### Implementation guidance

In TLS accept path:

```rust
let handshake = tokio::time::timeout(header_timeout, tls_accept.accept(stream)).await;
match handshake {
    Ok(Ok(tls_stream)) => serve_connection(...),
    Ok(Err(_)) => return,
    Err(_) => return,
}
```

Do not log raw TLS errors by default; keep logs low-noise unless structured logging is expanded later.

### Plaintext mode under TLS-enabled binary

The current TLS-enabled binary supports plaintext mode when no cert/key is supplied. Decide and document explicitly:

```text
TLS feature compiled + no TLS flags -> plaintext HTTP, same as default build.
TLS feature compiled + cert/key flags -> HTTPS.
Partial TLS flags -> error.
```

Add a CLI test for this behavior under the TLS feature if possible.

### Tests

Add tests for:

```text
partial TLS args still fail under tls feature
TLS feature with no tls args parses as plaintext mode
TLS handshake timeout path is covered by unit test if possible, or integration smoke if feasible
```

If direct handshake timeout testing is too flaky, keep the implementation small and document manual validation.

### Acceptance criteria

```text
TLS handshake cannot hold a connection permit indefinitely.
TLS/plaintext mode semantics are documented.
TLS feature is validated in CI.
No ACME, reload, SNI, or mTLS scope is added.
```

## Workstream C: filesystem denial taxonomy cleanup

### Problem

`PathRejection::SymlinkDenied` and `PathRejection::RootEscapeDenied` were added, but `ResolvedResource::Denied(())` discards the reason. This leaves dead variants and loses the internal diagnostic value Plan 008 requested.

### Target behavior

Pick one of two directions.

Preferred direction: keep denial reasons.

```rust
pub(crate) enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(PathRejection),
}
```

Use specific variants:

```text
dotfile component denied -> DotfileDenied
symlink component denied -> SymlinkDenied
canonical target outside root -> RootEscapeDenied
parser-level `..` -> ParentComponent
```

HTTP still maps all filesystem policy denials to 403.

Alternative direction: remove unused variants.

```text
If filesystem-layer diagnostic reasons are intentionally not kept, delete SymlinkDenied and RootEscapeDenied and document why.
```

Preferred: carry `PathRejection` internally. It costs little and improves debugging/test clarity.

### Tests

Update lower-level tests to assert specific denial reasons:

```text
resolve_symlink_denied -> Denied(SymlinkDenied)
resolve_intermediate_symlink_denied_when_symlinks_denied -> Denied(SymlinkDenied)
resolve_intermediate_symlink_escape_denied_when_follow_enabled -> Denied(RootEscapeDenied)
resolve_final_symlink_outside_root_denied_when_follow_enabled -> Denied(RootEscapeDenied)
resolve_dotfile_denied -> Denied(DotfileDenied)
```

Do not expose these reasons in HTTP response bodies.

### Acceptance criteria

```text
No dead PathRejection variants remain.
Filesystem denials are internally distinguishable.
HTTP denial response remains generic 403.
Tests assert specific denial reasons where useful.
```

## Workstream D: Python API runtime validation

### Problem

The Python API uses type hints for `log_format` and other fields, but runtime callers can pass invalid values. Some invalid settings are allowed through to the Rust binary, where they fail later. For a small public API, config errors should fail before spawning.

### Target behavior

Add `ServeConfig.validate()` or internal validation in `ServerProcess.start()`.

Validate:

```text
port is int and 1 <= port <= 65535
log_format is one of text/json/none
bind is not 0.0.0.0 or :: unless public=True
bind is a bare IP/host compatible with Rust CLI expectations, or document that Rust handles it
policy is StaticPolicy or None only at construction helper level
```

Because `ServeConfig` is a frozen dataclass, validation can occur in `__post_init__`:

```python
def __post_init__(self):
    if self.log_format not in ("text", "json", "none"):
        raise ValueError(...)
    if not isinstance(self.port, int) or not (1 <= self.port <= 65535):
        raise ValueError(...)
```

For `public` + bind checks, `__post_init__` is appropriate because it is pure config validation. Keep the same check in Rust CLI as defense in depth.

### Public bind recognition

At minimum handle:

```text
0.0.0.0
::
[::] if accepted by user input
```

Do not implement complex DNS resolution in Python unless needed. The Rust CLI currently parses IP/socket-style binds, not arbitrary hostname resolution. Python docs should match this.

### Tests

Add Python tests:

```text
invalid_log_format_raises
port_zero_raises
port_above_65535_raises
non_int_port_raises
public_ipv4_without_public_raises_at_config_or start
public_ipv6_without_public_raises_at_config or start
valid_public_bind_with_public_true_allowed
```

Update tests if validation moves from `start()` to `ServeConfig.__post_init__`.

### Acceptance criteria

```text
Invalid Python config fails before spawning the binary.
Python API tests cover validation.
Docs describe runtime validation behavior.
Rust CLI remains authoritative and validates independently.
```

## Workstream E: Rust public API boundary decision

### Problem

The current core crate has a partially-public surface:

```text
pub mod config
pub mod fs, but useful fs types are crate-private
pub mod limits
pub mod policy
pub mod service
pub mod telemetry
pub use response::BoxBodyInner from a crate-private module
```

This is awkward for release. Either eggserve-core is a public library with a deliberate API, or it is an internal crate used by the binary for now.

### Recommended direction for alpha

Use an internal-first alpha posture:

```text
Public/stable-ish: config, limits, policy
Experimental: service::handle_request only if needed by integration users
Internal: fs, path, response internals, telemetry internals unless public docs justify them
```

Concrete cleanup:

```rust
pub mod config;
pub mod limits;
pub mod policy;
pub mod service; // optional, marked experimental in docs
pub(crate) mod fs;
pub(crate) mod path;
pub(crate) mod response;
pub(crate) mod mime;
pub(crate) mod error;
pub(crate) mod telemetry; // unless bin needs it publicly
```

If `eggserve-bin` needs `telemetry::log_startup`, either:

```text
move telemetry to bin crate, or
keep a narrow public function explicitly documented as unstable, or
make a public `startup_summary` API in config and print from bin.
```

The public `BoxBodyInner` re-export should be reconsidered. If `handle_request` remains public, its body type may force exposing `BoxBodyInner`. Prefer hiding this behind a `pub type` with clear experimental docs, or make `handle_request` crate-private until library stabilization.

### Documentation

Add a section to `docs/architecture.md` or `docs/release-criteria.md`:

```text
eggserve-core public API status
stable-ish types
experimental APIs
internal modules
semver expectations before 1.0
```

### Tests

Run `cargo doc --workspace --no-deps` if feasible. Consider adding a docs check in CI later, not necessarily this pass.

### Acceptance criteria

```text
No accidentally public module exists without a reason.
Public API status is documented.
BoxBodyInner/service exposure is intentional or removed.
The project can honestly publish or withhold eggserve-core based on clear API posture.
```

## Workstream F: release documentation sync

Update docs after the above changes.

Required docs:

```text
README.md
docs/security-policy.md
docs/tls.md
docs/python-api.md
docs/dependency-policy.md
docs/release-checklist.md
docs/release-criteria.md
docs/security-review.md
docs/architecture.md
```

Specific points to document:

```text
TLS handshake timeout behavior
TLS feature compiled but no TLS flags means plaintext HTTP
Python API validation behavior
CI matrix and validation commands
cargo deny is part of release gate
core public API status
filesystem traversal still blocks 1.0 unless descriptor-relative/openat decision is made
```

### Acceptance criteria

```text
Docs match implementation.
No release docs claim CI coverage that does not exist.
No docs imply 1.0 filesystem guarantees before descriptor-relative traversal/reparse-point decisions are closed.
```

## Validation checklist

Required before closing this pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p eggserve-bin --features tls
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
cd crates/eggserve-python && maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import ServeConfig, StaticPolicy, ServerProcess, serve_directory
print(ServeConfig())
PY
```

If any command is intentionally omitted from CI, document why.

## Suggested commit sequence

```text
ci: validate tls feature python api tests and cargo deny
fix(tls): bound TLS handshakes with timeout
refactor(fs): preserve filesystem denial reasons internally
fix(python): validate ServeConfig at runtime
docs(api): clarify eggserve-core public API boundary
docs(release): sync release validation and TLS/Python behavior
```

## Acceptance criteria for the whole pass

This pass is complete when:

```text
CI validates Rust core, TLS feature, Python API tests, cargo audit, and cargo deny.
TLS handshakes are timeout-bounded.
Filesystem denials either preserve meaningful PathRejection reasons or unused variants are removed.
Python config validation fails before spawning the binary.
Rust public API exposure is deliberate and documented.
Release docs match actual validation and known limitations.
No new product feature surface is added.
```

## Next decision after this pass

After this pass, choose one of two paths:

```text
Path A: publish a clearly labeled alpha/beta with known filesystem traversal limitations.
Path B: start the final filesystem hardening track for descriptor-relative Unix traversal and Windows reparse-point handling before public release.
```

Do not proceed to feature expansion until that decision is made.
