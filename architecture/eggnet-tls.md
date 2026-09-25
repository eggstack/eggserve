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
- trust-root, CRL, identity-chain, identity-count, SNI, and ALPN bounds;
- immutable `TlsServerConfig` snapshots and `TlsReloadHandle` replacement for
  new handshakes;
- product-neutral ALPN construction: `http2(bool)` / `http_alpn_protocols`
  are the HTTP-only convenience, while `alpn_protocols(..)` /
  `load_tls_config_with_alpn(..)` let non-HTTP transports advertise their
  own identifiers (empty means no ALPN; last call wins).

The crate deliberately does not own filesystem watchers, certificate issuance,
transport stream adapters, listener lifecycle, QUIC configuration, logging,
or application policy. HTTP/3 QUIC assembly lives once in `eggserve-h3`;
Tokio stream wrapping stays consumer-owned.

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

## ALPN defaults and bounds

The `http2` feature controls the ALPN default: `load_tls_config` follows
`cfg!(feature = "http2")` (`crates/eggnet-tls/src/lib.rs`), delegating
through `load_tls_config_with_http2` to `http_alpn_protocols(http2)` (offers
`h2` before `http/1.1` when enabled, `http/1.1` only otherwise). The neutral
`alpn_protocols(..)` builder hook / `load_tls_config_with_alpn(..)` lets
non-HTTP transports advertise their own identifiers (empty means no ALPN);
an explicit override wins over the `http2(bool)` convenience (last call wins).

Bounded inputs (`crates/eggnet-tls/src/lib.rs`):

| Bound | Value | Source |
|---|---|---|
| Max SNI identities | 64 | `MAX_TLS_IDENTITIES` |
| Max SNI name length | 253 chars | `MAX_SNI_LEN` |
| Max trust roots | 256 | `MAX_TRUST_ROOTS` |
| Max CRLs | 16 | `MAX_CRLS` |
| Max ALPN protocols | 16 × 255 bytes | `MAX_ALPN_PROTOCOLS` / `MAX_ALPN_PROTOCOL_LEN` |
| Max certificates per identity chain | 8 | `MAX_IDENTITY_CHAIN` |
| Max trust/CRL PEM input | 1 MiB | `MAX_TRUST_PEM_BYTES` |

## Consumer contract

EggServe re-exports the crate through `eggserve_core::tls` to preserve the
existing 0.1 import path. `tokio-rustls` wrapping belongs to EggServe's
accept loop and consumer transport adapters, not to this crate: the crate
has no Tokio dependency (production dependencies are `rustls` +
`rustls-pki-types` only per `crates/eggnet-tls/Cargo.toml`; `tokio` +
`tokio-rustls` plus `rcgen` appear solely in `dev-dependencies` for tests). Its HTTP/3 adapter separately builds the TLS 1.3/`h3` QUIC
configuration from `eggnet-tls::load_identity`. `TlsReloadHandle` changes the
configuration seen by new TCP handshakes; established sessions are unchanged.

## Cross-repository status (Plan 222)

Plan 222 uses this crate as the shared server-side TLS substrate. The
eggserve-side work is complete; the eggress/eggfetch work lives in those
repositories (eggserve never depends on either as a product).

- **Eggress (migration follow-up, in the eggress repository).** The
  `eggress-transport-tls` server builder duplicates certificate/root parsing
  and server client-auth construction already owned here, and its optional
  client-auth path is semantically wrong: the non-required branch builds a
  plain `WebPkiClientVerifier` without `allow_unauthenticated()`
  (`crates/eggress-transport-tls/src/server.rs`), so "optional" currently
  requires a client certificate. The migration keeps egress-specific
  transport accept/connect, proxy policy, logging, client-side TLS (its
  semantics differ: system roots, insecure switches, hostname policy), and
  protocol-specific ALPN choices, while replacing the duplicated server
  PEM/root/verifier construction with `eggnet-tls` — using the neutral
  `alpn_protocols` hook for non-HTTP transports rather than the HTTP
  `http2` convenience. Before migrating, eggress must add the Plan 222
  integration test: optional + no cert succeeds, optional + valid cert
  succeeds/authenticated, optional + invalid cert fails, required + no cert
  fails, required + valid cert succeeds. This crate already models
  `Optional` with `allow_unauthenticated()` and covers all five cases in
  `crates/eggnet-tls/tests/neutral_tls.rs`.
- **Eggfetch (evaluation: no dependency).** Eggfetch is primarily a TLS
  *client*; its `TlsConfig`, trust-store selection, native-vs-WebPKI
  fallback, insecure switches, hostname verification, protocol-version
  policy, SNI enablement, connection-policy identity, and pool/route
  interaction stay locally owned per Plan 222. The neutral parsing helpers
  (`parse_identity_pem`, `parse_trust_roots_pem`, key/cert pairing) remain
  available if eggfetch later finds a genuine reduction, but no dependency
  is forced: client policy must not be absorbed into the shared crate.
- **Version floors.** At Plan 222 sign-off, all three repositories resolved
  rustls 0.23.45, but only eggserve manifests enforce the Plan 218 `0.23.45` caret floor
  (RUSTSEC-2026-0285), including the excluded Python manifest. Eggress
  (`rustls = "0.23"`, tokio-rustls 0.26) and eggfetch-core
  (`rustls = "0.23"` optional, hyper-rustls 0.27, tokio-rustls 0.26) still
  declare bare floors, so a fresh resolve there could pick a pre-patch
  rustls. Raising those floors is a follow-up in each repository; coordinate
  rustls/rustls-webpki/tokio-rustls updates rather than letting them drift.
- **Publishing.** `eggnet-tls` remains physically in the eggserve workspace
  and is published as its own versioned crates.io package (the release
  package gate already stages it for layered publication). Cross-repo use is
  a normal versioned dependency, never a git dependency. A move to a
  neutral repository is deferred until a second publisher actually needs
  it; no universal transport/TLS mega-crate is created.

## Validation

`crates/eggnet-tls/tests/neutral_tls.rs` covers valid identities, malformed PEM,
identity and trust bounds, SNI rejection rules, reload snapshot replacement,
neutral ALPN construction/validation/negotiation (custom protocols, empty
advertisement, bound rejection, last-wins precedence, end-to-end handshake),
and disabled/optional/required client authentication, including anonymous,
valid-certificate, and invalid-certificate handshakes. The topology gate also
rejects workspace or transport dependencies from the neutral crate.
