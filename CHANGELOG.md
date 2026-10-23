# Changelog

## 0.5.0 — published 2026-10-22

The 0.4.0 line shipped on 2026-09-25 without the defect batch that landed
in the repository immediately afterwards. This line publishes that batch.
It is a **minor** bump of the Rust line because the compatibility
`RuntimeConfig` gains four public fields (source-incompatible for
exhaustive struct literals — the Plan 285 class) and because the batch
changes observable behavior for several request paths.

- **Conditional requests.** `If-Match: *` now requires an ETag
  (RFC 9110 §13.1.1); `If-Modified-Since` is no longer evaluated through
  the date branch when the ETag is suppressed; an unsatisfiable `Range`
  combined with a failed `If-Range` serves `200` full
  (RFC 9110 §13.1.5); `If-Range` with a date uses `<=`; a wildcard `304`
  keeps `Last-Modified`; a second `Range` header joins into
  `MultipleRanges` and serves `200` full (RFC 9110 §5.2); the range unit
  is matched case-insensitively.
- **Header combining.** `Range`, `If-Range`, and date headers use
  first-wins; only `If-Match` and `If-None-Match` comma-join. A second
  `Range` no longer silently overrides the first.
- **Framing and strictness.** A non-numeric `Content-Length` is a `400`
  framing error; the parse is strict `1*DIGIT` (also for `TRACE`); an
  unknown-length stream rejects any declared `Content-Length` and
  duplicates; `Http1Upgrade` requires a protocol (a bare `101` is
  rejected); an opaque `Expect` is `417`; every `Expect` value is
  inspected, not just the first.
- **Error representation.** A `499` collapses to `500` on the
  `ServiceError` wire path; a `HEAD` error response carries the `GET`
  length; a `HEAD`-only `EmptyWithLength` collapses to `Empty` on non-`HEAD`
  so framing and bytes agree; `PrematureEof`/`LengthMismatch` map to
  `IncompleteError`, a transport error is sanitized, and `PathRejected`
  maps to `PermissionError`.
- **Fail-closed negotiation.** `version_to_http` and `from_state` return
  `Result` instead of panicking or silently downgrading a future HTTP
  version.
- **Confinement and durability.** Child path resolution inherits the
  confined backslash policy; fd-relative resolution re-applies the
  platform component checks; a buffer-export failure fails the stream
  instead of truncating it silently; a listener/address pair is carried as
  one value so it cannot be mispaired into a panic; shutdown drain is
  bounded by `graceful_shutdown_timeout`.
- **Host authority.** The Tower adapter requires the authority to agree
  with a mutated `Host`.
- **Python facade.** The synchronous callback permit keeps its documented
  queue-behind-permit contract (the fail-fast shed belongs to the
  separate async task limit); conditional/range lookups are
  case-insensitive; stream chunks are split at 64 KiB against a
  byte-bounded channel; empty header values are accepted and
  `Content-Length` parsing is trimmed.
- **Single authorities (no observable change).** The runtime error
  representation now has one authority in `eggserve-primitives` instead of
  two parallel builders. The HTTP/3 `Alt-Svc` advertisement is single
  sourced in `eggserve-primitives` (additive public API) instead of being
  duplicated in the compatibility path and the H3 adapter. The Plan 253
  connection-overlap ledger is corrected and machine-checked by a drift
  bound.
- **Compatibility configuration.** The compatibility `RuntimeConfig` now
  carries `http1_request_target_mode`, `policy_ownership`,
  `admission_ownership`, and `runtime_rejection_presenter` instead of
  hardcoding the direct crate's defaults. **The defaults are unchanged**,
  so an unconfigured deployment behaves exactly as before; the knobs are
  reachable only from Rust. Adding public fields to `RuntimeConfig` is
  source-incompatible for exhaustive struct literals — see the migration
  guide.

Prepared as `eggserve-primitives 0.2.3`, `eggserve-server`/
`eggserve-static`/`eggserve-h3`/`eggserve-core 0.5.0`, `eggserve-bin 0.2.3`,
wheel `eggserve 0.2.5`. H2/H3 tiers, TLS, static confinement defaults, and
every safe default are unchanged. `eggnet-tls` is not republished (its
published `0.2.0` content still matches the local tree).

## 0.4.0 — published 2026-09-25 (superseded by 0.5.0)

- Published the Rust line as `eggserve-primitives 0.2.2`,
  `eggserve-server`/`eggserve-static`/`eggserve-h3`/`eggserve-core 0.4.0`,
  `eggserve-bin 0.2.2`, and the wheel as `eggserve 0.2.4`. The minor bump
  carries the Plan 296 WP-A feature edge: the `tower` feature no longer
  activates `tower-layer` (the adapter drives `Tower` services but never
  implements the `Layer` contract, so downstream `Layer` authors depend on
  `tower-layer` directly).
