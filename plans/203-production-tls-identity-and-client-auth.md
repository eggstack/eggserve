# Plan 203 — Production TLS Identity, SNI, Client Authentication, and Reload

## Status

**PLANNED.** Prerequisites: Plan 197 connection context; Plan 201 listener ownership should be compatible. Builds on existing rustls/TLS support and H2/H3 ALPN work.

## Purpose

Evolve the existing single-identity TLS termination path into a production embedding substrate without turning EggServe into certificate automation or PKI software.

The required capabilities are:

- multiple server identities selected by SNI;
- optional required or optional client-certificate authentication (mTLS);
- typed TLS session metadata available to downstream applications;
- bounded TLS handshake policy;
- atomic reload of certificate/key/trust configuration for new connections;
- coherent ALPN policy across H1/H2 and the H3 identity boundary.

ACME issuance/renewal, secret storage, external KMS signing, and certificate discovery remain downstream/operator responsibilities.

## Current ecosystem input

Current rustls 0.23 exposes dynamic server-certificate resolution (`ResolvesServerCert`/SNI resolvers), WebPKI-backed client certificate verification, and server configuration paths for custom client verifiers and certificate resolvers. Use those maintained mechanisms rather than implementing TLS parsing/verification policy in EggServe.

## Track A — Separate TLS material from listener/runtime config

Introduce an EggServe-owned TLS identity/configuration model that can be built from loaded PEM material or caller-provided validated identity objects without leaking rustls types through the general service API.

Conceptually:

```text
TlsServerConfig
  identities/resolver
  client_auth
  alpn/profile
  handshake_timeout
  session/ticket policy (existing defaults unless explicitly exposed)
```

Keep certificate/key parsing errors at startup/reload boundaries. Never defer obvious invalid key/cert pairing to first hostile connection if it can be validated earlier.

Do not put file paths into the canonical runtime state as the only form; embedders need in-memory/caller-supplied material.

## Track B — SNI/multi-certificate identity selection

Support multiple named identities using rustls certificate resolution.

Requirements:

- exact DNS names and well-defined wildcard behavior; do not invent hostname matching rules inconsistent with TLS conventions;
- optional explicit default identity for clients without SNI;
- fail handshake if no identity matches and no default exists;
- identity selection has no blocking filesystem/network IO in the handshake path;
- SNI hostname is validated/bounded before observability/application metadata use;
- key/certificate contents are never logged;
- ALPN selection remains coherent with enabled H1/H2 protocol configuration.

Expose only sanitized selected server-name/identity metadata to applications if useful; do not expose private-key objects.

## Track C — Client certificate authentication

Add explicit modes:

```text
Disabled
Optional(trust_roots / verifier policy)
Required(trust_roots / verifier policy)
```

Use rustls/WebPKI verification for the built-in trust-root path. Advanced custom verifier injection may remain a Rust-only experimental TLS adapter if exposing it is necessary; do not create a Python callback invoked inside TLS verification.

Requirements:

- `Required`: handshake fails when no acceptable client cert is supplied;
- `Optional`: unauthenticated clients remain allowed, authenticated clients expose verified identity metadata;
- trust roots and CRLs, if supported, are bounded/validated at configuration load;
- revocation behavior is explicit—do not imply revocation checking when no CRL/OCSP policy is configured;
- application receives only verified certificate metadata in a trusted field, never raw unverified identity claims;
- raw DER chain exposure, if offered, is opt-in and size bounded.

A useful request-context representation includes `peer_certificates_present`, verified authentication state, and optionally a bounded DER chain/fingerprint. Avoid parsing X.509 subject/SAN into a home-grown identity policy in core; downstream applications can use a dedicated X.509 library if they require semantic identities.

## Track D — TLS metadata contract

Extend `ConnectionInfo`/request context with trustworthy TLS metadata such as:

- TLS active yes/no;
- negotiated protocol version;
- negotiated ALPN;
- SNI/server name if supplied/accepted;
- client-auth state;
- optional peer certificate chain/fingerprint under explicit configuration.

Do not expose cipher/provider-specific internals as stable enums unless there is a demonstrated consumer need. A small non-exhaustive metadata model is preferable.

Caller-owned transports may assert TLS metadata only through the existing caller-trusted API boundary; distinguish caller-asserted from EggServe-terminated TLS where provenance matters.

