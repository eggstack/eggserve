# Plan 208 — HTTP Server Foundation Release and API-Stability Closure

## Status

**CLOSED 2026-09-11 — terminal closure for the Plan 196 program.** Plans
197–204, 206, and 207 are implemented; Plan 205 is explicitly deferred (see
closure record); H2/H3 promotion gates from Plans 191–195 stand as blockers.
Full evidence is in `release/plan-208-foundation-release-closure.md`.

## Purpose

Convert the implementation and conformance evidence from Plans 197–207 into a truthful supported product/API boundary. This plan decides what is stable versus experimental, performs the final security/API/documentation review, and closes the full HTTP server foundation program without turning EggServe into an application framework.

Implementation existence is not sufficient for promotion. Any capability that misses mandatory qualification remains experimental or explicitly unsupported.

## Track A — Re-audit the intended product boundary

At closure, the repository should truthfully describe EggServe as:

> A hardened HTTP server runtime and static-server product with a reusable application-facing service substrate for Rust and low-level Python consumers. The runtime owns transport/protocol correctness, limits, lifecycle, framing, TLS, and canonical HTTP semantics; downstream projects own framework/application-server semantics.

Confirm that no roadmap implementation accidentally added framework responsibilities such as routing, app lifespan, worker supervision, WebSocket codec logic, reverse proxying, ACME, sessions, or middleware policy.

Update `docs/non-goals.md` so it reflects current capabilities without preserving obsolete prohibitions from Plans 172/176/183.

## Track B — API support-tier decision

Inventory every public application-server-facing item introduced or changed by Plans 197–205 and classify it:

### Candidate stable/pre-1.0 semver-considered
- canonical HTTP metadata/value types;
- `Request`/`RequestHead` ordinary metadata access;
- `RequestBody` incremental semantics;
- `RequestLifecycle` cancellation observer;
- `Response`/ordinary response body types;
- connection/TLS/proxy metadata values that have completed cross-protocol qualification.

### Candidate supported but still experimental
- listener injection/socket activation if platform evidence is incomplete;
- trailer/interim APIs whose upstream protocol libraries impose limitations;
- generic tunnel/Extended CONNECT capability;
- H2/H3 runtime configuration;
- Tower adapter depending on ecosystem/API churn;
- async Python bridge.

Do not force everything to stable in one release. Stability is a promise about API/semantic maturity, not a reward for finishing the plan.

For each public item record:

- feature gate;
- source module/path;
- supported protocols;
- ownership/cancellation contract;
- compatibility policy;
- known limitations;
- migration path from the Plan 196 baseline.

## Track C — HTTP protocol support-tier decision

Re-evaluate H1/H2/H3 against Plan 207 evidence and previous promotion gates.

### HTTP/1.1

Must remain fully supported and regression-safe. Plan 196 work must not reduce static-server or compatibility correctness.

### HTTP/2

Promote from experimental only if mandatory interoperability/platform/application-semantic evidence now passes. Reuse the strict fail-closed qualification philosophy from Plan 191 rather than lowering the gate because more features exist.

### HTTP/3

Promote only if Plan 193-era dependency/platform blockers and Plan 207 application-semantic requirements are resolved. Otherwise retain experimental classification with exact blockers.

Support status can differ between Rust runtime and Python low-level consumer if Python qualification lags. State this explicitly.

## Track D — Security review

Perform a focused threat-model review of the new attack surfaces:

- trailers/interim response state machines;
- H1 upgrade transition and buffered bytes;
- H2/H3 Extended CONNECT tunnel lifetime/flow control;
- Tower/http adapter conversion loss or policy bypass;
- inherited/prebound descriptors and Unix socket ownership;
- PROXY protocol parsing and forwarding-header trust;
- SNI/client-auth/reload races and key-material handling;
- async Python cross-thread/GIL/cancellation/channel ownership;
- new observability metadata/privacy;
- resource limit interactions across long-lived streams/tunnels.

Required properties:

- no route to bypass canonical response framing/final privacy policy;
- all attacker-controlled metadata is bounded before expensive work;
- no trusted client/TLS/proxy identity can be forged by ordinary headers;
- every long-lived resource class has a hard admission/lifetime/shutdown owner;
- no detached tunnel/Python/background task can outlive forced server shutdown unintentionally;
- no new secret/body/header leakage appears in default logs/errors.

Run supply-chain audit/deny checks for all feature combinations used in release artifacts.

## Track E — Public API/migration review

Before a release bump:

- run API snapshot/public consumer tests;
- compare the current public surface with the previous release/baseline;
- remove accidental public re-exports/internal types introduced during refactors;
- ensure `#[non_exhaustive]` is applied where future protocol/category growth is expected;
- ensure no Hyper/h2/h3/Quinn/rustls implementation type leaked into general stable signatures;
- document intentional breaking changes under the project's pre-1.0 policy;
- verify examples compile from an external-consumer perspective.

If a problematic API was discovered during qualification, fix it before stabilization even if that means one final planned pre-1.0 break. Do not stabilize a known-bad seam to avoid churn.

