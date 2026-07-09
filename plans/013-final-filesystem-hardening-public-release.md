# Plan 013: final filesystem hardening and public release track

## Goal

Move eggserve from release-candidate alpha/beta shape toward a public release posture by closing the remaining filesystem-hardening gap. The current implementation is already careful: request paths are normalized through a confined parser, symlink components are denied under safe defaults, symlink-follow mode still checks canonical root containment, request bodies are rejected, file streaming is bounded, TLS handshakes are timeout-bounded, and CI now covers Rust, TLS, Python API, audit, and deny checks.

The remaining blocker is the filesystem traversal model. It is currently component-wise `symlink_metadata` plus final `canonicalize` and root-prefix verification. That is reasonable for alpha, but it is still check-then-open and therefore not the final hardened story. This plan starts the final filesystem-hardening track and then prepares for public release.

The target is to replace or augment the current model with descriptor-relative traversal on Unix, produce a clear Windows reparse-point policy, and make public-release claims only where the implementation is validated.

## Non-goals

Do not add product features in this track. Specifically avoid:

```text
Range requests
HTTP/2
compression
CORS
authentication
uploads or write support
ASGI/WSGI
Python request callbacks
routing or middleware
ACME / automatic certificate management
hot TLS reload
configuration file format
index-name customization
```

This track is security hardening and public-release preparation only.

## Current state summary

The release-candidate baseline has:

```text
safe loopback default
explicit public bind acknowledgement
GET/HEAD only
zero request-body default with deterministic body-metadata rejection
component-wise symlink denial under safe defaults
root containment check under follow-enabled symlink mode
file-stream semaphore held for body lifetime
HEAD no file-stream permit consumption
safe default directory listing disabled
listing hides symlinks when symlinks are denied
optional feature-gated TLS
TLS handshake timeout using header timeout
minimal Python subprocess API with runtime config validation
CI jobs for fmt, Rust matrix tests, TLS feature tests, Python API tests, wheel smoke, cargo audit, cargo deny
alpha public API boundary docs
```

Known release-gating gaps:

```text
1. Unix traversal remains check-then-open rather than descriptor-relative.
2. Opened file path is still based on a path after validation; TOCTOU risk remains if an attacker can mutate the served tree concurrently.
3. Windows parser policy is strong, but reparse-point handling and final Windows filesystem security posture are not yet audited enough for production claims.
4. Follow-enabled symlink semantics need a more precise security decision under descriptor-relative traversal.
5. Directory listing currently reads by path, not by directory handle.
6. Tests cover many path cases but do not yet stress concurrent mutation/race behavior.
7. Release docs must distinguish Unix-hardened, Windows-experimental, and generic-alpha claims if platform parity is not achieved.
```

## Security target

Define the core invariant before implementation:

```text
For a request accepted by the parser and filesystem policy, eggserve must only open a file or directory that is reachable from the configured root under the active policy at the moment of opening, without following denied symlinks or reparse points.
```

Under safe defaults:

```text
no path component may be a symlink/reparse point
no dotfile component may be served
no parent/current/absolute/prefix/ADS/reserved Windows component may be accepted
no path outside the root may be opened even if the filesystem changes concurrently
```

Under explicit `--follow-symlinks`:

```text
inside-root symlinks may be followed only if the final opened object is still confined to the root
outside-root escape remains denied
behavior must be documented as less strict than safe defaults
```

If follow-enabled descriptor-relative implementation becomes complex, it is acceptable to narrow behavior for release:

```text
Option A: Keep --follow-symlinks, but implement safe root-containment with carefully documented limitations.
Option B: Temporarily mark --follow-symlinks experimental and not covered by final hardening guarantees.
Option C: Defer follow-enabled symlink serving entirely for public release and keep the flag hidden/disabled.
```

Prefer Option A only if implementation and tests are convincing. Otherwise prefer Option B over overclaiming.

## Workstream A: design final filesystem abstraction

