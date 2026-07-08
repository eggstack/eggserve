# Plan 008: polish and validation pass

## Goal

Complete the post-tightening polish needed before eggserve moves into optional TLS, Python API, and public library stabilization work. The current implementation has closed the major filesystem-policy gap: symlink denial is now component-wise, index lookup routes through directory components, and request-body metadata is rejected deterministically for `GET`/`HEAD`. This pass should focus on small correctness edges, test hygiene, documentation precision, CI visibility, and release-readiness validation.

This is not a feature expansion pass. Do not add TLS, Range, HTTP/2, compression, CORS, authentication, config files, uploads, ASGI/WSGI semantics, or a Python request API here.

## Current state

The repo now has:

```text
component-wise symlink rejection under safe defaults
inside-root symlink following only when explicitly enabled
outside-root symlink denial even when following is enabled
index lookup through `ResolvedDirectory` components and `RootGuard::resolve_child`
invalid Content-Length rejection
Transfer-Encoding rejection on GET/HEAD
Unix symlink tests cfg-gated at the function level
Linux/macOS/Windows CI matrix
Python wheel smoke job
cargo-audit job
```

Remaining polish areas:

```text
directory listing treatment of symlinks and metadata errors
request-body policy documentation consistency
CI workflow status visibility and local validation notes
Windows-specific filesystem caveats
error taxonomy clarity for Denied variants
pre-release README/doc polish
fuzz target coverage refresh after filesystem changes
```

## Workstream A: directory listing symlink and metadata policy

### Problem

`build_listing_entries` currently lists directory entries by reading `fs::read_dir`, filtering dotfiles, then calling `entry.metadata()?.is_dir()`. `metadata()` follows symlinks. Under safe defaults, symlink traversal is denied, but directory listings may still display symlink names and classify symlink-to-directory entries as directories. Clicking the entry should still be denied by the request path, but the listing policy is not as strict as the serving policy.

### Target behavior

Directory listings should reflect the active filesystem policy. Under safe defaults:

```text
symlink entries should be hidden or displayed as non-followable denied entries; prefer hiding initially
symlink-to-directory should not be rendered as a normal directory link
metadata errors should not fail the whole listing unless necessary
entries should never reveal absolute targets or symlink destinations
```

Recommended first behavior: hide symlink entries when `StaticPolicy.symlinks == SymlinkPolicy::Denied`.

When symlink following is enabled:

```text
inside-root symlink entries may be listed if resolving them would remain inside root
outside-root symlink entries should be hidden or listed as denied; prefer hidden until a clearer UX exists
```

To keep this pass small, implement the safe-default case first and document that follow-enabled directory listing remains conservative.

### Implementation guidance

Change listing construction so it uses `symlink_metadata()` before `metadata()`:

```text
for each entry:
  get file_name
  apply dotfile policy
  if symlinks denied and symlink_metadata says symlink: continue
  classify directory status without exposing symlink target
```

If classification fails for an entry due to permission or racing deletion, skip the entry rather than returning 500 for the whole listing. A directory listing should degrade gracefully.

### Tests

Add tests:

```text
directory_listing_hides_symlink_entries_when_symlinks_denied
listing_does_not_classify_symlink_to_dir_as_dir_when_denied
listing_skips_unreadable_or_racing_entry_if feasible
listing_never_contains_symlink_target_path
```

Unix-gate symlink listing tests.

### Acceptance criteria

```text
Directory listing policy no longer contradicts safe symlink denial.
Symlink names are not exposed in listings under safe defaults.
Listing tests cover symlink-to-file and symlink-to-directory cases.
```

## Workstream B: request-body metadata policy cleanup

### Problem

The service now rejects invalid Content-Length, positive Content-Length under zero-body policy, Transfer-Encoding, and conflicting body headers. The behavior is correct, but docs and tests should make status mapping explicit.

### Target behavior

Document and test:

```text
Content-Length: 0 -> allowed
Content-Length: positive -> 413 under default zero-body limit
Content-Length: invalid, negative, overflow -> 400
Transfer-Encoding present -> 400
Content-Length + Transfer-Encoding -> 400
Unsupported methods -> 405 without serving body
```

### Tests

Add or verify tests for:

```text
HEAD with Transfer-Encoding -> 400
HEAD with invalid Content-Length -> 400
POST with body metadata still returns 405 and does not reach file serving
```

Do not add body parsing.

### Acceptance criteria

