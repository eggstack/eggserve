# Plan 007: filesystem policy tightening pass

## Goal

Close the remaining correctness gap between eggserve's documented safe default policy and the current implementation. The repo is now a solid alpha: file-stream permits are lifetime-correct, no-op limit flags were removed, CLI bind semantics were repaired, CI is broader, and Python wheel packaging has a smoke job. The remaining hardening issue is that filesystem policy still allows intermediate symlink traversal under `StaticPolicy::safe_default()`, and index/directory handling still relies on canonicalize/prefix checks rather than a component-wise policy model.

This pass should make the current implementation's safe default honest: if symlinks are denied, no symlink component may be traversed, whether final or intermediate. It should also tighten request-body metadata handling and clean up misleading tests/docs introduced by the previous corrective pass.

This is not a feature pass. Do not add Range, TLS, HTTP/2, CORS, compression, authentication, config files, or a public Python API.

## Current findings to address

The latest repo state fixes several earlier defects but leaves these items:

```text
1. Intermediate symlink components are allowed under safe defaults.
2. The test `intermediate_symlink_followed_but_canonicalized` expects 200 under `StaticPolicy::safe_default()`, contradicting the symlink-denied security posture.
3. `RootGuard` still uses canonicalize/prefix checks and final-object symlink checks as the main model.
4. `resolve_index_at` improves service centralization but still takes an absolute canonical directory path, appends `index.html`, and checks only through canonicalize/prefix and final-object symlink detection.
5. The `dotfile_index_in_subdir_denied` test creates `.index.html`, but the implementation only looks for `index.html`; the test passes because no index exists, not because a dotfile index was actively denied.
6. Invalid `Content-Length` and `Transfer-Encoding` on `GET`/`HEAD` are not rejected deterministically.
7. Documentation should clearly distinguish the current alpha canonicalize/prefix implementation from the final descriptor-relative goal.
```

## Non-goals

Avoid widening the project scope:

```text
No Range support.
No TLS/rustls work.
No HTTP/2 work.
No reverse proxy behavior.
No upload/write support.
No dynamic handlers.
No ASGI/WSGI semantics.
No Python request API.
No compression.
No CORS/authentication.
No ACME.
```

## Workstream A: make symlink denial component-wise

### Problem

Under safe defaults, symlinks are documented as denied. The current implementation checks `symlink_metadata` for the final candidate path before canonicalizing. This catches a final symlink such as `/link.txt`, but it does not reject an intermediate symlink such as `/link_dir/file.txt` when `link_dir` points to another directory inside the root.

That behavior is inconsistent with a strict symlink-denied policy and undermines the security model. It also makes future reasoning harder because `Denied` does not mean every component was policy-clean.

### Target behavior

When `StaticPolicy.symlinks == SymlinkPolicy::Denied`, every traversed component must be checked before following it. Any symlink/reparse-like component must yield `ResolvedResource::Denied(...)`, even if the symlink target remains inside root.

Expected behavior under safe defaults:

```text
/file.txt -> OK if regular file
/link.txt -> 403 if link.txt is a symlink
/link_dir/file.txt -> 403 if link_dir is a symlink
/real_dir/link_file.txt -> 403 if link_file.txt is a symlink
/nested/link_dir/file.txt -> 403 if any intermediate component is a symlink
```

Expected behavior when `SymlinkPolicy::Follow`:

```text
symlink to a target inside root -> allowed if final canonical target remains inside root
symlink to a target outside root -> denied
intermediate symlink to inside root -> allowed if final canonical target remains inside root
intermediate symlink to outside root -> denied
```

### Implementation guidance

Introduce an internal traversal function in `RootGuard` rather than bolting more checks onto the current final-candidate logic.

Suggested shape:

```rust
impl RootGuard {
    pub fn resolve(&self, confined: &ConfinedPath, policy: &StaticPolicy) -> ResolvedResource {
        self.resolve_components(confined.components(), policy)
    }

    fn resolve_components(&self, components: &[String], policy: &StaticPolicy) -> ResolvedResource {
        ...
    }
}
```

For the current alpha implementation, a component-wise metadata walk is acceptable:

```text
candidate = canonical_root
for component in components:
  apply dotfile policy to component
  candidate.push(component)
  if symlinks denied:
    symlink_metadata(candidate)
    if symlink: deny
    if not found: not found
    if other error: not found or internal depending on policy
canonicalize candidate
ensure canonical starts with canonical_root
classify with metadata
```

This is still not a full TOCTOU-safe descriptor-relative traversal, but it closes the immediate semantic bug. Document it as an alpha-level improvement and keep a later plan for `openat`/directory-fd traversal.

Important: avoid checking only the final candidate. The loop must perform `symlink_metadata` after each component is appended.

### Unix-specific note

A stronger Unix implementation can use `cap-std`, `openat`, `rustix`, or direct `std::os::unix` patterns later. Do not pull in a new capability filesystem crate during this pass unless the dependency decision is explicitly documented. The purpose here is to make current behavior policy-correct without broad dependency churn.

