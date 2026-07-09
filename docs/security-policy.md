# Security Policy

## Safe defaults

eggserve ships with the following safe defaults. These are not configurable without explicit CLI flags:

| Default | Behavior |
|---------|----------|
| **bind to loopback** | Server binds to `127.0.0.1` unless `--public` is passed |
| **GET and HEAD only** | All other HTTP methods are rejected with 405 |
| **request bodies rejected** | Incoming request bodies are discarded without processing |
| **no symlink following** | Symlinks are denied unless `--follow-symlinks` is passed |
| **no dotfile serving** | Files starting with `.` are not served |
| **no directory listing** | Directory contents are not listed unless `--directory-listing` is passed |
| **unknown MIME as application/octet-stream** | Unrecognized file extensions are served with a safe binary MIME type |
| **malformed request targets rejected** | Invalid paths (traversal, encoding abuse, null bytes) return 400 |
| **logs sanitized** | Paths and headers are sanitized before writing to logs |
| **resource limits enabled** | Max 64 concurrent connections, 32 file streams, 10s header timeout, 60s write timeout, request bodies rejected |

These defaults are enforced at the library level in `eggserve-core`. They are not advisory — the code rejects non-conforming requests before any filesystem access.

## Path confinement implementation

The path confinement layer enforces the following before any filesystem access:

1. **Request-target parsing** — only HTTP origin-form paths (`/path`) are accepted. Absolute-form, authority-form, and asterisk-form are rejected with 400.
2. **Percent decoding** — single-pass decoding only. The percent-decoder converts `%XX` sequences to their byte value exactly once. Double-encoded traversal (`%252e%252e`) decodes to `%2e%2e` (a literal filename), not to `..`. After decoding, each component is re-checked: if the decoded result equals `.` or `..`, the request is rejected. This conservative approach means double-encoded paths are treated as literal filenames — they will resolve to 404 if no such file exists.
3. **Component validation** — `.` and `..` components are rejected. Empty components are normalized away. Components containing NUL, `/`, or `\` (by default) are rejected.
4. **Dotfile policy** — components starting with `.` are denied unless `DotfilePolicy::Serve` is explicitly configured.
5. **Platform checks** — Windows reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9), alternate data stream syntax (`:`), and drive prefixes (`C:`) are rejected cross-platform.
6. **Root confinement** — the resolved filesystem path is verified to remain within the configured root directory.
7. **Symlink policy** — symlinks are denied by default. On Unix, descriptor-relative traversal uses `statat(AT_SYMLINK_NOFOLLOW)` before each `openat(..., O_NOFOLLOW)` call to detect symlinks, so both final and intermediate symlinks are rejected. The `O_NOFOLLOW` flag also prevents an attacker from swapping a symlink into place between the stat and the open. On non-Unix or when `--follow-symlinks` is enabled, `symlink_metadata` is checked per component and the final canonical target is verified against the root.

Malformed syntax returns 400 Bad Request. Policy violations return 403 Forbidden. No local filesystem paths are leaked in response bodies.

## Unsafe or weaker options

The following options weaken security defaults. Each requires an explicit CLI flag and is **not** the default:

### `--public`

Binds to all network interfaces (`0.0.0.0`) instead of loopback. Use only when the server must be accessible from other machines. The operator is responsible for network-level access control.

### `--follow-symlinks`

Enables following symbolic links. When enabled, both final and intermediate symlinks are followed, and the resolved canonical path is still checked against the configured root. Symlinks whose final canonical target escapes the root are denied regardless of this flag.

**This mode falls back to canonicalize-based resolution and is weaker than the safe-default descriptor-relative path.** It is **not** covered by the same TOCTOU-hardening guarantee that applies to safe-default symlink-denied mode on Unix. Avoid `--follow-symlinks` for untrusted mutable roots.

## Request body metadata handling

For read-only methods (`GET`, `HEAD`), eggserve rejects any request that signals a body:

- `Content-Length: 0` — allowed
- `Content-Length: <positive integer>` — rejected with `413 Payload Too Large` under the default zero-body policy
- `Content-Length: <non-integer, negative, or overflowing value>` — rejected with `400 Bad Request`
- `Transfer-Encoding: <anything non-empty>` — rejected with `400 Bad Request`
- Both `Content-Length` and `Transfer-Encoding` present — rejected with `400 Bad Request`

This closes the previous behavior where malformed `Content-Length` values were silently ignored and `Transfer-Encoding` was not checked at all.

## Implementation status and limitations

On Unix (Linux, macOS) with safe defaults, eggserve resolves request paths relative to an opened root directory descriptor. Components are checked with `statat(..., AT_SYMLINK_NOFOLLOW)` and opened with `openat(..., O_NOFOLLOW)`. This prevents the service layer from reopening validated absolute paths and closes the primary final-object symlink-swap issue. Files are always opened during resolution — never re-opened later by absolute path.

On non-Unix platforms, or when `--follow-symlinks` is enabled, the implementation falls back to `symlink_metadata` checks plus `canonicalize` with root verification. Follow-symlinks mode is **not** covered by the descriptor-relative hardening guarantee.

The configured root is canonicalized and opened as a directory descriptor during request resolution (per request), not once at server startup. Caching the root descriptor across requests is a future optimization; current behavior is correct and tested.

Windows reparse-point detection beyond what the parser already denies is deferred. Do not use eggserve on Windows for untrusted mutable public roots. Directory listings hide symlink entries when symlink policy is denied.

### `--directory-listing`

Enables HTML directory listing for directories without an index file. Under safe defaults, symlink entries are hidden from listings. Directory listings expose file names and directory status.

### `--tls-cert` and `--tls-key` (requires `tls` feature)

Enables native TLS termination using rustls. When both flags are provided, the server accepts HTTPS connections. Certificate and key must be PEM-encoded. Encrypted private keys are not supported. The TLS feature is optional and not included in the default build. For public-facing deployments, a reverse proxy (Caddy, nginx, Traefik) is usually preferred over native TLS.

TLS handshakes are bounded by the same timeout as HTTP header reads (`--header-timeout`, default 10 seconds). A slow or stalled TLS client cannot tie up a connection beyond this window.

## Compatibility mode

eggserve may offer a compatibility mode that relaxes some defaults to match the behavior of `python -m http.server` more closely. If implemented:

- Compatibility mode will be clearly marked in CLI help and startup output
- It will require an explicit flag (e.g., `--unsafe-compat` or `--http-server-compat`)
- It will never be the default
- It will log a warning at startup when enabled
- It will not weaken path confinement or symlink escape prevention

The exact shape of compatibility mode is deferred to a later plan. The core security contract (path confinement, no root escape) is non-negotiable regardless of mode.
