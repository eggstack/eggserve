# Security Model — Deep Dive

The runtime boundary keeps static file capabilities handle-backed until
transport conversion, which owns the single server-wide file-stream pool.
Static and custom services share that admission point; custom startup has no
implicit static root. Body policy is service-declared per actual method, while
TRACE content and incomplete streamed bodies close at the transport boundary.

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library with a Python `http.server`-shaped facade. Security
is the foundational constraint that shapes every architectural decision.

## Central Invariant

> **Under safe defaults, no remotely supplied request path may resolve to content outside the configured root, and no denied filesystem object class may be served.**
>
> Normative form in [`docs/threat-model.md`](../docs/threat-model.md): no
> remotely supplied request may cause eggserve to read or serve an object
> outside the pinned root, and malformed or ambiguous HTTP input must not
> cause cross-request or frontend/backend message-boundary confusion.

Root identity is pinned at startup: the serving root is opened once and the resulting file descriptor is retained for the server lifetime. Renaming or replacing the configured pathname does not redirect the running server to a different tree. This prevents an attacker who can mutate the filesystem from steering the server to alternate content after startup.

## Safe Defaults

Every security default is enforced at the library level unless the user explicitly passes a CLI flag:

| Default | Behavior | Opt-out Flag |
|---------|----------|-------------|
| Bind to loopback | `127.0.0.1` only | `--public` |
| GET/HEAD only (static) | Other methods → 405 | N/A (hardcoded) |
| Request bodies rejected (static) | Body-bearing requests rejected before dispatch (413); bodyless requests reach handler | Custom service body policy |
| No symlink following | Denied at path + filesystem layers | `--follow-symlinks` |
| No dotfile serving | `.` components rejected | `--allow-dotfiles` |
| No directory listing | Directories → 403 | `--directory-listing` |
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
│  eggserve policy + runtime authority                │
│  (primitives policy, server runtime, static          │
│   authority; core is the compatibility umbrella)     │
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

### 1. Path Confinement (5-stage pipeline + filesystem root check)

All HTTP request targets are classified by `RequestTarget::parse()`, then their
validated path component passes through `ConfinedPath::from_path_component()`
before touching the filesystem. `ConfinedPath::parse()` remains the public
raw-target compatibility adapter:

1. **Request-target parsing** — origin-form (`/path`) accepted by default
   (`OriginOnly`, Plan 278); absolute-form is opt-in per service, and static
   serving still rejects absolute-form targets pre-resolution.
   Absolute/authority/asterisk forms are otherwise rejected
2. **Single-pass percent decoding** — `%XX` decoded exactly once; double-encoded traversal (`%252e%252e`) becomes literal `%2e%2e`, not `..`
3. **Normalization** — `//` collapsed, trailing slashes trimmed; `.` and `..` rejected by validation
4. **Component splitting** — path split into segments
5. **Per-component validation** — reject `.`, `..`, NUL bytes, backslash (default), dotfiles (default), Windows reserved names, ADS syntax, drive prefixes (cross-platform)

Root confinement (resolved path verified to remain within root) is enforced during filesystem resolution, after the path handoff returns.

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
- **Body framing** — lone CL alongside TE discarded by Hyper 1.11 (TE wins per RFC 9112 §6.1); duplicate CL → 400; malformed CL → wire-level rejection
- **Request body policy** — static bodies are rejected before dispatch; custom services declare buffering or streaming for the actual method

### 5. Resource Limits

| Resource | Default | Effect |
|----------|---------|--------|
| Max connections | 64 | TCP accept semaphore; new connections dropped when exhausted |
| Max in-flight requests | 64 | Concurrent service executions (503 on exhaustion) |
| Max file streams | 32 | Concurrent file streaming; 503 when exhausted |
| Max requests per connection | 0 (unlimited) | Completed requests per connection |
| Max buf size | 64 KiB | HTTP/1 parser/read buffer ceiling (min 8192) |
| Max headers | 100 | Request header field count (Hyper answers 431) |
| Max header bytes | 32 KiB | Aggregate header name+value bytes (431 pre-service) |
| Max request-target bytes | 8192 | Request-target length (414 pre-service) |
| Max listing entries / response bytes | 4096 / 1 MiB | Directory listing enumeration/response ceilings |
| Stream chunk size | 128 KiB | File streaming read chunk size (64 B–1 MiB) |
| Header read timeout | 10s | Slowloris protection |
| Connection total timeout | 60s | Slow response protection (wraps entire Hyper connection future); `ZERO` opts out of only this hard lifetime |
| Handler timeout | 30s | Per-request timeout for service processing |
| Body read timeout | 30s | Total deadline for body consumption |
| Keep-alive idle timeout | 60s | Idle keep-alive close after inactivity (resets on activity) |
| Response write timeout | 30s | Response no-progress timeout (steady progress never trips) |
| Graceful shutdown timeout | 10s | Drain period after SIGTERM |
| Request body size | 0 (rejected) | No bodies processed by default |

Full CLI flag names live in `docs/cli.md`; full timeout semantics in
`docs/timeout-reference.md`.

### 6. Response Normalization + Framing Enforcement

All response producers converge on a single normalization path (framing stays
runtime-owned; see [`docs/threat-model.md`](../docs/threat-model.md) for the
normative layer taxonomy):

- **HEAD suppression** — body bytes discarded, representation headers preserved
- **Body-forbidden enforcement** — 1xx, 204, 205, 304 bodies discarded
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

- Local privileged attacker (root/kernel), malicious operator-provided root directory, or compromised edge proxy outside the explicit Plan 202 trust set (concurrent filesystem mutation by an unprivileged local writer IS in scope: namespace/reparse/TOCTOU)
- Kernel or filesystem compromise
- Full reverse-proxy threat model (a compromised edge proxy outside the explicit Plan 202 trust set remains untrusted; trusted-proxy mode only narrows the inbound metadata boundary, never forwards requests upstream)
- TLS certificate lifecycle automation

Adapter ownership and seam scope: the optional `http-interop`/Tower
adapters are server-owned (`eggserve-server`; core forwards as
compatibility re-exports), and the CONNECT/tunnel seam is inbound-only —
EggServe accepts server-side tunnel requests but performs no proxy
routing/forwarding (Plan 223 holds).

## Platform Security

| Platform | Security Model | Limitations |
|----------|---------------|-------------|
| Linux (x86_64, aarch64) | Descriptor-relative traversal via `statat`+`openat` | Hardened local profile; public reverse-proxy deployment still requires its documented qualification |
| macOS (x86_64, aarch64) | Same descriptor-relative guarantees | Same as Linux; platform qualification per `docs/toolchain-support.md` |
| Windows (x86_64) | Parser-level checks + handle-relative child resolution + directory enumeration + manual adversarial qualification suites | Functionally qualified for executed classes; two open-descendant root-rename cases are skipped by NTFS path-rename semantics, so not for untrusted mutable public content |

## Unsafe-code boundary

The workspace denies Rust `unsafe_code` by default. EggServe has two narrow,
documented production exceptions: Windows handle-relative filesystem FFI in
`crates/eggserve-static/src/fs/windows.rs` and systemd descriptor adoption in
`crates/eggserve-core/src/server/listener.rs`, plus two documented test-only
boundaries (Unix FIFO fixtures, Windows qualification fixtures). Each production call is locally annotated with its
pointer, buffer, and ownership invariants; PyO3 and other dependencies do not
expand the application-owned unsafe surface. See the complete inventory in
[the unsafe Rust policy](../docs/unsafe-code-policy.md).

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
