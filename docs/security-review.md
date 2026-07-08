# Security Review — Alpha

This document summarizes the security posture of eggserve as of the alpha release. It is intended for early adopters evaluating the project.

## Threat model summary

eggserve defends against:

- **Path traversal** — request targets containing `..`, encoded traversal, or malformed paths are rejected before filesystem access
- **Symlink escape** — symlinks are denied by default; when allowed, canonical path verification prevents escape from the configured root
- **Dotfile exposure** — hidden files are excluded unless explicitly opted in
- **Directory listing leakage** — directory contents are not exposed unless explicitly enabled
- **Method abuse** — only GET and HEAD are allowed; all other methods return 405
- **Request body abuse** — request bodies on GET/HEAD are rejected (Content-Length > 0 returns 413, malformed framing returns 400)
- **Resource exhaustion** — connection limits, file-stream concurrency limits, header size limits, and timeouts prevent resource exhaustion
- **Log injection** — paths and headers are sanitized before logging

## Safe defaults

Every security default is enforced at the library level in `eggserve-core`. See [security-policy.md](security-policy.md) for the full list.

| Default | Enforcement |
|---------|-------------|
| Loopback bind | `127.0.0.1` unless `--public` |
| GET/HEAD only | 405 for other methods |
| No request bodies | 413/400 for body signals |
| No symlinks | Component-wise `symlink_metadata` check |
| No dotfiles | Component-level dotfile check |
| No directory listing | Explicit opt-in required |
| Unknown MIME as `application/octet-stream` | Safe fallback |

## Filesystem traversal model

### Current implementation (alpha)

The current implementation uses **component-wise metadata checks** plus **canonical-root verification**:

1. Each path component is validated (no `.`, `..`, NUL, backslash)
2. Dotfile policy is checked per component
3. Symlink policy is checked per component using `symlink_metadata`
4. The final resolved path is canonicalized and verified to remain within the configured root

This is sufficient to deny:
- Parent traversal (`../../../etc/passwd`)
- Symlink escape (both intermediate and final)
- Double-encoded traversal (`%252e%252e`)

Each denial reason is preserved in the `ResolvedResource::Denied(PathRejection)` variant so the boundary between parser-layer and filesystem-layer denials is explicit. `SymlinkDenied`, `RootEscapeDenied`, and `DotfileDenied` are produced by the filesystem layer; all other variants are produced by the parser.

### Known limitation

This is **not** descriptor-relative (`openat`-style) traversal. There is a theoretical TOCTOU window between the `symlink_metadata` check and the eventual file open. This window is narrow in practice (no concurrent filesystem modification assumed) but is acknowledged as a hardening target for 1.0.

### Future hardening (1.0 target)

Descriptor-relative traversal using `openat`/`O_DIRECTORY` on Unix would eliminate the TOCTOU window by resolving paths through directory file descriptors rather than absolute paths.

## Request body policy

For read-only methods (GET, HEAD), eggserve rejects any request that signals a body:

- `Content-Length: 0` — allowed
- `Content-Length: <positive integer>` — rejected with 413
- `Content-Length: <malformed>` — rejected with 400
- `Transfer-Encoding: <anything>` — rejected with 400
- Both headers present — rejected with 400

This prevents request-smuggling and body-based attacks on a read-only server.

## Directory listing policy

When directory listing is enabled:
- Symlink entries are hidden when symlink policy is denied
- Only file names and directory status are exposed
- No file contents are exposed through listings

## Dependency review status

- All dependencies are from crates.io
- No git dependencies
- No unknown registries
- `cargo audit` is run as part of CI
- `cargo-deny` configuration is present for automated license/advisory checking

## Known limitations

1. **Component-wise traversal, not descriptor-relative** — TOCTOU window exists between metadata check and file open
2. **Windows reparse-point hardening** — not fully audited; Windows is supported with parser-level checks but production hardening is deferred
3. **No Range requests** — full-file streaming only
4. **No HTTP/2** — HTTP/1.1 only
5. **No native TLS by default** — requires `tls` feature flag
6. **No request body processing** — all bodies rejected on GET/HEAD
7. **No authentication** — access control is network-level only (loopback bind)

## Reporting process

To report a security vulnerability, email dbowman91@proton.me. See [SECURITY.md](../SECURITY.md) for details.
