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
- Direct Tower/Axum adapter hot-path and downstream-application footprint optimization (Milestones 294–297), evidence-gated and API-preserving unless a separately justified semver change is required.

## 3. Non-goals

- No H2/H3 promotion by this subsystem (separate `h2-h3-transports` roadmaps + scoped promotion plans).
- No WebSocket framing (tunnel handoff only; framing downstream).
- No middleware stack on `Service`; no reverse proxying; no raw-socket response writers.

## 4. Current state

Core capability closure remains complete through `eggserve-server 0.3.1`. Milestones 294–297 are closed as a bounded direct-Tower/application-server polish campaign, and Milestone 299 is closed as the scoped H1 response-trailer wire-correctness repair for the F3 gap discovered by 294; neither reopens the completed performance/footprint campaign. Single-H1-authority + shutdown corrective (249–250), supervisory lifecycle + total-lifetime opt-out (270–271), absolute-form seam (278–279), policy/admission/connection-policy/rejection/tunnel program (280–286, published `0.3.0`), parser/header/metadata follow-up with registry proof (288–291, published `0.3.1`). Defaults unchanged; H2/H3 untouched; runtime failures stay under `ResponsePolicy` with explicit service/runtime provenance.

## 5. Target architecture

Preserve the direct authority, compatibility delegation shape, and explicit-ownership (not silent-default) seam discipline while reducing avoidable work for direct application-server consumers.

The target direct Tower shape is still one H1 parser/framing/security authority. Optimization may avoid redundant canonical↔`http` materialization or introduce narrower internal body representations only when the same validation, body-limit, response-policy, cancellation, and shutdown authorities remain singular. Feature/capability separation may remove file-body or tunnel machinery from builds that cannot exercise it, but must not move filesystem policy out of `eggserve-static` or create a second H1 runtime.

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
                               |
                               +--> direct Tower/footprint polish (294–297, closed)
                               |
                               `--> H1 response-trailer wire correctness (299)
```

Milestone 294 was the hard evidence dependency for 295 and 296; 297 closed that campaign after retained 295/296 work. Milestone 299 is independent of further performance work: its hard dependencies are the existing canonical trailer contract (Plan 198), the closed direct H1 authority/ownership work through 291, and the raw-wire F3 reproduction retained by 294/297.

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

### Milestone 4 — Direct Tower + footprint baseline

Class: polish

Objective: measure the exact direct `eggserve-server/tower` request/response hot path and compile/link footprint used by downstream application servers before changing production code.

Dependencies: Milestone 3 (hard, closed).

Deliverable boundary: retained allocation/CPU/latency/profile evidence for native H1 versus Tower/Axum, streaming-response adapter state, dependency ancestry, Tokio feature activation, and stripped fixture/downstream-like binary size.

User or operator value: prevents speculative optimization and identifies which fixed costs are real for embedded application servers.

Exit conditions: Milestone 294 closure records KEEP/NO-GO targets for 295/296.

### Milestone 5 — Tower/Axum hot-path optimization

Class: polish

Objective: remove measured redundant request/response adaptation work for direct Tower consumers without adding a second validation/framing implementation.

Dependencies: Milestone 4 (hard evidence dependency).

Deliverable boundary: only measured costs retained; candidate areas include duplicate header/target materialization and streaming response body/trailer boxing/synchronization.

User or operator value: lower per-request allocation/CPU and lower SSE/streaming overhead for application-server embedders.

Exit conditions: focused parity/security tests plus same-machine A/B show a retained simplification or measurable win with no semantic regression.

### Milestone 6 — Direct-profile capability and dependency footprint

Class: polish

Objective: reduce code/dependency footprint for direct application-server builds by gating machinery they cannot exercise and pruning unnecessary feature edges.

Dependencies: Milestone 4 (hard evidence dependency); Milestone 5 is independent.

Deliverable boundary: evidence-gated file-body transport separation, optional tunnel gating only if clean, unused direct-profile dependency removal, and Tokio/futures feature tightening. No new crate solely for size.

User or operator value: smaller direct application binaries and narrower supply-chain/build graph.

Exit conditions: direct Tower consumer keeps required behavior while graph/binary evidence improves or each candidate is explicitly NO-GO.

### Milestone 7 — Downstream-like qualification and closure

Class: polish

Objective: qualify every retained 295/296 change against native/direct Tower behavior and an EggPool-shaped streaming application fixture, then make the semver/publication decision.

Dependencies: Milestones 5/6 for retained changes (hard); registry publication only if a release is selected (operational).

Exit conditions: allocation/CPU/latency/tail/RSS/dependency/binary evidence, full correctness/security verification, exact keep/revert/defer decisions, and publication strategy recorded.

