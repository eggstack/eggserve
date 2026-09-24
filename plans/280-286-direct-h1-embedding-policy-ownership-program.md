# Plans 280–286 — Direct H1 embedding policy-ownership and runtime-contract program

## Status

**CLOSED — Plans 280–286 implemented, qualified, published, and proven with registry-only consumers.**

Per maintainer direction, Plan 277 publication and Plan 279 registry proof
were consolidated into Plan 286. No standalone Plan 277 candidate was
published. Evidence: `release/plan-286-embedding-contract-publication-closure.md`.

Planning baseline: `main` at
`4c2e5b6e0ce52256b61a96f0d8a0b0d20ca32922` (2026-09-24).

## Purpose

Make `eggserve-server` a better generic H1 mechanism/runtime for embedders
that already own application policy, without weakening EggServe's hardened
standalone defaults and without adding downstream-project-specific adapters.

The motivating qualification work found that the direct crate shape is now
substantially correct:

- `eggserve-server` is the direct H1 runtime authority;
- caller-owned `AsyncRead + AsyncWrite` streams are first class;
- Tower/http interop is optional on the direct crate;
- the direct graph no longer needs `eggserve-core`, `eggserve-static`, or
  PHF merely for Tower composition;
- total connection lifetime can already be explicitly disabled.

The remaining adoption friction is policy ownership. A sophisticated host may
already own handler/body/write/idle deadlines, request/body ceilings, request
admission, tunnel admission, and runtime-error presentation. Today EggServe
still imposes several of those policies unconditionally. Mapping them to large
sentinel values would create hidden double policy rather than a clean boundary.

This program adds explicit opt-in external ownership while preserving all
current secure defaults.

## Architecture target

```text
caller-owned listener / TLS / ALPN / supervision
                    |
                    v
          eggserve-server H1 driver
            parser/framing authority
                    |
        +-----------+-----------+
        |                       |
 EggServe-owned policy     External policy
   (current default)        (explicit opt-in)
        |                       |
 deadlines / limits        host/service owns
 admission / errors        selected concerns
        +-----------+-----------+
                    |
            canonical Service
```

EggServe always remains the HTTP framing/parser authority. External ownership
is allowed only for policies that can be relinquished without exposing an
invalid H1 parser or raw Hyper transport.

## Program sequence

```text
279  forward-proxy seam source-closed; registry proof with Plan 286
 |
280  explicit external ownership for selected deadlines and semantic ceilings
 |\
 | 281  external service/tunnel admission ownership
 |/
282  narrow direct-H1 connection-policy/config projection
 |
283  typed runtime-rejection presentation hook
 |
 +----284  TunnelIo transport-bridge optimization (evidence-gated, may NO-GO)
 |        (may execute in parallel after 279; 285 consumes its final decision)
 |
285  combined embedding/default-regression qualification + version decision
 |
286  crates.io publication + registry-only embedding closure
```

## Plan index

- `plans/280-direct-h1-external-policy-ownership.md`
- `plans/281-direct-runtime-external-admission-ownership.md`
- `plans/282-direct-h1-connection-policy-projection.md`
- `plans/283-typed-runtime-rejection-presentation.md`
- `plans/284-tunnel-io-transport-bridge-optimization.md`
- `plans/285-embedding-contract-qualification-and-version-decision.md`
- `plans/286-embedding-contract-publication-and-registry-closure.md`

## Frozen invariants

The program must preserve:

- safe standalone defaults;
- existing `RuntimeConfig` public fields and existing entry points unless
  Plan 285 explicitly determines a minor-version break is unavoidable;
- mandatory H1 parser safety: explicit Hyper timer, bounded parser buffer,
  bounded header count, framing validation, request normalization;
- final canonical HTTP framing authority inside EggServe;
- default request-body rejection/limits;
- default service/tunnel admission limits;
- default response privacy/error representation;
- caller-owned transport support with no raw Hyper type in public signatures;
- compatibility/core facade paths;
- static confinement;
- H2/H3 support tiers;
- Rust 1.89 MSRV;
- no downstream-project-specific types or feature names.

## Explicit non-goals

This program does not:

- make EggServe a reverse proxy, forward proxy, WAF, CDN, or application server;
- absorb downstream routing, worker supervision, TLS termination, H2/H3
  orchestration, WebSocket framing, or application authorization;
- make parser memory/header-count protections externally disableable;
- expose `hyper::Request<Incoming>`, `OnUpgrade`, rustls session objects, or
  raw sockets as the public service contract;
- use arbitrarily large numbers as a substitute for external ownership;
- remove current secure bounds from `Server::builder()` defaults;
- silently reinterpret zero for fields where zero already has another meaning
  (notably `max_request_body_bytes = 0` means reject bodies);
- weaken response framing/privacy validation merely because response
  presentation becomes customizable.

## Closure definition

The program is closed only when:

1. all selected policies have explicit, documented ownership;
2. the legacy/default path remains behavior-compatible and fully bounded;
3. a direct embedder can avoid duplicate handler/body/write/idle and
   service/tunnel admission policy;
4. the direct H1 execution path consumes a narrow effective policy rather than
   unrelated listener/TLS/static settings;
5. runtime-generated rejection presentation can be customized without changing
   protocol status/framing authority;
6. the tunnel bridge has a recorded KEEP/REVERT/NO-GO decision from
   reproducible evidence;
7. a generic caller-owned TLS-stream fixture proves the combined embedding
   contract without `eggserve-core` or static dependencies;
8. the appropriate compatible/minor release is published and proven from a
   clean registry-only consumer.

Plan 286 is the only publication/unblock authority for this program.
