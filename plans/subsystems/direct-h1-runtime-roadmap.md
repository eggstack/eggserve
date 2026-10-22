# Direct H1 Runtime Roadmap

Status: active

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

Core capability closure remains complete through `eggserve-server 0.3.1`. Milestones 294–297 are closed as a bounded direct-Tower/application-server polish campaign, and Milestone 299 is closed as the scoped H1 response-trailer wire-correctness repair for the F3 gap discovered by 294; neither reopens the completed performance/footprint campaign. Milestone 9 (Plan 300) published the 0.4.0 line on 2026-09-25 and is now **conditionally closed** by Milestone 10's closure record: the publication is real and fully evidenced, but the published artifact predates the post-tag corrective batch (`v0.4.0..465b80c` — B1–B89 plus the CI fix), so the live crates still carry those defects. Milestone 11 prepared the corrective `0.5.0` line and gated it locally; the upload, tag, release, and wheel steps remain outstanding because they need the maintainer's registry credentials and a green exact-SHA hosted CI run on the bump commit. Milestones 12–14 are closed: the runtime error representation has one authority in `eggserve-primitives` (Milestone 12, byte-identical wire output), the Plan 253 connection-overlap ledger is corrected, measured, and drift-bounded (Milestone 13), and the compatibility configuration carries the four direct-H1 ownership values with unchanged defaults while its builder delegates to the direct builder (Milestone 14). Milestone 15 (one accept-error classification authority) is the remaining item in this corrective program. Single-H1-authority + shutdown corrective (249–250), supervisory lifecycle + total-lifetime opt-out (270–271), absolute-form seam (278–279), policy/admission/connection-policy/rejection/tunnel program (280–286, published `0.3.0`), parser/header/metadata follow-up with registry proof (288–291, published `0.3.1`). Defaults unchanged; H2/H3 untouched; runtime failures stay under `ResponsePolicy` with explicit service/runtime provenance.

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
                               `--> H1 response-trailer wire correctness (299, closed)
                               |
                               +--> 0.4.0 publication closure (304)
                                        |
                                        `--> corrective 0.4.x publication (305)

corrective program over the compatibility/direct overlap
    |
    +--> runtime error-representation single authority (306)
             |
             +--> connection-overlap classification accuracy (307)
             |
             +--> compatibility config projection parity (308)
             |
             `--> accept-error classification parity (309)
```

Milestone 294 was the hard evidence dependency for 295 and 296; 297 closed that campaign after retained 295/296 work. Milestone 299 is independent of further performance work: its hard dependencies are the existing canonical trailer contract (Plan 198), the closed direct H1 authority/ownership work through 291, and the raw-wire F3 reproduction retained by 294/297.

Milestone 304 closes the publication that Milestone 9 performed; Milestone 305 has a hard dependency on 304 and an operational dependency on the maintainer's registry credentials. Milestones 306–309 have no hard dependency on each other beyond the shared consolidation pattern, but each depends on the closed H1 authority work through 291, and Milestone 307 has an interface dependency on Milestone 306's gate pattern.

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

### Milestone 9 — 0.4.0 release publication

Class: polish

Objective: publish the 0.4.0 Rust line (primitives 0.2.2;
server/static/h3/core 0.4.0; bin 0.2.2) plus wheel 0.2.4 with
registry-only consumer proof, tag `v0.4.0`, and a GitHub Release. No
production behavior beyond closed 297/299 work; versions, requirements,
lockfiles, and release metadata only.

Dependencies: Milestones 1–8 (hard, closed); live crates.io/PyPI state at
execution (operational); maintainer crates.io token and `pypi` environment
approval (operational).

Deliverable boundary: manifests/lockfiles/docs-only diff; layered package
proof; serial index-gated publication with checksums; registry-only
consumer ancestry; tag + release; TestPyPI→PyPI wheel path.

User or operator value: the qualified 0.4.0 line (tower feature edge +
H1 trailer wire delivery) becomes consumable from the registries.

Exit conditions: Plan 300 closure with per-crate checksums, consumer
proof, tag/release links, and wheel run evidence.

### Milestone 8 — H1 response-trailer wire correctness

Class: invariant

Objective: restore the already-documented H1 response-trailer contract so an HTTP/1.1 client that advertises `TE: trailers` can receive validated terminal trailer fields on the wire, while HTTP/1.0 and non-opted-in H1.1 continue to suppress them.

