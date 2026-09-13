# Plan 216 — Direct Generic Tunnel and Upgrade Parity

## Status

Proposed — follows Plan 215 and completes the Plan 199 capability extraction left behind by Plan 214.

## Purpose

Move EggServe's mature generic HTTP tunnel/upgrade contract out of `eggserve-core` and into the direct crate architecture without violating the dependency boundary of `eggserve-primitives`.

This is not a WebSocket feature plan. The required capability is the same generic transport handoff already established by Plan 199: validate HTTP transition intent, expose a one-shot acceptance capability to the downstream service, let the runtime own HTTP handshake/framing semantics, and let the downstream own the post-transition protocol codec.

The immediate reason for this plan is architectural correctness as much as feature parity. Plan 214 left tunnel-related source in an inconsistent transitional state: the direct primitives documentation refers to a tunnel capability, and a `crates/eggserve-primitives/src/primitives/tunnel.rs` source file exists, but the direct `RequestContext` does not actually carry/export the mature capability. The orphan tunnel source still names Hyper upgrade and Tokio I/O machinery, which cannot simply be exported from `eggserve-primitives` because the crate's topology explicitly forbids Hyper and Tokio production dependencies.

The implementation therefore requires a deliberate capability-boundary extraction, not a file-copy/re-export patch.

A reverse proxy or WAF such as Synvoid is useful downstream evidence because it needs HTTP Upgrade transport handoff while retaining its own WebSocket codec and security policy. No Synvoid-specific type or protocol policy belongs in EggServe.

## Prerequisites

Plan 215 must establish:

- one direct authoritative HTTP/1 connection driver;
- direct `Service` ownership;
- direct `RuntimeState`/connection lifecycle authority;
- caller-owned connection serving;
- direct connection context/shutdown/outcome vocabulary.

Tunnel work must attach to that direct pipeline. Do not implement a second direct connection driver for tunnels.

Plan 217 may build H2 Extended CONNECT on this contract after the H1/direct capability boundary is settled.

## Existing mature contract to preserve

Plan 199 established the compatibility-core semantics that remain the behavioral reference:

- `TunnelKind::{Http1Upgrade, Connect, ExtendedConnect}`;
- bounded validated generic `ProtocolName`, not a WebSocket-specific enum;
- validated `TunnelRequest` metadata;
- one-shot acceptance ownership;
- service inspection before acceptance;
- ordinary HTTP denial when the capability is ignored/dropped;
- `101 Switching Protocols` for validated H1 Upgrade;
- successful non-101 response semantics for CONNECT/Extended CONNECT;
- runtime-owned framing/handshake headers;
- no second accept after commitment;
- no acceptance after final response commitment;
- bounded tunnel concurrency/admission;
- lifecycle cancellation propagation;
- H1 post-handshake read-ahead preservation;
- no raw Hyper/H2/H3/Quinn types in public downstream signatures;
- no WebSocket framing, ping/pong, fragmentation, close-code, compression, SOCKS, CONNECT routing policy, or application codec in EggServe.

Those semantics should move with minimal behavioral change.

## Current-state defect to resolve

The direct topology currently contains a misleading partial extraction:

- `eggserve-primitives::RequestContext` documents tunnel capability behavior but has only connection/lifecycle/interim fields;
- `eggserve-primitives/src/primitives/tunnel.rs` exists but is not exported as the direct canonical contract;
- that file contains transport/runtime concepts including private `hyper::upgrade::OnUpgrade` and Tokio-oriented duplex I/O behavior;
- `eggserve-primitives` is required to remain free of Hyper, Hyper-util, Tokio, TLS, QUIC, and filesystem dependencies;
- the mature working tunnel attachment/acceptance path remains in `eggserve-core`.

The implementation must remove this ambiguity. A source file living in the primitives crate is not sufficient evidence of ownership if it cannot satisfy the crate's dependency contract.

## Goals

1. Preserve Plan 199's generic tunnel semantics while moving implementation authority toward the direct crates.
2. Keep `eggserve-primitives` transport-neutral and free of Hyper/Tokio dependencies.
3. Give direct `eggserve-server` consumers a first-class generic H1 Upgrade/CONNECT capability.
4. Keep the native service contract generic rather than introducing WebSocket-specific API.
5. Preserve one-shot acceptance, commitment safety, lifecycle cancellation, bounded concurrency, and read-ahead correctness.
6. Convert compatibility-core H1 tunnel behavior to a facade/adapter over direct ownership.
7. Design the capability boundary so Plan 217 can add H2 Extended CONNECT without a new service API.
8. Keep H3 compatibility behavior and dependency limitations under Plan 213/core until separately extracted.
9. Remove or replace the orphan direct tunnel source so the repository has one truthful ownership story.

