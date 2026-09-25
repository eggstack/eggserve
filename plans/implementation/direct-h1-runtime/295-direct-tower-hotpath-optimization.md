# Direct H1 Runtime Milestone 295 — Direct Tower hot-path optimization

Status: blocked

Repository baseline: `81605fc36b970440d6e3edbfd1e0d1cfa3ec4d91` (planning baseline; implementation must refresh after 294)

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-5--toweraxum-hot-path-optimization`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `architecture/adr-003-custom-service-ownership.md`

Primary class: polish

## 1. Objective

Remove only the direct Tower/Axum adaptation costs proven material or mechanically redundant by Milestone 294, with special attention to duplicate request-head/header materialization and streamed-response boxing/trailer synchronization.

The implementation must preserve one canonical H1 validation/framing/security authority. It must not create a second parser, duplicate request-smuggling defense, or let Tower/Axum bypass runtime response policy.

## 2. Why this milestone is ready

It is not yet dependency-ready. Hard dependency: Milestone 294 closure must name the exact costs and candidate(s) to retain. Once 294 closes, execute only the candidates marked PROCEED.

## 3. Current implementation evidence

Planning-time candidates:

- inbound path: Hyper request → canonical `RequestHead`/`HeaderBlock` → `request_head_to_http` → Tower/Axum;
- outbound streaming path: Tower `http_body::Body` → canonical response stream/trailer rendezvous → Hyper body;
- `TowerToEggserve::call` clones the service per request and boxes the returned native service future;
- response adaptation uses heap/synchronization state for trailers even when ordinary SSE-like responses have none.

These are candidates, not authorization to change them without 294 evidence.

## 4. Invariants that must not regress

- Hyper-facing request validation still executes exactly once under EggServe authority before application dispatch.
- Host/authority, request-target, TE/CL, parser/header/body ceilings, and runtime rejection semantics remain unchanged.
- Tower middleware cannot bypass framing, denylist, Date/Server policy, body limits, no-progress timeout, lifecycle, or shutdown.
- Streaming stays incremental; no full-body buffering.
- Trailers remain terminal-only and bounded.
- HEAD and body-forbidden responses never poll/drop producer state incorrectly.
- Service errors remain sanitized and committed-response failures do not synthesize a second response.

## 5. Scope

### In scope

Depending on 294:
- avoid redundant canonical→`http` header/target materialization while preserving canonical validation authority;
- reuse validated ecosystem request metadata where safe;
- reduce boxing/dynamic dispatch in the direct Tower response path;
- replace per-stream `Arc<Mutex>` trailer rendezvous with a single-owner concrete state machine if 294 confirms it is material and semantics remain simpler;
- specialize no-trailer/known-empty response paths;
- preserve current public API unless a private/internal bridge suffices.

### Explicitly out of scope

- no native `Service` contract redesign;
- no unsafe zero-copy header aliasing;
- no unchecked header constructors;
- no second Hyper service implementation with separate policy logic;
- no H2/H3 work;
- no file/tunnel capability gating (296);
- no custom executor/allocator.

## 6. Required production changes

### Crates and ownership

Changes should remain in `eggserve-server` and, only if a neutral representation improvement is necessary, `eggserve-primitives`. Do not make primitives depend on `http`, Hyper, Tokio, or Tower.

### Config and policy

No new user knob solely for performance. If a fast path needs an opt-in because semantics differ, stop and reassess; equivalent semantics should be selected internally.

### Protocol and compatibility

Preserve the existing `TowerToEggserve` and core compatibility re-export contracts. New direct helper APIs are allowed only when unavoidable and must be semver-classified.

### Runtime and concurrency

Prefer single-owner state where the body is exclusively polled by one connection task. Do not replace safe synchronization with assumptions that fail under cancellation, trailer futures, or body movement.

### Security and confinement

All canonical validation and response finalization controls must remain on the actual production path and be exercised by negative tests.

## 7. Ordered work packages

### Work package A — Confirm 294 target

Copy the exact 294 candidate disposition into the implementation branch notes. If no direct-Tower hot-path candidate is PROCEED, close 295 as NO-GO without production edits.

### Work package B — Request-path simplification

Implement the minimum design that removes confirmed duplicate work.

Preferred direction: separate “validate/project” from “materialize canonical owned copy” internally so direct Tower can receive validated `http` metadata without round-tripping every field, while native services still receive canonical owned types.

Hard rule: validation functions and policy decisions remain shared single authorities.

### Work package C — Streaming response body

If 294 supports it, introduce a concrete internal response-body adapter/state machine that can represent the direct Tower body without unnecessary nested boxing/rendezvous. Preserve trailers, size hints, cancellation, write-progress observation, producer errors, and HEAD/body-forbidden suppression.

A no-trailer fast path may be retained independently if it is mechanically simpler.

### Work package D — A/B and keep/revert

Compare each candidate independently to the 294 baseline. KEEP only when allocation/CPU/tail/resource evidence improves or the change removes mechanically redundant work with equal/simpler code. REVERT when complexity rises without evidence.

## 8. Failure, cancellation, restart, and contention semantics

Explicitly test service readiness failure, body producer error, producer panic containment where applicable, client disconnect, cancellation before/after first chunk, terminal trailers, server quiesce/drain, and multiple concurrent slow streams. No lock or allocation removal may detach lifecycle ownership.

## 9. Compatibility and migration

Prefer source-compatible private changes. If a public type/feature/API must change, stop and record the semver consequence before implementation. Core compatibility Tower paths must continue to forward to the direct authority.

## 10. Required tests

Focused:
- request metadata/header duplicate/order/opaque-value parity;
- body streaming and size hint;
- terminal trailers and invalid trailers;
- HEAD/204/205/304 suppression;
- response error after commitment;
- Tower readiness/error mapping.

Integration:
- Axum router with buffered + SSE-like streaming responses;
- direct H1 parity/security cases;
- cancellation/shutdown with active streams.

## 11. Required verification commands

```bash
cargo test -p eggserve-server --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
./scripts/verify.sh fast
```

Run the 294 benchmark matrix against baseline/candidate on the same machine.

## 12. Documentation updates

Update direct-embedding/runtime architecture docs only if internal behavior or public contracts materially change. Record benchmark evidence and keep/revert decisions.

## 13. Acceptance criteria

- Every retained optimization maps to a 294 finding.
- No duplicate parser/framing/security authority is introduced.
- Direct Tower/Axum allocation or CPU cost improves, or a mechanically redundant layer is removed with simpler/equal code.
- Streaming behavior, trailers, cancellation, timeout, and shutdown semantics remain equivalent.
- Native `Service` behavior and core compatibility forwarding remain green.
- No new production dependency is introduced without explicit justification.

## 14. Stop conditions

Stop if optimization requires unchecked validation, borrowed public lifetimes across the service contract, a second framing implementation, broad `Service` redesign, or semantic weakening. Stop and close NO-GO if 294 does not show a worthwhile target.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/295-direct-tower-hotpath-optimization.md` with before/after allocation/CPU/latency/resource data, per-candidate KEEP/REVERT/DEFER decisions, exact verification commands, and invariant mapping.

## 16. Handoff notes

The strongest expected downstream workload is many long-lived SSE/token streams plus ordinary small metadata endpoints. Provider latency is not part of the benchmark; isolate server overhead.
