# Plan 177 — Corrective Downstream-Substrate Documentation Closure

## Status

**IMPLEMENTED / CLOSED.**

Prerequisites: Plans 172–175 implemented in substance; Plan 176 closed/deferred pending a concrete upgrade consumer.

## Purpose

Close the documentation/state-management gap left after the downstream application-server substrate work landed.

The implementation work from Plans 173–175 is complete enough to support an HTTP-only downstream application-server project through EggServe's public canonical Rust boundary, and Plan 176 has an explicit deferred/no-go disposition for upgrades. The remaining problem is documentation truthfulness: Plan 172 still presents the roadmap as active/planned, while the original `plans/ROADMAP.md` still describes a narrower static-serving-only library boundary that predates the explicit downstream embedding decision.

This is a documentation and planning-state correction only. Do not use this plan to add runtime features, ASGI/WSGI behavior, WebSocket support, new public APIs, dependencies, CI machinery, or compatibility changes.

## Current-state findings

### 1. Plan 172 is stale as an active roadmap

`plans/172-downstream-application-server-substrate-roadmap.md` still has a planned/open status and acceptance checklist even though its child work has reached terminal dispositions:

- Plan 173 — octet-preserving canonical HTTP metadata: implemented/closed;
- Plan 174 — deferred request-body ownership and request lifecycle: implemented/closed;
- Plan 175 — external downstream application-server consumer qualification: implemented/closed;
- Plan 176 — optional generic HTTP upgrade handoff: closed/deferred because no concrete upgrade consumer currently exists.

Plan 172 should therefore become a truthful closure record rather than remain an apparently active work order.

### 2. The root roadmap predates the downstream-embedding decision

`plans/ROADMAP.md` correctly says EggServe is not itself an application server, ASGI/WSGI runtime, reverse proxy, framework, CDN, or Granian-style general server. Preserve that boundary.

However, the roadmap also frames the reusable core primarily as static-serving primitives for Python projects and says dynamic handlers/request-body parsing generally belong out of scope unless a later roadmap explicitly revisits the boundary. Plans 161 and 172–175 are that later explicit revisit: the Rust core now intentionally exposes a hardened, transport-owning canonical HTTP runtime/service seam suitable for a separate downstream application-server implementation.

The roadmap must distinguish these two statements clearly:

1. EggServe does **not** implement application-server/framework semantics itself.
2. `eggserve-core` **does** intentionally support separate downstream application-server projects through its public Rust HTTP substrate.

Those statements are compatible and should be documented together.

### 3. WebSocket/upgrade support remains intentionally absent

Plan 176 established that the current canonical boundary does not expose Hyper's upgrade capability and that adding a generic upgrade handoff without a concrete consumer would prematurely freeze a public abstraction.

Documentation must not imply that downstream WebSocket-class servers are currently supported. HTTP-only downstream app-server use is qualified; upgraded-protocol support remains a future conditional extension.

### 4. Current documentation should have one consistent product boundary

Plan 175 added downstream-app-server documentation and updated several current-state documents. This corrective pass should audit the small set of authoritative/current documents for contradictory wording rather than rewrite historical plans wholesale.

Historical plans should remain historical evidence. Only Plan 172 needs status/closure correction because it is the parent roadmap being closed by this work.

## Track A — Close Plan 172 truthfully

Update `plans/172-downstream-application-server-substrate-roadmap.md`.

### A1. Status

Change the status to a terminal state such as:

```text
IMPLEMENTED / CLOSED.
```

The closure must explicitly note that Plan 176 is intentionally deferred, not accidentally incomplete, and that this does not block the HTTP-only downstream-substrate objective.

### A2. Closure record

Add a concise closure record mapping the roadmap workstreams to their actual outcomes:

- metadata fidelity → Plan 173 closed;
- deferred request-body ownership/lifecycle → Plan 174 closed;
- external consumer qualification → Plan 175 closed;
- generic HTTP upgrade handoff → Plan 176 closed/deferred until a concrete consumer exists.

Record the resulting capability boundary:

- a separate HTTP-only application server can be built against public `eggserve-core` primitives/server APIs;
- no Hyper/private-module escape is required for the qualified HTTP bridge;
- EggServe remains the transport/runtime substrate rather than the application server;
- WebSocket/upgraded-protocol consumers are not yet supported through the canonical boundary.

### A3. Acceptance checklist reconciliation

Reconcile the Plan 172 checklist with evidence from child-plan closure records.

Mark requirements satisfied by Plans 173–175 as complete. For upgrade-related acceptance items, do not mark an unimplemented feature as implemented. Instead make the roadmap closure semantics explicit: the upgrade workstream reached its planned go/no-go gate and was deliberately deferred, so it is not required for closure of the HTTP-only objective.

Avoid ambiguous unchecked boxes in a plan labeled closed. Use one of these patterns consistently:

- `[x]` for completed requirements;
- `N/A — deferred by Plan 176 until concrete consumer` for conditional upgrade requirements.

