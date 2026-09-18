# Plan 222 — Consolidate server TLS behind eggnet-tls across eggserve and eggress

## Purpose

Use `eggnet-tls` as the shared server-side TLS identity/trust/client-auth substrate across the eggstack networking repositories where semantics are actually common.

The review found substantial duplication in `eggress-transport-tls`, including certificate/root parsing and server client-auth construction already handled more completely by `eggnet-tls`.

## Repositories

Primary:
- `eggstack/eggserve`
- `eggstack/eggress`

Secondary evaluation only:
- `eggstack/eggfetch`

## Goals

- Keep `eggnet-tls` neutral and reusable.
- Migrate eggress server TLS configuration to `eggnet-tls`.
- Verify/fix optional mTLS semantics in eggress during migration.
- Share low-level certificate/trust parsing with eggfetch only where this does not absorb client policy into the shared crate.
- Align rustls-family security floors across repositories.

## Non-goals

- Do not make eggserve depend on eggress or eggfetch.
- Do not move eggfetch client routing/pooling/TLS verification policy into `eggnet-tls`.
- Do not create a universal transport/TLS mega-crate.
- Do not add ACME/PKI automation.

## Security issue to verify

Eggress's current server TLS builder documents optional client certificates, but the optional and required branches appear to construct the same `WebPkiClientVerifier` unless `allow_unauthenticated()` is used.

Before migration, add an integration test:
- optional mode + no client certificate => handshake succeeds,
- optional mode + valid certificate => succeeds/authenticated,
- optional mode + invalid certificate => fails,
- required mode + no certificate => fails,
- required mode + valid certificate => succeeds.

`eggnet-tls` already models `ClientAuthMode::Optional` using `allow_unauthenticated()`.

## Work

### 1. Define neutral substrate contract

Keep in `eggnet-tls`:
- bounded PEM certificate parsing,
- private-key parsing and key/cert match validation,
- bounded trust roots/CRLs,
- SNI exact/wildcard/default identities,
- disabled/optional/required WebPKI client auth,
- immutable server config snapshots/reload,
- ALPN list construction hooks that do not assume a product.

No Tokio, Hyper, Quinn, tracing, CLI, proxy, or product crate dependencies.

### 2. Eggress migration

Replace duplicated server builder behavior with `eggnet-tls`.

Keep egress-specific:
- transport accept/connect functions,
- proxy policy,
- logging,
- client-side TLS configuration if its semantics differ,
- protocol-specific ALPN choices.

Deprecate/remove duplicate PEM/root/server-auth code after parity tests.

### 3. Eggfetch evaluation

Evaluate extracting only genuinely neutral helpers:
- cert-chain parsing,
- private-key parsing,
- key/cert validation,
- custom root parsing.

Keep in eggfetch:
- `TlsConfig`,
- trust-store selection policy,
- native-vs-webpki fallback,
- insecure verifier switches,
- hostname verification policy,
- min/max protocol version policy,
- SNI enablement,
- connection-policy identity,
- pool/route interaction.

Do not force a dependency if the helper extraction increases coupling or binary footprint.

### 4. Version alignment

Use a common minimum patched rustls floor across repositories. Coordinate rustls/rustls-webpki/tokio-rustls updates rather than allowing avoidable drift.

### 5. Publishing/dependency direction

If `eggnet-tls` remains physically in eggserve, decide whether cross-repo use should be:
- published as its own crates.io package, or
- moved to a neutral repository.

Do not use git dependencies for normal released builds unless explicitly justified; prefer crates.io versioned dependency.

## Tests

- eggserve TLS suite,
- eggress TLS client/server suite,
- new optional mTLS regression suite,
- SNI wildcard/default tests,
- CRL/trust bounds,
- key mismatch tests,
- dependency graph checks proving no product dependency leaks into `eggnet-tls`.

## Rollback

Keep the old egress builder behind a temporary internal compatibility layer until new tests pass. Once deleted, do not restore duplicated security logic; fix the shared substrate or an egress-specific adapter instead.

## Acceptance criteria

- Eggserve and eggress server TLS use one neutral authority.
- Optional mTLS behavior is explicitly tested and correct.
- Eggnet-tls remains product/transport neutral.
- Eggfetch client policy remains locally owned.
- Rustls-family floors are aligned.
- No new cyclic or product-to-product dependency is introduced.

## Status

**Eggserve-side complete — 2026-09-18.** The neutral substrate contract
(items 1, 4 in part, and 5) is implemented and documented in this
repository. Eggress migration (item 2), the eggfetch helper decision
(item 3), and the sibling rustls-floor bumps (item 4) are follow-ups owned
by those repositories; per Plan 212 they were deliberately not modified in
the eggserve commit.

## Implementation record (eggserve)

- Neutral ALPN hook: `TlsServerConfigBuilder::alpn_protocols(..)` and
  `load_tls_config_with_alpn(..)` advertise caller-supplied protocol
  identifiers (empty means no ALPN) with `MAX_ALPN_PROTOCOLS` ×
  `MAX_ALPN_PROTOCOL_LEN` (16×255) bounds surfaced as the new
  `TlsError::InvalidAlpn` variant, validated before identity loading.
  The HTTP `http2(bool)` convenience now delegates to the public
  `http_alpn_protocols(..)` helper, is documented HTTP-only, and follows
  last-call-wins precedence against the explicit hook. Non-HTTP transports
  must never reuse the HTTP helper.
- Tests: `crates/eggnet-tls/tests/neutral_tls.rs` gains custom-ALPN
  construction/advertisement, empty-ALPN acceptance, bound rejection,
  last-wins precedence in both directions, fail-before-identity file-loader
  ordering, and an end-to-end custom-ALPN handshake asserting the
  negotiated protocol on both ends. The existing five-case optional/required
  mTLS suite (the regression suite Plan 222 requires of eggress) continues
  to pass unchanged.
- No new dependencies: the production graph remains `rustls` +
  `rustls-pki-types` only; the topology gate is unmodified and passes.
- Eggserve rustls floors stay at the `0.23.45` caret floor in every
  constraining manifest including the excluded Python crate.
- Cross-repo contract documented in `architecture/eggnet-tls.md`: the
  eggress optional-mTLS defect (non-required branch missing
  `allow_unauthenticated()`, with file/line reference and the five-case
  test prescription), the eggress adapter sketch (keep transport/proxy/
  logging/client-TLS/ALPN-choices local; replace duplicated server
  PEM/root/verifier construction), the eggfetch no-dependency evaluation,
  the sibling floor table, and the publishing decision (remain in the
  eggserve workspace; versioned crates.io package; never git dependencies).
- User-facing updates: `docs/tls.md` (neutral ALPN hook), `README.md`
  (Plan 222 status), `AGENTS.md` and the `eggserve-dev` skill (Plan 222
  bullet), `architecture/crate-topology.md` (ALPN ownership),
  `docs/dependency-policy.md` (sibling floor follow-up).

## Validation record

Local validation: `cargo fmt`, topology/conformance/release-metadata gates,
workspace checks (default, `http2,tls`, `http3,tls`), clippy with
`-D warnings`, workspace tests, the excluded Python manifest check, and the
`tls`/`http2,tls`/`http3,tls` feature test lanes per routine CI. The eggress
TLS suite, the new eggress optional-mTLS regression suite, and the sibling
floor bumps run in their own repositories when those follow-ups land.
