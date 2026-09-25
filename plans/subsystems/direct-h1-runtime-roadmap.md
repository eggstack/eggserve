# Direct H1 Runtime Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#2` (single H1 authority, single `Service` contract)
- `plans/001-terminology-and-domain-model.md` (`Service`, `RequestBody`, `RequestContext`, `H1ConnectionPolicy`, `RuntimeRejection`)
- `plans/002-long-term-roadmap.md` (phase 2, 3, 5)

Related ADRs:

- `architecture/adr-003-custom-service-ownership.md` (accepted; stays in place, linked not moved)

## 1. Purpose and ownership boundary

Owns the mature H1 connection runtime and transport boundary: strict-H1 driver over any `AsyncRead + AsyncWrite` stream, single `Service` contract (+ additive `call_with_tunnel`), `RuntimeConfig`/`RuntimeState` admission pool, per-connection structured shutdown, listener TCP `Server`, tunnel execution, `OpsContext` authority, response policy, shared limit kernel, supervisory `ServerControl`/`ServerCompletion`, external policy/admission ownership seams, typed rejection presentation, opt-in absolute-form dispatch, and parser/header/metadata ownership controls (Plans 288–289).

Consumes: `primitives` only; never static/core. `http2`/`tls` are inert compatibility feature names on this crate. H2 execution, TLS/proxy/listener composition, and static orchestration stay in core/leaf owners.

## 2. Work classification

### Invariants

- Compatibility `Auto` classifies before any Hyper service exists; every H1 path delegates to the direct driver; core executes H2 only (Plan 249).
- Per-connection shutdown is structured under the connection task (no detached forwarder).
- Response framing belongs to the runtime only; framing/denylist stay runtime-owned across all ownership transfers.
- `RequestBody` one-shot; default policy `Reject`; static declares `Reject`.

### Capabilities

- Direct TCP `Server` (bind + prebound + accounted accept loop + `wait()`/`ops_snapshot()`).
- Caller-owned stream entry (`serve_http1_connection`, no socket required).
- Supervisory completion split (`into_parts()` + cloneable `ServerControl` + cancellation-safe `ServerCompletion::wait()`; legacy `wait(self)` source-compatible).
- Opt-in `OriginOrAbsolute` dispatch (static still rejects absolute-form).

### Infrastructure

- `RuntimeState` admission pool, `H1ConnectionPolicy` validated projection, `PolicyOwner`/`AdmissionOwnership` external ownership, `RuntimeRejectionKind`/`RuntimeRejection` presentation hook, `TunnelIo` direct transport, parser-range/header-ceiling/metadata ownership controls.

### Polish

- H1 dispatch-state and connection-metadata fixed-cost cleanup (Plans 228/237); activity/dispatch simplification with same-machine A/B evidence.

## 3. Non-goals

- No H2/H3 promotion by this subsystem (separate `h2-h3-transports` roadmaps + scoped promotion plans).
- No WebSocket framing (tunnel handoff only; framing downstream).
- No middleware stack on `Service`; no reverse proxying; no raw-socket response writers.

## 4. Current state

Closed through `eggserve-server 0.3.1`. Single-H1-authority + shutdown corrective (249–250), supervisory lifecycle + total-lifetime opt-out (270–271), absolute-form seam (278–279), policy/admission/connection-policy/rejection/tunnel program (280–286, published `0.3.0`), parser/header/metadata follow-up with registry proof (288–291, published `0.3.1`). Defaults unchanged; H2/H3 untouched; runtime failures stay under `ResponsePolicy` with explicit service/runtime provenance.

## 5. Target architecture

No change: preserve the direct authority, compatibility delegation shape, and explicit-ownership (not silent-default) seam discipline.

## 6. Dependency graph

```text
direct H1 parity + tunnel convergence (closed: 215–217)
    |
    +--> shutdown/authority convergence (closed: 243–244/249–250)
             |
             +--> supervisory + lifetime (closed: 270–271)
                      |
                      +--> absolute-form + ownership program (closed: 278–286)
                               |
                               `--> boundary-ownership follow-up (closed: 288–291)
```

Parser work (288) and metadata work (289) ran in parallel from one baseline; combined qualification (290) gated publication (291): interface dependency between 288/289, hard dependency of 290 on both, operational dependency of 291 on 290's semver decision + registry state.

## 7. Milestones

### Milestone 1 — Single H1 authority + structured shutdown

Class: invariant

Objective: every H1 path executes the direct driver; no detached shutdown forwarders.

Dependencies: 215–217 parity fixtures (hard, closed).

Exit conditions: `direct_h1_parity.rs` 16 scenarios green; topology gate rejects core H1 Hyper machinery; shutdown-lifetime proof in `release/plan-250-*`.

### Milestone 2 — Embedding ownership seams

Class: infrastructure (with capability-visible supervisory split)

Objective: external deadline/ceiling/admission ownership + narrow policy projection + typed rejection presentation + absolute-form opt-in, all explicit.

Dependencies: Milestone 1 (hard).

Exit conditions: combined embedding qualification + version decision (`release/plan-285-*`) + registry publication (`release/plan-286-*`).

### Milestone 3 — Boundary-ownership follow-up

Class: infrastructure

Objective: wider explicit parser ranges, external aggregate-header ownership, successful service-response Date/Server ownership with provenance.

Dependencies: Milestone 2 (hard); live crates.io state at execution (operational).

Exit conditions: `release/plan-290-*` qualification + `release/plan-291-*` registry closure (`eggserve-server 0.3.1`).

## 8. Cross-cutting requirements

Config: `RuntimeConfig` validated once; `Duration::ZERO` opts out of only the total lifetime. Security: TE+CL framing validation, bounded parser ceilings, sanitized errors, permits released once. Concurrency: JoinSet drain, level-triggered idempotent shutdown, `max_in_flight_requests` pre-response bound. Observability: explicit `OpsContext`, no library `println!`. Compat: exhaustive `RuntimeConfig` literals classified at each publication decision.

## 9. Verification strategy

Direct-vs-compatibility parity suite, tunnel suites, app-server consumer + application-service-contract fixtures, cross-protocol conformance subset, topology gate, `verify.sh fast` + feature-lane clippy/tests, TLS-bin lane, registry-only consumer proof + exact-SHA hosted CI for publication milestones.

## 10. Risks and decision points

None open. Future H1-boundary widening needs provenance-preserving design + a new scoped plan.

## 11. Completion definition

All three milestones closed with immutable qualification/publication records and no open corrective. Met at `release/plan-291-direct-h1-boundary-ownership-publication-closure.md`.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | legacy `plans/243-*`, `244-*`, `249-*`, `250-*` | `release/plan-250-h1-authority-lifetime-corrective-closure.md` | — |
| 2 | closed | legacy `plans/270-*`, `271-*`, `278-*`–`286-*` | `release/plan-286-embedding-contract-publication-closure.md` | — |
| 3 | closed | legacy `plans/288-*`, `289-*`, `290-*`, `291-*` | `release/plan-291-direct-h1-boundary-ownership-publication-closure.md` | — |