### Goal

Separate platform-specific traversal from HTTP serving and path parsing. The service layer should not know whether the backend uses canonicalize/prefix, Unix `openat`, or Windows handle APIs.

### Proposed modules

```text
crates/eggserve-core/src/fs/
  mod.rs
  resolver.rs          # common types and policy interface
  unix.rs              # descriptor-relative traversal
  windows.rs           # Windows-specific policy/implementation
  fallback.rs          # canonicalize-based fallback only if intentionally kept
  tests.rs or test_support.rs
```

### Core trait/internal interface

Keep this internal initially:

```rust
pub(crate) trait FsResolver {
    fn resolve(&self, confined: &ConfinedPath, policy: &StaticPolicy) -> ResolvedResource;
    fn resolve_child(&self, dir: &ResolvedDirectory, child: &str, policy: &StaticPolicy) -> ResolvedResource;
    fn list_directory(&self, dir: &ResolvedDirectory, policy: &StaticPolicy) -> Result<Vec<ListingEntry>, FsError>;
}
```

The concrete public behavior should remain unchanged:

```text
GET file -> file response
HEAD file -> header-only response
directory + index.html -> index response
directory + listing disabled -> 403
directory + listing enabled -> safe listing
```

### Resolved resource redesign

The current `ResolvedFile` carries `PathBuf` and metadata. That forces the service to open the file later by path, reintroducing a race. For final hardening, resolve should ideally return an already-open file handle.

Target shape:

```rust
pub(crate) struct ResolvedFile {
    pub(crate) file: tokio::fs::File,
    pub(crate) metadata: std::fs::Metadata,
    pub(crate) display_name_or_path_hint: Option<PathBuf>, // for MIME only, never for access
}
```

However, MIME mapping needs extension information. Keep safe relative/request component info for MIME decisions, not absolute access paths:

```rust
pub(crate) struct ResolvedFile {
    pub(crate) file: std::fs::File or tokio::fs::File,
    pub(crate) metadata: fs::Metadata,
    pub(crate) safe_relative_components: Vec<String>,
}
```

If using `std::fs::File` from descriptor-relative open, convert to Tokio:

```rust
tokio::fs::File::from_std(file)
```

### Acceptance criteria

```text
Service code no longer opens resolved files by absolute path.
ResolvedFile either carries an open file or a platform-safe handle that cannot race to another target.
MIME detection uses safe relative components or an extension hint, not access path authority.
Directory listing can be implemented through resolver, not direct service-level read_dir by path.
Platform-specific traversal is isolated behind internal APIs.
```

## Workstream B: Unix descriptor-relative traversal

### Goal

Implement hardened Unix traversal using directory file descriptors and no-follow semantics. This should be the default Unix implementation for public release.

### Dependency decision

Choose one:

```text
Option 1: Use rustix for openat/openat2-style APIs.
Option 2: Use cap-std for capability-oriented directory traversal.
Option 3: Use libc directly.
```

Recommended: evaluate `rustix` first. It is lower-level, auditable, and avoids adopting a larger abstraction if the project only needs openat primitives. If `cap-std` materially reduces mistakes with acceptable dependency weight, document that decision in `docs/dependency-policy.md`.

Avoid ad hoc unsafe libc code unless there is a strong reason. If unsafe is required, isolate it in `fs/unix.rs`, keep it tiny, and add safety comments.

### Safe-default traversal algorithm

For `SymlinkPolicy::Denied`:

```text
open root directory as fd
current = root fd
for each component except final:
  reject dotfile if policy denies dotfiles
  openat(current, component, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW)
  if symlink -> deny SymlinkDenied
  if not directory -> not found / forbidden as appropriate
  current = opened directory fd
for final component:
  reject dotfile if policy denies dotfiles
  openat(current, final, O_RDONLY | O_CLOEXEC | O_NOFOLLOW)
  if symlink -> deny SymlinkDenied
  fstat opened object
  classify file vs directory
```

