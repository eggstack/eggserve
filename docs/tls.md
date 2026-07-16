# TLS Support

eggserve supports optional native TLS termination via [rustls](https://docs.rs/rustls). TLS is behind a feature flag and is **not** included in the default build.

## When to use native TLS

Native TLS is suitable for:

- Simple local development with HTTPS
- Lab or testing environments
- Controlled internal networks where a reverse proxy is not practical

For public-facing production deployments, a mature TLS terminator (Caddy, nginx, Traefik, cloud load balancer) is usually preferred. See [deployment.md](deployment.md) for deployment patterns.

## Production profile

Native TLS maps to the `unix-direct-https` production profile (status: candidate). It is supported as a limited HTTP/1.1 static-server deployment, not an edge platform. It does not imply ACME, virtual hosting, HTTP/2, or edge parity.

For production deployments, the `unix-reverse-proxy` profile (Caddy/nginx/Traefik termination) is preferred. See `release/support-profiles.toml` for the full profile definitions and `docs/deployment.md` for deployment patterns.

## Building with TLS

```sh
cargo install --path crates/eggserve-bin --features tls
```

Or when building from the workspace root:

```sh
cargo build -p eggserve-bin --features tls
```

### TLS feature compiled, no TLS flags

When eggserve is built with the `tls` feature but invoked without `--tls-cert` and `--tls-key`, the binary runs as plain HTTP. The TLS feature only adds the capability to terminate TLS when both flags are supplied; it does not force TLS. This is deliberate: operators who keep the feature enabled in their distribution get a working plaintext server by default and opt into HTTPS explicitly per invocation.

## Usage

```sh
eggserve --tls-cert cert.pem --tls-key key.pem
eggserve --tls-cert cert.pem --tls-key key.pem --port 8443
```

Both `--tls-cert` and `--tls-key` must be provided together. If only one is provided, eggserve exits with an error.

## Handshake timeout

TLS handshakes are bounded by the same timeout as HTTP header reads (`--header-timeout`, default 10 seconds). A client that opens a TCP connection but never completes the TLS handshake will hold a connection permit for at most that duration; after timeout the connection is dropped silently. The connection-permit semaphore prevents an unbounded number of pending handshakes, and the per-handshake timeout prevents a single slow client from holding a permit indefinitely.

## Certificate requirements

- **Format:** PEM-encoded certificate chain and PEM-encoded private key
- **Certificates:** At least one certificate must be present in the cert file
- **Key:** Exactly one private key must be present (PKCS#1, PKCS#8, or SEC1)
- **Encrypted keys:** Not supported (eggserve will error with a clear message)
- **Key file:** Must not be empty or contain non-PEM content

## Startup output

With TLS enabled:

```
eggserve 0.1.0
Serving root: ./public
Listening: https://127.0.0.1:8000
TLS: enabled, certificate: cert.pem
```

Without TLS:

```
eggserve 0.1.0
Serving root: ./public
Listening: http://127.0.0.1:8000
```

## Published binaries and wheels

The `tls` feature in `eggserve-bin` is **non-default**. This means:

- **`cargo install eggserve`** installs a plaintext-only binary unless you pass `--features tls`.
- **Published PyPI wheels** do not include TLS. The wheel bundles the platform-native CLI binary built without the `tls` feature.
- To obtain a TLS-capable binary, build from source with `--features tls` or use a reverse proxy in front of the plaintext server.

Release gates validate TLS functionality by explicitly enabling the feature during CI. The `rust.test.server-tls` and `rust.test.client-tls` gates in `release/criteria.toml` cover TLS correctness; they are not satisfied by a default (non-TLS) build.

## Limitations

eggserve's TLS support is intentionally minimal:

- No ACME / Let's Encrypt automation
- No certificate renewal
- No SNI virtual hosting
- No client certificate authentication
- No HTTP/2
- No OCSP stapling
- No hot certificate reload
- No multi-cert routing

If you need any of these features, use a reverse proxy or a dedicated TLS-terminating load balancer.