- **This published artifact predates the corrective batch.** The `v0.4.0`
  tag points at the bump commit; the B1–B89 defect fixes and the CI fix
  landed after it and are **not** in these crates. Consumers of
  `eggserve-* 0.4.0` are running with the conditional-request, header
  combining, framing-strictness, fail-closed-negotiation, confinement
  inheritance, and Python callback-permit defects enumerated under 0.5.0.
  Move to the 0.5.0 line.
- Publication evidence (per-crate checksums, the exact-SHA hosted CI run,
  registry-only consumer proofs, the tag, the GitHub Release, and the
  wheel set) is recorded in
  `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`.

## 0.3.1 — published (Plans 288–291)

- Removed the EggServe hard validation maxima for H1 parser buffer size and
  header count; the 8192-byte Hyper minimum and positive field count remain,
  with defaults unchanged.
- Added direct-H1 ownership controls for the post-parse aggregate request
  header-byte ceiling and successful service-response Date/Server metadata.
  Runtime errors, response framing, and the stripped-header denylist remain
  runtime-owned. Registry-only qualification proves these APIs on the
  published artifact.

Published 2026-09-25 as `eggserve-server 0.3.1` only. The exact checksum,
registry consumers, and hosted CI evidence are in
`release/plan-291-direct-h1-boundary-ownership-publication-closure.md`.

## 0.3.0 — published

- Moved the optional HTTP interop and Tower adapter implementation to
  `eggserve-server` (Plans 276–277), where direct H1 consumers enable
  `http-interop` or `tower` without depending on `eggserve-core`,
  `eggserve-static`, or PHF. Historical core adapter paths remain
  compatibility re-exports. The direct H1 + Tower graph is proven free of
  `eggserve-static`/PHF by registry-only consumers.
- Added the opt-in forward-proxy absolute-form seam (Plans 278–279):
  `OriginOnly` default, static stays origin-only and rejects absolute-form.
- Added explicit external policy/admission ownership (Plans 280–281),
  the narrow `H1ConnectionPolicy` projection (Plan 282), and
  presentation-only typed runtime rejection (Plan 283). Secure defaults
  stay EggServe-owned; `0.3.0` is a minor bump because exhaustive
  `RuntimeConfig` literals and direct service/tunnel semaphore accessors
  are source-incompatible (Plan 285).
- Kept the direct opaque H1 tunnel transport (Plan 284 KEEP, evidence-gated).
- H2/H3 tiers, TLS, static confinement, and Python runtime behavior are
  unchanged. No Python wheel release is included.

Published 2026-09-24 as `eggserve-primitives 0.2.1`,
`eggserve-server`/`eggserve-static`/`eggserve-h3`/`eggserve-core 0.3.0`,
`eggserve-bin 0.2.1`; see [the Plan 286 closure evidence](release/plan-286-embedding-contract-publication-closure.md)
for timestamps, checksums, and the registry-only consumer proof.

## 0.2.3 — folded into 0.3.0, never published alone

- Moved the optional HTTP interop and Tower adapter implementation to
  `eggserve-server`, where direct H1 consumers can enable `http-interop` or
  `tower` without depending on `eggserve-core`, `eggserve-static`, or PHF.
- Kept the historical core adapter paths as compatibility re-exports and
  forwarded core's adapter features to the server. Core remains the
  compatibility/static umbrella and keeps its static-serving dependencies.
- No static-serving, H1 transport, lifecycle, H2/H3, TLS, or Python runtime
  behavior changed. The adapters remain experimental at their existing tier.
- Workspace and Python source metadata are synchronized at 0.2.3; no Python
  wheel release is included.

## 0.2.2 — published

- Repaired the optional `http-interop`/`tower` feature path with a core-owned
  `HttpRequestBody` adapter after the canonical `RequestBody` moved to the
  primitives crate and made the former external-trait/foreign-type
  implementation illegal.
- Preserved canonical `RequestBody` and direct H1 runtime contracts. Adapter
  and server APIs remain experimental; routine CI now covers the advertised
  adapter feature profiles and Axum 0.8 composition.
- Python runtime behavior is unchanged. This synchronized source version does
  not constitute a Python wheel release.

## 0.2.1 — published

- Added `eggserve-server::ServerHandle::into_parts()` with cloneable
  `ServerControl` and typed, cancellation-safe `ServerCompletion` supervision.
- Propagated top-level and escaping runtime-owned connection-task panic or
  cancellation through `ServerError::Terminal`; legacy `wait(self) -> ()`
  remains source-compatible.
- Made `Duration::ZERO` an explicit opt-out for the total connection lifetime.
  The 60-second default and independent timeout, admission, and shutdown
  protections remain unchanged. The existing Python `Server` timeout
  parameter maps zero to the same opt-out; its default is unchanged.
- Added a leaf-crate-only prebound-listener and supervised keep-alive fixture.
- Generalized layered crate package verification to derive release versions
  from Cargo metadata.

Published from implementation candidate
`466cf6301f20c7202f696c495e6eb8d5e74664be`. The registry-only consumer proof
resolves `eggserve-server 0.2.1` and passes the supervised keep-alive smoke;
see [the Plan 272 closure evidence](release/plan-272-downstream-embedding-qualification-closure.md).
