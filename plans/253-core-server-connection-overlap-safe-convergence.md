# Plan 253 — Core/server connection-overlap classification and safe convergence

## Purpose

Reduce maintenance/drift risk in the remaining parallel connection machinery
between `eggserve-core` and `eggserve-server` after Plans 249–250 established
a single executable H1 authority.

Planning baseline:

```text
0ee02acd69f1c63d32134f8265283fff04e4630c
```

This is an API- and capability-preserving maintenance plan. It is not
authorization to maximize deletion or to redesign crate boundaries.

The review confirmed that H1 execution itself is now single-authority in
`eggserve-server`, but both crates still contain similarly named and often
substantially similar modules:

- `connection/activity.rs`;
- `connection/deferred_body.rs`;
- `connection/driver.rs`;
- `connection/lifecycle.rs`;
- `connection/pipeline.rs`;
- `connection/request.rs`;
- `connection/response.rs`;
- `connection/transport.rs`.

At the review baseline, `transport.rs` is byte-identical in both crates and
several other pairs remain substantially similar. This does not by itself mean
there are two H1 implementations: core still owns H2 execution and
multiprotocol composition. The problem is maintenance ambiguity and latent
semantic drift in code that appears protocol-neutral or near-neutral.

## Hard constraints

- Keep every existing public Rust path/signature/type identity.
- Keep `eggserve-server` H1-only in actual capability.
- Do not make its accepted inert `http2` or `tls` feature names active.
- Keep core ownership of the current H2 execution and extended
  TLS/listener/proxy composition.
- Do not move H2 support into the direct crate in this plan.
- Do not add a new crate solely to share private transport helpers.
- Do not expose private Hyper/Tokio transport types publicly merely so two
  crates can call the same helper.
- Do not add a broad production dependency.
- Do not alter H1/H2 wire behavior, timeout semantics, admission, response
  privacy, request-body behavior, tunnel semantics, proxy handling, or
  shutdown.
- H2 remains experimental.
- A documented intentional duplicate is preferable to a worse public API.

## Desired result

Every remaining overlapping responsibility must end in one of four states:

1. **single direct authority** — core delegates/re-exports without behavior;
2. **core H2-specific** — implementation remains only because H2 needs it;
3. **composition adapter** — core adapts multiprotocol/TLS/proxy/listener state
   into the direct H1 or H2 execution boundary;
4. **accepted bounded duplication** — the implementation is genuinely useful
   to both H1 and H2, but sharing it across the crate boundary would require a
   prohibited public/package change; parity tests and topology rules then make
   that duplication explicit rather than accidental.

No overlapping production module/function should remain unclassified.

## Track A — build a function/responsibility overlap ledger

Inventory the eight module pairs above plus any additional direct/core
connection helpers discovered during implementation.

For each significant item, record:

- symbol/function/type;
- direct-crate owner/use;
- core owner/use;
- H1 use;
- H2 use;
- public/private visibility;
- dependencies on Hyper H1, Hyper H2, generic Hyper, Tokio, primitives,
  RuntimeState, response policy, or tunnel state;
- whether type identity matters;
- whether current source is identical/substantially similar/different;
- target classification from the four states above;
- proposed action: delegate/delete/retain/DEFER.

Place the durable summary in an architecture or release artifact rather than
leaving the reasoning only in commit messages.

Do not use line-count similarity as proof of semantic equivalence. Use call
graphs, feature-gated builds, and behavior tests.

## Track B — prove current execution ownership before moving anything

Retain and extend the Plan 249 structural proof that:

- no production core H1 Hyper builder exists;
- no production core `http1::Connection` or
  `UpgradeableConnection` is driven;
- every explicit/Auto/TLS/PROXY/Unix H1 path delegates into
  `eggserve-server`;
- core Hyper execution is H2-only;
- direct crate features do not gain H2/TLS capability.

Add enough source/call-graph evidence that deleting a similar-looking helper
cannot accidentally remove H2 behavior.

## Track C — remove residual H1-only/dead core machinery

For every core connection symbol classified as H1-only or dead after Plan 249:

1. prove no H2/composition caller remains;
2. prove no public compatibility item relies on it;
3. remove it or collapse it into a thin direct delegation;
4. add/retain a topology rule preventing resurrection if the removed authority
   would recreate a second H1 path.

Examples to inspect include old conversion/pipeline helpers that survived
because they were historically shared by H1/H2 but are no longer used by H2.

Do not delete a helper merely because its name contains `http1`; rely on the
actual call graph.

## Track D — converge exact or protocol-neutral duplicates where the existing crate boundary allows it

