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
| No symlinks | Descriptor-relative `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)` on Unix; component-wise `symlink_metadata` on non-Unix |
| No dotfiles | Component-level dotfile check |
| No directory listing | Explicit opt-in required |
| Unknown MIME as `application/octet-stream` | Safe fallback |

## Filesystem traversal model

### Current implementation

On Unix (Linux, macOS), eggserve uses **descriptor-relative traversal** for safe-default mode (symlinks denied):

1. Each path component is validated (no `.`, `..`, NUL, backslash)
2. Dotfile policy is checked per component
3. The configured root is canonicalized and opened as a directory descriptor during request resolution (per request)
4. Each path component is resolved using `statat(AT_SYMLINK_NOFOLLOW)` to detect symlinks before opening
5. Intermediate components are opened with `openat(O_DIRECTORY | O_NOFOLLOW)`, final components with `openat(O_RDONLY | O_NOFOLLOW)`
6. The canonical path is tracked and verified to remain within the configured root

The `O_NOFOLLOW` flag prevents the service layer from following a symlink that an attacker swapped into place between the `statat` check and the `openat`. Without this flag, the descriptor-relative path would still be safer than absolute-path reopens, but it would not be hardened against swap attacks.

On non-Unix platforms, or when `--follow-symlinks` is enabled, eggserve falls back to **canonicalize-based** resolution: `symlink_metadata` checks per component, then `fs::canonicalize` on the final path with root-containment verification. Files are opened during resolution (not re-opened later). **Follow-symlinks mode is not covered by the descriptor-relative hardening guarantee and is explicitly weaker than the safe-default path.**

This is sufficient to deny:
- Parent traversal (`../../../etc/passwd`)
- Symlink escape (both intermediate and final)
- Symlink-swap between `statat` and `openat` for the final component on Linux and macOS
- Double-encoded traversal (`%252e%252e`)

Each denial reason is preserved in the `ResolvedResource::Denied(PathRejection)` variant so the boundary between parser-layer and filesystem-layer denials is explicit. `SymlinkDenied`, `RootEscapeDenied`, and `DotfileDenied` are produced by the filesystem layer; all other variants are produced by the parser.

### macOS note

On macOS, `openat` with `O_DIRECTORY|O_NOFOLLOW` on a symlink-to-directory does not always return `ELOOP` (some filesystems return success and the kernel may follow the symlink). The implementation relies on `statat(AT_SYMLINK_NOFOLLOW)` before `openat` to detect symlinks explicitly and deny the request before reaching `openat`. The `openat` `O_NOFOLLOW` flag is a defense-in-depth backstop for swap attacks on platforms where it behaves correctly. A small TOCTOU window for swap attacks on intermediate directory components may remain on some macOS configurations; the final-component swap window is closed by the no-follow open.

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

1. **Windows reparse-point hardening** — not fully audited; Windows is supported with parser-level checks but production hardening is a documented non-goal (`docs/non-goals.md`). Windows is explicitly a trusted/local-use platform. **Do not use on Windows for untrusted mutable public roots.**
2. **Follow-symlinks mode uses canonicalize-based resolution** — TOCTOU window exists when `--follow-symlinks` is enabled; final canonical path is still verified against root. This mode is **not** covered by the descriptor-relative hardening guarantee and is treated as weaker/experimental.
3. **macOS intermediate-component TOCTOU** — on macOS, the statat-to-openat gap for intermediate directory components may not be fully closed by `O_NOFOLLOW` on some filesystem configurations; the final component's `O_NOFOLLOW` open prevents swap attacks where supported by the platform.
4. **Single-range requests only** — multi-range requests are not supported
5. **No HTTP/2** — HTTP/1.1 only
6. **No native TLS by default** — requires `tls` feature flag
7. **No request body processing** — all bodies rejected on GET/HEAD
8. **No authentication** — access control is network-level only (loopback bind)

## Reporting process

To report a security vulnerability, email dbowman91@proton.me. See [SECURITY.md](../SECURITY.md) for details.
