# Security Model — Deep Dive

eggserve is designed as a hardened replacement for `python -m http.server`. Security is not a feature — it is the foundational constraint that shapes every architectural decision.

## Central Invariant

> **Under safe defaults, no remotely supplied request path may resolve to content outside the configured root, and no denied filesystem object class may be served.**

Root identity is pinned at startup: the serving root is opened once and the resulting file descriptor is retained for the server lifetime. Renaming or replacing the configured pathname does not redirect the running server to a different tree. This prevents an attacker who can mutate the filesystem from steering the server to alternate content after startup.

## Safe Defaults

Every security default is enforced at the library level unless the user explicitly passes a CLI flag:

| Default | Behavior | Opt-out Flag |
|---------|----------|-------------|
| Bind to loopback | `127.0.0.1` only | `--public` |
| GET/HEAD only | Other methods → 405 | N/A (hardcoded) |
| Request bodies rejected | Discarded without processing | Body policy config |
| No symlink following | Denied at path + filesystem layers | `--follow-symlinks` |
| No dotfile serving | `.` components rejected | `--allow-dotfiles` |
| No directory listing | Directories → 404 | `--directory-listing` |
| Unknown MIME → octet-stream | Safe binary fallback | N/A |
| Malformed targets → 400 | Traversal, encoding abuse, NUL | N/A |
| Sanitized logs | Paths/headers sanitized | N/A |
| Resource limits | 64 conns, 32 streams, 10s header, 60s write | CLI flags |

These defaults are not advisory — the code rejects non-conforming requests before any filesystem access.

## Trust Boundaries

```
┌─────────────────────────────────────────────────────┐
│                    Operator (trusted)                │
│  • CLI flags define policy                           │
│  • Root directory is operator-controlled content     │
└───────────────────────┬─────────────────────────────┘
                        │ CLI flags
                        ▼
┌─────────────────────────────────────────────────────┐
│                  eggserve-core (policy layer)        │
│  • Path confinement pipeline                         │
│  • Policy enforcement (symlink, dotfile, listing)    │
│  • Resource limits                                   │
│  • Canonical response normalization                  │
└───────────────────────┬─────────────────────────────┘
                        │ validated, confined requests
                        ▼
┌─────────────────────────────────────────────────────┐
│              Filesystem root (operator-controlled)   │
│  • Only files within root are accessible             │
│  • Descriptor-relative on Unix (no TOCTOU)           │
└─────────────────────────────────────────────────────┘
                        ▲
                        │ untrusted HTTP requests
┌─────────────────────────────────────────────────────┐
│                    Network (untrusted)               │
│  • Arbitrary HTTP requests                            │
│  • Malformed targets, traversal attempts              │
│  • Slowloris, resource exhaustion                     │
└─────────────────────────────────────────────────────┘
```

## Defensive Layers

### 1. Path Confinement (7-stage pipeline)

All request paths pass through `ConfinedPath::parse()` before touching the filesystem:

1. **Request-target parsing** — only origin-form (`/path`) accepted; absolute/authority/asterisk forms rejected
2. **Single-pass percent decoding** — `%XX` decoded exactly once; double-encoded traversal (`%252e%252e`) becomes literal `%2e%2e`, not `..`
3. **Normalization** — `//` collapsed, trailing slashes trimmed; `.` and `..` rejected by validation
4. **Component splitting** — path split into segments
5. **Per-component validation** — reject `.`, `..`, NUL bytes, backslash (default), dotfiles (default)
6. **Platform checks** — Windows reserved names, ADS syntax, drive prefixes (cross-platform)
7. **Root confinement** — resolved path verified to remain within root

See [path-confinement.md](path-confinement.md) for the full pipeline.

### 2. Policy Enforcement (layered)

Policies are checked at multiple stages:

| Stage | Policy | Effect |
|-------|--------|--------|
| Path validation | `path::DotfilePolicy` | Reject dotfile paths early |
| Path validation | `reject_backslash` | Reject `\` in paths |
| Filesystem resolution | `SymlinkPolicy` | Deny symlinks (descriptor-relative on Unix) |
| Filesystem resolution | Root confinement | Deny path escape |
| Response construction | `policy::DotfilePolicy` | Deny dotfiles at serving level |
| Response construction | `DirectoryListingPolicy` | Deny/allow directory listing |

Both `path::DotfilePolicy` and `policy::DotfilePolicy` must agree for dotfiles to be served — a double-check that ensures defense in depth.

See [policy-system.md](policy-system.md) for policy types and enforcement.

### 3. Filesystem Confinement (descriptor-relative on Unix)

Under safe defaults, symlink denial is **descriptor-relative**:

```
open(root_fd, O_DIRECTORY | O_NOFOLLOW)
    │
    for component in path:
        statat(fd, component, AT_SYMLINK_NOFOLLOW)
            → symlink? → Denied(SymlinkDenied)
        openat(fd, component, O_DIRECTORY | O_NOFOLLOW)  // intermediate
        openat(fd, component, O_RDONLY | O_NOFOLLOW)     // final
            → ELOOP/EMLINK? → Denied(SymlinkDenied)
    │
    final fd → ResolvedFile (never reopened by path)