### Milestone 8 — H1 response-trailer wire correctness

Class: invariant

Objective: restore the already-documented H1 response-trailer contract so an HTTP/1.1 client that advertises `TE: trailers` can receive validated terminal trailer fields on the wire, while HTTP/1.0 and non-opted-in H1.1 continue to suppress them.

Dependencies: Plan 198 trailer model (hard, closed); Milestones 1–3 H1 authority (hard, closed); F3 raw-wire reproduction from Milestones 4/7 (evidence dependency, closed).

Deliverable boundary: head-time trailer-field declaration owned by the runtime, correct H1 framing (no conflicting `Content-Length`; Hyper-selected chunked transfer), native/Tower parity, and fail-closed handling when produced fields violate the declaration. H2/H3 protocol-native trailers are not redesigned.

User or operator value: the existing experimental trailer capability stops silently losing H1 response trailers after application code successfully produces them.

Exit conditions: raw TCP tests observe the terminal trailer section for opted-in HTTP/1.1, suppression remains correct elsewhere, framing/security invariants remain singular, and the closure record classifies any additive public API and release impact.

## 8. Cross-cutting requirements

Config: `RuntimeConfig` validated once; `Duration::ZERO` opts out of only the total lifetime. Security: TE+CL framing validation, bounded parser ceilings, sanitized errors, permits released once. Concurrency: JoinSet drain, level-triggered idempotent shutdown, `max_in_flight_requests` pre-response bound. Observability: explicit `OpsContext`, no library `println!`. Compat: exhaustive `RuntimeConfig` literals classified at each publication decision.

## 9. Verification strategy

Direct-vs-compatibility parity suite, tunnel suites, app-server consumer + application-service-contract fixtures, cross-protocol conformance subset, topology gate, `verify.sh fast` + feature-lane clippy/tests, TLS-bin lane, registry-only consumer proof + exact-SHA hosted CI for publication milestones.

## 10. Risks and decision points

- A Tower fast path is acceptable only if it reuses the canonical H1 validation/framing authorities; a parallel parser/security implementation is a stop condition.
- Concrete response-body optimization must preserve trailers, HEAD/body-forbidden suppression, producer cancellation, no-progress accounting, and committed-response failure semantics.
- File-body/tunnel feature separation may affect source or feature compatibility. Milestone 296 must classify semver before landing a public feature-layout change; create an ADR only if ownership or the durable public architecture would change.
- Binary-size conclusions must use stripped identical fixtures and an EggPool-shaped consumer; dependency-count reduction alone is not proof of linked-size improvement.
- The Milestone 299 fix must not poll a trailer future early or buffer the full response merely to discover field names. If Hyper requires head-time names, use explicit bounded declaration metadata rather than weakening runtime framing authority.
- Existing `ResponseStream::with_trailers` callers without head-time declarations must not silently gain unsafe H1 behavior; preserve H2/H3 semantics and define an explicit H1 suppression/migration path.
- Future H1-boundary widening still needs provenance-preserving design + a new scoped plan.

## 11. Completion definition

The capability milestones 1–3 remain closed and the 294–297 polish campaign is closed. The subsystem can return to closed status after Milestone 299 restores H1 response-trailer wire delivery (or records an evidence-backed contract correction) with no unresolved medium+ trailer correctness defect.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | legacy `plans/243-*`, `244-*`, `249-*`, `250-*` | `release/plan-250-h1-authority-lifetime-corrective-closure.md` | — |
| 2 | closed | legacy `plans/270-*`, `271-*`, `278-*`–`286-*` | `release/plan-286-embedding-contract-publication-closure.md` | — |
| 3 | closed | legacy `plans/288-*`, `289-*`, `290-*`, `291-*` | `release/plan-291-direct-h1-boundary-ownership-publication-closure.md` | — |
| 4 | closed | `plans/implementation/direct-h1-runtime/294-direct-tower-footprint-baseline.md` | `plans/closure/direct-h1-runtime/294-direct-tower-footprint-baseline.md` | — |
| 5 | closed | `plans/implementation/direct-h1-runtime/295-direct-tower-hotpath-optimization.md` | `plans/closure/direct-h1-runtime/295-direct-tower-hotpath-optimization.md` | — |
| 6 | closed | `plans/implementation/direct-h1-runtime/296-direct-profile-footprint-capability-split.md` | `plans/closure/direct-h1-runtime/296-direct-profile-footprint-capability-split.md` | — |
| 7 | closed | `plans/implementation/direct-h1-runtime/297-direct-application-server-qualification-closure.md` | `plans/closure/direct-h1-runtime/297-direct-application-server-qualification-closure.md` | — |
| 8 | closed | `plans/implementation/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md` | `plans/closure/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md` | — |