For root path with zero components:

```text
return opened root directory handle/classification
```

For directories:

```text
ResolvedDirectory should carry a directory handle or enough resolver-owned state to resolve children without path re-open races.
```

### Directory index

`index.html` resolution must use the directory handle:

```text
resolve_child(dir_handle, "index.html", policy)
```

Do not reconstruct `root/path/index.html` by path.

### Directory listing

Listing should use the opened directory handle. If Rust stdlib cannot list by fd portably, use platform APIs via chosen dependency.

Safe listing policy:

```text
read entries from directory fd
skip . and ..
skip dotfiles if denied
if symlinks denied, lstat/openat nofollow to classify and skip symlink entries
skip entries that disappear or cannot be classified
never include absolute paths or symlink targets
sort by display name
```

If fd-relative listing is too large for this phase, keep listing disabled by default and either:

```text
Option A: implement hardened listing before release
Option B: mark directory listing experimental and not part of hardening guarantee
Option C: disable directory listing from public release build/docs
```

Prefer implementing hardened listing if the code remains small.

### Follow-enabled symlinks on Unix

This is the hardest part. Define before coding.

Possible policy:

```text
safe defaults: O_NOFOLLOW every component, no symlinks
follow mode: allow symlink following, but verify final opened file is under root using device/inode ancestry or openat2 RESOLVE_BENEATH/RESOLVE_IN_ROOT where available
```

Linux `openat2` can help with `RESOLVE_BENEATH` / `RESOLVE_IN_ROOT` / `RESOLVE_NO_SYMLINKS`, but macOS/BSD do not have it. For portability, safe-default no-follow is easier than secure follow mode.

Recommended release posture:

```text
Fully harden safe-default SymlinkPolicy::Denied on Unix.
Keep SymlinkPolicy::Follow as experimental unless a robust cross-Unix containment strategy is implemented.
For follow mode, continue canonicalize/root-prefix fallback only with explicit docs, or gate it behind experimental status.
```

Do not overclaim follow mode.

### Tests

Unix integration tests:

```text
safe_default_regular_file_serves
safe_default_nested_regular_file_serves
safe_default_final_symlink_denied
safe_default_intermediate_symlink_denied
safe_default_nested_intermediate_symlink_denied
safe_default_symlink_to_outside_denied
follow_inside_root_symlink_behavior_matches_documented_policy
follow_outside_root_symlink_denied_or marked experimental according to chosen policy
index_lookup_uses_directory_handle
listing_hides_symlink_under_safe_default
listing_skips_racing_entries
```

Race-oriented tests:

```text
swap_final_file_to_symlink_between_resolve_and_stream_does_not_escape
swap_intermediate_dir_to_symlink_does_not_escape
rename_root_child_during_request_does_not_escape
```

Race tests can be probabilistic, but avoid flaky CI. Prefer deterministic test seams where resolver pauses between component opens only in tests.

### Acceptance criteria

```text
Unix safe-default traversal is fd-relative and no-follow.
Service never opens by post-validation absolute path.
Index lookup is fd-relative.
Directory listing is fd-relative or explicitly not part of hardening guarantee.
TOCTOU tests demonstrate no escape under controlled mutation.
Docs identify follow-enabled symlink status honestly.
```

## Workstream C: Windows reparse-point policy

### Goal

Define and implement a conservative Windows filesystem policy. The parser already rejects many Windows path hazards, but filesystem-level reparse points require explicit handling before public production claims on Windows.

### Release decision

Choose a Windows support posture:

```text
Option A: Windows production-supported after reparse-point hardening.
Option B: Windows alpha-supported with strong parser policy but documented filesystem caveats.
Option C: Windows CI-supported but not recommended for untrusted public roots until reparse-point work lands.
```

Recommended: Option C unless the implementation has enough Windows-specific handle/reparse-point testing.

### Windows hazards to address

