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
| **resource limits enabled** | Connection count, request size, and rate limits are active |

These defaults are enforced at the library level in `eggserve-core`. They are not advisory — the code rejects non-conforming requests before any filesystem access.

## Path confinement implementation

The path confinement layer enforces the following before any filesystem access:

1. **Request-target parsing** — only HTTP origin-form paths (`/path`) are accepted. Absolute-form, authority-form, and asterisk-form are rejected with 400.
2. **Percent decoding** — single-pass decoding only. Double-encoded traversal (`%252e%252e`) is not decoded twice. Malformed encodings are rejected.
3. **Component validation** — `.` and `..` components are rejected. Empty components are normalized away. Components containing NUL, `/`, or `\` (by default) are rejected.
4. **Dotfile policy** — components starting with `.` are denied unless `DotfilePolicy::Serve` is explicitly configured.
5. **Platform checks** — Windows reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9), alternate data stream syntax (`:`), and drive prefixes (`C:`) are rejected cross-platform.
6. **Root confinement** — the resolved filesystem path is canonicalized and verified to remain within the configured root directory.
7. **Symlink policy** — symlinks are denied by default. When denied, `symlink_metadata` is used to detect symlinks before following them.

Malformed syntax returns 400 Bad Request. Policy violations return 403 Forbidden. No local filesystem paths are leaked in response bodies.

## Unsafe or weaker options

The following options weaken security defaults. Each requires an explicit CLI flag and is **not** the default:

### `--public`

Binds to all network interfaces (`0.0.0.0`) instead of loopback. Use only when the server must be accessible from other machines. The operator is responsible for network-level access control.

### `--follow-symlinks`

Enables following symbolic links. When enabled, symlinks are resolved and the resolved path is still checked against the configured root. Symlinks that resolve outside the root are denied regardless of this flag.

### `--directory-listing`

Enables HTML directory listing for directories without an index file. Directory listings expose file names, sizes, and modification times.

## Compatibility mode

eggserve may offer a compatibility mode that relaxes some defaults to match the behavior of `python -m http.server` more closely. If implemented:

- Compatibility mode will be clearly marked in CLI help and startup output
- It will require an explicit flag (e.g., `--unsafe-compat` or `--http-server-compat`)
- It will never be the default
- It will log a warning at startup when enabled
- It will not weaken path confinement or symlink escape prevention

The exact shape of compatibility mode is deferred to a later plan. The core security contract (path confinement, no root escape) is non-negotiable regardless of mode.