For identical/substantially identical helpers, first ask whether an existing
public/direct authority already exposes the needed abstraction without API
growth.

Allowed convergence mechanisms include:

- using an already-public canonical primitive from `eggserve-primitives`;
- using an already-public `eggserve-server` type/function;
- using a compatibility `pub use` where identity already belongs to the
  public contract;
- moving a helper to an already-correct existing authority when the move does
  not require exposing a new public transport type and all callers can consume
  it through existing interfaces.

Do not move Hyper/Tokio transport execution into `eggserve-primitives`.

The byte-identical `connection/transport.rs` pair is a mandatory review item:
determine whether it can be eliminated through an existing authority. If the
only route is to expose a new transport-internal API or add a sharing crate,
retain it as bounded duplication and document why.

## Track E — isolate H2-specific core code more clearly

Where safe and internal-only, make H2 ownership obvious in module/function
structure and `cfg(feature = "http2")` boundaries.

Goals:

- H2-only imports and helpers should not compile into default core builds;
- generic-looking names should not obscure an H2-only implementation;
- comments should describe the invariant/ownership rather than historical plan
  chronology;
- no public path/signature changes.

This track may split private modules/functions if doing so reduces ambiguity,
but do not perform a broad file-reorganization for aesthetics alone.

## Track F — parity guards for accepted bounded duplication

For each retained intentionally duplicated responsibility, define how drift is
detected.

Depending on the helper, use:

- direct-vs-compatibility H1 behavior tests;
- H1/H2 canonical conversion tests;
- shared corpus inputs;
- request/response policy test vectors;
- compile-time type/constant equality checks;
- topology/source inventory assertions.

Avoid brittle full-file text equality unless the files are intentionally
required to remain identical. Prefer behavior/invariant checks.

The overlap ledger must explain the guard for every accepted duplicate.

## Track G — strengthen topology classification

Extend `scripts/check-crate-topology.py` only as necessary so the current
ownership model is mechanically enforced.

The checker should distinguish:

- forbidden second H1 authority;
- allowed H2-specific core execution;
- compatibility facades;
- accepted bounded duplicate helpers.

Do not turn the checker into a line-count/similarity linter.

Plan 255 may later refactor the checker's internal organization; Plan 253 owns
the semantic rules needed for connection authority.

## Track H — direct and compatibility qualification

Run focused tests covering both construction paths:

- direct H1 server;
- compatibility cleartext Auto→H1;
- compatibility explicit H1;
- PROXY-prefixed H1;
- Unix H1 on Unix;
- TLS ALPN H1;
- H2 prior knowledge;
- TLS ALPN H2;
- caller-owned H1;
- caller-owned multiprotocol stream;
- streaming request/response bodies;
- trailers/interim;
- tunnel accept/deny;
- handler/service error privacy;
- keep-alive/max-requests;
- shutdown/drain and connection timeout.

Any change to shared canonical conversion must also run the canonical
conformance and wire-correctness suites.

## Required qualification

At minimum:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test -p eggserve-server
cargo test -p eggserve-core
cargo test -p eggserve-core --features http2,tls
cargo test --workspace
```

Run the focused parity suites named by the current repository, including
`direct_h1_parity`, `direct_service_convergence`,
`cross_protocol_conformance`, and Plan 249/250 authority regressions.

If any H3-facing shared canonical code is touched, also run the existing
`http3,tls` core qualification.

## Acceptance criteria

- [ ] every remaining core/server connection overlap is classified.
- [ ] no executable core H1 authority exists.
- [ ] all removable H1-only/dead compatibility machinery identified by the
      audit is removed or delegated.
- [ ] protocol-neutral duplicates are converged where this can be done through
      an existing boundary without API/package changes.
- [ ] every retained duplicate has a documented reason and drift guard.
- [ ] H2-only core ownership is clearer and remains feature-gated.
- [ ] direct `http2`/`tls` compatibility feature names remain inert.
- [ ] no new crate/public transport API/broad dependency is introduced.
- [ ] H1/H2 behavior, capability, and support tiers are unchanged.
- [ ] topology and parity tests fail on representative authority regressions.
- [ ] required local qualification is green.

## Mandatory DEFER conditions

Record DEFER for an overlap instead of forcing convergence when sharing would
require any of:

- a new public Hyper/Tokio transport type;
- making a private server helper public solely for core;
- activating direct-crate H2/TLS capability;
- moving H2 execution ownership;
- introducing a new crate only for internal source reuse;
- adding a broad dependency;
- changing public type identity or signatures.

A DEFER under these conditions is a successful Plan 253 result when the
duplication is explicitly bounded and mechanically guarded.
