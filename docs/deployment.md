# Deployment Guide

eggserve is a hardened static file server intended for local development, internal tools, and controlled environments. Production deployment is defined through explicit profiles — see README.md for the full profile table. This guide covers common deployment patterns.

## Pattern 1: Local-only HTTP

The simplest usage. Serve files on loopback only:

```sh
eggserve --directory public
eggserve 9000 public
```

The server binds to `127.0.0.1:8000` by default. Only local processes can connect. This is the recommended pattern for local development.

## Pattern 2: Reverse proxy TLS

For public-facing deployments, terminate TLS at a reverse proxy and forward to eggserve on loopback:

**Caddy:**

```
example.com {
    reverse_proxy 127.0.0.1:8000
}
```

**nginx:**

```nginx
server {
    listen 443 ssl;
    server_name example.com;
    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://127.0.0.1:8000;
    }
}
```

Then start eggserve without TLS:

```sh
eggserve --directory public
```

This is the recommended pattern for production deployments. Reverse proxies handle certificate management, renewal, HTTP/2, and other TLS features that eggserve intentionally does not implement.

### Connection metadata behind a reverse proxy

When eggserve runs behind a reverse proxy, connection metadata (`remote_addr`, `local_addr`, `scheme`, `tls`) reflects the **transport peer** — the proxy's address, not the end client's. eggserve does not automatically trust `Forwarded` or `X-Forwarded-*` headers. If you need end-client identity, implement proxy-header validation in your service layer with an explicit allowlist.

### Body handling behind a reverse proxy

eggserve rejects request bodies by default (safe default). When a reverse proxy forwards requests with bodies (e.g., POST, PUT), the runtime enforces body policy before invoking any service code. A lone `Content-Length` alongside `Transfer-Encoding` is discarded by Hyper 1.11 during parsing (`Transfer-Encoding` wins per RFC 9112 §6.1); duplicate `Content-Length` fields are rejected with 400 at the origin. When a handler returns without fully consuming the body, the connection is closed to prevent request smuggling through leftover bytes. Reverse proxies should be configured to forward `Content-Length` and `Transfer-Encoding` headers without modification to preserve framing integrity.

### Production profile: unix-reverse-proxy

The reverse-proxy profile is the preferred public deployment. eggserve binds to loopback, the reverse proxy terminates TLS and handles public binding. External qualification evidence collection is pending; the profile remains functional until all gates pass. See README.md for the full specification.

### Production profile: unix-direct-https

Native TLS is functional for small deployments or internal tools where reverse proxy complexity is not warranted. It is limited to HTTP/1.1 with manual certificate management. It is not an edge platform — no ACME, virtual hosting, HTTP/2, or multi-certificate routing. External qualification pending. See README.md for the full specification.

## Per-profile resource defaults (Plan 164)

Independent budgets for connections, in-flight service work, parser memory, and connection lifecycle. Tune via CLI flags, `Limits`, or `RuntimeConfig`; the stdlib Python compatibility facade keeps conservative defaults and does not expose every knob.

| Setting (CLI flag) | Default | Reverse-proxy production | Direct TLS | Embedded anonymity-sensitive |
|---|---|---|---|---|
| `--max-connections` | 64 | 512–2048 (size to proxy concurrency) | 128 | 16–32 |
| `--max-in-flight-requests` | 64 | 256–1024 (handler concurrency, independent of idle keep-alives) | 64–128 | 8–16 |
| `--max-file-streams` | 32 | 64–256 | 32–64 | 8 |
| `--max-buf-size` (bytes) | 65536 | 65536 | 65536 | 16384 |
| `--max-headers` | 100 | 100 | 100 | 40–60 |
| `--max-header-bytes` | 32768 | 32768 | 32768 | 8192 |
| `--max-request-target-bytes` | 8192 | 8192 | 8192 | 2048–4096 |
| `--header-timeout` (s) | 10 | 30–60 (must cover proxy keep-alive gaps; Hyper also applies it while idle) | 10–30 | 5 |
| `--keep-alive-idle-timeout` (s) | 60 | 60–120 (shorter than the header timeout for distinct idle accounting) | 60 | 10–20 |
| `--max-requests-per-connection` (0 = unlimited) | 0 | 0 (idle/write timers bound reuse instead) | 0 | 100–1000 |
| `--response-write-timeout` (s) | 30 | 30–60 | 30 | 10–15 |
| `--connection-total-timeout` (s) | 60 | 3600+ (hard lifetime only; idle/write timers do the routine bounding) | 600–3600 | 120–300 |
| `--handler-timeout` / `--body-read-timeout` (s) | 30 / 30 | 30 / 30 (must stay ≤ total) | 30 / 30 | 10–15 |

