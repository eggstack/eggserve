# Corrective Roadmap — Runtime, Windows, and HTTP Correctness

## Purpose

This roadmap closes the correctness defects identified after implementation of the production-hardening roadmap through Plan 073. It is a corrective program, not a feature expansion.

The work preserves eggserve's existing scope: a hardened static file server and reusable static-serving/runtime primitive. It must not turn eggserve into an application framework, reverse proxy, general edge server, middleware platform, or protocol gateway.

The corrective program is divided into five releases. This handoff defines detailed plans for Releases A through C. Releases D and E remain roadmap-level until the earlier contracts stabilize.

## Corrective findings driving this roadmap

The implementation review identified the following material risks:

1. Windows native string lengths are derived from UTF-8 byte counts rather than UTF-16 code units in several `UNICODE_STRING` call sites.
2. Windows raw-handle ownership contains at least one latent borrowed-versus-owned hazard and uses panic-capable duplication paths.
3. `response_write_timeout` currently behaves as a total connection-lifetime timeout rather than a progress-aware response write timeout.
4. forced shutdown can drop `JoinHandle` values without aborting and joining every remaining task, allowing detached work after the server reports completion.
5. the custom-service builder accepts a service value that is not retained by the built server.
6. connection metadata is populated with placeholder loopback addresses and no real TLS/scheme information.
7. `RequestBodyPolicy::Reject` can invoke the service before rejection, allowing handler side effects for a request the runtime claims to reject.
8. incomplete-body drain configuration is advertised without a complete active drain implementation.
9. runtime/static/frontend configuration contains duplicate or ineffective resource controls, including file-stream limits.
10. direct Rust construction can create zero-valued limits that later panic or deadlock rather than returning validation errors.
11. HEAD normalization is incomplete for error and directory-listing paths.
12. directory index resolution bypasses direct-file conditional and range handling.
13. validators are weak enough to collide for same-size, same-second modifications.
14. Windows hardened traversal remains incomplete for child resolution and directory enumeration.
15. JSON logging is advertised but not emitted as structured JSON.
16. streaming and accept-loop internals contain avoidable allocation and persistent-error hot-loop risks.

## Governing invariants

All corrective work must preserve these invariants:

- every public configuration field has one authoritative owner;
- public APIs do not silently discard supplied values;
- rejected requests do not invoke user code;
- lifecycle state reflects actual task and resource ownership;
- timeout names match their enforcement semantics;
- hardened filesystem access remains handle-relative from pinned root to the final opened object;
- logically equivalent resource paths share one HTTP response planner;
- performance work does not weaken protocol, confinement, or shutdown guarantees;
- support claims remain profile-specific and evidence-backed.

## Release sequence

### Release A — Critical safety and lifecycle correction

Release A closes the defects most likely to cause memory/handle unsafety, incorrect long-running behavior, or false shutdown completion.

Plans:

- Plan 075 — Corrective baseline and release containment
- Plan 076 — Windows Unicode and handle-ownership correctness
- Plan 077 — Runtime timeout semantics and structured shutdown

Release A is a prerequisite for every subsequent corrective release.

### Release B — Embedded runtime contract correction

Release B repairs the custom-service API, connection metadata, request-body policy, and configuration ownership shared by Rust, CLI, and Python frontends.

Plans:

- Plan 078 — Custom-service ownership and real connection metadata
- Plan 079 — Request-body rejection and incomplete-body policy
- Plan 080 — Configuration authority and frontend parity

Release B depends on the lifecycle primitives delivered by Release A.

### Release C — HTTP semantic correction

Release C unifies direct-file and directory-index behavior and closes GET/HEAD, range, conditional, and validator inconsistencies.

Plans:

- Plan 081 — Unified static-file and directory-index response path
- Plan 082 — HEAD, error-response, and validator correctness
- Plan 083 — HTTP conformance and raw-wire corrective closure

Release C depends on the configuration and service contracts delivered by Release B.

### Release D — Windows hardened-profile completion

Release D will preserve directory handles through final child resolution and directory enumeration, complete handle-relative Windows index lookup, replace path-based hardened enumeration, and qualify supported filesystem classes.

Expected future plans:

- Windows directory-handle retention and child resolution
- Windows handle-relative directory enumeration
- Windows adversarial reparse/filesystem qualification

Windows hardened support must not be promoted before Release D closes.

### Release E — Operational and performance closure

Release E will implement truthful JSON logging, remove unconditional library printing, classify listener failures, optimize streaming allocations using evidence, run sustained soak tests, and perform final release qualification.

Expected future plans:

- structured logging and operational errors
- streaming allocation and buffer benchmarks
- long-running soak and corrective release closure

Performance work must not begin until Releases A through D have established stable semantics.

## Dependency graph

The required order is:

