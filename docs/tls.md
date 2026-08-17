# TLS Support

eggserve supports optional native TLS termination via [rustls](https://docs.rs/rustls). TLS is behind a feature flag and is **not** included in the default build.

## When to use native TLS

Native TLS is suitable for:

- Simple local development with HTTPS
- Lab or testing environments
- Controlled internal networks where a reverse proxy is not practical

For public-facing production deployments, a mature TLS terminator (Caddy, nginx, Traefik, cloud load balancer) is usually preferred. See [deployment.md](deployment.md) for deployment patterns.

## Production profile

Native TLS maps to the `unix-direct-https` production profile (status: candidate). It is supported as a limited HTTP/1.1 static-server deployment, not an edge platform. It does not imply ACME, virtual hosting, HTTP/2, or edge parity. External qualification evidence is pending. The profile remains candidate until full qualification passes.

For production deployments, the `unix-reverse-proxy` profile (Caddy/nginx/Traefik termination) is preferred. Production profiles are documented in README.md and `docs/deployment.md`.

## Building with TLS

```sh
cargo install --path crates/eggserve-bin --features tls
```

Or when building from the workspace root:

```sh
cargo build -p eggserve-bin --features tls
```

### TLS feature compiled, no TLS flags

When eggserve is built with the `tls` feature but invoked without TLS flags, the
binary runs as plain HTTP. The TLS feature only adds the capability to
terminate TLS; it does not force TLS.

## Usage

```sh
eggserve --tls-cert cert.pem                 # combined cert/key PEM
eggserve --tls-cert cert.pem --tls-key key.pem
eggserve --tls-cert cert.pem --tls-key key.pem --port 8443
```

`--tls-cert` is required for TLS. If `--tls-key` is omitted, the certificate
path is also used as the private-key path, allowing a combined PEM file. A
key-only configuration remains invalid.

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

The `tls` feature in `eggserve-bin` is **non-default**. The Python extension
uses the same Rust TLS loader for `HTTPSServer` and `ThreadingHTTPSServer`.
This means:

- **`cargo install --path crates/eggserve-bin`** installs a plaintext-only binary unless you pass `--features tls`.
- **Published PyPI wheels** can provide Python HTTPS classes and the
  extension-backed CLI; both are built with the Python crate's enabled `tls`
  feature.
- To obtain a TLS-capable binary, build from source with `--features tls` or use a reverse proxy in front of the plaintext server.

Release gates validate TLS functionality by explicitly enabling the feature during CI. TLS tests (`clippy` and `cargo test` with `--features tls`) cover TLS correctness; they are not satisfied by a default (non-TLS) build.

## Limitations

eggserve's TLS support is intentionally minimal:

- No ACME / Let's Encrypt automation
- No certificate renewal
- No SNI virtual hosting
- No client certificate authentication
- No HTTP/2 (ALPN is restricted to `http/1.1`)
- No OCSP stapling
- No hot certificate reload
- No multi-cert routing

If you need any of these features, use a reverse proxy or a dedicated TLS-terminating load balancer.