Notes:

- **Reverse-proxy production** favors persistent connections, bounded parser memory, meaningful service concurrency, and idle/write-stall defense. Raise the total lifetime into the hours so it acts purely as defense-in-depth; the idle and write timers bound routine use. Keep `header-timeout` at or above the proxy's keep-alive gap, otherwise Hyper closes healthy idle connections and they count as header timeouts.
- **Direct TLS** uses the same core bounds plus the existing TLS handshake budget (`--tls-*`, 10s default).
- **Embedded anonymity-sensitive** uses stricter open-connection, header, keep-alive, request-count, and write-stall bounds suitable for resource-constrained direct origins. This is still not rate limiting: all clients share the same generic resource budgets — there are no per-IP/client/user token buckets, authentication quotas, or reputation logic anywhere in the core.
- Every production claim must name a profile from the production profiles table in README.md. Hardened profiles must not allow symlink following.

## Response privacy and minimal-fingerprint profile (Plan 165)

Final origin-response metadata is an explicit `ResponsePolicy`, applied after
service construction at one Hyper boundary (Hyper automatic `Date` disabled;
EggServe is the sole `Date` authority). Normal defaults stay RFC-compatible
(suppressed `Server`, one system-clock `Date`, minimal generic errors).

```rust
use eggserve_core::server::{RuntimeConfig, response_policy::ResponsePolicy};
use eggserve_core::policy::StaticMetadataPolicy;

let privacy = ResponsePolicy::minimal_fingerprint();
let config = RuntimeConfig::builder()
    .response_policy(privacy)
    .date_policy(eggserve_core::server::response_policy::DatePolicy::SystemClock)
    .build()?;
// Static validators:
let mut static_policy = eggserve_core::policy::StaticPolicy::safe_default();
static_policy.static_metadata = StaticMetadataPolicy::minimal_fingerprint();
```

Profile composition:

- `Server` suppressed; optional fixed value only (never versions).
- `Date`: `SystemClock` by default; `Custom(provider)` with a trusted
  network-adjusted clock is the preferred anonymity-sensitive mode
  (provider returns a time value, EggServe owns formatting); `Suppress` is an
  explicit RFC 9110 tradeoff. No fixed/stale or randomized dates.
- Denylist (`stripped_response_headers`): validated, post-service, all
  duplicates removed; framing/hop-by-hop/`date`/`content-range` cannot be
  denylisted. Built-in preset strips `x-powered-by`; extend for project
  fields. No wildcard.
- Static validators: `StaticMetadataPolicy::minimal_fingerprint()` suppresses
  `ETag` + `Last-Modified` (suppression over hashing); retained
  `Last-Modified` never exceeds `Date`.
- Errors: `Minimal` fixed plain-text bodies or `Empty` (no bytes for
  runtime-generated errors only; application `Ok` 4xx/5xx never rewritten;
  `HEAD` correct; no version/path/exception text).
- No wire imitation of nginx/Apache, no header-order randomization, no TLS
  impersonation, no request-side fingerprint normalization beyond HTTP
  correctness, no per-client rate limiting, no application-body rewriting.

This minimizes gratuitous fingerprint signals; it does not make the server
un-fingerprintable. The router/WAF (for example an I2P router) owns peer
identity, rate limiting, and tunnel policy; EggServe owns HTTP parsing/framing
and local resource safety with no I2P-specific types in core. CLI/Python keep
standards-compliant defaults; advanced policy is Rust-only so the stdlib
facade does not silently diverge.

