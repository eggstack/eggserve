# Plan 102 — Runtime Correctness, Scope, and Size Closure Roadmap

## Status

**IMPLEMENTED — RECLOSED BY VERIFIED PLAN 109.** Plans 102–108 implemented the
roadmap; Plan 109 completed the bounded admission ownership, wire-verification,
and evidence correction.

Corrective roadmap for the repository state at:

```text
83e941ee42430e7c727971c66625eabc37bf4938
```

This roadmap reopens only the specific correctness, ownership, scope, binary-size, and verification issues identified after Plans 094–101. It does not reopen the completed Python `http.server` compatibility workstream generally, and it does not authorize a new feature track.

Implementation is split into four independently reviewable plans:

```text
Plan 103 — CLI, static-serving, and configuration correctness
Plan 104 — Generic runtime boundary and service ownership correction
Plan 105 — Product-surface freeze and measured binary-size reduction
Plan 106 — Verification, CI, documentation, and final closure
```

Plan 106 is the closure plan for this roadmap. Do not add a separate evidence, certification, or release-readiness plan unless implementation uncovers a security defect that cannot be closed within these four phases.

## Product goal

EggServe remains:

- a hardened HTTP/1.1 static file server for local development, LAN deployment, and controlled reverse-proxy origin use;
- a safer replacement for `python -m http.server`;
- a bounded Python `http.server`-shaped facade whose sockets, framing, limits, path resolution, and file streaming remain Rust-owned;
- a reusable Rust library exposing correct HTTP and confinement primitives;
- small enough to audit, build, package, and iterate without enterprise release ceremony.

EggServe is not becoming:

- an ASGI or WSGI server;
- a general application framework;
- a routing or middleware platform;
- a reverse proxy;
- an upload or multipart server;
- a WebSocket server;
- an HTTP/2 or HTTP/3 implementation;
- an ACME or virtual-hosting platform;
- a generalized socketserver compatibility layer;
- a full HTTP client product maintained in parallel with the server.

Downstream projects may build those capabilities on the public primitives. They are not EggServe release deliverables.

## Why this roadmap is required

The hardened static-serving path is coherent and substantially meets the intended goal. The remaining defects are concentrated in the newer generic runtime and in configuration surfaces that grew across multiple implementation phases.

The current repository has the following material issues:

1. `--quiet` and `--log-format none` are parsed but do not suppress the structured logger.
2. The generic runtime applies method/body policy that belongs to `StaticService`, including rejecting bodies for methods that RFC 9110 does not universally forbid.
3. `RuntimeConfig` exposes fields that are inert, duplicated, or owned by another object.
4. `start_with_service()` still requires static-serving state and pins a filesystem root even for non-static services.
5. user-provided semaphore counts are not bounded against Tokio's maximum permit count and can panic during state construction.
6. several directory-listing limits look enforceable in the public API but are not applied at their actual boundaries.
7. unknown or unsupported request conversion can silently alter semantics instead of failing closed.
8. direct Rust static serving and Python-compatible static serving disagree on index-page fallback.
9. rejected bodies are drained for a fixed hard-coded duration rather than following a simple close-oriented policy.
10. Python package metadata contains an inconsistent license classifier.
11. the repository contains more client/runtime/fuzz/verification surface than is justified by the narrow local static-server product.
12. binary-size decisions are not based on reproducible artifact measurements.

This roadmap corrects those issues without broad redesign.

## Governing principles

### 1. Preserve the hardened static path

Do not weaken:

- pinned-root confinement;
- descriptor-relative Unix traversal;
- handle-relative Windows traversal;
- dotfile denial;
- symlink denial;
- directory-listing denial by default;
- single-pass path decoding;
- conditional and range response behavior;
- authoritative response normalization;
- file-handle-owned streaming;
- connection and file-stream admission limits;
- bounded Python callback and response behavior.

Refactoring is permitted only where tests prove these invariants remain intact.

### 2. Put policy at the correct layer

Transport correctness belongs to the runtime. Static-serving policy belongs to `StaticService`.

The runtime may enforce:

- parse and framing validity;
- supported HTTP version;
- bounded headers and body consumption;
- transfer-length consistency;
- connection lifecycle and timeouts;
- service-declared body policy;
- response normalization;
- transport-owned headers and file-stream admission.

The runtime must not globally impose `GET`/`HEAD` static-server semantics on custom services.

### 3. One owner for every resource limit

Each limit must have one authoritative owner and one enforcement point. No public field may be retained solely because prior plans documented it.

Preferred ownership:

- `RuntimeConfig`: listener, connection, handler/body deadlines, keep-alive, server header, request-body ceiling, and transport file-stream admission;
- static service configuration/state: root, path policy, listing policy, index names, and listing bounds;
- Python facade: callback-worker and in-memory handler-response bounds translated into the native runtime.

### 4. Remove inert API instead of simulating configurability

Before 1.0, deleting an unused or misleading field is preferable to maintaining a false compatibility promise.

Do not add placeholder implementations, ignored setters, compatibility shims, or deprecation frameworks for fields that have never worked as documented.

### 5. Measure size before changing architecture

Binary-size work must compare actual stripped release artifacts. Do not split crates, replace dependencies, remove package behavior, or alter runtime scheduling based on intuition alone.

A size change is accepted only when:

- behavior and supported features are unchanged;
- relevant tests pass;
- the artifact improvement is measurable and recorded;
- the implementation does not materially reduce auditability;
- no permanent CI size gate is added.

### 6. Verification remains proportional

Routine CI remains two jobs: Rust and installed-wheel Python verification on Ubuntu.

Do not add:

- a platform matrix to every pull request;
- scheduled fuzzing;
- evidence aggregation;
- artifact attestation infrastructure;
- release gating engines;
- generated compliance records;
- automated crates.io or PyPI publication;
- new long-running soak tests in ordinary CI.

Cross-platform wheel checks belong to the manual release workflow, not the iterative development loop.

## Plan sequence

### Plan 103 — CLI, static-serving, and configuration correctness

Correct the defects that can be closed without changing the generic service architecture:

- make `--quiet` and `--log-format none` truthful;
- validate concurrency counts before semaphore construction;
- reconcile listing limits with actual enforcement;
- unify `index.html`/`index.htm` behavior;
- remove the fixed rejected-body drain from the static path where applicable;
- correct Python package metadata;
- add focused regression coverage.

Plan 103 may proceed before Plan 104, provided it does not introduce a second long-term owner for runtime file-stream limits.

### Plan 104 — Generic runtime boundary and service ownership correction

Correct the architecture of the reusable service runtime:

- allow custom services to run without a static root;
- separate runtime transport state from static filesystem state;
- make runtime file-stream admission authoritative for all canonical file responses;
- apply service-declared body policy instead of static method assumptions;
- remove or implement inert `RuntimeConfig` fields;
- make request conversion fail closed without semantic substitution;
- preserve static-server behavior through `StaticService`.

This is the only intentionally API-breaking phase. Keep the change set narrow and complete it before binary or verification cleanup.

### Plan 105 — Product-surface freeze and measured binary-size reduction

After correctness and ownership are stable:

- establish artifact-size baselines;
- add a distribution profile;
- evaluate a current-thread standalone CLI runtime;
- trim unused Tokio feature flags;
- confirm TLS remains absent from the default CLI artifact;
- freeze client and application-serving expansion;
- retain only measured, behavior-preserving size wins;
- document non-decisions where a proposed optimization is not worthwhile.

Do not remove the bundled Python CLI, client primitives, TLS, or platform support in this plan. Those would be feature or packaging decisions requiring separate user direction.

### Plan 106 — Verification, CI, documentation, and final closure

Once Plans 103–105 are implemented:

- remove low-value routine verification work;
- consolidate redundant fuzz targets while retaining security-critical fuzz coverage;
- keep deep suites manual;
- add minimal installed-wheel smoke checks to manual release builds;
- reconcile active API, architecture, security, and deployment documentation;
- record measured binary results;
- execute same-commit local and hosted closure.

## Required execution order

```text
1. Plan 103: independent correctness defects
2. Plan 104: runtime/state ownership and public API correction
3. Plan 105: size measurement and accepted reductions
4. Plan 106: verification/documentation reconciliation and closure
```