### Windows-specific note

For Windows, use available metadata APIs conservatively. If reparse-point detection is not complete in this pass, document the gap. The parser already denies Windows path prefixes, ADS, backslashes, and reserved names. CI should at least validate parser behavior on Windows.

### Tests

Replace the current expectation for intermediate symlinks. Add Unix-gated tests where symlink creation is used.

Required tests:

```text
intermediate_symlink_denied_when_symlinks_denied:
  root/real_dir/file.txt exists
  root/link_dir -> root/real_dir
  GET /link_dir/file.txt under safe default -> 403

intermediate_symlink_inside_root_allowed_when_follow_enabled:
  same fixture
  policy symlinks Follow
  GET /link_dir/file.txt -> 200

intermediate_symlink_escape_denied_when_follow_enabled:
  outside_dir/secret.txt exists
  root/out -> outside_dir
  policy symlinks Follow
  GET /out/secret.txt -> 403

final_symlink_outside_root_denied_when_follow_enabled:
  root/escape.txt -> outside_dir/secret.txt
  policy symlinks Follow
  GET /escape.txt -> 403

nested_intermediate_symlink_denied:
  root/a/link_b -> root/b
  GET /a/link_b/file.txt under safe default -> 403
```

If a test is Unix-only, mark it with `#[cfg(unix)]` at the test function level, not only the symlink creation line. The current pattern that conditionally creates the symlink but still runs the test on non-Unix can produce misleading results.

### Acceptance criteria

```text
Safe defaults reject intermediate symlink components.
Symlink-follow mode still denies outside-root targets.
Existing final symlink tests still pass.
The misleading intermediate symlink test is renamed and corrected.
No new broad dependency is added unless documented.
```

## Workstream B: make index resolution use component-relative policy

### Problem

`handle_directory` now calls `RootGuard::resolve_index_at(dir_path, policy)`, which is better than direct service-level `dir_path.join("index.html")`, but the API still takes a canonical absolute path and appends `index.html`. This preserves some centralization but keeps index handling conceptually separate from request-relative component policy.

### Target behavior

Index lookup should use the same component-wise resolution engine as ordinary requests. Prefer passing safe relative components, not only a canonical absolute path.

### Implementation options

Preferred option: enrich `ResolvedDirectory` with safe relative components.

```rust
pub struct ResolvedDirectory {
    pub path: PathBuf,
    pub components: Vec<String>,
}
```

When `RootGuard::resolve` returns a directory, include the original safe components. Then index lookup can do:

```rust
let mut index_components = dir.components.clone();
index_components.push("index.html".to_string());
guard.resolve_components(&index_components, policy)
```

This ensures `index.html` traversal uses the same dotfile, symlink, canonical-root, and classification logic as every other path.

Alternative option: add a `ResolvedDirectory` method or `RootGuard::resolve_child` that accepts a trusted child component:

```rust
pub fn resolve_child(
    &self,
    dir: &ResolvedDirectory,
    child: &str,
    policy: &StaticPolicy,
) -> ResolvedResource
```

This method must validate the child component and then call the shared component-wise resolver.

Avoid leaving `resolve_index_at(dir_canonical: &Path, ...)` as the main path if it can be removed.

### Tests

Required tests:

```text
index_regular_file_served
index_final_symlink_denied_when_symlinks_denied
index_final_symlink_allowed_when_follow_enabled_if_inside_root
index_final_symlink_outside_root_denied_when_follow_enabled
index_under_intermediate_symlink_denied_when_symlinks_denied
index_under_intermediate_symlink_allowed_when_follow_enabled_if_inside_root
```

Clean up `dotfile_index_in_subdir_denied`. Since eggserve only searches for `index.html`, `.index.html` should not be described as a dotfile-index denial test. Replace with either:

```text
hidden_index_name_is_not_considered_index:
  create .index.html only
  GET /subdir -> 403 because no configured index exists
```

or remove it entirely until custom index names exist.

### Acceptance criteria

```text
Index lookup uses shared component-wise resolution.
`resolve_index_at` is removed or no longer used by service code.
Index symlink behavior matches normal file symlink behavior.
Misleading dotfile-index test is corrected.
```

## Workstream C: tighten request body metadata rejection

### Problem

`GET` and `HEAD` currently check `Content-Length`, but malformed values are silently ignored, and `Transfer-Encoding` is not visibly rejected. A hardened read-only static server should fail closed for body-bearing metadata on methods that do not accept bodies.

### Target behavior

For `GET` and `HEAD`:

```text
No Content-Length and no Transfer-Encoding -> allowed
Content-Length: 0 -> allowed
Content-Length: positive integer > max_request_body_bytes -> 413 Payload Too Large
Content-Length: invalid/non-integer/negative/overflow -> 400 Bad Request
Transfer-Encoding present -> 400 Bad Request or 413; prefer 400 for unsupported body framing
Both Content-Length and Transfer-Encoding -> 400 Bad Request
```

Since `max_request_body_bytes` defaults to zero, any positive `Content-Length` should return 413 under current defaults.

