# Threat Model

## Assets

eggserve protects the following assets:

- **filesystem root confidentiality** — contents of the served directory must not be exposed beyond what the operator intends
- **filesystem root integrity** — served content must not be modified through the serving interface
- **process availability** — the server must not be crashed or rendered unresponsive by malicious input
- **log integrity** — log output must not be injectable through request paths or headers
- **host resource stability** — the server must not consume excessive memory, CPU, or file descriptors
- **operator expectation that public serving is intentional** — if the operator did not pass `--public`, the server must not bind to a public interface

## Trust boundaries

- **The filesystem root:** everything under the configured directory is operator-controlled content. Everything outside it is out of scope for serving.
- **The network:** the server accepts HTTP requests from the network. Requests are untrusted input.
- **The CLI interface:** flags passed by the operator are trusted. The operator is assumed to be non-adversarial.
- **The process boundary:** the server process runs under the operator's user. Privilege escalation is out of scope.

## Attacker capabilities

An attacker can:

- Send arbitrary HTTP requests to the server
- Send malformed request targets (e.g., path traversal, invalid percent-encoding)
- Use percent-encoded traversal attempts (`%2e%2e%2f`)
- Hold connections open slowly (slowloris-style)
- Request large files repeatedly to exhaust resources
- Attempt log injection through paths or headers
- Attempt symlink/reparse-point escape to serve files outside the root
- Attempt platform-specific path bypasses (Windows `\\?\`, UNC paths, etc.)

**Windows note:** Parser-level protections reject Windows reserved names, ADS syntax, drive prefixes, and backslash in path components. However, filesystem-level reparse-point/NTFS junction hardening is deferred (documented non-goal). Windows is explicitly a trusted/local-use platform — do not use with untrusted mutable public content on Windows.

## Out-of-scope attacker capabilities (initial version)

The following are explicitly out of scope for the initial version:

- Local privileged attacker modifying served files concurrently
- Kernel or filesystem compromise
- Malicious operator-provided root directory that intentionally contains sensitive files
- Full reverse-proxy threat model
- TLS certificate lifecycle automation

## Central invariant

> **Under safe defaults, no remotely supplied request path may resolve to content outside the configured root, and no denied filesystem object class may be served.**

## Defensive layers

1. **Path confinement** — all request paths are parsed, percent-decoded, validated, and resolved against the configured root. The `ConfinedPath` parser rejects traversal (`..`), absolute paths, NUL bytes, malformed percent-encoding, backslash ambiguity, and platform-specific attacks (Windows reserved names, ADS, drive prefixes). The `RootGuard` canonicalizes the final path and verifies it remains within the root.
2. **Policy enforcement** — a security policy object controls what is allowed (methods, symlink following, dotfiles, directory listing). Defaults deny everything except direct file GET/HEAD.
3. **Input validation** — malformed request targets are rejected before path resolution. Percent-encoding is decoded exactly once. Double-encoded traversal is caught by per-component decode checks.
4. **Filesystem checks** — when symlink policy denies symlinks, on Unix, descriptor-relative traversal uses `statat(AT_SYMLINK_NOFOLLOW)` before each `openat(..., O_NOFOLLOW)` to detect symlinks at each path component and to refuse to follow them at open time. Intermediate components are opened with `O_DIRECTORY|O_NOFOLLOW`, final components with `O_RDONLY|O_NOFOLLOW`. On non-Unix or when `--follow-symlinks` is enabled, `symlink_metadata` is checked per component and the final canonical path is verified against the root; this fallback is **weaker** than the descriptor-relative path and is explicitly outside the hardened guarantee. Files are opened during resolution — never re-opened later by absolute path. Canonical root escape is rejected with `PathRejection::RootEscapeDenied`. Dotfile policy checks components at both the path-validation and filesystem-resolution layers. Directory listings also respect symlink policy and hide symlink entries when denied.
5. **Resource limits** — connection count (64 max), file-stream count (32 max), header read timeout (10s), response write timeout (60s), and request body metadata rejection (`Content-Length > 0`, invalid `Content-Length`, or any `Transfer-Encoding` on GET/HEAD) are enforced to prevent resource exhaustion.
6. **Sanitized logging** — all logged paths and headers are sanitized to prevent log injection.

## Primitive consumer trust boundaries

### Rust embedding consumers

- Must route all paths through `SecureRoot` or `ConfinedPath` parsing.
- Must not reconstruct paths from `safe_relative_components()` and reopen them — descriptor-relative hardening applies only when files are opened during resolution via `openat(O_NOFOLLOW)`.
- Must preserve `StaticPolicy` defaults unless the user explicitly opts in.

### Python primitive consumers

- Native primitives provide the same security posture as Rust primitives.
- `SecureRoot`, `ConfinedPath`, `StaticPolicy`, and response planners are backed by the same Rust code.
- Reopening paths in Python (e.g. using `open()` with a reconstructed path) is outside the security guarantee.

### Python server callback consumers

- The `Server` primitive runs a tokio runtime in a background thread. Python receives parsed `Request` objects and returns `Response` values.
- Socket I/O, HTTP parsing, connection acceptance, and timeout enforcement are handled by Rust.
- File streaming is handled by Rust; file bodies never pass through Python memory in the server path.
- The GIL is released during I/O operations, allowing other Python threads to run.
- Python callbacks may be untrusted from a latency/resource perspective but are not sandboxed. Rust enforces connection and I/O policy around them — timeout limits, connection caps, and file-stream quotas are not affected by Python callback behavior.
- Callback timeout does not cancel Python execution. The request task stops waiting at the configured deadline and returns 504, but the Python callback continues executing in the background. The callback semaphore permit is held until the Python function returns, meaning timed-out callbacks still count against the concurrency limit until they complete.
- Forced shutdown closes the Rust runtime and listener but cannot safely terminate Python code. Blocked Python work does not retain the listener or runtime task registry.
- The subprocess API manages the Rust binary; Python does not handle socket I/O.

### Downstream adapter authors

- ASGI/WSGI adapters should live out-of-tree (see `docs/extension-contract.md`).
- New APIs added for adapter authors must remain protocol- and framework-neutral.

## Unsafe path reconstruction risk

Extracting paths from `safe_relative_components()` and reopening them manually bypasses descriptor-relative TOCTOU hardening. This is safe for read-only metadata inspection but not for reopening files for serving.

## Request-body policy risk

eggserve rejects non-empty request bodies on GET/HEAD. Downstream adapters must enforce the same policy or explicitly document the difference.

## Header spoofing/normalization risk

Downstream adapters must be careful about header normalization and spoofing. eggserve's primitives validate method and body framing but do not normalize arbitrary headers.

## Response serialization risk

`StaticResponsePlan` values are framework-independent. Downstream adapters must correctly translate status codes, headers, and body plans to their framework's response API without losing security-relevant headers (e.g. `x-content-type-options: nosniff`).

## Callback-induced latency and backpressure risk

If a downstream adapter introduces Python callbacks into the request path, those callbacks may introduce latency and backpressure that eggserve's resource limits (connection count, file streams, timeouts) were not designed to handle.

## Trust boundary between Rust runtime and Python user code

Rust enforces path confinement, policy, and I/O limits. Python user code runs in the same process (via PyO3) or in a managed subprocess. eggserve does not sandbox Python application code.