Dependencies: Plan 198 trailer model (hard, closed); Milestones 1–3 H1 authority (hard, closed); F3 raw-wire reproduction from Milestones 4/7 (evidence dependency, closed).

Deliverable boundary: head-time trailer-field declaration owned by the runtime, correct H1 framing (no conflicting `Content-Length`; Hyper-selected chunked transfer), native/Tower parity, and fail-closed handling when produced fields violate the declaration. H2/H3 protocol-native trailers are not redesigned.

User or operator value: the existing experimental trailer capability stops silently losing H1 response trailers after application code successfully produces them.

Exit conditions: raw TCP tests observe the terminal trailer section for opted-in HTTP/1.1, suppression remains correct elsewhere, framing/security invariants remain singular, and the closure record classifies any additive public API and release impact.

### Milestone 10 — 0.4.0 publication closure

Class: polish

Objective: close Plan 300 against the publication that actually happened,
and reconcile the planning control surface that still reports it as
outstanding.

Dependencies: Milestones 1–9 (hard, closed); live crates.io/PyPI state
(operational, satisfied).

Deliverable boundary: closure record + registry/roadmap reconciliation.
The outcome is **conditionally closed**: the published 0.4.0 artifact
predates the post-tag corrective batch (`v0.4.0..465b80c`), which
Milestone 11 republishes.

User or operator value: downstream consumers learn exactly what the live
0.4.0 artifacts contain — including the defects they still carry — instead
of reading a control surface that says the release has not happened.

Exit conditions: Plan 300 closure record with per-crate timestamps and
checksums; registry and roadmap no longer report Plan 300 as active.

### Milestone 11 — Corrective 0.4.x publication

Class: invariant

Objective: publish the corrective batch committed after the `v0.4.0` tag
so the live artifacts stop shipping the B1–B89 defects and the CI fix.

Dependencies: Milestone 10 (hard, closed); the corrective commits are
already landed in the tree.

Deliverable boundary: version/requirement/lockfile metadata, publication,
registry-only consumer proof, tag + release, wheel, changelog and
migration-guide entries. No new production behavior.

User or operator value: consumers of the published line stop receiving the
defective behaviors (conditional-request correctness, framing strictness,
range interaction, confinement parity, failure mapping, robustness).

Exit conditions: closure record with semver rationale, per-crate
timestamps/checksums, consumer proof, and changelog/migration-guide
entries.

### Milestone 12 — Runtime error-representation single authority

Class: invariant

Objective: collapse the parallel runtime error-body construction in
`eggserve-core/src/response.rs` and `eggserve-server/src/response.rs` onto
the `eggserve-primitives` authority, and add the gate rule that prevents
the collapse from reversing.

Dependencies: Plan 215 facade authority (hard, closed); the primitives
error authority (hard, closed).

Deliverable boundary: one error-representation implementation; byte-identical
error output proven against a baseline capture; topology-gate facade rule
plus `--self-test` mutation.

User or operator value: the error representation the CLI and the wheel
emit is produced by the same authority the primitives crate documents as
the sole one, so a policy change cannot miss a surface.

Exit conditions: the equivalence matrix is green and the gate fails when a
parallel implementation is reintroduced.

### Milestone 13 — Connection-overlap classification accuracy

Class: invariant

Objective: make the Plan 253 classification ledger true, bound the drift
of the classified parallel pairs mechanically, and collapse the duplicated
Alt-Svc advertisement policy.

Dependencies: Plan 253 ledger (hard, closed); Milestone 12 (interface —
same consolidation pattern).

Deliverable boundary: corrected ledger; machine-checked per-pair drift
ceiling with a `--self-test` mutation; one Alt-Svc advertisement authority
in `eggserve-primitives`; dead classified helpers removed or justified.

User or operator value: the classification stops understating the
divergence, the divergence cannot silently grow, and the H1/H2/H3 Alt-Svc
advertisement can no longer desynchronize.

Exit conditions: the drift rule fires on a perturbed tree; the Alt-Svc
header bytes are unchanged across the decision matrix.

### Milestone 14 — Compatibility config projection parity

Class: capability

Objective: make the Plan 278/289 ownership knobs reachable through the
compatibility config, and converge the duplicated `RuntimeConfigBuilder`
setters.

Dependencies: Milestones 1–3 ownership seams (hard, closed); Milestones
12/13 (interface).

Deliverable boundary: compatibility config carries the four ownership
values with unchanged defaults; the compatibility builder delegates the 28
duplicated setters; a gate rule prevents re-duplication. **No CLI flag
and no Python option** — exposing them on those surfaces is a separate
plan.