### Implementation guidance

Add a helper in `service.rs` or a new request-policy module:

```rust
fn validate_no_request_body<B>(req: &Request<B>, max_body_bytes: u64) -> Result<(), BodyRejection>
```

Suggested rejection enum:

```rust
pub enum BodyRejection {
    InvalidContentLength,
    BodyTooLarge,
    UnsupportedTransferEncoding,
    ConflictingBodyHeaders,
}
```

Map:

```text
InvalidContentLength -> 400
UnsupportedTransferEncoding -> 400
ConflictingBodyHeaders -> 400
BodyTooLarge -> 413
```

Keep response bodies generic; do not echo header values.

### Tests

Add tests for:

```text
get_content_length_zero_allowed
get_content_length_positive_rejected_413
head_content_length_positive_rejected_413
get_invalid_content_length_rejected_400
get_overflow_content_length_rejected_400
get_transfer_encoding_chunked_rejected_400
get_content_length_and_transfer_encoding_rejected_400
unsupported_method_still_405_without attempting to parse/stream body
```

### Acceptance criteria

```text
Invalid body metadata cannot silently pass.
Positive body metadata is rejected under default zero-body policy.
GET/HEAD remain bodyless serving methods.
Tests cover all header cases above.
```

## Workstream D: clean up platform-specific tests

### Problem

Some tests create symlinks behind `#[cfg(unix)]` statements inside functions that still execute on non-Unix platforms. That can produce false positives or tests that do not validate the intended condition on Windows.

### Target behavior

Unix symlink tests should be function-level gated:

```rust
#[cfg(unix)]
#[tokio::test]
async fn symlink_case_name() { ... }
```

Windows path parser tests should run on Windows through the CI matrix, but they should not require creating reparse points unless that support is explicitly implemented.

### Required cleanup

Review all tests containing `std::os::unix::fs::symlink`. Convert them to function-level Unix gates.

Add parser-only tests that are not Unix-gated for:

```text
/C:/Windows/System32
/c%3a/Windows/System32
/file.txt:stream
/CON
/AUX.txt
/foo\bar
/%5cetc%5cpasswd
```

These already exist in path unit tests; ensure CI actually runs them on Windows.

### Acceptance criteria

```text
No symlink test silently degrades on non-Unix.
Windows CI runs parser tests.
Windows-specific filesystem gaps are documented if not implemented.
```

## Workstream E: documentation synchronization

### Required documentation changes

Update these docs after implementation:

```text
docs/security-policy.md
docs/compatibility.md
docs/release-criteria.md
docs/architecture.md
README.md if it mentions symlink behavior
```

Document the current policy precisely:

```text
Under safe defaults, eggserve denies any symlink component in the requested path. This includes both final symlinks and intermediate directory symlinks.
When symlink following is explicitly enabled, eggserve still denies any request whose final canonical target escapes the configured root.
```

Document the current implementation limitation:

```text
The current alpha implementation uses component-wise metadata checks plus canonical-root verification. A later hardening pass should replace or augment this with descriptor-relative traversal on Unix to reduce TOCTOU exposure.
```

Update release criteria so 1.0 remains blocked on the descriptor-relative/openat design unless the project explicitly decides the current model is sufficient.

### Acceptance criteria

```text
Docs no longer imply that final-only symlink checks are sufficient.
Docs match test expectations.
Release criteria retain a stronger filesystem traversal gate for production/1.0.
```

## Workstream F: validation

Run or ensure CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For Python packaging, ensure existing CI still runs:

```bash
cd crates/eggserve-python
maturin build --release -o dist
pip install dist/*.whl
python -m eggserve --help
```

If local validation is not possible, rely on GitHub Actions but report status explicitly.

## Acceptance criteria for the full pass

The pass is complete when:

```text
Safe defaults reject intermediate symlink traversal.
Symlink-follow mode permits only inside-root targets.
Index lookup uses the same component-wise resolution path as ordinary file lookup.
Misleading dotfile-index tests are removed or renamed.
GET/HEAD reject malformed Content-Length and Transfer-Encoding.
Unix-only symlink tests are correctly cfg-gated.
Docs explicitly describe current symlink behavior and remaining TOCTOU limitations.
CI passes or any failures are documented with follow-up items.
No new broad server feature is introduced.
```

## Suggested commit sequence

Use small commits:

```text
fix(fs): deny intermediate symlink components by default
fix(fs): route index lookup through component resolver
fix(http): reject invalid body metadata on GET and HEAD
test(fs): add symlink escape and index policy regressions
test: cfg-gate unix symlink tests correctly
docs: synchronize filesystem policy and alpha limitations
```

## Review checklist

Before merge, check:

```text
No service code directly opens index paths by ad hoc path joins.
No symlink test expects 200 under safe defaults.
No symlink-to-outside request succeeds under any default or follow-enabled policy.
No invalid Content-Length silently falls through.
No Transfer-Encoding request reaches file serving.
No new public feature was added opportunistically.
```