1. Plan 075 establishes the source/evidence baseline and blocks unsupported releases.
2. Plans 076 and 077 may proceed in parallel after Plan 075.
3. Plan 078 requires Plan 077's shared connection task/lifecycle structure.
4. Plan 079 requires Plan 077's explicit connection termination and bounded drain hooks.
5. Plan 080 follows Plans 078 and 079 so final configuration ownership reflects the repaired APIs.
6. Plan 081 follows Plan 080 so the static service consumes one validated configuration model.
7. Plan 082 builds on the unified response path from Plan 081.
8. Plan 083 is the independent closure gate for Release C and reruns invalidated runtime/body tests from Releases A and B.
9. Release D begins only after Plan 083 closes.

Parallel implementation is allowed only where affected modules do not overlap and the merge order preserves these dependencies.

## Scope firewall

The corrective roadmap must not add:

- ASGI or WSGI support;
- routing, middleware, sessions, templates, authentication, or application frameworks;
- reverse proxying, caching proxy behavior, ACME, virtual hosts, HTTP/2, or HTTP/3;
- per-user or distributed rate limiting;
- a metrics/admin network endpoint;
- speculative platform support intended only to broaden marketing claims;
- performance changes without benchmark and correctness evidence.

API breakage is acceptable before 1.0 when required to remove misleading or unimplementable contracts. Compatibility shims must not retain incorrect behavior.

## Evidence policy

Each plan must produce evidence tied to the exact implementation commit. Evidence must include:

- command or workflow identity;
- source SHA;
- platform and architecture;
- enabled features;
- test or benchmark result;
- artifact identity when testing installed binaries or wheels;
- known exclusions and their rationale.

A plan is not complete merely because code was written. Its acceptance criteria and invalidated release gates must pass.

## Release A exit criteria

Release A closes only when:

- Windows native string construction is UTF-16-correct and non-ASCII tests pass on Windows;
- no borrowed handle can be accidentally closed through an ownership wrapper;
- handle duplication failures propagate without panic;
- write timeout semantics are accurate and progress-aware, or the field is renamed and documented to match implemented behavior;
- shutdown aborts and joins all remaining tasks after the grace deadline;
- `Stopped` is not observable while server-owned work remains;
- listener errors cannot create an unbounded hot loop;
- repeated start/stop and saturation tests return tasks, sockets, handles, and permits to baseline.

## Release B exit criteria

Release B closes only when:

- no builder accepts and discards a custom service;
- Rust and Python handlers receive real peer/local connection metadata;
- body rejection occurs before user service invocation;
- every advertised incomplete-body policy is implemented or removed;
- every shared resource control has one authoritative owner;
- Python, CLI, and Rust configuration values reach the actual enforcing primitive;
- invalid zero or out-of-range values return structured validation errors rather than panicking or producing zero-capacity deadlocks;
- frontend parity tests cover all shared settings.

## Release C exit criteria

Release C closes only when:

- `/directory/` and `/directory/index.html` use the same response planner for file representations;
- conditional and range behavior is equivalent for both resource forms;
- HEAD transmits no body and preserves corresponding GET status and representation headers;
- HEAD error and directory-listing paths are normalized consistently;
- ETags use sufficiently precise and platform-appropriate identity metadata;
- raw-wire tests cover direct/index, GET/HEAD, conditional, range, body-forbidden, and keep-alive behavior;
- canonical planner tests and production socket tests agree.

## Documentation obligations

The following documents must be updated as behavior changes:

- README and support matrix;
- threat model;
- security policy;
- deployment guide;
- Rust API documentation and examples;
- Python API documentation;
- timeout and request-body policy documentation;
- release criteria and evidence invalidation mappings;
- migration notes for removed or renamed configuration fields.

Documentation must describe actual behavior, not intended future behavior.

## Corrective closure policy

For every discovered follow-up defect:

1. assign a severity and affected support profiles;
2. add a deterministic reproduction where possible;
3. make the smallest scope-preserving correction;
4. add a regression test or corpus case;
5. rerun all invalidated gates;
6. update documentation and migration notes;
7. record the closing commit and evidence;
8. require independent review for critical/high findings.

No critical or high finding may be waived for a release. Medium findings may be deferred only by narrowing the affected profile and documenting the limitation without contradicting public support claims.

## Final handoff state

After Plans 075–083 close, the repository should have:

- safe Windows native-string and handle ownership primitives;
- truthful timeout and shutdown behavior;
- a coherent embedded Rust/Python service contract;
- enforceable request-body policies;
- one validated configuration model;
- unified static-file and directory-index semantics;
- corrected GET/HEAD/conditional/range behavior;
- raw-wire evidence suitable to begin Windows hardened-profile completion.

Release D and Release E should then be planned against the corrected implementation rather than the pre-correction architecture.