```text
Request-body policy is fully documented.
GET and HEAD test coverage is symmetrical.
Unsupported methods remain 405.
```

## Workstream C: error taxonomy polish

### Problem

Several policy denials reuse `PathRejection::ParentComponent`, including symlink denial and outside-root canonicalization failure. That is functional but imprecise. It makes logs, debugging, and future API semantics less clear.

### Target behavior

Add explicit rejection variants where useful:

```rust
SymlinkDenied
RootEscapeDenied
```

Use them internally for:

```text
component symlink denied by policy -> SymlinkDenied
canonical target outside root -> RootEscapeDenied
actual `..` path component -> ParentComponent
```

HTTP responses can still map all policy denials to 403. The goal is internal clarity and better future diagnostics.

### Tests

Update lower-level `RootGuard` tests to assert specific denial variants where possible:

```text
final symlink denied -> SymlinkDenied
intermediate symlink denied -> SymlinkDenied
symlink follow outside root -> RootEscapeDenied
literal parent component parse -> ParentComponent
```

### Acceptance criteria

```text
Denial reasons distinguish parser-level parent traversal from symlink and root-escape denial.
HTTP behavior remains unchanged.
Tests assert specific variants at the filesystem layer.
```

## Workstream D: CI status and release validation hygiene

### Problem

The workflow file has been expanded, but connected-status checks have not been visible during review. The project should have a clear local and CI validation story before later phases.

### Target behavior

Document the exact local validation sequence and ensure CI covers it:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cd crates/eggserve-python && maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

Optional: add a `scripts/check.sh` or `justfile` only if the project is already using such tooling. Avoid adding a task-runner dependency solely for this.

### CI adjustments

Review `.github/workflows/ci.yml` for:

```text
unnecessary cargo fmt on all OSes; fmt can run once on ubuntu
clippy/test on all OSes
Python wheel smoke on ubuntu, later macOS/Windows if needed
cargo audit install caching or use a maintained action if acceptable
```

Do not overcomplicate CI in this pass.

### Acceptance criteria

```text
README or docs/release-criteria includes local validation commands.
CI shape is documented.
No unknown no-op validation claims remain.
```

## Workstream E: fuzz target refresh

### Problem

Fuzz targets exist, but filesystem-policy changes added component-wise resolution and request-body metadata parsing that are not necessarily covered.

### Target behavior

Refresh fuzz/readme coverage:

```text
path_components fuzz target still covers residual encoded dot-components
request_target fuzz target covers unsupported forms and query stripping
percent_decode target covers malformed encodings and invalid UTF-8
body metadata parser gets unit/property-style tests; fuzz optional
```

Do not fuzz live filesystem traversal yet unless a deterministic fixture model is added.

### Acceptance criteria

```text
fuzz/README explains how to run current fuzz targets
fuzz targets compile after latest path changes
any missing body metadata fuzzing is documented as future work
```

## Workstream F: documentation precision

Update docs to state current implementation boundaries precisely:

```text
safe defaults deny all symlink components
symlink-follow mode still denies final canonical root escape
current alpha traversal uses component-wise metadata checks plus canonical-root verification
1.0 still requires descriptor-relative/openat-style Unix traversal or a documented security decision to accept current model
Windows reparse-point handling is not yet final
```

Docs to check:

```text
README.md
docs/security-policy.md
docs/architecture.md
docs/compatibility.md
docs/release-criteria.md
docs/threat-model.md
AGENTS.md
```

### Acceptance criteria

```text
Docs match code behavior.
Alpha limitations are visible.
No docs imply production-hardening is complete before descriptor-relative traversal is addressed.
```

## Validation checklist

Run or rely on CI for:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
```

Python packaging smoke:

```bash
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

## Acceptance criteria for the full pass

This pass is complete when:

```text
directory listings respect symlink-denied policy
body metadata behavior is fully documented and symmetrically tested for GET/HEAD
denial reasons distinguish symlink/root escape from parent traversal
CI/local validation commands are documented
fuzz docs are refreshed
release docs clearly preserve descriptor-relative traversal as a future hardening gate
no new feature surface is introduced
```

## Suggested commit sequence

```text
fix(listing): hide symlink entries under symlink-denied policy
fix(http): complete GET/HEAD body metadata tests and docs
refactor(fs): add explicit symlink and root-escape rejection variants
chore(fuzz): refresh fuzz docs after path-policy changes
docs: polish validation and alpha filesystem limitations
```