## Non-goals

- Implementing a WebSocket codec.
- Adding `tokio-tungstenite` as a production dependency.
- Adding application CONNECT routing/authorization policy.
- Adding SOCKS, MASQUE, WebTransport, CONNECT-UDP application behavior, or generic proxy policy.
- Moving HTTP/3 tunnel transport out of the Plan 213 compatibility boundary.
- Promoting H2/H3 support tiers.
- Making `eggserve-primitives` depend on Tokio, Hyper, H2, H3, Quinn, or Rustls.
- Adding a generic untyped `Any`/extension map to `RequestContext` solely to smuggle runtime state into primitives.
- Exposing raw sockets or Hyper's `Upgraded` type.
- Adding downstream-specific tunnel hooks.

## Architectural gate — choose a dependency-correct capability placement

Implementation must begin with a short architecture record that compares viable placements against the constraints below.

The current compatibility implementation cannot simply move verbatim because `TunnelCapability` privately owns transport runtime machinery while `RequestContext` is a primitives-owned type.

At least these designs must be evaluated:

### Option A — transport-neutral capability contract in primitives

Keep only transport-neutral intent/state/control vocabulary in primitives and hide concrete transport machinery behind a runtime-provided opaque operation object or neutral async interface.

Requirements if selected:

- no Hyper/Tokio dependency in primitives;
- public I/O contract is runtime-neutral;
- direct server can back it efficiently without unbounded buffering;
- compatibility core can preserve practical Tokio-facing behavior through a thin adapter if necessary;
- the abstraction must not become a generic extension map.

### Option B — server-owned service capability context

Keep canonical `Request`/`RequestContext` transport-neutral and expose transport capabilities through an additive server-owned request/service context passed by the direct runtime.

Requirements if selected:

- ordinary existing services remain source-compatible where practical;
- tunnel-capable services have an explicit typed path rather than `Any`;
- there remains one canonical native service abstraction, or any additive advanced entry point has a clear compatibility story and does not create competing application models;
- compatibility core can preserve or intentionally migrate the current `RequestContext::take_tunnel()` source contract during the pre-1.0 line;
- H2 can reuse the same capability shape.

### Option C — another narrowly scoped neutral capability layer

A new crate is allowed only if A/B cannot satisfy dependency direction without a substantial API or runtime compromise.

This is not the preferred outcome. Any new crate must have a clearly reusable role beyond tunnel extraction and must not become a dumping ground for transport abstractions.

### Decision criteria

The selected design must satisfy all of the following:

1. `eggserve-primitives` remains Hyper/Tokio-free.
2. `eggserve-server` remains independent of core/static.
3. No raw transport implementation type appears in downstream public signatures.
4. H1 post-upgrade bytes are preserved exactly once.
5. Backpressure is bounded.
6. Lifecycle cancellation wakes/terminates tunnel work.
7. Acceptance is one-shot and commitment-safe.
8. The direct API is usable by downstream protocol codecs without mandatory buffering of tunnel traffic.
9. H2 can reuse the contract without another breaking redesign.
10. Compatibility cost is documented and bounded.
11. There is one implementation authority after migration.

Do not start by exporting the current orphan `eggserve-primitives::tunnel` module and adding forbidden dependencies.

## Workstream A — Split pure tunnel vocabulary from transport execution

Regardless of the selected placement, identify and move the transport-neutral pieces to their proper direct owner.

Likely primitives-owned concepts include:

- `TunnelKind`;
- `ProtocolName` and its RFC-token/length validation;
- `TunnelRequest` validated intent;
- stable tunnel error/status vocabulary that does not require runtime types;
- bounded handshake metadata validation constants where canonical-header-only;
- commitment/acceptance state if it can remain standard-library-only and does not expose runtime internals.

Transport/runtime-owned concepts belong in `eggserve-server`, including:

- Hyper `OnUpgrade` acquisition;
- `Upgraded` handling;
- H1 read-ahead preservation;
- Tokio or runtime-specific duplex bridging;
- tunnel task spawning;
- runtime admission permits;
- connection/request lifecycle integration;
- connection-driver `.with_upgrades()` behavior;
- post-handshake cleanup and cancellation.

### Acceptance

The final module layout should make it impossible to accidentally add Hyper/Tokio to primitives merely by enabling tunnel support.

## Workstream B — Direct H1 tunnel detection and validation

Move the mature H1 tunnel request detection into the direct server connection pipeline.

Required behavior:

