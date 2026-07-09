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
4. **Filesystem checks** — when symlink policy denies symlinks, on Unix, descriptor-relative traversal uses `statat(AT_SYMLINK_NOFOLLOW)` before `openat` to detect symlinks at each path component. Intermediate components are opened with `O_DIRECTORY`, final components with `O_RDONLY`. On non-Unix or when `--follow-symlinks` is enabled, `symlink_metadata` is checked per component and the final canonical path is verified against the root. Files are opened during resolution — never re-opened later by absolute path. Canonical root escape is rejected with `PathRejection::RootEscapeDenied`. Dotfile policy checks components at both the path-validation and filesystem-resolution layers. Directory listings also respect symlink policy and hide symlink entries when denied.
5. **Resource limits** — connection count (64 max), file-stream count (32 max), header read timeout (10s), response write timeout (60s), and request body metadata rejection (`Content-Length > 0`, invalid `Content-Length`, or any `Transfer-Encoding` on GET/HEAD) are enforced to prevent resource exhaustion.
6. **Sanitized logging** — all logged paths and headers are sanitized to prevent log injection.
