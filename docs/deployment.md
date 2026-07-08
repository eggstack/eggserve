# Deployment Guide

eggserve is a hardened static file server intended for local development, internal tools, and controlled environments. This guide covers common deployment patterns.

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

## Pattern 3: Native TLS

eggserve can terminate TLS directly when built with the `tls` feature:

```sh
eggserve --tls-cert cert.pem --tls-key key.pem --directory public
```

See [tls.md](tls.md) for details on the TLS feature, certificate requirements, and limitations.

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
