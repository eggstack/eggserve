# Plan 009: optional TLS and deployment guidance

## Goal

Add optional native TLS support and deployment guidance without changing eggserve's core scope. eggserve should remain a hardened static-serving foundation, not a full edge proxy or certificate automation platform. TLS should be feature-gated, minimal, and documented as one deployment option alongside reverse-proxy TLS.

This phase should start only after Plan 008 polish is complete or explicitly deferred.

## Scope

In scope:

```text
optional rustls/tokio-rustls feature
CLI flags for cert and key paths
TLS listener wrapping around the existing Hyper HTTP/1 service
safe startup output showing http vs https
clear deployment docs for native TLS and reverse-proxy TLS
certificate/key loading errors with safe messages
CI build coverage for TLS feature
basic TLS smoke test where feasible
```

Out of scope:

```text
ACME / Let's Encrypt automation
certificate renewal
SNI virtual hosting
client certificate authentication
TLS termination proxy behavior
HTTP/2
OCSP stapling
hot certificate reload
multi-cert routing
configuration file format unless already introduced elsewhere
```

## Design principles

TLS must not bloat the minimal build. Native TLS support should be behind a feature flag:

```toml
[features]
default = []
tls = ["rustls", "tokio-rustls", "rustls-pemfile"]
```

Exact dependency names may vary, but the dependency policy must be updated with justification.

The default HTTP-only binary should remain valid. It should warn clearly when public HTTP serving is used, but it should not force TLS. Many deployments should put eggserve behind Caddy, nginx, Traefik, a cloud load balancer, or WireGuard/private network exposure.

## CLI behavior

Add flags only when the TLS feature is enabled:

```text
--tls-cert <PATH>       PEM certificate chain
--tls-key <PATH>        PEM private key
```

Behavior:

```text
neither cert nor key -> plain HTTP
both cert and key -> HTTPS listener
only one provided -> config error
invalid cert/key -> config error
encrypted private key unsupported initially unless deliberately implemented
```

Startup banner:

```text
Listening: http://127.0.0.1:8000
```

or:

```text
Listening: https://127.0.0.1:8000
TLS: enabled, certificate: cert.pem
```

Do not print private key paths unless useful; never print key contents.

## Architecture

Keep serving logic unchanged. TLS should wrap the accepted TCP stream before passing it into the existing Hyper service path.

Suggested structure:

```text
crates/eggserve-bin/src/tls.rs
crates/eggserve-bin/src/listener.rs or equivalent
```

Minimal split:

```rust
#[cfg(feature = "tls")]
struct TlsConfig { ... }

#[cfg(feature = "tls")]
async fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<rustls::ServerConfig>, TlsError>
```

Avoid leaking TLS types into `eggserve-core`. TLS is a process/listener concern, not a static-serving primitive.

## Certificate loading

Support PEM certificate chains and PEM private keys.

Validation requirements:

```text
certificate file exists and is readable
key file exists and is readable
at least one certificate parsed
exactly one usable private key parsed or clear error if multiple/none
unsupported encrypted keys produce clear error
errors do not dump PEM contents
```

## Testing

Unit tests:

```text
missing cert/key pair errors
only cert errors
only key errors
invalid cert file errors
invalid key file errors
```

Integration/smoke test if practical:

```text
generate or include test cert fixture
start eggserve with --tls-cert and --tls-key on local port
request with a TLS client that disables verification for test cert
assert static file body matches
```

If TLS smoke testing is too heavy for this phase, at minimum add a CI `cargo check --workspace --features tls` job.

## Documentation

Add or update:

```text
docs/deployment.md
docs/tls.md
docs/security-policy.md
docs/dependency-policy.md
README.md
```

Deployment docs should show three patterns:

```text
local-only HTTP: python -m eggserve --directory public
reverse-proxy TLS: Caddy/nginx/TLS terminates, eggserve binds 127.0.0.1
native TLS: eggserve --tls-cert cert.pem --tls-key key.pem
```

State explicitly:

```text
eggserve does not manage certificates
eggserve does not implement ACME
eggserve native TLS is intended for simple deployments, lab use, or controlled environments
public production deployments should usually prefer a mature TLS terminator unless native TLS is sufficient for the threat model
```

## CI

Add checks:

```text
cargo check --workspace --features tls
cargo test --workspace --features tls if tests are feature-compatible
```

If feature unification creates dependency problems, isolate TLS feature to `eggserve-bin` only.

## Acceptance criteria

```text
Minimal/default build does not include TLS dependencies.
TLS feature builds successfully.
CLI rejects partial TLS config.
CLI can start HTTPS with valid cert/key when feature is enabled.
TLS does not alter core static-serving policy.
Docs clearly recommend reverse proxy TLS for many deployments.
No ACME or certificate lifecycle code is added.
```

## Suggested commit sequence

```text
chore(deps): add feature-gated rustls dependencies
feat(cli): add tls cert and key arguments behind tls feature
feat(server): wrap accepted streams with rustls when configured
test(tls): add cert/key loading tests and optional smoke test
docs: add TLS and deployment guidance
ci: add tls feature build check
```

## Risks

The main risk is scope creep. Native TLS can quickly become a certificate-management product. Keep the implementation intentionally small. If users need automatic public HTTPS, recommend a reverse proxy.
