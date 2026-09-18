# Plan 223 — Consolidate outbound HTTP CONNECT wire mechanics between eggfetch and eggress

## Purpose

Remove duplicated outbound HTTP/1 CONNECT handshake code between `eggfetch-http-connect` and `eggress-protocol-http` without making eggserve depend on an HTTP client or proxy stack.

This is a cross-repo maintenance plan; eggserve itself should remain unaffected at runtime.

## Repositories

- `eggstack/eggfetch`
- `eggstack/eggress`
- `eggstack/eggserve` only as planning/architecture coordination

## Goals

- One small neutral implementation of caller-owned-stream outbound HTTP CONNECT mechanics.
- Preserve egress-specific inbound CONNECT server parsing/forwarding.
- Preserve eggfetch-specific proxy selection, dialing, timeout/retry, and TLS policy.
- Avoid product-to-product dependency layering.

## Preferred shape

Promote/rename the current `eggfetch-http-connect` concept into a neutral crate such as `eggnet-http-connect` if cross-repo naming matters.

The primitive should own only:
- CONNECT authority formatting,
- request-head encoding,
- optional Proxy-Authorization encoding,
- bounded response-head parsing,
- response status extraction,
- preservation of read-ahead bytes,
- neutral structured errors.

Caller owns:
- socket dialing,
- DNS,
- TLS,
- timeout wrapping,
- retry,
- status policy beyond parse/result classification if desired,
- proxy routing,
- tunnel lifecycle.

## Non-goals

- Do not share egress inbound CONNECT authentication/server parser unless a later review proves identical semantics.
- Do not move ordinary HTTP forwarding into the neutral crate.
- Do not make eggserve depend on the CONNECT crate.
- Do not combine H2/H3 CONNECT with the H1 wire primitive.

## Work

### 1. Compare behavior

Diff current eggfetch and eggress outbound implementations for:
- IPv6 authority rendering,
- empty/invalid host rejection,
- credential control-character rejection,
- header/count/byte limits,
- status-line parsing,
- obs-text handling,
- read-ahead preservation,
- 2xx acceptance policy,
- 407/403/502/504 mapping.

Resolve semantic differences explicitly before deleting either implementation.

### 2. Neutral API

Keep the API stream-generic over caller-owned `AsyncRead + AsyncWrite` where practical.

Do not expose eggfetch or eggress target-address types. Use a neutral target/authority representation or caller-supplied validated authority.

### 3. Eggfetch migration

Keep its current proxy orchestration and use the neutral wire primitive for encode/read.

### 4. Eggress migration

Replace only outbound `http_connect` wire mechanics. Retain:
- inbound `handle_connect`,
- proxy auth policy,
- target abstractions,
- forwarding,
- relay,
- H2 CONNECT pool,
- H3 CONNECT behavior.

### 5. Dependency footprint

The neutral crate should remain extremely small. Avoid pulling `http`, Hyper, URL, rustls, or a framework if the current byte-level implementation does not need them.

## Security invariants

- No credential bytes appear in errors/logs.
- Limits are enforced during read, not after allocation.
- Bytes read beyond the CONNECT head are never discarded.
- Authority encoding cannot permit CR/LF/request-line injection.
- IPv6 authority uses brackets.
- Timeouts remain caller-owned so the primitive cannot silently widen policy.

## Tests

- fragmented response head,
- response head + tunneled bytes in one read,
- oversized head,
- too many headers,
- malformed status,
- obs-text header value,
- IPv4/domain/IPv6 authority,
- credential injection attempts,
- non-2xx statuses,
- cancellation/timeout via caller wrapper,
- parity tests in both products.

## Acceptance criteria

- Eggfetch and eggress no longer maintain separate outbound H1 CONNECT encoders/parsers.
- Neutral crate has no product, TLS, DNS, socket, retry, or routing dependency.
- Eggress inbound CONNECT remains locally owned.
- Eggserve dependency graph is unchanged.

## Status

**Eggserve-side complete — 2026-09-18.** No runtime change in this
repository: eggserve owns only inbound server-side CONNECT/tunnel
acceptance (Plans 199/216) and has no outbound H1 CONNECT encoder/parser
to consolidate. The neutral outbound wire primitive
(`eggnet-http-connect` or equivalent), the eggfetch migration, and the
eggress outbound migration are follow-ups owned by those repositories;
per the plan they were deliberately not implemented in the eggserve
commit. Eggserve neither depends on the neutral CONNECT crate nor on
eggfetch/eggress as products.

## Implementation record (eggserve)

- Verified no outbound client handshake exists here: the only CONNECT
  handling is inbound server-side tunnel acceptance — neutral intent
  vocabulary in `eggserve-primitives::tunnel`, transport execution in
  `eggserve-server::tunnel`, compatibility H1/H2 delegation plus H3
  stream bridging — with no authority formatting, request-head encoding,
  `Proxy-Authorization` encoding, or response-head parsing for a
  client-side proxy dial.
- Documented the inbound/outbound boundary so the two directions are not
  conflated: inbound acceptance stays locally owned in eggserve
  (`architecture/runtime.md`, `architecture/crate-topology.md`,
  `docs/non-goals.md`, `docs/extension-contract.md`); the outbound
  caller-owned-stream wire primitive (authority formatting, request-head
  encoding, optional `Proxy-Authorization`, bounded response-head
  parsing, status extraction, read-ahead preservation, neutral errors;
  dialing/DNS/TLS/timeout/retry/routing/lifecycle caller-owned) lives
  outside eggserve and must never become an eggserve dependency.
- No new dependencies: the workspace graph is unchanged and the topology
  gate is unmodified and passes. The `No HTTP client stack without a
  plan` rule in `docs/dependency-policy.md` now names Plan 223
  explicitly.
- User-facing updates: `README.md` (Plan 223 status),
  `architecture/overview.md` (outbound client out of scope),
  `architecture/crate-topology.md` (Plan 223 boundary),
  `architecture/runtime.md` (inbound-only tunnel scope),
  `docs/non-goals.md` (outbound CONNECT client non-goal),
  `docs/dependency-policy.md` (no CONNECT-crate dependency),
  `docs/extension-contract.md` (no outbound client provided),
  `AGENTS.md` and the `eggserve-dev` skill (Plan 223 bullet).

## Validation record

Local validation: `cargo fmt`, topology/conformance/release-metadata
gates, workspace checks (default, `http2,tls`, `http3,tls`), clippy with
`-D warnings`, workspace tests, the excluded Python manifest check, and
the `tls`/`http2,tls`/`http3,tls` feature test lanes per routine CI.
Inbound tunnel suites (`eggserve-server/tests/tunnel_upgrade.rs`,
`eggserve-core/tests/tunnel_upgrade.rs`) continue to pass unchanged.
The neutral-crate unit/parity suites (fragmented head, head plus
tunneled bytes, oversized head, header limits, malformed status,
obs-text, authority forms, credential injection, non-2xx, caller
timeout/cancellation) run in the sibling repositories when those
follow-ups land.
