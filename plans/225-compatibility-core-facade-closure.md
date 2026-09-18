# Plan 225 — eggserve-core compatibility-facade closure

## Purpose

Close the 217–224 architecture program by making `eggserve-core` a compatibility facade rather than an implementation authority, validating dependency reduction, documentation, and release posture.

This plan should not introduce major new behavior. It is the proof/cleanup gate after the preceding migrations.

## Preconditions

Required:
- Plan 217 request/service convergence,
- Plan 219 static/confinement authority collapse.

Strongly preferred:
- Plan 220 H3 adapter extraction,
- Plan 221 first-party frontend migration.

Plan 218 may land independently earlier and should already be complete.

Plans 222–224 are cross-repo/optional follow-ons and do not block core facade closure unless they expose a concrete ownership problem.

## Target architecture

```
eggserve-primitives   # neutral canonical HTTP/policy/request/response vocabulary
eggserve-server       # generic transport-owning H1 runtime/service/tunnel authority
eggserve-static       # hardened static-file service + confinement authority
eggnet-tls            # neutral server TLS identity/trust substrate
eggserve-h3           # optional H3/QUIC adapter
eggserve-bin          # thin first-party consumer
eggserve-python       # PyO3 adapter over canonical leaf crates
eggserve-core         # 0.x compatibility facade only
```

## Goals

- Remove remaining duplicated implementations from core.
- Remove dependencies core no longer requires.
- Make compatibility ownership explicit in docs and topology checks.
- Verify the direct crates are sufficient for downstream app-server/library consumers.
- Measure default and feature dependency closures after cleanup.
- Prepare deprecation/removal strategy for a future major/pre-1.0 breaking release if core can eventually disappear.

## Work

### 1. Implementation inventory

Every production module in core must be classified:
- compatibility re-export,
- compatibility adapter,
- unavoidable orchestration,
- remaining implementation blocker.

No unclassified parser, filesystem resolver, protocol state machine, TLS builder, runtime limit table, or service model may remain.

### 2. Dependency minimization

Run `cargo tree` for:
- core default,
- bin default,
- server direct,
- static direct,
- Python wheel,
- H2/TLS,
- H3/TLS.

Remove direct dependencies from core that are only left over from deleted implementations.

Record before/after package counts and notable security-sensitive dependencies. Binary size may be recorded but is secondary to ownership clarity.

### 3. First-party proof

Ensure bin/Python no longer need core for production behavior (or document any narrow remaining blocker).

Ensure downstream examples/tests can build using only direct crates.

### 4. Topology hardening

Extend `check-crate-topology.py` with a final compatibility-core gate.

Examples:
- no core-owned static resolver,
- no core-owned canonical request/service definitions,
- no direct Quinn/H3 state machine,
- no second TLS server builder,
- no duplicate runtime limits/ops/errors,
- facade modules contain only approved imports/adapters.

Do not implement this as arbitrary maximum lines per file; check ownership markers/import direction.

### 5. Documentation

Update:
- `plans/ROADMAP.md`
- architecture/crate topology docs,
- downstream-app-server docs,
- migration guide,
- dependency policy,
- README crate descriptions.

Mark `eggserve-core` as compatibility facade in all current architecture diagrams.

### 6. Release evidence

Run full:
- fmt/clippy/test workspace,
- TLS/H2/H3 feature matrices,
- supply-chain audit for both lockfiles,
- Python wheel tests,
- package verification,
- examples/docs tests.

Do not change support tiers based solely on this architecture work.

## Core deletion question

This plan does not require deleting `eggserve-core`. Before 1.0, evaluate whether:
- keeping a compatibility umbrella crate is useful for ergonomics, or
- a later breaking release should deprecate it in favor of direct crates.

Any removal requires a separate explicit migration plan.

## Rollback

If a compatibility adapter must temporarily retain implementation, document it as a blocker with an owner and follow-up. Do not silently re-expand core.

## Acceptance criteria

- Core is demonstrably a facade/adapter layer.
- Security-critical and protocol-critical authorities live in the leaf crates.
- First-party products exercise the direct architecture.
- Dependency topology CI enforces the ownership model.
- Full test/security/package matrix passes.
- Documentation matches the actual crate graph.
- A future contributor can identify the owner of any security-sensitive behavior without tracing duplicate implementations.