- recognize only valid H1 Upgrade intent when `Connection`/`Upgrade` semantics are valid;
- validate/bound the protocol token before service dispatch;
- validate CONNECT authority form according to the existing canonical policy;
- do not infer a trusted application protocol from arbitrary malformed headers;
- attach the typed capability only when the runtime actually has a transport handoff available;
- ordinary non-tunnel requests receive no capability;
- malformed transition requests fail before downstream acceptance.

The validation authority should be shared with compatibility paths; do not leave a second core parser.

## Workstream C — One-shot acceptance and commitment safety

Move the mature acceptance state machine into the direct implementation.

Preserve:

- exactly one successful capability take/accept;
- deterministic failure after acceptance;
- deterministic failure after final response commitment;
- ordinary HTTP denial when service declines to accept;
- runtime-generated transition status/framing;
- bounded application handshake headers;
- stripping/rejection of application attempts to control runtime-owned hop-by-hop framing;
- no forged tunnel handshake via an ordinary `Response`.

The final response/acceptance mechanism must integrate with the Plan 215 direct response-commitment pipeline rather than bypass it.

### H1 handshake requirements

For `Http1Upgrade`:

- emit `101 Switching Protocols` only through the validated capability;
- runtime owns `Connection: upgrade` and `Upgrade` values;
- service-provided ordinary headers may be carried only within existing safe bounds/policy;
- after handshake commitment, transport ownership transfers exactly once.

For plain H1 CONNECT where supported by the existing contract:

- preserve the existing successful status semantics;
- do not synthesize WebSocket semantics;
- transfer the transport through the same generic tunnel mechanism.

## Workstream D — Bounded transport handoff

The direct server must preserve Plan 199's bounded, single-owner post-transition transport behavior.

Required properties:

- no unbounded queue proportional to peer input;
- no accidental duplicate reader/writer ownership;
- explicit, bounded backpressure;
- read-ahead bytes already consumed by Hyper are not lost or reordered;
- peer close/reset is visible to the tunnel handler;
- runtime shutdown/drain past deadline cancels tunnel work;
- tunnel work holds the appropriate runtime permit until ownership ends;
- permit release and task cleanup happen on every terminal path;
- errors are sanitized and do not reflect payload bytes.

If compatibility currently exposes a Tokio `AsyncRead + AsyncWrite` `TunnelIo`, preserve that ergonomics through a direct server type or a compatibility wrapper when feasible. Do not move Tokio traits into primitives to achieve source compatibility.

## Workstream E — Runtime limits and observability

Move or reconnect tunnel limits to the direct `RuntimeState` established by Plan 215.

Preserve:

- maximum concurrent tunnel admission;
- bounded bridge buffer sizes;
- any tunnel-specific close/shutdown accounting;
- structured events for accepted/declined/failed/cancelled transitions where already part of the mature ops contract;
- per-runtime rather than process-global state where Plan 181 established that behavior.

Do not add application-level tunnel metrics hooks in this plan.

## Workstream F — Compatibility-core facade

After direct H1 tunnel parity is green:

- route compatibility H1 Upgrade/CONNECT through the direct implementation;
- re-export transport-neutral tunnel values from the direct owner;
- preserve compatibility public paths with aliases/wrappers where practical;
- delete the duplicate core H1 tunnel validation/state/transport implementation;
- keep H2-specific compatibility glue only until Plan 217 lands;
- keep H3-specific compatibility glue under Plan 213.

The orphan direct `eggserve-primitives/src/primitives/tunnel.rs` must either become a valid transport-neutral implementation or be split/deleted. It must not remain as misleading dead/parity source.

## Workstream G — Prepare H2 without implementing it twice

The direct tunnel contract must represent `ExtendedConnect` and validated protocol metadata in a way Plan 217 can use.

Do not implement an H2-specific service API in this plan.

If Plan 217 has not landed, H2 transport attachment may remain compatibility-owned temporarily, but the pure tunnel types and service-facing capability contract must not need another redesign when H2 moves.

H3 remains explicitly outside this plan. Existing dependency limitations around generic H3 `:protocol` values remain documented rather than bypassed with custom wire parsing.

## Workstream H — Downstream codec interoperability

Use a development/test-only protocol codec to prove the boundary is sufficient for a real consumer.

The existing dev-only `tokio-tungstenite` fixture is appropriate evidence for H1 WebSocket interoperability because it tests a downstream codec without making WebSocket an EggServe production feature.

Qualification must prove:

- HTTP Upgrade succeeds through the generic capability;
- a downstream codec can read/write after handoff;
- buffered post-handshake bytes are preserved;
- bidirectional data is not reordered;
- close/disconnect terminates cleanly;
- ignored tunnel capability produces an ordinary HTTP response/denial;
- double acceptance is impossible;
- acceptance after commitment fails;
- tunnel concurrency limit is enforced;
- shutdown cancels active tunnel tasks as specified.