A Plan 103 implementation may be reviewed independently, but Plan 106 must verify the combined final state.

## Commit discipline

Prefer small commits aligned to the plans:

```text
Plan 103
  1. logger/CLI truthfulness
  2. limit and listing correctness
  3. index and metadata consistency
  4. focused tests/docs

Plan 104
  1. runtime/static state separation
  2. body-policy and request-conversion corrections
  3. RuntimeConfig cleanup
  4. migration tests/docs

Plan 105
  1. measurement baseline and dist profile
  2. Tokio/runtime feature reduction
  3. retained size wins and results

Plan 106
  1. fuzz/CI simplification
  2. manual release smoke checks
  3. documentation reconciliation
  4. final validation and closure record
```

Do not combine all implementation and closure evidence into one opaque commit.

## Unified acceptance criteria

This roadmap is complete only when all of the following are true.

### Product behavior

- The default CLI serves static files correctly with secure defaults.
- `--quiet` and `--log-format none` behave exactly as documented.
- `index.html` and `index.htm` fallback is consistent across supported static interfaces.
- no valid configuration value can panic semaphore construction.
- every retained listing limit is enforced; every unenforced listing field is removed.
- the Python six-class facade continues to pass installed-wheel compatibility tests.

### Runtime architecture

- A custom service starts without creating or pinning any filesystem root.
- `RuntimeConfig.max_file_streams` has one transport-level enforcement point for all canonical file responses.
- static path policy remains in `StaticService`.
- custom services may declare body handling for methods whose content semantics are service-defined.
- TRACE content remains rejected as required by HTTP semantics.
- request methods, versions, and headers are never silently substituted or emptied on conversion failure.
- every public runtime field changes runtime behavior or is removed.

### Scope

- no ASGI/WSGI, routing, middleware, upload, proxy, WebSocket, HTTP/2, HTTP/3, ACME, or virtual-hosting capability is added.
- no new general-purpose client feature is added.
- the existing client surface is frozen and isolated from default server artifacts.
- no new framework dependency is introduced.

### Size

- default CLI, TLS CLI, Python extension, bundled CLI, and wheel sizes are recorded before and after Plan 105.
- distribution builds are stripped and reproducible using documented commands.
- accepted optimizations retain behavior and pass focused performance checks.
- `panic = "abort"` is not used because service-panic containment is a supported runtime property.
- no optimization is retained solely for a negligible or unmeasured size change.

### Verification

- routine CI remains no more than the existing two jobs.
- no automated publication is added.
- deep race, fault, corpus, proxy, and fuzz campaigns remain manual.
- security-critical path, framing, range, response-normalization, and filesystem tests remain present.
- manual release wheel builds perform a minimal installed-artifact smoke test on each built platform.
- `./scripts/verify.sh fast` and `./scripts/verify.sh full` pass on the final commit.
- hosted Rust and Python jobs pass on that same commit.

## Explicit rejection criteria

Reject an implementation that:

- weakens root confinement or reopens paths by pathname after authorization;
- moves filesystem policy into the generic transport runtime;
- retains an ignored public configuration field;
- adds another state/configuration object without removing the duplicated owner;
- introduces a new CI workflow for size, fuzzing, release evidence, or platform qualification;
- turns manual deep tests into pull-request blockers;
- removes a supported feature merely to improve a size number;
- uses process aborts to simplify panic behavior;
- adds a large abstraction layer to avoid a small alpha API break;
- broadens Python compatibility toward raw socketserver internals;
- expands the HTTP client or application-serving surface.

## Handoff completion

The implementation agent should begin with Plan 103 and proceed in numerical order. Plans may be implemented in separate pull requests or direct commits, but Plan 106 must evaluate the combined final commit.

The final closure response must distinguish:

- defects fixed;
- API fields removed or made effective;
- binary-size changes accepted or rejected with measurements;
- verification removed, retained, or moved to manual execution;
- any remaining Windows adversarial-qualification limitation already documented by the project.

Do not claim this roadmap complete until the final same-commit validation criteria in Plan 106 are satisfied.