## Track F — Feature/dependency graph closure

Document and test supported feature combinations, including at least:

```text
default/minimal H1
TLS
HTTP/2
TLS + HTTP/2
HTTP/3 (and required TLS/QUIC)
Rust HTTP interop
Tower adapter
all qualified Rust features
Python wheel feature set
```

Requirements:

- minimal H1 does not pull H3/Quinn/Tower/Python async dependencies;
- optional feature combinations compile without hidden transitive assumptions;
- duplicate/incompatible crypto-provider configuration is resolved/documented;
- MSRV claim is tested against every feature set included in the support contract or narrowed truthfully.

Do not support every mathematically possible feature combination if some are nonsensical; define the supported matrix explicitly.

## Track G — Documentation closure

Synchronize at minimum:

- `README.md`;
- `plans/ROADMAP.md`;
- `docs/non-goals.md`;
- `docs/public-api-boundary.md`;
- `docs/api-stability.md`;
- `docs/library-capability-matrix.md`;
- `docs/downstream-app-server.md`;
- protocol docs for H2/H3;
- TLS/timeout/lifecycle/proxy/listener docs;
- Python architecture/compatibility docs;
- `AGENTS.md` and developer skill guidance where invariant ownership changed.

Clearly distinguish:

1. EggServe native Rust runtime;
2. optional Rust ecosystem adapters;
3. synchronous Python `http.server` compatibility;
4. async Python low-level application-server substrate;
5. downstream application servers such as ASGI implementations.

Avoid marketing claims like “full ASGI server” if only the substrate/fixture exists.

## Track H — Examples and downstream starter fixtures

Maintain small canonical examples that compile/run in verification:

- native Rust custom service;
- streaming/full-duplex Rust service;
- Tower service on EggServe;
- prebound listener;
- optional trusted proxy/TLS configuration example using local fixtures only;
- async Python handler;
- ASGI qualification fixture or pointer to downstream example;
- generic tunnel fixture (not production WebSocket codec).

Examples must demonstrate safe defaults and bounded ownership. Do not teach users to disable trust/security checks for convenience.

## Track I — Release qualification

Run all routine verification plus Plan 207 release matrix and existing platform workflows. Capture exact tool/client/platform versions and commit SHA.

Mandatory release evidence should include:

- Linux full Rust/Python feature matrix;
- macOS/Windows supported product paths according to capability matrix;
- H2/H3 external interop where promotion is claimed;
- installed Python wheels on supported interpreters/platforms;
- static-server regression suite;
- application-server contract suite;
- security/adversarial corpora and fuzz regression replay;
- supply-chain checks;
- same-machine performance sanity evidence;
- package/install smoke tests.

If external tooling/platform evidence is unavailable, mark the affected support tier blocked rather than recording a pass.

## Track J — Version/release decision

After API classification and evidence, choose the appropriate pre-1.0 release version according to actual compatibility impact. Do not assume a version number written in older plans remains correct.

Synchronize Rust crates, Python package metadata, docs, and release artifacts. Preserve the project's manual/simple release philosophy unless existing release automation is required for cross-platform wheels.

## Program acceptance criteria

Plan 196 can close when:

- [ ] ordinary downstream Rust applications have a documented, qualified native service/request/response/lifecycle contract;
- [ ] trailers and interim responses have truthful support/limitation status across H1/H2/H3;
- [ ] generic tunnel handoff supports the qualified HTTP protocols without importing WebSocket codec semantics into core;
- [ ] `http`/`http-body`/Tower adapters are optional and cannot bypass EggServe hard policies;
- [ ] prebound/listener/process-manager integration uses the same runtime rather than duplicated accept loops;
- [ ] trusted proxy/PROXY metadata has explicit fail-closed provenance rules;
- [ ] production TLS identity/client-auth/reload capabilities are qualified and do not expose secret internals;
- [ ] async Python substrate is bounded/cancellation-safe and sufficient for the downstream ASGI qualification fixture;
- [ ] cross-protocol application-server conformance and resource recovery pass;
- [ ] support tiers are evidence-driven and H2/H3 remain experimental if their gates still fail;
- [ ] public APIs contain no accidental protocol implementation types;
- [ ] default/minimal dependency graph remains appropriately small;
- [ ] current documentation matches implementation and non-goals exactly;
- [ ] no framework/router/proxy/worker/ACME/WebSocket-codec scope has entered EggServe core.

## Closure record template

When implemented, append:

- final commit/release SHA;
- support-tier table;
- API changes/migrations;
- protocol/platform matrix;
- security review findings/corrective plans;
- external interoperability evidence;
- performance evidence location;
- verification commands/results;
- explicitly deferred follow-up items with rationale.

## Handoff

After this plan closes, downstream projects should be able to build a Rust application server or a maintained Python ASGI server on EggServe without importing EggServe internals or reproducing HTTP transport/security machinery. Future EggServe work should then be driven by concrete downstream gaps rather than general framework feature accumulation.