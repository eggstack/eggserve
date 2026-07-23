# Deployment Guide

eggserve is a hardened static file server intended for local development, internal tools, and controlled environments. Production deployment is defined through explicit profiles — see `release/support-profiles.toml`. This guide covers common deployment patterns.

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

### Production profile: unix-reverse-proxy

The reverse-proxy profile is the preferred public deployment. eggserve binds to loopback, the reverse proxy terminates TLS and handles public binding. This profile is hardened once all required CI gates pass. Plan 089 qualification covers proxy interop (Caddy, nginx), desync corpus, stateful fuzz replay, filesystem race probing, fault injection, 24-hour soak, installed artifact validation, SBOM/provenance, and independent review. See `release/support-profiles.toml` for the full specification.

### Production profile: unix-direct-https

Native TLS is a candidate production profile for small deployments or internal tools where reverse proxy complexity is not warranted. It is limited to HTTP/1.1 with manual certificate management. It is not an edge platform — no ACME, virtual hosting, HTTP/2, or multi-certificate routing. Native TLS abuse/limits qualification and 24-hour soak are part of Plan 089 qualification.

## Pattern 3: Native TLS

eggserve can terminate TLS directly when built with the `tls` feature:

```sh
eggserve --tls-cert cert.pem --tls-key key.pem --directory public
```

See [tls.md](tls.md) for details on the TLS feature, certificate requirements, and limitations.

## Windows deployment

Windows implements handle-relative confinement (Plans 084–085) with parser-level protections rejecting Windows reserved names, ADS syntax, drive prefixes, and backslash in path components. Directory listing is disabled by default. Plan 086 adversarial qualification test scaffold is established (113 tests covering reparse-point denial matrix, namespace normalization, race harness, root identity, file validators, ACL/sharing, resource stability, installed artifact parity, fuzz corpus replay). Independent safety review and profile promotion decision are awaited. Windows remains functional-only until those human gates complete.

See `release/support-profiles.toml` for Windows-specific profiles (windows-reverse-proxy, windows-direct-https, windows-functional).

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
- Every production deployment must name a profile from `release/support-profiles.toml`. No document should claim production support without naming the profile.
- **Directory listing is opt-in and disabled by default.** When enabled with `--directory-listing`, it exposes file names and directory structure. Listing responses are bounded (max 4096 entries, 1 MiB body, 255-byte filenames, 30s timeout). Symlink entries are hidden from listings by default. Do not enable directory listing for untrusted content without understanding the information disclosure implications.
- **Connection metadata is transport-peer metadata.** `remote_addr` on the `Request` object reflects the TCP peer address (proxy address when behind a reverse proxy). Do not use it for end-client identification without proxy-header validation.

## Structured Logging

eggserve emits structured operational logs to stderr. Use `--log-format` to select the output mode:

- `--log-format json` — JSON Lines to stderr. One valid JSON object per line with fields: `schema_version`, `severity`, `event`, `timestamp`, `message`, `connection_id`, `request_seq`, `fields`.
- `--log-format text` — Human-readable text to stderr (default). Format: `[severity] event_name: message`. Control characters are sanitized and long fields are truncated.
- `--log-format none` — Disables request logs. Only fatal startup diagnostics are emitted.

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