```

Key properties:
- **No TOCTOU** — `O_NOFOLLOW` prevents symlink-swap between stat and open
- **Kernel-enforced** — symlink rejection is enforced by the kernel, not userspace
- **Pre-opened handles** — `ResolvedFile` carries a `File` handle; the file is never re-opened by path
- **Per-request isolation** — each request gets its own `RootGuard` and directory descriptor
- **Root identity** — the root directory is opened once at startup; subsequent renames or replacements of the original path do not redirect the server

See [filesystem-confinement.md](filesystem-confinement.md) for the full traversal algorithm.

### 4. Input Validation

- **Percent decoding** — single-pass only; double-encoded traversal becomes literal
- **Method validation** — only GET/HEAD for static serving; other methods return 405
- **Body framing** — TE+CL conflict → 400; duplicate CL → 400; malformed CL → wire-level rejection
- **Request body rejection** — bodies on GET/HEAD discarded before service invocation

### 5. Resource Limits

| Resource | Default | Effect |
|----------|---------|--------|
| Max connections | 64 | TCP accept semaphore; new connections dropped when exhausted |
| Max file streams | 32 | Concurrent file streaming; 503 when exhausted |
| Header read timeout | 10s | Slowloris protection |
| Response write timeout | 60s | Slow response protection |
| Graceful shutdown timeout | 10s | Drain period after SIGTERM |
| Request body size | 0 (rejected) | No bodies processed by default |
| Handler timeout | configurable | Per-request timeout for service processing |

### 6. Response Normalization

All response producers converge on a single normalization path:

- **HEAD suppression** — body bytes discarded, representation headers preserved
- **Body-forbidden enforcement** — 1xx, 204, 304 bodies discarded
- **Hop-by-hop stripping** — `Transfer-Encoding` removed (runtime-owned)
- **Content-Length computation** — set to actual body length
- **Duplicate preservation** — end-to-end duplicate headers preserved

Services cannot bypass final framing policy through the safe API.

### 7. Sanitized Logging

All logged paths and headers are sanitized to prevent log injection through request paths or headers.

## Attacker Model

### In Scope

An attacker can:
- Send arbitrary HTTP requests to the server
- Send malformed request targets (path traversal, invalid percent-encoding)
- Use percent-encoded traversal attempts (`%2e%2e%2f`)
- Hold connections open slowly (slowloris-style)
- Request large files repeatedly to exhaust resources
- Attempt log injection through paths or headers
- Attempt symlink/reparse-point escape to serve files outside the root
- Attempt platform-specific path bypasses (Windows `\\?\`, UNC paths, etc.)

### Out of Scope

- Local privileged attacker modifying served files concurrently
- Kernel or filesystem compromise
- Malicious operator-provided root directory
- Full reverse-proxy threat model
- TLS certificate lifecycle automation

## Platform Security

| Platform | Security Model | Limitations |
|----------|---------------|-------------|
| Linux (x86_64, aarch64) | Descriptor-relative traversal via `statat`+`openat` | None (fully hardened) |
| macOS (x86_64, aarch64) | Same descriptor-relative guarantees | None (fully hardened) |
| Windows (x86_64) | Parser-level checks + handle-relative child resolution (Plan 084) + directory enumeration (Plan 085) + adversarial qualification scaffold (Plan 086) | Reparse-point qualification in progress; not for untrusted mutable public content until Plan 086 closes |

## Consumer Trust Boundaries

### Rust Embedders

- Must route all paths through `SecureRoot` or `ConfinedPath`
- Must not reconstruct paths from `safe_relative_components()` and reopen them
- Must preserve `StaticPolicy` defaults unless user explicitly opts in

### Python Consumers

- Native primitives provide the same security posture as Rust
- Reopening paths in Python (e.g. `open()` with reconstructed path) is outside the guarantee

### Python Server Callbacks

- Socket I/O, HTTP parsing, timeout enforcement handled by Rust
- File streaming handled by Rust; files never pass through Python memory
- Callback timeout does not cancel Python execution — the callback continues in background
- Callback semaphore permit held until Python function returns
- Python callbacks are not sandboxed; Rust enforces I/O limits around them

## See Also

- [../docs/threat-model.md](../docs/threat-model.md) — Full threat model
- [../docs/security-policy.md](../docs/security-policy.md) — Safe defaults and opt-in behaviors
- [../docs/security-review.md](../docs/security-review.md) — Alpha security posture
- [path-confinement.md](path-confinement.md) — Path validation pipeline
- [filesystem-confinement.md](filesystem-confinement.md) — Descriptor-relative traversal
- [policy-system.md](policy-system.md) — Policy types and enforcement
- [overview.md](overview.md) — Architecture overview