No production dependency on the test codec may be introduced.

## Workstream I — Anti-duplication/topology enforcement

Extend repository checks so the transitional defect cannot recur.

At minimum assert:

- transport-neutral tunnel types have one direct owner;
- primitives tunnel modules do not import `hyper`, `hyper_util`, `tokio`, `h2`, `h3`, or `quinn`;
- H1 tunnel transport machinery has one implementation in `eggserve-server`;
- compatibility core does not retain a second H1 Upgrade parser/acceptance state machine after migration;
- `tokio-tungstenite` remains dev-only;
- the default non-tunnel consumer graph does not acquire H2/H3/QUIC dependencies.

## Validation matrix

At minimum run:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test -p eggserve-primitives
cargo test -p eggserve-server
cargo test -p eggserve-core
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Add focused direct-crate tests for:

- `ProtocolName` validation and limits;
- Upgrade/CONNECT intent parsing;
- absent capability on ordinary requests;
- one-shot take/accept;
- commitment race;
- handshake-header bounds and forbidden headers;
- read-ahead preservation;
- bounded backpressure;
- handler disconnect/cancellation;
- tunnel admission exhaustion;
- graceful/forced runtime shutdown;
- compatibility/direct parity.

Feature-gated H2/H3 tests must remain green during migration, but direct H2 parity is Plan 217 and H3 ownership remains Plan 213.

## Documentation updates

Update:

- `README.md`;
- `AGENTS.md`;
- `architecture/crate-topology.md`;
- tunnel/upgrade architecture documentation;
- downstream application-server guide;
- public API/migration guide;
- dependency policy if a new small transport-neutral dependency is justified by the selected design.

Documentation must clearly distinguish:

- transport-neutral tunnel intent/capability types;
- server-owned transport execution;
- downstream-owned application codec/policy;
- H1 direct support;
- H2 direct status from Plan 217;
- H3 experimental compatibility limitations.

## Implementation sequencing

Recommended order:

1. Close Plan 215 or land the required direct H1 connection/runtime prerequisites.
2. Write the capability-placement architecture record and select a dependency-correct design.
3. Split pure tunnel vocabulary from transport runtime machinery.
4. Correct/remove the orphan direct primitives tunnel source.
5. Attach validated H1 tunnel intent/capability in the direct request pipeline.
6. Move one-shot acceptance/commitment logic.
7. Move bounded H1 transport handoff/read-ahead behavior.
8. Integrate direct tunnel admission/lifecycle/observability.
9. Run direct H1 codec interoperability and adversarial tests.
10. Convert compatibility H1 tunnel paths to direct ownership.
11. Add topology/anti-duplication guards and update docs.
12. Hand the shared direct contract to Plan 217 for H2 Extended CONNECT integration.

## Acceptance criteria

Plan 216 is complete only when all of the following are true:

1. The direct crate topology has a truthful, compilable tunnel ownership model.
2. `eggserve-primitives` remains free of Hyper/Tokio/H2/H3/Quinn production dependencies.
3. Direct `eggserve-server` consumers can inspect and accept a validated generic H1 Upgrade/CONNECT transition without importing `eggserve-core` or Hyper transport types.
4. The capability remains generic and does not encode WebSocket framing/application behavior.
5. One-shot acceptance and post-commit rejection semantics match Plan 199.
6. H1 read-ahead bytes are preserved correctly across transport handoff.
7. Tunnel I/O is bounded, lifecycle-aware, and releases all runtime permits/tasks on termination.
8. Ignoring/declining a capability remains ordinary HTTP behavior.
9. Compatibility H1 tunnel paths delegate to direct ownership instead of maintaining a second implementation.
10. The direct contract can support H2 Extended CONNECT in Plan 217 without another service-facing API redesign.
11. H3 remains isolated/experimental according to Plan 213 and is not pulled into the direct default/H1 graph.
12. Production dependencies do not gain a WebSocket codec.
13. Topology, routine CI, H1 tunnel qualification, and compatibility parity tests pass.

## Closure evidence

When implemented, record:

- the selected capability-placement design and rejected alternatives;
- dependency graph proof for `eggserve-primitives` and `eggserve-server`;
- direct/core ownership before/after matrix;
- H1 tunnel conformance and dev-codec interoperability results;
- read-ahead and backpressure evidence;
- shutdown/cancellation/admission tests;
- proof that compatibility H1 tunnel behavior reaches the direct implementation;
- any pre-1.0 migration note required to reconcile the previous `RequestContext::take_tunnel()` path with the dependency-correct direct design.