```text
reparse points / junctions
symlinks
mount points
case-insensitive path ambiguity
8.3 short names if relevant
alternate data streams (parser denies colon already)
reserved device names (parser denies)
absolute/prefix paths (parser denies)
backslash ambiguity (parser denies)
```

### Conservative implementation

At minimum under safe defaults:

```text
use symlink_metadata-equivalent checks to reject symlinks/reparse points for every component
reject FILE_ATTRIBUTE_REPARSE_POINT if detectable
preserve canonical root containment check
keep parser denials for ADS/reserved/prefix/backslash
```

If using Windows APIs directly, isolate in `fs/windows.rs` and document unsafe blocks or dependency decisions.

### Tests

Windows parser tests should already run in CI. Add filesystem tests where feasible:

```text
windows_reserved_names_denied
windows_ads_denied
windows_backslash_denied
windows_drive_prefix_denied
windows_reparse_point_denied_if fixture creation is feasible
```

If CI cannot create symlinks/junctions due to privileges, add manual validation docs and mark the release claim accordingly.

### Acceptance criteria

```text
Windows release posture is explicitly chosen.
Safe-default reparse/symlink behavior is implemented or documented as not production-hardened.
Docs do not overclaim Windows public-root hardening.
CI continues to run parser tests on Windows.
```

## Workstream D: service integration changes

### Goal

Adapt service layer to consume resolver-opened resources and avoid reintroducing path-based opens.

### Required changes

Current service pattern:

```rust
ResolvedResource::File(file) => {
    let tokio_file = tokio::fs::File::open(&file.path).await?;
    file_response(tokio_file, ...)
}
```

Target pattern:

```rust
ResolvedResource::File(file) => {
    let tokio_file = file.into_tokio_file();
    file_response(tokio_file, ...)
}
```

or:

```rust
ResolvedResource::File(file) => file_response(file.file, ...)
```

MIME detection:

```text
Use safe relative component extension, not trusted absolute path.
```

Directory listing:

```text
Delegate to resolver.list_directory(&dir, policy).
```

Index lookup:

```text
Delegate to resolver.resolve_child(&dir, "index.html", policy).
```

### Acceptance criteria

```text
No service-layer `tokio::fs::File::open(&file.path)` remains for request serving.
No service-layer direct `fs::read_dir(dir.path)` remains unless explicitly marked fallback/experimental.
All file/directory operations flow through fs resolver.
Existing HTTP behavior remains unchanged.
```

## Workstream E: test matrix and CI hardening

### Goal

Add CI coverage that validates the final filesystem resolver behavior without flaky race tests.

### CI additions

```text
Unix resolver tests on ubuntu and macOS
Windows parser/reparse-policy tests on windows-latest
TLS/Python/audit/deny remain intact
optional cargo miri for selected pure path tests if practical
optional cargo test --features fs-test-hooks if deterministic race hooks are introduced
```

Do not require privileged Windows symlink creation in normal CI unless it is reliable.

### Test hooks

If needed, add internal-only test hooks:

```rust
#[cfg(test)]
struct ResolverTestHook { ... }
```

Use hooks to pause between component traversal steps for deterministic mutation tests. Do not expose hooks in public API.

### Fuzzing

Refresh fuzz coverage:

```text
path parser fuzz unchanged
component validation fuzz unchanged
resolver model fuzz optional with virtual filesystem model
body metadata tests remain unit-level
```

A virtual filesystem model can test policy decisions without OS races. Do not overinvest if it delays release.

### Acceptance criteria

```text
CI validates Unix descriptor-relative resolver on Linux/macOS.
CI validates Windows parser and documented Windows policy.
Race-sensitive behavior has deterministic tests or is manually documented.
No flaky sleep-based race tests are added to required CI.
```

## Workstream F: documentation and release claim reset

### Goal

Update every public-facing doc to match the final filesystem implementation and platform posture.

Docs to update:

