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

### Production profile: unix-reverse-proxy

The reverse-proxy profile is the preferred public deployment. eggserve binds to loopback, the reverse proxy terminates TLS and handles public binding. This profile is hardened once all required CI gates pass. See `release/support-profiles.toml` for the full specification.

### Production profile: unix-direct-https

Native TLS is a candidate production profile for small deployments or internal tools where reverse proxy complexity is not warranted. It is limited to HTTP/1.1 with manual certificate management. It is not an edge platform — no ACME, virtual hosting, HTTP/2, or multi-certificate routing.

## Pattern 3: Native TLS

eggserve can terminate TLS directly when built with the `tls` feature:

```sh
eggserve --tls-cert cert.pem --tls-key key.pem --directory public
```

See [tls.md](tls.md) for details on the TLS feature, certificate requirements, and limitations.

## Windows deployment

Windows is functional-only. Parser-level protections reject Windows reserved names, ADS syntax, drive prefixes, and backslash in path components. Filesystem-level reparse-point hardening is an active roadmap item (Plans 062–065). Do not use with untrusted mutable public content on Windows.

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
