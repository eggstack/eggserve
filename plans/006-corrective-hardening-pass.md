# Plan 006: corrective hardening pass

## Goal

Close the highest-risk gaps found after the first implementation passes. The repo is in strong alpha shape, but several items currently weaken the security and production-readiness story: file-stream permits are not held for the lifetime of the stream, some exposed limits are not enforced, index handling partially bypasses the central filesystem policy path, CLI bind semantics do not match the help text, Python packaging is not validated by CI, and cross-platform/path hardening is still incomplete.

This is a corrective pass, not a feature expansion pass. Do not add Range, TLS, CORS, authentication, config files, compression, HTTP/2, or a Python API during this work unless explicitly required to fix a defect.

## Current state summary

The repository has a useful core implementation: a Hyper/Tokio HTTP/1 server, safe loopback defaults, `GET`/`HEAD` file serving, directory-index behavior, opt-in directory listings, path parsing and denial logic, static MIME mapping, weak ETags, Last-Modified, security headers, CLI parsing, and a thin Python wheel launcher skeleton.

The main issues are correctness and hardening gaps:

```text
1. file_stream_semaphore permits are dropped immediately after constructing the response body instead of being held until EOF/body drop.
2. CLI exposes max_header_bytes, max_request_target_bytes, idle_timeout, and max_in_flight_requests semantics that are not fully enforced.
3. index.html lookup uses a direct join from a resolved directory instead of the same RootGuard/policy path used for normal file requests.
4. RootGuard uses canonicalize + prefix checks and final-object symlink detection; this is acceptable for alpha but insufficient as the final anti-TOCTOU model.
5. --bind help says HOST, but implementation parses SocketAddr and therefore requires host:port.
6. Python packaging crate is excluded from the workspace and not validated by the default CI.
7. CI is Linux-only and does not validate Windows/macOS path behavior.
8. Conservative second-pass rejection of components such as %2e%2e is implemented but not clearly documented as a deliberate policy.
```

## Non-goals for this pass

Do not broaden eggserve into a general web server. Specifically avoid:

```text
Range requests
TLS/rustls integration
HTTP/2
CORS
Basic auth
upload/write methods
ASGI/WSGI concepts
Python request callbacks
reverse proxying
compression
ACME
config files unless required for tests
new CLI features beyond correcting exposed semantics
```

## Workstream A: hold file-stream permits for the real stream lifetime

### Problem

`handle_request` currently acquires a file-stream permit, builds a response body, then drops the permit before the body is consumed. This means `max_file_streams` limits only response construction, not active file streaming.

### Target behavior

A file-stream permit must remain alive until one of these occurs:

```text
the file body reaches EOF
the response body is dropped because the client disconnects
the response fails or is cancelled
HEAD response completes immediately without consuming a file-stream permit, if no file body will be streamed
```

### Implementation guidance

Do not keep the current pattern:

```rust
let permit = semaphore.try_acquire_owned()?;
let resp = file_response(...);
drop(permit);
resp
```

Instead, change `file_response` or add a new response constructor that accepts an `OwnedSemaphorePermit` and moves it into the stream state. A simple design:

```rust
struct FileStreamState {
    file: tokio::fs::File,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
```

Then use `futures_util::stream::unfold(FileStreamState { file, _permit }, ...)` so the permit lives as long as the stream state. If EOF is reached, the state is dropped. If the body is dropped, the state is also dropped.

For `HEAD`, avoid opening the file stream and avoid acquiring a file-stream permit unless there is a concrete reason to preserve identical error behavior. The metadata has already been collected during resolution. Return headers only.

### Tests

Add a test that would fail with the current implementation. The current pre-acquire-all-permits test only proves exhaustion before response creation; it does not prove permits are held during streaming.

Recommended tests:

```text
file_stream_permit_held_until_body_drop:
  set max_file_streams = 1
  request large file A and keep its response body alive without fully collecting it
  request file B
  assert file B returns 503 while A body is alive
  drop A body
  request file B again
  assert file B now returns 200

head_does_not_consume_file_stream_permit:
  set max_file_streams = 0 or pre-acquire all permits
  HEAD existing file should still return 200 if design does not require streaming permit
```

If setting `max_file_streams = 0` is invalid, validate that zero is rejected at configuration/CLI parse time.

### Acceptance criteria

```text
File-stream permits are held for the actual response body lifetime.
The old exhaustion test is replaced or supplemented with a lifetime-sensitive test.
HEAD requests do not unnecessarily consume file-stream capacity.
No unbounded file stream path remains.
```

