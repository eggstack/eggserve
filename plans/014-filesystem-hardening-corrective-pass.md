# Plan 014: filesystem hardening corrective pass

## Goal

Close the remaining correctness and documentation gaps from the Plan 013 filesystem-hardening attempt. The repo now has the right high-level architecture: Unix safe-default requests route through a descriptor-relative resolver, resolved files carry pre-opened handles, the service no longer reopens by absolute path, directory children/indexes go through resolver APIs, and directory listings can be fd-relative on Unix.

The remaining issue is precision. The Unix resolver currently performs `statat(..., SYMLINK_NOFOLLOW)` before `openat`, but the `openat` calls themselves do not include `O_NOFOLLOW`. Some docs claim `openat` uses `O_NOFOLLOW` and that final-component behavior is atomic; that does not match the code. This pass should fix the implementation where possible, tighten platform-specific claims, and leave the repo in a release-candidate state for public alpha/beta.

This is a corrective hardening pass, not a feature pass.

## Non-goals

Do not add:

```text
Range requests
HTTP/2
compression
CORS
authentication
uploads/write support
ASGI/WSGI
Python request callbacks
ACME/certificate automation
configuration files
index customization
new public API surface
```

## Current state to preserve

Do not regress these improvements:

```text
ResolvedFile carries a pre-opened std::fs::File.
Service converts resolved handles with tokio::fs::File::from_std.
Service uses safe_relative_components for MIME detection.
Service does not call tokio::fs::File::open(&file.path) for request serving.
Directory index lookup uses RootGuard::resolve_child.
Directory listing uses RootGuard::list_directory.
Unix safe-default path uses fs/unix.rs resolver.
Follow-symlinks remains documented as weaker/canonicalize fallback.
Windows remains explicitly caveated rather than overclaimed.
```

## Workstream A: add open-time no-follow enforcement on Unix

### Problem

The Unix resolver currently checks for symlinks with:

```rust
statat(&current_fd, component.as_str(), AtFlags::SYMLINK_NOFOLLOW)
```

but then opens with flags like:

```rust
OFlags::RDONLY | OFlags::CLOEXEC
OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC
```

That leaves a check/open gap. It is better than the old absolute-path reopen model, but it does not satisfy the documented `openat` + `O_NOFOLLOW` claim.

### Target behavior

For Unix safe-default mode (`SymlinkPolicy::Denied`), use open-time no-follow flags for every opened component where supported by `rustix`:

```rust
let final_flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
let dir_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
```

If `rustix::fs::OFlags` names the flag differently on supported targets, use the appropriate cross-platform flag. If a target does not expose an equivalent, document that target-specific limitation and preserve the `statat` guard.

### Expected behavior

```text
final symlink -> Denied(SymlinkDenied)
intermediate symlink -> Denied(SymlinkDenied)
symlink swap between statat and openat -> openat fails rather than following
regular file -> served
regular directory -> resolved/listed/indexed according to policy
```

### Error mapping

When `openat` fails because of symlink/no-follow behavior, map to `ResolvedResource::Denied(PathRejection::SymlinkDenied)`.

Likely errno mappings:

```text
ELOOP -> SymlinkDenied
ENOTDIR -> NotFound or Forbidden; prefer NotFound to avoid leakage
ENOENT -> NotFound
EACCES/EPERM -> NotFound or Forbidden; current behavior tends to NotFound, preserve unless changing deliberately
```

Do not leak local filesystem paths or errno details into HTTP responses.

### Tests

Add or strengthen tests:

```text
unix_final_symlink_denied_by_open_nofollow
unix_intermediate_symlink_denied_by_open_nofollow
unix_symlink_swap_final_component_does_not_escape
unix_symlink_swap_intermediate_component_does_not_escape if deterministic
```

For deterministic swap tests, add test-only hooks rather than relying on sleeps:

```rust
#[cfg(test)]
static RESOLVER_TEST_HOOK: ...
```

