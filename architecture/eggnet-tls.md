# eggnet-tls — Neutral TLS Security Substrate

`eggnet-tls` is the reusable, transport-neutral home for EggServe's mature
server-side rustls security policy. It is a workspace crate so the extraction
is tested with EggServe, but it has no dependency on EggServe, Eggress,
EggFetch, HTTP, Tokio, proxy routing, tracing, CLI, Python, or QUIC code.

## Ownership

The crate owns:

- bounded PEM parsing for certificate chains, private keys, trust roots, and
  CRLs;
- certificate/key pairing checks before a configuration is ready;
- exact SNI and conventional single-label wildcard selection with an optional
  default identity;
- explicit `ClientAuthMode::{Disabled, Optional, Required}` using rustls's
  WebPKI verifier;
- trust-root, CRL, identity-chain, identity-count, and SNI bounds;
- immutable `TlsServerConfig` snapshots and `TlsReloadHandle` replacement for
  new handshakes.

The crate deliberately does not own filesystem watchers, certificate issuance,
transport stream adapters, listener lifecycle, HTTP ALPN policy beyond the
small `http2` feature-controlled advertisement, QUIC configuration, logging,
or application policy.

## Public API

```rust,no_run
use eggnet_tls::TlsServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

fn build(certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>)
    -> Result<TlsServerConfig, eggnet_tls::TlsError>
{
    TlsServerConfig::builder()
        .single_identity(certs, key)?
        .build()
}
```

`load_identity`, `parse_identity_pem`, `parse_trust_roots_pem`, and
`parse_crls_pem` are bounded helpers for applications that load operator-owned
material. `TlsServerConfig::server_config()` exposes an immutable rustls
configuration for the application's transport adapter; the builder does not
provide a mutation escape hatch that can bypass its invariants.

Optional client authentication uses `allow_unauthenticated()` internally, so
clients without a certificate are accepted while presented certificates are
still verified against the configured roots. Required authentication rejects
clients without an acceptable certificate. No revocation checking is implied
without explicitly configured CRLs.

## Consumer contract

EggServe re-exports the crate through `eggserve_core::tls` to preserve the
existing 0.1 import path. Its accept loop remains responsible for
`tokio-rustls`; its HTTP/3 adapter separately builds the TLS 1.3/`h3` QUIC
configuration from `eggnet-tls::load_identity`. `TlsReloadHandle` changes the
configuration seen by new TCP handshakes; established sessions are unchanged.

Eggress can later wrap `eggnet-tls` in its transport types while retaining
proxy-specific stream boxing and tracing. Its optional mTLS path must retain a
regression test for anonymous clients and for validation of presented client
certificates. EggFetch is not a current consumer; adopting the crate requires
an explicit MSRV decision in that repository.

## Validation

`crates/eggnet-tls/tests/neutral_tls.rs` covers valid identities, malformed PEM,
identity and trust bounds, SNI rejection rules, reload snapshot replacement,
and disabled/optional/required client authentication, including anonymous,
valid-certificate, and invalid-certificate handshakes. The topology gate also
rejects workspace or transport dependencies from the neutral crate.