## Workstream B: enforce or hide exposed resource limits

### Problem

The code exposes limit fields and CLI flags for `max_header_bytes`, `max_request_target_bytes`, `idle_timeout`, and `max_in_flight_requests`, but the implementation does not clearly enforce all of them. Exposed no-op safety knobs are worse than absent knobs.

### Target behavior

Every exposed limit must be enforced, tested, and documented. If a limit cannot be enforced cleanly with the current Hyper stack, remove the CLI flag and mark the field internal/future-only until it can be enforced.

### Required decisions

For each limit, choose `enforce now` or `hide now`:

```text
max_connections: enforce now; already enforced by accept-loop semaphore.
max_file_streams: enforce now after Workstream A.
max_header_bytes: enforce now if Hyper http1 builder supports this in the selected version; otherwise hide CLI flag.
max_request_target_bytes: enforce now in request/path layer before parse or at start of ConfinedPath::parse.
max_request_body_bytes: enforce now for Content-Length and reject transfer-encoded bodies if possible.
header_read_timeout: enforce now; currently applied through http1 Builder.
response_write_timeout: enforce now or document coarse connection-level behavior.
idle_timeout: enforce now or hide CLI flag.
max_in_flight_requests: for HTTP/1, either alias to max_connections and document it or hide the flag/field until pipelining semantics are explicit.
```

### Implementation guidance

`max_request_target_bytes` should be checked before percent decoding and before allocation-heavy operations. Add a length check to `ConfinedPath::parse` or an equivalent entrypoint that receives `Limits`/max length.

If the path parser should remain policy-only, add a `PathPolicy { max_raw_bytes: usize, ... }` field and set it from config.

For request bodies, `Content-Length > 0` already returns 413 under the default `max_request_body_bytes = 0`. Also handle ambiguous body-bearing requests:

```text
Transfer-Encoding present on GET/HEAD -> reject
Content-Length parse failure -> 400 Bad Request
Content-Length overflow/invalid -> 400 Bad Request
Content-Length > limit -> 413 Payload Too Large
```

For `max_header_bytes`, inspect Hyper 1.x builder capabilities. If available, wire it. If not, remove `--max-header-bytes` from CLI help and docs for now, and add a TODO in the code/docs explaining why.

For `idle_timeout`, if Hyper does not expose a straightforward keep-alive idle timeout, either implement a connection wrapper or hide the flag. Do not leave it as a no-op.

### Tests

Add tests for:

```text
request target over configured max returns 400 or 414; choose and document status
invalid Content-Length returns 400
Content-Length over limit returns 413
Transfer-Encoding on GET/HEAD is rejected
hidden/no-op flags are removed from CLI tests and docs, if not enforced
```

If header/idle timeouts are enforced only at the network layer, add integration tests with raw TCP where practical. Keep timeouts small in tests but avoid flakes.

### Acceptance criteria

```text
No exposed CLI limit flag is a no-op.
Every exposed limit has at least one test.
Docs/CLI help match the actual enforcement behavior.
No ignored safety field is presented as active protection.
```

## Workstream C: unify index handling with filesystem policy

### Problem

Normal file requests go through `ConfinedPath` and `RootGuard::resolve`, but directory index handling directly joins `dir_path.join("index.html")`, checks final metadata, and opens the result. This duplicates policy logic and risks drift.

### Target behavior

Index lookup must use the same policy semantics as normal file lookup. There should be one filesystem-policy path for serving file content.

### Implementation options

Preferred option: add a method to `RootGuard`:

```rust
pub fn resolve_index(
    &self,
    directory: &ResolvedDirectory,
    index_name: &str,
    policy: &StaticPolicy,
) -> ResolvedResource
```

This method should:

```text
validate index_name as a safe component
resolve it relative to the same root guard
apply dotfile/symlink policy
return File, NotFound, or Denied
```

Alternative option: have `ResolvedDirectory` carry enough safe relative path information to append `index.html` and call the normal resolve pipeline. This may be cleaner long-term but may require changing `ResolvedDirectory` from only absolute canonical path to include safe components.

Avoid direct `dir_path.join("index.html")` in service code.

### Tests

Add tests for:

```text
index.html regular file serves normally
index.html symlink denied when symlinks denied
index.html symlink allowed only when policy permits and target remains within root
index lookup does not leak absolute path on error
index lookup respects dotfile policy if custom index names are added later
intermediate symlink directory cases are covered at least on Unix
```

### Acceptance criteria