```text
README.md
docs/security-policy.md
docs/security-review.md
docs/architecture.md
docs/compatibility.md
docs/release-criteria.md
docs/release-checklist.md
docs/deployment.md
AGENTS.md
```

Required claims:

```text
Unix safe-default traversal is descriptor-relative and no-follow, if implemented.
Windows support level is explicitly stated.
Symlink-follow mode status is explicitly stated.
Directory listing hardening status is explicitly stated.
Known limitations remain visible.
Public release is alpha/beta/1.0 according to completed gates.
```

Remove or revise any text saying descriptor-relative traversal is future work if this plan completes it. If Windows remains caveated, say so plainly.

### Acceptance criteria

```text
Docs match implementation exactly.
No overbroad production-hardening claims remain.
Release checklist includes filesystem resolver validation.
Security review identifies platform-specific status.
```

## Workstream G: public release preparation

### Goal

After filesystem hardening lands, prepare for public release in a controlled way.

### Release type decision

Choose one:

```text
Alpha: if Unix descriptor-relative traversal lands but Windows remains caveated or follow-mode remains experimental.
Beta: if Unix safe-default traversal is hardened, Windows posture is honest, CI is green, docs are complete, and API boundary is stable enough.
1.0: only if descriptor-relative traversal, Windows reparse policy, API stability, docs, and packaging are all fully closed.
```

Recommended likely outcome: public beta or alpha, not 1.0, unless Windows and follow-mode are fully settled.

### Release checklist

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
cd crates/eggserve-python && maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

Package checks:

```bash
cargo package -p eggserve-core --allow-dirty
cargo package -p eggserve-bin --allow-dirty
cd crates/eggserve-python
python -m twine check dist/*   # if twine is used
```

Manual smoke:

```bash
eggserve --directory public --port 8000
curl http://127.0.0.1:8000/file.txt
eggserve --directory public --directory-listing
eggserve --directory public --follow-symlinks   # only if documented supported
python -m eggserve --directory public --port 8000
```

### Publishing posture

Do not auto-publish from CI. Keep manual publishing until the release process is proven.

Document:

```text
crates.io package names and order
PyPI package name
version synchronization
changelog entry
release notes template
known limitations
```

### Acceptance criteria

```text
Release type is explicitly chosen.
All release checklist commands pass or exceptions are documented.
Package dry runs pass.
Docs and release notes match platform/security status.
Manual publish steps are documented.
No automatic publishing is introduced.
```

## Suggested commit sequence

A safe implementation order:

```text
docs(fs): define final filesystem hardening invariants and platform posture
refactor(fs): introduce resolver abstraction and resolved handle types
feat(fs-unix): add descriptor-relative no-follow traversal
fix(service): serve resolver-opened files and resolver-listed directories
test(fs): add unix resolver and controlled mutation regressions
fix(fs-windows): add conservative reparse-point policy or document caveat
docs(security): update filesystem hardening and platform claims
ci: add resolver/platform validation coverage
chore(release): refresh checklist and package dry-run notes
```

## Review checklist

Before closing this track, verify:

```text
No request-serving path reopens a validated absolute path.
No index lookup uses string path joins outside resolver internals.
No directory listing bypasses resolver policy.
No symlink is followed under safe defaults.
Follow-enabled symlink behavior is either robustly implemented or explicitly experimental.
Windows public-root hardening claims match implementation.
TOCTOU/race test strategy is present and non-flaky.
All docs match the final platform posture.
CI remains green and not excessively slow.
```

## Exit criteria

This track is complete when:

```text
Unix safe-default traversal is descriptor-relative and no-follow.
Service uses opened handles from resolver rather than reopening by absolute path.
Index and listing flow through resolver policy.
Windows posture is conservative and documented.
Release docs no longer list descriptor-relative traversal as future work for Unix safe defaults.
CI validates the filesystem-hardening path.
A public release type and checklist are ready.
```

After this, proceed to public release preparation, not new feature development.