### A4. Handoff

Replace any stale “next implement Plan 173/174/175” language with a current handoff:

- downstream HTTP application-server work may begin in a separate project;
- reopen Plan 176 or create a successor only when that project has a concrete WebSocket/upgrade requirement;
- future EggServe changes should preserve the consumer qualification established by Plan 175.

## Track B — Reconcile `plans/ROADMAP.md`

Update the root roadmap conservatively. Do not turn it into a changelog or copy Plan 172 wholesale.

### B1. Purpose statement

Retain the hardened `python -m http.server` replacement/static-serving product identity, but broaden the reusable-Rust-core description to match current reality.

Recommended conceptual wording:

> EggServe is not itself an application server or ASGI/WSGI runtime. Its Rust core also exposes a hardened transport-owning HTTP runtime and canonical service boundary that separate downstream server projects may embed.

The distinction between product surface and reusable substrate must be obvious.

### B2. Scope boundary

Revise the old statement that dynamic handlers/request bodies are categorically out of scope. The correct current boundary is:

- framework/application semantics remain out of scope for EggServe itself;
- generic HTTP request/response streaming, lifecycle, cancellation, byte-correct metadata, and service embedding are legitimate substrate capabilities;
- Python stdlib-shaped APIs may remain deliberately narrower than the Rust canonical API;
- downstream adapters own ASGI/WSGI event models, Python event-loop integration, worker/process management, routing, middleware, framework loading, lifespan, and application concurrency policy.

Do not imply that the Python compatibility facade is the preferred application-server embedding seam.

### B3. Architectural target

Update the `eggserve-core` description so it includes the canonical HTTP primitives/runtime/service boundary in addition to static serving and policy.

The architecture should still keep:

- `eggserve-core` free of Python application-runtime awareness;
- CLI and Python compatibility surfaces as consumers of the core;
- Hyper as an internal transport implementation detail rather than a downstream public dependency.

### B4. Product principles

Preserve minimal-dependency, auditable, hardened semantics. Where the old “minimal protocol surface” language conflicts with already-supported generic request streaming/custom services, rephrase it around controlled protocol scope rather than static-only behavior.

Do not weaken the conservative default: static serving/default services may still reject request bodies by default even though custom downstream services can opt into streaming body policy.

### B5. Milestones and long-term promise

Do not rewrite every historical milestone to pretend downstream app-server embedding was part of the original plan.

Instead add a later-roadmap/current-state note explaining that Plans 161 and 172–175 explicitly extended the reusable Rust library boundary after the original milestones.

Update the long-term/1.0 wording where necessary so the promise is not falsely limited to static-serving primitives. A suitable boundary is:

- static serving remains the primary end-user product;
- stable hardened HTTP primitives/policies remain the core library promise;
- the transport-owning service/runtime seam is a supported downstream embedding path according to its documented stability classification;
- EggServe does not promise an ASGI/WSGI server, framework, process manager, reverse proxy, or WebSocket implementation.

Respect `docs/api-stability.md`: do not accidentally describe experimental server APIs as 1.0-stable if the current stability inventory does not.

## Track C — Cross-document consistency audit

Audit only authoritative current-state documentation likely to define product/library scope:

- `README.md`;
- `plans/ROADMAP.md`;
- `docs/downstream-app-server.md`;
- `docs/public-api-boundary.md`;
- `docs/api-stability.md`;
- `docs/library-capability-matrix.md`;
- `docs/runtime-architecture.md` or the current equivalent;
- `docs/non-goals.md`;
- `docs/extension-contract.md` if it still defines the embedding boundary.

### C1. Required consistent statements

All relevant current docs should agree that:

- EggServe is not an application server;
- a separate HTTP-only application server can use the public Rust core substrate;
- the supported seam is canonical `Request`/`Response`, `Service`, runtime/server APIs, request/response streaming, and lifecycle primitives;
- downstream consumers do not need Hyper/private internals for the Plan 175-qualified path;
- downstream coordination must remain bounded;
- application-task admission is downstream-owned when tasks outlive `Service::call()`;
- WebSocket/HTTP upgrade handoff is not currently exposed;
- Python compatibility APIs may be narrower and are not the canonical app-server substrate.

### C2. Stability language

Ensure docs distinguish semver-considered primitives from experimental server/runtime APIs exactly as the current stability policy does.

Do not upgrade stability merely because the consumer fixture passes. Qualification proves sufficiency/correctness of the current seam; it does not itself make every server type stable.

### C3. Historical documents

Do not mass-edit old numbered plans. Their old assumptions are useful historical context and should remain immutable unless they are explicitly current parent roadmaps being closed.

If a historical statement is likely to confuse readers, prefer a current roadmap note/link over retroactively rewriting many closed plans.

## Track D — Preserve explicit non-goals

The documentation correction must not broaden EggServe into the downstream product.

Keep these responsibilities outside EggServe unless separately approved later:

- ASGI/WSGI scope/event protocols;
- Python asyncio/uvloop integration;
- PyO3 application-server bridge implementation;
- worker processes and supervisors;
- application import/reload semantics;
- routing/middleware/framework loading;
- application lifespan protocol;
- application-level concurrency queues;
- WebSocket framing and protocol state;
- generic HTTP upgrade handoff until Plan 176 is reopened;
- HTTP/2/HTTP/3 expansion solely for downstream app-server parity.

The correction is about documenting a reusable HTTP substrate, not creating a general-purpose server roadmap.

## Track E — Documentation verification

Because this pass should not change Rust/Python implementation, verification should be proportional.

Required checks:

1. Search authoritative docs for stale categorical claims such as:
   - “dynamic handlers are out of scope”;
   - “request bodies are out of scope”;
   - “only static-serving primitives”;
   - claims that EggServe *is* an application server;
   - claims that WebSocket/upgrades are supported.
2. Verify every reference to Plans 172–176 has the correct terminal disposition.
3. Verify `plans/ROADMAP.md`, README, downstream-app-server docs, non-goals, and API-stability docs do not contradict one another.
4. Run documentation/link/API-doc checks already present in the repository where cheap and relevant.
5. Run `cargo test --doc -p eggserve-core` if Rustdoc prose/examples are touched.
6. Do not add a new CI job or documentation framework solely for this correction.

If no source code or executable examples change, a full cross-platform runtime suite is not required for this plan. The implementation was already qualified by Plans 173–175.

## Acceptance criteria

- [x] Plan 172 is terminally closed with a concise closure record tied to Plans 173–176;
- [x] Plan 172's checklist contains no misleading open implementation items;
- [x] Plan 176 is represented as intentionally deferred/conditional rather than implemented;
- [x] `plans/ROADMAP.md` explicitly distinguishes “not an application server” from “usable as a substrate for a separate application server”;
- [x] the root roadmap no longer categorically excludes generic request-body/custom-service capabilities already present in `eggserve-core`;
- [x] static serving remains the primary end-user/product identity and conservative default behavior remains clear;
- [x] downstream ASGI/WSGI/framework/event-loop/process semantics remain explicitly outside EggServe;
- [x] HTTP-only downstream application-server support is documented as the qualified current capability;
- [x] WebSocket/upgrade support is documented as absent/deferred pending a concrete consumer;
- [x] public API stability language matches `docs/api-stability.md` and does not overpromise 1.0 guarantees;
- [x] authoritative current-state documents agree on the embedding boundary;
- [x] historical closed plans are not broadly rewritten;
- [x] no runtime code, dependency, feature, or CI expansion is introduced by this corrective pass.

## Suggested implementation order

1. Read Plan 172 and the closure records of Plans 173–176 together.
2. Close/reconcile Plan 172.
3. Update the purpose/scope/current-state sections of `plans/ROADMAP.md`.
4. Audit the authoritative documents in Track C and make only contradiction-removing edits.
5. Run the targeted documentation verification in Track E.
6. Add a closure record to this plan listing exactly which documents changed and confirming that no runtime/API behavior changed.

## Closure record

- Plan 172 was reconciled and closed with Plans 173–175 recorded as complete
  and Plan 176 recorded as intentionally deferred pending a concrete upgrade
  consumer.
- `plans/ROADMAP.md` now distinguishes EggServe’s static-serving product and
  non-application-server boundary from the qualified HTTP-only Rust substrate
  available to separate downstream application-server projects.
- Current-state wording was aligned in `README.md`, `AGENTS.md`,
  `.opencode/skills/eggserve-dev/SKILL.md`, `architecture/overview.md`,
  `architecture/eggserve-core.md`, `architecture/runtime.md`,
  `docs/public-api-boundary.md`, `docs/api-stability.md`,
  `docs/library-capability-matrix.md`, `docs/non-goals.md`, and
  `docs/extension-contract.md`. No historical child plan was rewritten.
- The stale-wording audit found and corrected the root-roadmap static-only
  scope statement and the runtime description that treated every incomplete
  body as a forced close. Current docs consistently describe `Active` deferred
  body ownership, bounded downstream coordination, experimental server APIs,
  and absent/deferred upgrade handoff.
- Documentation/link checks, Rustdoc checks, and the repository’s local CI
  verification were run after these edits. No runtime code, public API,
  dependency, feature, or CI behavior changed.

HTTP-only downstream application-server substrate work is closed. Plan 176
remains conditional on a concrete upgrade consumer.

## Handoff

This is the final corrective documentation pass for the Plan 172 downstream-substrate line unless the audit discovers a concrete contradiction not covered above.

After closure, implementation work for an HTTP-only downstream application server should move to its own repository/project and consume EggServe through the documented public Rust boundary. Any requirement for WebSockets or another HTTP/1 upgraded protocol should first reopen Plan 176 (or create a narrowly scoped successor informed by the concrete consumer) rather than bypassing EggServe through Hyper internals.