```text
Service directory handling no longer opens index files by direct path join.
Index serving uses RootGuard or equivalent centralized filesystem policy.
Existing index tests still pass.
New symlink/index policy tests pass.
```

## Workstream D: document and test conservative percent-encoding semantics

### Problem

The percent decoder decodes exactly once, but component validation conservatively rejects components that would decode to `.` or `..` if decoded again. This is stricter than the initial written plan, which treated `%2e%2e` after one decode as a literal component.

### Target behavior

Keep the stricter behavior, but make it explicit. It is defensible for a hardened static server: paths containing residual encoded structural dot components are suspicious and should fail closed.

### Documentation update

Update `docs/security-policy.md`, `docs/compatibility.md`, and possibly `docs/threat-model.md` with language like:

```text
eggserve decodes request paths exactly once for path construction. After that decode, it also rejects any remaining component text that would decode to `.` or `..`. This is a conservative fail-closed rule intended to prevent ambiguity across clients, proxies, and filesystems. eggserve does not recursively decode paths for lookup.
```

### Test cleanup

Rename misleading tests if needed. For example, `reject_double_encoded_dotdot` is fine, but `reject_double_encode_does_not_double_decode` in `decode.rs` should be clearly understood as decoder-only behavior. Add a higher-level test explaining that the parser rejects residual encoded structural dot components by policy.

### Acceptance criteria

```text
Docs and tests agree on one policy.
The code still does not recursively decode for filesystem lookup.
Ambiguous residual encoded dot components are rejected by policy.
```

## Workstream E: improve filesystem escape and symlink test coverage

### Problem

The current tests cover final-object symlinks and basic traversal. They do not appear to fully cover intermediate symlink components, symlink-to-outside under `Follow`, or platform-specific Windows behavior in CI.

### Target behavior

Add regression tests that make the current and future filesystem policy behavior explicit. This is especially important before replacing canonicalize-prefix resolution with descriptor-relative traversal.

### Required Unix tests

Add Unix-gated tests for:

```text
intermediate_symlink_denied_when_symlinks_denied:
  root/linkdir -> root/real_dir
  request /linkdir/file.txt
  expect 403 under safe defaults

intermediate_symlink_escape_denied_even_when_follow_enabled:
  root/out -> /tmp/outside_dir
  request /out/secret.txt
  expect 403 even when symlinks Follow, unless policy explicitly permits outside-root follows, which it should not

final_symlink_to_outside_denied_when_follow_enabled:
  root/link.txt -> outside/secret.txt
  request /link.txt
  expect 403 even when symlinks Follow

nested_symlink_escape_denied:
  root/a/b link chain escapes root
  expect 403
```

If the current implementation fails any of these, fix the policy before expanding features.

### Windows tests

Add Windows CI and tests for:

```text
reserved names denied: CON, PRN, AUX, NUL, COM1, LPT1
ADS denied: file.txt:stream
backslash denied
percent-encoded backslash denied
absolute/drive-like prefixes denied
```

If Windows filesystem reparse-point tests are hard, add a documented TODO, but still run path parser tests on Windows.

### Acceptance criteria

```text
Intermediate symlink behavior is tested.
Outside-root symlink behavior is tested.
Windows path parser tests run in CI.
Any failing policy behavior is fixed before this pass closes.
```

## Workstream F: fix CLI bind semantics and docs

### Problem

Help text says `--bind <HOST>`, but the parser currently expects a full `SocketAddr`. This makes `--bind 127.0.0.1 --port 8000` fail even though the planned semantics and help imply it should work.

### Target behavior

`--bind` should accept a host/IP without a port. `--addr` should accept `HOST:PORT`. `--port` should control the port when `--bind` is host-only.

Recommended behavior:

```text
--bind 127.0.0.1 -> bind_ip = 127.0.0.1, keep current/default port
--bind localhost -> either support host resolution intentionally or reject with clear message; prefer IP-only for now unless DNS resolution is wanted
--bind 127.0.0.1 --port 9000 -> 127.0.0.1:9000
--addr 127.0.0.1:9000 -> full override
--addr 0.0.0.0:8000 without --public -> fail
--bind 0.0.0.0 --public -> allowed
--bind 0.0.0.0 without --public -> fail
```

If hostname support is deferred, update docs to say `--bind <IP>` rather than `HOST`.

### Tests

Add CLI tests:

```text
bind_host_only_uses_default_port
bind_host_only_with_port_flag
addr_overrides_bind_and_port
bind_unspecified_without_public_fails
bind_unspecified_with_public_succeeds
bind_with_colon_returns_helpful_error_suggesting --addr, if host-only is enforced
```