User or operator value: a compatibility-crate embedder can reach the
direct-H1 opt-ins instead of being silently pinned to the defaults, with
no change to any default behavior.

Exit conditions: the default path is byte-identical to the baseline and
each knob is demonstrably reachable.

### Milestone 15 — Accept-error classification parity

Class: invariant

Objective: use one accept-error classification authority in both the direct
and the compatibility accept loop.

Dependencies: Milestone 1 single H1 authority (hard, closed); the
compatibility implementation as the reference behavior (hard, closed).

Deliverable boundary: one classification authority; the direct loop gains
exhaustion detection, severity mapping, fatal classification, and the
backoff ramp; a gate rule prevents local re-implementation.

User or operator value: a direct embedder no longer hot-spins on
descriptor exhaustion and no longer retries a fatal accept error forever.

Exit conditions: both loops classify an injected error identically and the
backoff saturates at 500 ms.

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

The capability milestones 1–3 remain closed and the 294–297 polish campaign is closed. The subsystem can return to closed status after Milestone 299 restores H1 response-trailer wire delivery (or records an evidence-backed contract correction) with no unresolved medium+ trailer correctness defect, and after the corrective program (Milestones 304–309) has closed its findings: the publication is honestly recorded, the corrective line is published, the runtime error representation and the accept-error classification each have one authority, the Plan 253 classification is accurate and drift-bounded, and the compatibility projection preserves its defaults.

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
| 9 | conditionally closed | `plans/implementation/direct-h1-runtime/300-release-0-4-0-publication.md` | `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` | Publication completed 2026-09-25 and is fully evidenced (per-crate checksums, exact-SHA CI run `36176532051`, registry-only consumer proofs, tag + GitHub Release, PyPI wheel set + post-publish smokes). Open condition: the published artifact predates `v0.4.0..465b80c` (B1–B89 + the CI fix) → Milestone 11 |
| 10 | closed | `plans/implementation/direct-h1-runtime/304-040-publication-closure.md` | `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` | — |
| 11 | closed | `plans/implementation/direct-h1-runtime/305-corrective-040x-publication.md` | `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md` | Published 2026-10-22 from tag `v0.5.0` (commit `326aa1e`): `primitives 0.2.3`; `server`/`static`/`h3`/`core 0.5.0`; `bin 0.2.3`; wheel `0.2.5`. Exact-SHA CI `39794834966` green; six registry-only consumer proofs green; Release run `39799587527` green with ten post-publish smokes. |
| 12 | closed | `plans/implementation/direct-h1-runtime/306-runtime-error-representation-single-authority.md` | `plans/closure/direct-h1-runtime/306-runtime-error-representation-single-authority.md` | One runtime-error authority; wire output proven byte-identical (44-cell matrix + wire-level TCP); dead helper cluster removed; gate rule `check_plan306_error_authority` |
| 13 | closed | `plans/implementation/direct-h1-runtime/307-connection-overlap-classification-accuracy.md` | `plans/closure/direct-h1-runtime/307-connection-overlap-classification-accuracy.md` | Ledger corrected, measured, and drift-bounded (`check_plan253_drift`); Alt-Svc advertisement single-sourced; six dead helpers removed. Reported follow-ups: the `alt-svc` ownership question when H3 is off, and the now write-only `LifecycleDisposition` fields |
| 14 | closed | `plans/implementation/direct-h1-runtime/308-compatibility-config-projection-parity.md` | `plans/closure/direct-h1-runtime/308-compatibility-config-projection-parity.md` | Four ownership values carried with unchanged defaults; 37/38 shared builder setters delegate; gate rule `check_plan308_builder_delegation`. Reported follow-ups: `response_metadata_ownership` is not reachable through the compatibility path, and `server/http3.rs` keeps a second hardcoded projection |
| 15 | closed | `plans/implementation/direct-h1-runtime/309-accept-error-classification-parity.md` | `plans/closure/direct-h1-runtime/309-accept-error-classification-parity.md` | One accept-error classification authority (`eggserve_server::accept_error`) serves both accept loops; the direct loop now uses the shared ramp and terminates on a fatal accept error; no dependency delta; gate rule `check_plan309_accept_error_authority`. Reported follow-ups: a live-loop fatal-termination test and an exhaustion timing test are not expressible without an injected `accept(2)` failure |