## Track E — Handshake timeout and admission

Ensure TLS handshake has a dedicated bounded timeout and concurrency policy distinct from application service admission.

Audit ordering:

```text
accept permit / connection admission
 -> optional PROXY preamble (Plan 202)
 -> TLS handshake deadline
 -> protocol selection
 -> HTTP request work
```

Requirements:

- slow TLS clients cannot occupy unlimited handshake tasks;
- timeout closes transport and releases permits exactly once;
- certificate selection/client verification cannot perform unbounded blocking work;
- handshake errors use fixed sanitized categories in logs and never echo rustls internals to clients;
- H2 ALPN advertisement derives from enabled runtime protocol policy;
- H3 remains TLS 1.3/QUIC-specific and uses equivalent identity-selection/reload data where practical rather than a duplicated key store.

## Track F — Atomic configuration reload

Support replacing TLS identity/trust state for **new** handshakes without restarting the entire server.

Preferred ownership:

- immutable validated TLS configuration objects;
- an atomic/lock-protected pointer/resolver snapshot read by new handshakes;
- existing established connections continue using their original session state;
- reload either succeeds completely or leaves the prior configuration active.

API examples might be `ServerHandle::replace_tls_config(...)` or a cloneable `TlsConfigHandle` supplied to the builder. The exact form must avoid mixing general dynamic runtime config with this narrow reloadable state.

Do not add filesystem watchers. A helper may load PEM and call reload, but deciding when files changed belongs to the operator/downstream process.

For H3, determine whether the active Quinn endpoint can update server crypto configuration for new connections safely. If not, document that H3 identity rotation requires endpoint replacement/drain and provide a bounded handoff strategy rather than pretending reload is atomic across transports.

## Track G — Session resumption and 0-RTT policy

Preserve conservative defaults. H3/QUIC 0-RTT remains disabled unless a separate replay-safety product decision explicitly enables it. Do not enable HTTP early data merely because rustls/Quinn can support it.

If TLS session resumption/tickets are currently enabled by rustls defaults, document their ownership and bound server-side storage. Do not add a distributed ticket-key system.

## Track H — Python and embedding projection

Expose construction/reload through low-level Python only where material can be transferred safely and without retaining mutable Python buffers as key storage. Prefer paths/bytes copied into native validated state.

Do not expose client-certificate verification as arbitrary Python callbacks during handshake.

`eggserve.server.HTTPSServer` compatibility behavior should remain simple single-identity unless additive SNI configuration can be introduced without source/semantic breakage. Advanced TLS belongs in `eggserve.lowlevel`/downstream APIs first.

## Security verification

Required tests:

- exact and wildcard SNI selection;
- no-SNI/default/no-match behavior;
- invalid cert/key pair fails before ready;
- required/optional/disabled client auth;
- untrusted/expired/malformed client certificate rejection through rustls;
- reload success changes new handshakes but not established sessions;
- failed reload preserves old config;
- concurrent reload/handshake race safety;
- handshake timeout and permit recovery;
- PROXY->TLS ordering with Plan 202;
- H1/H2 ALPN parity after reload;
- H3 identity consistency/explicit endpoint-rotation behavior;
- logs never contain PEM/key bytes;
- TLS metadata exposed to services is verified/provenance-correct.

## Acceptance criteria

- [ ] multiple server identities can be selected by SNI using maintained rustls resolution mechanisms;
- [ ] optional and required mTLS are supported with explicit trust policy;
- [ ] verified TLS metadata is available through the canonical connection/request context without exposing private internals;
- [ ] handshake duration and concurrent resource use are bounded;
- [ ] certificate/key/trust reload is atomic for new applicable handshakes and failed reload is non-destructive;
- [ ] H1/H2 ALPN and H3 TLS identity policy remain coherent and documented;
- [ ] 0-RTT remains disabled unless separately authorized;
- [ ] no ACME, secret manager, PKI framework, filesystem watcher, or Python verification callback is added;
- [ ] synchronous compatibility HTTPS remains source-compatible unless separately documented.

## Handoff

Plan 204 may project verified TLS metadata into downstream Python application scopes. Plan 207 must qualify SNI/mTLS/reload behavior across representative transports.