### Acceptance criteria

```text
CLI help and parser behavior agree.
Python-http-server-like bind/port usage works.
Public exposure guard still works.
```

## Workstream G: Python packaging and CI validation

### Problem

The Python package skeleton exists, but it is excluded from the root workspace and is not validated by default CI. There is no visible maturin smoke test.

### Target behavior

Add a CI job that validates the Python wheel packaging without bloating normal Rust checks.

Recommended CI additions:

```text
rust-check: existing cargo fmt/clippy/test workspace job
python-wheel-smoke:
  checkout
  install Rust + Python 3.11
  pip install maturin
  cd crates/eggserve-python
  maturin build --release or maturin build --debug if supported
  pip install produced wheel
  python -m eggserve --version
  python -m eggserve --help
```

If starting the server in CI is easy, add a smoke test:

```text
create temp dir with hello.txt
start python -m eggserve --directory temp --port ephemeral or fixed high port
request hello.txt with Python stdlib urllib
terminate process
assert body matches
```

Avoid adding pytest unless needed. A tiny Python script under `scripts/python_wheel_smoke.py` is enough.

### Acceptance criteria

```text
CI validates that the Python wheel builds.
CI validates `python -m eggserve --version` and `--help`.
A smoke server test exists if reliable.
Docs mention how to run the packaging check locally.
```

## Workstream H: CI matrix and dependency guardrails

### Problem

Current CI is Linux-only. That is insufficient for a security-sensitive path-handling project that explicitly denies Windows-specific path forms.

### Target behavior

Expand CI to at least:

```text
ubuntu-latest
macos-latest
windows-latest
```

Keep the job set modest:

```text
cargo fmt on ubuntu only
cargo clippy on ubuntu, optionally all platforms if runtime is acceptable
cargo test --workspace on all platforms
python wheel smoke at least ubuntu, later all platforms
```

Add dependency tooling once the dependency tree is stable enough:

```text
cargo audit
cargo deny check advisories bans licenses sources
```

If adding `cargo deny` immediately is too noisy, add `deny.toml` with a permissive starting point and tighten in a later pass.

### Acceptance criteria

```text
Path parser tests run on Windows.
Core tests run on macOS and Linux.
CI runtime remains reasonable.
Dependency audit/deny path is documented or enabled.
```

## Workstream I: documentation synchronization

Update docs to match actual implementation after the corrections.

Required docs to touch:

```text
README.md
docs/cli.md
docs/security-policy.md
docs/compatibility.md
docs/python-packaging.md
docs/dependency-policy.md
docs/release-criteria.md
```

Specific updates:

```text
Explain enforced limits only; remove claims for hidden limits.
Document public bind behavior.
Document conservative residual encoded dot-component rejection.
Document current filesystem confinement model as alpha if descriptor-relative traversal is not yet implemented.
Document Python wheel smoke command.
Document that Range/TLS/compression remain out of scope for now.
```

## Validation checklist

Before closing this corrective pass, run locally where possible:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core
cargo test -p eggserve-bin
```

For Python packaging:

```bash
cd crates/eggserve-python
maturin build
python -m pip install --force-reinstall target/wheels/*.whl
python -m eggserve --version
python -m eggserve --help
```

If local packaging paths differ, document the exact command that works.

## Acceptance criteria for the whole pass

This pass is complete when:

```text
File-stream permits are lifetime-correct.
No exposed resource-limit flag is a no-op.
Index serving uses centralized filesystem policy.
Conservative percent-encoding behavior is documented.
Intermediate/outside-root symlink behavior is tested and safe.
CLI bind semantics match docs.
Python wheel build is validated by CI or a documented smoke job.
Windows/macOS CI runs at least the Rust test suite.
Docs are synchronized with actual behavior.
No new broad server features were introduced.
```

## Suggested commit sequence

Use small commits so review can isolate risk:

```text
fix(core): hold file stream permits for response lifetime
fix(core): enforce request target and request body limits
fix(core): unify index lookup with root guard policy
fix(cli): align bind and addr parsing semantics
chore(test): add symlink escape and path policy regressions
chore(ci): add platform matrix and python wheel smoke
 docs: synchronize security and CLI behavior
```

## Risk notes

The highest-risk change is filesystem policy refactoring. Avoid mixing it with unrelated CLI or CI changes. If descriptor-relative traversal is attempted in this pass, isolate it behind a small internal API and keep canonicalize-prefix fallback tests in place. It is acceptable to defer full descriptor-relative traversal to a later dedicated plan, but the current alpha limitation must remain documented.