## Pattern 3: Native TLS

eggserve can terminate TLS directly when built with the `tls` feature:

```sh
eggserve --tls-cert cert.pem --tls-key key.pem --directory public
```

See [tls.md](tls.md) for details on the TLS feature, certificate requirements, and limitations.

## Windows deployment

Windows implements handle-relative confinement with parser-level protections rejecting Windows reserved names, ADS syntax, drive prefixes, and backslash in path components. Directory listing is disabled by default. The adversarial qualification suite covers reparse-point denial, namespace normalization, race harnesses, root identity, file validators, ACL/sharing, resource stability, installed artifact parity, and fuzz corpus replay. Two open-descendant root-rename cases are skipped because NTFS rejects that external path operation, so Windows remains functional-only for public deployment.

See README.md for Windows-specific profiles (windows-reverse-proxy, windows-direct-https, windows-functional).

## Binding to all interfaces

To make eggserve accessible from other machines (without a reverse proxy), use `--public`:

```sh
eggserve --public --port 8000 --directory public
```

This binds to `0.0.0.0`. The `--public` flag is required to acknowledge public exposure intent. When binding publicly, consider using a reverse proxy for TLS termination and access control.

## Combining patterns

A common setup for small deployments:

- eggserve on `127.0.0.1:8000` (no TLS, no public exposure)
- Caddy or nginx on `0.0.0.0:443` (TLS termination, access control)
- Optional: WireGuard or Tailscale for private network access without a public endpoint

## Security considerations

- eggserve does **not** manage certificates. You must obtain, install, and renew certificates separately.
- eggserve does **not** implement ACME. Use certbot, Caddy's built-in ACME, or your hosting provider's certificate management.
- For production, always prefer a mature TLS terminator unless eggserve's native TLS is sufficient for your threat model.
- Never expose eggserve directly to the public internet without proper TLS and access control.
- Every production deployment must name a profile from the production profiles table in README.md. No document should claim production support without naming the profile.
- **Directory listing is opt-in and disabled by default.** When enabled with `--directory-listing`, it exposes file names and directory structure. Listing responses are bounded (max 4096 entries, 1 MiB body). Symlink entries are hidden from listings by default. Do not enable directory listing for untrusted content without understanding the information disclosure implications.
- **Connection metadata is transport-peer metadata.** `remote_addr` on the `Request` object reflects the TCP peer address (proxy address when behind a reverse proxy). Do not use it for end-client identification without proxy-header validation.

## Structured Logging

eggserve emits structured operational logs to stderr. Use `--log-format` to select the output mode:

- `--log-format json` — JSON Lines to stderr. One valid JSON object per line with fields: `schema_version`, `severity`, `event`, `timestamp`, `message`, `connection_id`, `request_seq`, `fields`.
- `--log-format text` — Human-readable text to stderr (default). Format: `[severity] event_name: message`. Control characters are sanitized and long fields are truncated.
- `--log-format none` — Disables all operational logs. No structured output is emitted during normal operation.

The full JSON Lines schema, complete event reference, operational counters, and troubleshooting recipes live in [the operations logging guide](ops-logging.md).

### Event Categories

| Category | Examples |
|----------|----------|
| Process/config | `process_starting`, `root_initialized`, `listener_ready`, `shutdown_requested` |
| Connection lifecycle | `connection_accepted`, `tls_handshake_success`, `keep_alive_closed` |
| Request/service | `request_completed`, `file_not_found`, `file_denied`, `body_policy_rejection` |
| Operational faults | `listener_transient_error`, `resource_exhaustion`, `blocking_worker_saturation` |

### Privacy

- No absolute filesystem paths in request logs (startup diagnostics only)
- No `Authorization` or `Cookie` headers in logs
- Query strings omitted from request path fields
- Request paths truncated to last component (max 128 chars)

### Stderr Destination

All log output goes to stderr. stdout remains clean for CLI conventions (e.g. piped output, scripted usage).
