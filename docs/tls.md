# TLS Support

eggserve supports optional native TLS termination via [rustls](https://docs.rs/rustls). TLS is behind a feature flag and is **not** included in the default build. The reusable identity, trust, client-authentication, and reload substrate is the neutral [`eggnet-tls`](../architecture/eggnet-tls.md) crate; EggServe re-exports it through `eggserve_core::tls` while retaining only transport and HTTP/3-specific assembly.

## When to use native TLS

Native TLS is suitable for:

- Simple local development with HTTPS
- Lab or testing environments
- Controlled internal networks where a reverse proxy is not practical
- Rust embedders needing SNI multi-identity and optional mTLS without a separate terminator (Plan 203)

For public-facing production deployments, a mature TLS terminator (Caddy, nginx, Traefik, cloud load balancer) is usually preferred. See [deployment.md](deployment.md) for deployment patterns.

## Production profile

Native TLS maps to the `unix-direct-https` production profile (status: candidate). It is supported as a limited static-server deployment, not an edge platform. The default `tls` build is HTTP/1.1-only; an experimental `http2,tls` Rust build negotiates `h2` before `http/1.1`. Plans 186, 190, and 191 keep native H2 experimental: deterministic tests, two-family interop, and Linux wire checks pass, while browser/platform evidence, trailer-scope determinism, and a public safe per-stream reset hook remain open. Plan 203 adds SNI multi-identity, WebPKI mTLS, verified metadata, and atomic reload to the Rust substrate, but does not imply ACME or edge parity. The profile remains candidate until its applicable release gates pass.

For production deployments, the `unix-reverse-proxy` profile (Caddy/nginx/Traefik termination) is preferred. Production profiles are documented in README.md and `docs/deployment.md`.

## Building with TLS

```sh
cargo install --path crates/eggserve-bin --features tls
# Experimental Rust H1/H2 + TLS build:
cargo install --path crates/eggserve-bin --features http2,tls
```

Or when building from the workspace root:

```sh
cargo build -p eggserve-bin --features tls
```

### TLS feature compiled, no TLS flags

When eggserve is built with the `tls` feature but invoked without TLS flags, the
binary runs as plain HTTP. The TLS feature only adds the capability to
terminate TLS; it does not force TLS.

## CLI usage (single identity)

```sh
eggserve --tls-cert cert.pem                 # combined cert/key PEM
eggserve --tls-cert cert.pem --tls-key key.pem
eggserve --tls-cert cert.pem --tls-key key.pem --port 8443
```

`--tls-cert` is required for TLS. If `--tls-key` is omitted, the certificate
path is also used as the private-key path, allowing a combined PEM file. A
key-only configuration remains invalid. The CLI remains single-identity;
multi-identity/SNI and mTLS are Rust `TlsServerConfig` APIs (below), not CLI flags.

## Rust production identity (Plan 203)

`eggnet_tls::TlsServerConfigBuilder` (re-exported as
`eggserve_core::tls::TlsServerConfigBuilder`) builds an immutable, validated
identity from in-memory PEM/DER or loaded paths, using maintained rustls
mechanisms (`ResolvesServerCert` SNI resolution, WebPKI client verification).
No ACME, secret storage, KMS signing, discovery, filesystem watcher, or Python
verification callback is added; operators supply material and decide when to reload.

```rust,no_run
use eggserve_core::tls::{ClientAuthMode, TlsServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

// `certs`/`key` from PEM files, embedded bytes, or a secret manager (operator-owned).
fn build(certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>)
    -> Result<TlsServerConfig, eggserve_core::tls::TlsError>
{
    TlsServerConfig::builder()
        .add_identity("example.com", certs.clone(), key.clone_key())?
        .add_identity("*.example.com", certs.clone(), key.clone_key())?
        .default_identity(certs, key)?
        .build()
}
```

SNI rules:

- Exact DNS names (lowercased, max 253 chars, validated) take priority.
- `*.suffix` wildcards are single-level only (`foo.example.com` matches, bare
  `example.com` and `a.b.example.com` do not) and require a dotted suffix
  (e.g. `*.example.com`, never `*.com`).
- Optional default identity serves no-SNI/no-match clients; without a default,
  the handshake fails.
- Selection performs no blocking filesystem/network IO; SNI is bounded before
  observability use; key/cert bytes are never logged.

Client authentication:

- `Disabled` (default, backwards compatible), `Optional(trust roots)`, or
  `Required(trust roots)` via `client_auth_disabled()` / `client_auth_optional()` /
  `client_auth_required()`, with optional explicit CRLs via `with_crls()`.
- Built-in WebPKI verification; custom verifier injection stays Rust-only
  experimental and never becomes a Python handshake callback.
- `Required` fails the handshake with no acceptable cert; `Optional` allows
  unauthenticated clients while exposing verified metadata for authenticated ones.
- Trust roots/CRLs are bounded (256 roots, 16 CRLs, 1 MiB PEM) and validated at
  build; without CRLs no revocation checking is implied.
- Services receive only verified metadata (`TlsInfo`); raw DER chain exposure is
  opt-in and bounded (see below).

Verified metadata (`ConnectionInfo.tls` / `Request.connection()`):

- `protocol_version`, `server_name` (bounded SNI), `alpn` (`h2`/`http/1.1`/`h3`),
  `client_authenticated` / `peer_certificates_present` (present implies verified),
  and opt-in `peer_certificate_chain` (max 8 × 64 KiB, else `None`).
- Enable the chain with `RuntimeConfigBuilder::tls_expose_peer_chain(true)`
  (default `false`); presence flags are always populated.
- Caller-owned transports assert via `ConnectionContext`; distinguish
  caller-asserted from EggServe-terminated where provenance matters.

Handshake and admission:

- Order: accept permit → optional PROXY preamble (Plan 202) → TLS handshake
  deadline (`tls_handshake_timeout`, default 10s) → ALPN selection → HTTP.
- Slow clients cannot occupy unlimited tasks (connection semaphore + per-handshake
  deadline; permits released exactly once). Selection/verification performs no
  unbounded blocking work. Errors use fixed sanitized categories and never echo
  rustls internals to clients. H2 ALPN derives from `RuntimeConfig.http2.enabled`;
  H3 uses a separate TLS 1.3/`h3` QUIC config (see below).

Atomic reload:

```rust,no_run
use eggserve_core::tls::{TlsReloadHandle, TlsServerConfig};
// Initial:
let initial = TlsServerConfig::builder().single_identity(certs, key)?.build()?;
let reload = TlsReloadHandle::from_tls_server_config(&initial);
let config = eggserve_core::server::RuntimeConfig::builder()
    .bind("127.0.0.1:8443".parse()?)
    .tls_reload_handle(reload.clone())
    .build()?;
let server = eggserve_core::server::Server::builder().runtime(config).build()?;
let handle = server.start_with_service(svc).await?;
// Later (operator decides when files changed):
let next = TlsServerConfig::builder().single_identity(new_certs, new_key)?.build()?;
handle.replace_tls_server_config(&next)?; // new handshakes only; established keep session
# Ok::<(), Box<dyn std::error::Error>>(())
```

- Immutable validated objects + atomic snapshot read by new handshakes; failed
  builds never touch live state. No filesystem watcher. H3 rotation requires
  endpoint replacement/drain (Quinn `set_server_config` handoff); TCP reload does
  not atomically rotate H3 — see “H3 coherence”.

Session resumption and 0-RTT:

- Conservative defaults preserved: `max_early_data_size = 0` explicitly (no
  application 0-RTT on TCP or QUIC/H3), rustls default ticket policy (no stateful
  tickets / `NeverProducesTickets`). No distributed ticket-key system. H3/QUIC
  0-RTT remains disabled unless a separate replay-safety decision enables it.

H3 coherence:

- H3 uses a separate TLS 1.3-only QUIC config with `h3` ALPN and 0-RTT disabled,
  built from `ServerBuilder::http3_identity` PEM paths. TCP SNI/mTLS reload does
  not atomically rotate the QUIC endpoint; rotate H3 via endpoint replacement and
  drain. Documented here so operators do not assume cross-transport atomicity.

Python projection:

- `eggserve.server.HTTPSServer` / `ThreadingHTTPSServer` remain simple
  single-identity (source-compatible). Advanced SNI/mTLS/reload belongs in
  `eggserve.lowlevel`/downstream Rust first; no Python verification callback is
  exposed. Material crosses via paths/bytes copied into native validated state,
  never as retained mutable buffers.

## Handshake timeout

TLS handshakes are bounded by `tls_handshake_timeout` (CLI `--header-timeout` surface, default 10 seconds; `RuntimeConfig.tls_handshake_timeout`). A client that opens a TCP connection but never completes the TLS handshake will hold a connection permit for at most that duration; after timeout the connection is dropped silently. The connection-permit semaphore prevents an unbounded number of pending handshakes, and the per-handshake timeout prevents a single slow client from holding a permit indefinitely. Permits recover after timeout; new handshakes succeed.

## Certificate requirements

- **Format:** PEM-encoded certificate chain and PEM-encoded private key
- **Certificates:** At least one certificate must be present (max 8 per identity)
- **Key:** Exactly one private key must be present (PKCS#1, PKCS#8, or SEC1)
- **Pairing:** Validated before readiness (`keys_match`); mismatches fail fast, never on first hostile connection
- **Encrypted keys:** Not supported (eggserve will error with a clear message)
- **Key file:** Must not be empty or contain non-PEM content
- **SNI names:** Max 64 identities, exact or single-level `*.suffix`, max 253 chars
- **Trust roots/CRLs:** Max 256 roots, 16 CRLs, 1 MiB PEM; validated at build

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

Release gates validate TLS functionality by explicitly enabling the feature during CI. TLS tests (`clippy` and `cargo test` with `--features tls`) cover TLS correctness; they are not satisfied by a default (non-TLS) build. Plan 203 qualification lives in `crates/eggserve-core/tests/tls_identity.rs` (SNI, mTLS, reload, timeout recovery, PROXY→TLS ordering, ALPN parity, log hygiene, metadata provenance).

## Limitations

eggserve's TLS support is intentionally narrow (no PKI framework):

- No ACME / Let's Encrypt automation
- No certificate renewal or discovery
- No secret manager / KMS signing
- No OCSP stapling (and no implied revocation without explicit CRLs)
- No filesystem watcher (operators decide when to reload)
- No Python verification callback
- CLI remains single-identity (multi-identity is Rust `TlsServerConfig`)
- HTTP/2 is not advertised by the default `tls` build; the experimental
  `http2,tls` Rust build advertises `h2`, then `http/1.1`, and selects the
  matching Hyper driver. Python HTTPS remains HTTP/1.1-only.
- H3 uses a separate QUIC identity; TCP reload does not atomically rotate H3.

If you need ACME, PKI automation, or edge termination features beyond this substrate, use a reverse proxy or a dedicated TLS-terminating load balancer.