A simpler acceptable test is to assert that direct symlinks are denied with the new open flags. Race hooks can be deferred if they are too intrusive, but document the deferral.

### Acceptance criteria

```text
Unix safe-default openat calls include NOFOLLOW or equivalent.
Symlink-denial tests still pass.
Docs no longer overstate behavior unsupported by code.
No service-layer path reopen is reintroduced.
```

## Workstream B: remove or minimize stat/open doc overclaiming

### Problem

Docs currently say descriptor-relative traversal eliminates the TOCTOU window, but the implementation has a pre-open `statat` and then `openat` pattern. Once `O_NOFOLLOW` is added, final symlink swap protection is stronger, but intermediate directory handling may still have platform nuances, especially macOS.

### Target docs language

Use precise claims:

```text
On Unix with safe defaults, eggserve resolves request paths relative to an opened root directory descriptor. Components are checked with statat(..., AT_SYMLINK_NOFOLLOW) and opened with openat(..., O_NOFOLLOW) where supported. This prevents the service layer from reopening validated absolute paths and closes the primary final-object symlink-swap issue. Platform-specific semantics around directory no-follow behavior are documented below.
```

Avoid unconditional statements like:

```text
eliminates all TOCTOU windows
final component is atomic
root directory is opened once at startup
```

unless the code actually guarantees them.

### Docs to update

```text
crates/eggserve-core/src/fs/mod.rs module docs
crates/eggserve-core/src/fs/unix.rs module docs
docs/security-policy.md
docs/security-review.md
docs/architecture.md
docs/release-criteria.md
README.md if it makes hardening claims
```

### Acceptance criteria

```text
Docs match exact code behavior.
Docs distinguish safe-default Unix, follow-symlink fallback, and non-Unix behavior.
No doc claims root fd is opened once at server startup unless it is cached in state.
No doc claims complete cross-platform filesystem hardening.
```

## Workstream C: decide root fd lifecycle

### Problem

`RootGuard::new(&config.root)` is still created per request. On Unix, that means the root directory fd is opened during each request, not once at server startup. This is acceptable, but docs currently imply startup-level root fd behavior.

### Options

Option A: keep per-request RootGuard and fix docs.

```text
Pros: minimal code churn, current behavior works.
Cons: repeated root canonicalize/open per request; docs must avoid startup-root-fd claim.
```

Option B: cache RootGuard or resolver state in ServeState.

```text
Pros: root fd truly opened at startup/config construction; lower per-request overhead; cleaner security story.
Cons: ServeState construction now can fail; API changes; test setup changes.
```

Recommended for this pass: Option A unless implementation is simple. This pass is about correctness, not lifecycle refactoring. If choosing Option B, keep it small and do not destabilize the public API unnecessarily.

### If choosing Option A

Update docs:

```text
The configured root is canonicalized and opened as a directory descriptor during request resolution.
```

### If choosing Option B

Refactor:

```rust
pub struct ServeState {
    pub config: Arc<ServeConfig>,
    pub root_guard: RootGuard,
    ...
}
```

or:

```rust
pub struct ServeState {
    pub config: Arc<ServeConfig>,
    pub resolver: FsResolverState,
    ...
}
```

This likely requires `ServeState::new` to return `Result<Self, io::Error>` or adding `ServeState::try_new`. Avoid breaking existing simple tests without benefit.

### Acceptance criteria

```text
Either root fd lifecycle is cached and tested, or docs precisely state per-request resolver creation.
No inaccurate startup-root-fd claim remains.
```

## Workstream D: clarify follow-symlinks release posture

### Problem

`--follow-symlinks` intentionally uses the fallback canonicalize/root-prefix model. That is weaker than Unix safe-default descriptor-relative traversal and retains a TOCTOU window if the served tree can be mutated concurrently.

### Target behavior

Keep follow mode available only if docs and release notes explicitly mark it as weaker/experimental.

Recommended language:

```text
The hardened filesystem guarantee applies to safe-default symlink-denied mode on Unix. `--follow-symlinks` is an explicit compatibility/advanced option that falls back to canonical path verification. It still denies final root escape, but it is not covered by the same TOCTOU-hardening guarantee.
```

### Optional implementation tightening

Consider startup warning when `--follow-symlinks` is enabled:

```text
warning: --follow-symlinks uses weaker canonicalize-based confinement; avoid for untrusted mutable roots
```

Only add this if logging/startup output already has a clean warning path.

### Acceptance criteria

```text
Docs identify follow mode as weaker/experimental.
Release docs exclude follow mode from hardened guarantee.
Optional startup warning if straightforward.
Tests still verify outside-root symlink denial under follow mode.
```

## Workstream E: Windows public release posture

### Problem

Windows reparse-point hardening is not complete. The parser rejects Windows path syntax hazards, but filesystem-level reparse/junction behavior is not production-audited.

### Target posture

For public release, state:

```text
Linux/macOS safe-default mode: hardened descriptor-relative traversal.
Windows: parser-level confinement and conservative policy, but reparse-point hardening is not yet audited; do not use for untrusted mutable public roots.
```

Do not claim Windows production-hardening until a separate Windows resolver plan lands.

### Docs to update

```text
README.md
docs/security-review.md
docs/security-policy.md
docs/compatibility.md
docs/release-criteria.md
docs/release-checklist.md
```

### Acceptance criteria

```text
Windows is not overclaimed.
Windows parser tests remain in CI.
A future Windows reparse-point plan is listed as post-beta/pre-1.0 work.
```

## Workstream F: CI and validation

### Required validation commands

Ensure these pass locally or in CI:

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

### Targeted filesystem tests

Add or verify:

```text
final symlink denied under Unix safe default
intermediate symlink denied under Unix safe default
nested intermediate symlink denied
index symlink denied under safe default
directory listing hides symlink entries under safe default
follow mode outside-root symlink still denied
docs or tests confirm service uses resolved file handles, not path reopen
```

### CI status note

If GitHub Actions status is not visible through automation, manually verify in GitHub UI or note inability to confirm. Do not claim green CI without evidence.

### Acceptance criteria

```text
All relevant tests pass.
No flaky race tests are introduced.
CI remains bounded and practical.
```

## Workstream G: release-readiness decision after corrections

After this pass, decide one of:

```text
Public beta: if Unix safe-default O_NOFOLLOW enforcement lands, docs are precise, Windows/follow-mode caveats are explicit, and CI passes.
Public alpha: if docs are precise but some hardening tests remain limited.
No public release yet: if O_NOFOLLOW cannot be applied reliably or CI fails.
```

Recommended target: public beta if the open-time no-follow fix is straightforward and CI passes.

## Suggested commit sequence

```text
fix(fs-unix): add open-time nofollow to descriptor-relative traversal
fix(fs-unix): map nofollow symlink errors to SymlinkDenied
 test(fs): strengthen unix symlink and index/listing regressions
docs(security): correct descriptor-relative traversal claims
docs(release): mark follow mode and Windows posture explicitly
chore(release): refresh validation checklist after filesystem corrections
```

## Review checklist

Before closing this pass, verify:

```text
openat calls in Unix safe-default path include NOFOLLOW or documented equivalent.
Docs no longer claim O_NOFOLLOW unless code uses it.
Docs no longer claim all TOCTOU windows are eliminated.
Service still serves pre-opened handles.
Directory listing still goes through resolver.
Follow mode remains outside hardened guarantee.
Windows remains caveated.
CI passes or failures are documented.
```

## Exit criteria

This corrective pass is complete when:

```text
Unix safe-default resolver has open-time no-follow enforcement where supported.
Documentation exactly matches the implementation.
Root fd lifecycle is either cached or described accurately.
Follow-symlink and Windows limitations are release-visible.
All targeted filesystem tests pass.
The repo is ready for a public alpha/beta release decision.
```
