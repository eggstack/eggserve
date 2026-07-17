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

## Central invariant

> **No remotely supplied request may cause eggserve to read or serve an object outside the pinned root, and malformed or ambiguous HTTP input must not cause cross-request or frontend/backend message-boundary confusion.**

## Attacker model

### Remote unauthenticated network attacker

An attacker with unrestricted network access to the server. The attacker can:

- Send arbitrary HTTP requests to the server
- Send malformed request targets (e.g., path traversal, invalid percent-encoding)
- Use percent-encoded traversal attempts (`%2e%2e%2f`)
- Hold connections open slowly (slowloris-style)
- Request large files repeatedly to exhaust resources
- Attempt log injection through paths or headers
- Attempt symlink/reparse-point escape to serve files outside the root
- Attempt platform-specific path bypasses (Windows `\\?\`, UNC paths, etc.)
- Send requests with ambiguous or conflicting framing (TE+CL, duplicate Content-Length) to confuse front-end/back-end message boundaries
- Send oversized headers, targets, or bodies to exhaust memory or file descriptors

### Slowloris and connection-hoarding attacker

An attacker who opens many connections simultaneously and holds them open with slow or incomplete requests, consuming the connection budget and preventing legitimate clients from connecting. The server enforces connection count limits (64 max), header read timeouts (10s), and response write timeouts (60s) to bound this behavior.

### Malformed HTTP/framing attacker

An attacker who sends HTTP requests that violate the HTTP/1.1 grammar or framing rules. This includes malformed percent-encoding, missing delimiters, invalid header syntax, and ambiguous content-length or transfer-encoding signals. eggserve rejects malformed input at the parser level before any filesystem access. Requests with TE+CL conflicts or duplicate Content-Length fields are rejected with 400 before the service is invoked.

### Request-smuggling attacker operating through a reverse proxy

An attacker who sends requests to a reverse proxy (Caddy, nginx, HAProxy, cloud load balancer) with the intent that the proxy and eggserve disagree on request boundaries. eggserve's hardened framing checks (TE+CL rejection, duplicate Content-Length rejection, wire-level validation) ensure that ambiguous requests are rejected at the origin, preventing desynchronization. This attacker is in scope because reverse-proxy deployment is the preferred production profile.

### Filesystem namespace attacker able to mutate content within or adjacent to the root

An attacker with the ability to create, rename, delete, or modify files within the serving root or in directories adjacent to it. The attacker may attempt:

- Swapping symlinks to redirect resolution outside the root
- Placing files with special names (dotfiles, reserved names) to influence behavior
- Replacing files during a TOCTOU window between validation and open

On Unix with safe defaults, descriptor-relative traversal (`statat` + `openat` with `O_NOFOLLOW`) eliminates the TOCTOU window for symlink swaps. On non-Unix or follow-symlinks modes, this attacker is only partially mitigated.

### Windows reparse and namespace attacker

An attacker who can place reparse points (NTFS junctions, symbolic links, mount points) within or adjacent to the serving root on Windows. Under the hardened Windows profile, all reparse-point components are denied. Parser-level protections reject Windows reserved names, ADS syntax, drive prefixes, and backslash in path components. However, filesystem-level reparse-point hardening is deferred until Plans 062–065 complete. Windows is currently functional-only, not hardened.

### Resource-exhaustion attacker

An attacker who sends requests designed to consume excessive server resources (memory, CPU, file descriptors, file streams). The server enforces resource limits including: connection count (64 max), file-stream count (32 max), header read timeout (10s), response write timeout (60s), request target size limits, header count/size limits, and directory listing entry/byte limits. Request bodies are rejected by default on GET/HEAD.

### Log-injection attacker

An attacker who crafts request paths or header values containing newline characters, control characters, or log-format metacharacters to forge log entries or disrupt log analysis. All logged paths and headers are sanitized before writing.

### Malicious or stalled Python callback

A Python callback registered through the `Server` primitive that is slow, unresponsive, or consumes excessive resources. This is treated as a resource and lifecycle concern, not as a sandboxed adversary. Rust enforces connection and I/O policy around callbacks: timeout limits, connection caps, and file-stream quotas are not affected by callback behavior. Callback timeout does not cancel Python execution — the request task stops waiting at the configured deadline and returns 504, but the Python callback continues executing in the background. The callback semaphore permit is held until the Python function returns, meaning timed-out callbacks still count against the concurrency limit until they complete. Forced shutdown closes the Rust runtime and listener but cannot safely terminate Python code.

## Out-of-scope attacker capabilities

The following are explicitly out of scope:

- **Compromised reverse proxy** — if the edge/origin proxy is compromised, the attacker can inject arbitrary requests and no origin-level defense is meaningful
- **Kernel or filesystem compromise** — if the kernel or filesystem layer is compromised, path confinement and file-open guarantees are moot
- **Privileged local attacker** — a local attacker with root or equivalent privileges can bypass all process-level controls
- **Malicious operator root directory** — an operator who intentionally places sensitive files in the serving root and then serves it is responsible for the outcome
- **TLS certificate lifecycle automation** — ACME, renewal, and multi-certificate routing are out of scope

## Production profiles

eggserve defines production readiness through explicit profiles rather than one undifferentiated claim. Each profile specifies a security posture, supported platform, and required configuration. The production profiles are defined in `release/support-profiles.toml` and validated by contract consistency tests.

### unix-reverse-proxy

**Status:** supported-hardened (primary production profile)

- Linux or macOS;
- eggserve bound to loopback or a private interface;
- Caddy, nginx, HAProxy, a cloud load balancer, or equivalent terminates public TLS;
- the edge may provide HTTP/2 or HTTP/3, while the origin remains HTTP/1.1;
- the serving root is operator-controlled and mounted read-only where practical;
- safe defaults remain enabled;
- symlink-following mode is outside the hardened guarantee.

This is the preferred public deployment profile. Reverse proxies handle certificate management, renewal, HTTP/2, and other TLS features that eggserve intentionally does not implement.

### unix-direct-https

**Status:** candidate (secondary production profile)

- Linux or macOS;
- eggserve terminates TLS using rustls;
- one certificate chain and one key configuration;
- restart-required certificate rotation;
- HTTP/1.1 only;
- no ACME, virtual hosting, OCSP stapling, client certificates, or multi-certificate routing.

Native TLS is limited and does not imply ACME, virtual hosting, HTTP/2, or edge parity.

### windows-reverse-proxy

**Status:** candidate (functional-only until Plans 062–065 complete)

- supported Windows release on a local NTFS volume;
- pinned root directory handle;
- component-by-component handle-relative traversal;
- all reparse points denied under the hardened profile;
- final files and directories served from already validated handles;
- loopback or private-interface origin behind a mature edge.

Windows reparse-point hardening is an active roadmap item. Windows remains functional-only until evidence supports promotion.

### windows-direct-https

**Status:** functional (production only after both Windows confinement and native TLS qualification complete)

Windows direct HTTPS requires both Windows handle-relative filesystem hardening and native TLS qualification. Neither is complete.

### local-development

**Status:** supported-hardened (non-production)

- Any platform;
- loopback binding only;
- safe defaults enforced;
- used for local development and testing.

### windows-functional

**Status:** functional-only

- Windows SMB/network-share roots;
- Windows non-NTFS filesystems;
- Windows cloud-placeholder or third-party filesystem roots;
- `--follow-symlinks` or any Windows reparse-following mode.

These configurations are functional but fall outside the hardened production claim.

### link-following-compat

**Status:** functional-only

- `--follow-symlinks` on any platform;
- `--directory-listing` without a trusted origin;
- public plaintext HTTP without TLS termination.

These configurations are weaker than the hardened profile and are not production candidates.

## Profile-specific security notes

### Unix reverse-proxy profile

The origin communicates with the edge over HTTP/1.1 on loopback. The edge terminates TLS, handles client identity, and enforces connection policy. eggserve does not acquire edge-server responsibilities — it must not implicitly trust forwarding headers, provide certificate automation, or implement public client-identity policy. The edge should use its own logs for client attribution. eggserve logs sanitized request paths and headers, but these are not suitable for client attribution behind a proxy.

### Unix direct-HTTPS profile

eggserve terminates TLS directly. Certificate management is manual — the operator must provide certificate and key files and rotate them through a restart. There is no ACME, no SNI-based routing, and no OCSP stapling. The server is HTTP/1.1 only; the edge cannot negotiate HTTP/2 or HTTP/3. This profile is suitable for small deployments or internal tools where the complexity of a reverse proxy is not warranted.

### Windows profiles

Parser-level protections reject Windows reserved names, ADS syntax, drive prefixes, and backslash in path components. However, filesystem-level reparse-point/NTFS junction hardening is deferred (documented non-goal until Plans 062–065 complete). Windows is explicitly a trusted/local-use platform — do not use with untrusted mutable public content on Windows. The hardened Windows profile is not yet promoted.

## Defensive layers

1. **Path confinement** — all request paths are parsed, percent-decoded, validated, and resolved against the configured root. The `ConfinedPath` parser rejects traversal (`..`), absolute paths, NUL bytes, malformed percent-encoding, backslash ambiguity, and platform-specific attacks (Windows reserved names, ADS, drive prefixes). The `RootGuard` canonicalizes the final path and verifies it remains within the root.
2. **Policy enforcement** — a security policy object controls what is allowed (methods, symlink following, dotfiles, directory listing). Defaults deny everything except direct file GET/HEAD.
3. **Input validation** — malformed request targets are rejected before path resolution. Percent-encoding is decoded exactly once. Double-encoded traversal is caught by per-component decode checks.
4. **Filesystem checks** — when symlink policy denies symlinks, on Unix, descriptor-relative traversal uses `statat(AT_SYMLINK_NOFOLLOW)` before each `openat(..., O_NOFOLLOW)` to detect symlinks at each path component and to refuse to follow them at open time. Intermediate components are opened with `O_DIRECTORY|O_NOFOLLOW`, final components with `O_RDONLY|O_NOFOLLOW`. On non-Unix or when `--follow-symlinks` is enabled, `symlink_metadata` is checked per component and the final canonical path is verified against the root; this fallback is **weaker** than the descriptor-relative path and is explicitly outside the hardened guarantee. Files are opened during resolution — never re-opened later by absolute path. Canonical root escape is rejected with `PathRejection::RootEscapeDenied`. Dotfile policy checks components at both the path-validation and filesystem-resolution layers. Directory listings also respect symlink policy and hide symlink entries when denied.
5. **Resource limits** — connection count (64 max), file-stream count (32 max), header read timeout (10s), response write timeout (60s), and request body metadata rejection (`Content-Length > 0`, invalid `Content-Length`, or any `Transfer-Encoding` on GET/HEAD) are enforced to prevent resource exhaustion.
6. **Sanitized logging** — all logged paths and headers are sanitized to prevent log injection.
7. **Framing enforcement** — TE+CL conflict, duplicate Content-Length, and malformed Content-Length are rejected before the service is invoked. This prevents request smuggling where front-end and back-end servers disagree on message boundaries.

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

Extracting paths from `safe_relative_components()` and reopening them manually bypasses descriptor-relative TOCTOU hardening. This is safe for read-only metadata inspection but not for reopening files for serving. The root is pinned at startup via `PinnedRoot`; once a file is resolved, it must be accessed through the opened handle, never by reconstructing and reopening a path.

## Request-body policy risk

eggserve rejects non-empty request bodies on GET/HEAD. Downstream adapters must enforce the same policy or explicitly document the difference.

## Request-body framing risk

eggserve enforces hardened HTTP/1 framing before body ingestion:

- **TE+CL conflict**: Requests containing both `Transfer-Encoding` and `Content-Length` are rejected with 400 before the service is invoked. This prevents HTTP request smuggling attacks where front-end and back-end servers disagree on message boundaries.
- **Duplicate Content-Length**: Requests with more than one `Content-Length` field are rejected with 400, even when values are identical. This eliminates ambiguity about which length value to trust.
- **Malformed Content-Length**: Non-numeric, negative, signed, overflowing, or non-decimal `Content-Length` values are rejected at the HTTP/1 wire level by Hyper before eggserve processes them.

Downstream adapters must apply the same framing checks before invoking their handlers. If an adapter bypasses the runtime's framing validation, it must implement equivalent checks to prevent smuggling attacks.

## Header spoofing/normalization risk

Downstream adapters must be careful about header normalization and spoofing. eggserve's primitives validate method and body framing but do not normalize arbitrary headers.

## Response serialization risk

`StaticResponsePlan` values are framework-independent. Downstream adapters must correctly translate status codes, headers, and body plans to their framework's response API without losing security-relevant headers (e.g. `x-content-type-options: nosniff`).

## Callback-induced latency and backpressure risk

If a downstream adapter introduces Python callbacks into the request path, those callbacks may introduce latency and backpressure that eggserve's resource limits (connection count, file streams, timeouts) were not designed to handle.

## Trust boundary between Rust runtime and Python user code

Rust enforces path confinement, policy, and I/O limits. Python user code runs in the same process (via PyO3) or in a managed subprocess. eggserve does not sandbox Python application code.
