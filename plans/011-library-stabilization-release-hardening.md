# Plan 011: library stabilization and release hardening

## Goal

Prepare eggserve for a credible public alpha/beta release by stabilizing the Rust library surface, tightening release criteria, documenting supported platforms, and establishing repeatable publishing workflows. This phase should make the current CLI, Python wheel, and Rust core usable by early adopters without overstating production-readiness.

This is not the final 1.0 pass. Descriptor-relative filesystem traversal, Windows reparse-point hardening, and any unresolved TOCTOU decisions must remain explicit release gates unless completed here.

## Scope

In scope:

```text
Rust public API review for eggserve-core
crate documentation and rustdoc examples
feature flag review
dependency/license/advisory policy
versioning policy across crates and Python package
release checklist
crates.io dry-run or package validation
PyPI wheel build validation
README and docs polish
supported platform matrix
security review notes
```

Out of scope:

```text
new server features
Range requests
HTTP/2
full descriptor-relative traversal unless explicitly scoped as a subtask
ASGI/WSGI
large Python API expansion
ACME/certificate automation
```

## Workstream A: Rust public API audit

Review `eggserve-core` public exports and decide what is stable enough for external use.

Current likely public concepts:

```text
ServeConfig
ServeState, maybe internal only
Limits
StaticPolicy
DirectoryListingPolicy
DotfilePolicy
SymlinkPolicy
ConfinedPath
PathPolicy
PathRejection
ResolvedResource / ResolvedFile / ResolvedDirectory
handle_request, maybe not stable public API
response helpers, probably internal
```

Classify each as:

```text
public stable-ish
public but experimental
crate-private/internal
```

Prefer conservative visibility. If a type is only public because tests or crates need it, consider `pub(crate)` or a clearly documented experimental module.

Add module-level docs:

```rust
//! Hardened static-serving primitives for eggserve.
```

Add `#[must_use]` where appropriate for builder/config-returning helpers.

## Workstream B: API boundary decisions

Decide whether `eggserve-core` is intended for library consumers immediately or only as an internal crate until 1.0.

Option 1: internal-first alpha.

```text
Publish only binary/Python package initially.
Keep eggserve-core public but documented as unstable.
Avoid strong semver promises for core internals.
```

Option 2: library alpha.

```text
Publish eggserve-core to crates.io with documented experimental API.
Expose typed static-serving primitives.
Accept semver responsibility for public types.
```

Recommended: internal-first alpha unless there is immediate demand for Rust library users. The core API can stabilize after one or two implementation cycles.

## Workstream C: feature flag and dependency review

Review dependency tree against `docs/dependency-policy.md`.

Required checks:

```bash
cargo tree
cargo tree -e features
cargo audit
cargo deny check   # if cargo-deny is added
```

If `cargo-deny` is not present, add a minimal `deny.toml` covering:

```text
licenses
advisories
duplicate dependency warnings if reasonable
unknown registries/sources
```

Do not make the deny policy so strict that it creates noise before release. Start practical and tighten later.

## Workstream D: release checklist

Create or update `docs/release-checklist.md`.

Checklist should include:

```text
version numbers synchronized
CHANGELOG updated
cargo fmt/clippy/test clean
platform CI green
Python wheel smoke green
cargo audit clean or exceptions documented
dependency/license review complete
README examples manually verified
security policy reviewed
known limitations documented
crates.io package dry-run clean if publishing crates
PyPI package build clean if publishing wheel
no accidental broad feature claims
```

Add a release-blockers section:

```text
Descriptor-relative/openat traversal not complete -> blocks 1.0, not necessarily alpha.
Windows reparse-point policy not complete -> blocks Windows production claim.
Range/TLS absent -> documented non-feature, not blocker unless release notes claim it.
```

## Workstream E: docs and examples polish

Review docs for consistency and public expectations.

Docs to polish:

```text
README.md
docs/security-policy.md
docs/architecture.md
docs/compatibility.md
docs/dependency-policy.md
docs/release-criteria.md
docs/python-packaging.md
docs/deployment.md if created
docs/python-api.md if created
```

Examples to add or verify:

```text
python -m eggserve --directory public
python -m eggserve --directory public --public --bind 0.0.0.0
python -m eggserve --directory public --directory-listing
cargo run -p eggserve-bin -- --directory public
```

Keep warnings visible for:

```text
public bind
symlink following
allowing dotfiles
directory listing
alpha filesystem traversal limitation
```

## Workstream F: platform support matrix

Create or update a support matrix:

```text
Linux x86_64: supported/tested
Linux aarch64: planned/tested if CI exists
macOS arm64: supported/tested
macOS x86_64: supported/tested if CI exists
Windows x86_64: parser tested; filesystem hardening caveats documented
```

Do not claim full Windows production hardening until reparse-point behavior is audited and tested.

## Workstream G: security review note

Add `docs/security-review.md` or update `SECURITY.md` with a current alpha review note.

Include:

```text
threat model summary
safe defaults
known limitations
filesystem traversal model
request-body policy
directory listing policy
dependency review status
reporting process
```

Known limitations should be explicit, not buried:

```text
Current filesystem traversal is component-wise metadata + canonical-root verification, not final descriptor-relative traversal.
Windows reparse-point behavior requires additional hardening before production claims.
No request bodies are supported.
No Range support.
No native TLS unless Plan 009 has landed.
```

## Workstream H: publishing dry runs

For Rust:

```bash
cargo package -p eggserve-core --allow-dirty
cargo package -p eggserve-bin --allow-dirty
```

If crates are not ready for publication, document why.

For Python:

```bash
cd crates/eggserve-python
maturin build --release -o dist
python -m twine check dist/*   # if twine is acceptable
```

Do not publish automatically through CI yet unless explicitly approved. The repo preference should remain manual publishing until release process is mature.

## Acceptance criteria

```text
Public Rust API is reviewed and visibility tightened.
Docs accurately state alpha/beta readiness.
Release checklist exists.
Security review note exists.
Dependency/license/advisory policy is documented or enforced.
Platform support matrix exists.
Package dry-run results are documented.
No unsupported production claims remain.
```

## Suggested commit sequence

```text
refactor(core): tighten public API visibility before release
 docs: add release checklist and platform matrix
 docs: add alpha security review note
 chore(deps): add cargo-deny baseline
 chore(release): add package dry-run notes
 docs: polish README and compatibility claims
```

## Exit criteria for moving beyond Phase 11

After this phase, eggserve should be ready for one of two choices:

```text
public alpha release with explicit limitations
or a final pre-1.0 hardening track focused on descriptor-relative traversal, Windows reparse points, and deeper fuzzing
```

Do not proceed to broad feature additions until one of those paths is selected.
