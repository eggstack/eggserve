# Corrective Roadmap — Runtime, Windows, and HTTP Correctness

## Purpose

This roadmap closes the correctness defects identified after implementation of the production-hardening roadmap through Plan 073. It is a corrective program, not a feature expansion.

The work preserves eggserve's existing scope: a hardened static file server and reusable static-serving/runtime primitive. It must not turn eggserve into an application framework, reverse proxy, general edge server, middleware platform, or protocol gateway.

The corrective program is divided into five releases. Detailed execution files now exist for the full sequence, Plans 075–089. Releases D and E absorb the still-relevant Windows, internet-deployment, soak, artifact, and release-qualification obligations from the earlier production-hardening roadmap after the corrected runtime and HTTP contracts stabilize.

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
17. reverse-proxy, native TLS, stateful fuzz, soak, installed-artifact, provenance, and independent-review evidence remain incomplete for final production-profile claims.

## Governing invariants

All corrective work must preserve these invariants:

- every public configuration field has one authoritative owner;
- public APIs do not silently discard supplied values;
- rejected requests do not invoke user code;
- lifecycle state reflects actual task and resource ownership;
- timeout names match their enforcement semantics;
- hardened filesystem access remains handle-relative from pinned root to the final opened object;
- logically equivalent resource paths share one HTTP response planner;
- directory enumeration metadata is never authority to reopen by path;
- performance work does not weaken protocol, confinement, body, or shutdown guarantees;
- installed-artifact evidence is distinct from source-tree evidence;
- support claims remain profile-specific and evidence-backed;
- application serving, proxying, and edge-platform responsibilities remain downstream or out of scope.

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

Release D preserves directory handles through final child resolution and directory enumeration, completes handle-relative Windows index lookup, removes path-based authority from hardened enumeration, and qualifies local NTFS against reparse points, races, namespace ambiguity, resource exhaustion, and installed-artifact behavior.

Plans:

- Plan 084 — Windows directory-handle retention and child resolution
- Plan 085 — Windows handle-relative directory enumeration
- Plan 086 — Windows adversarial filesystem qualification

Windows hardened support must not be promoted before Release D closes. `windows-direct-https` also remains blocked on the common direct-TLS and release qualification in Release E.

### Release E — Operational, performance, internet, and release closure

Release E implements truthful structured logging, removes unconditional library printing, classifies listener and streaming failures, optimizes measured allocation/streaming costs without weakening semantics, and performs the integrated proxy, TLS, fuzz, race, soak, artifact, provenance, independent-review, and profile-decision closure.

Plans:

- Plan 087 — Structured logging and operational error closure
- Plan 088 — Streaming allocation and buffer performance qualification
- Plan 089 — Production soak, internet, artifact, and release closure

Plan 089 is the terminal qualification gate. It absorbs the outstanding production evidence obligations of Plans 066–073 where those plans were not independently implemented and closed.

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
9. Plan 084 begins only after Plan 083 closes and retains Windows directory authority through child/index resolution.
10. Plan 085 depends on Plan 084's retained directory handles and implements handle-based enumeration.
11. Plan 086 depends on Plans 084–085 and performs end-to-end Windows adversarial qualification and profile decisions.
12. Plan 087 begins after corrected runtime/filesystem semantics are stable and defines truthful operational events and error behavior.
13. Plan 088 depends on Plan 087 counters/events and performs evidence-driven optimization only after Releases A–D are stable.
14. Plan 089 depends on all prior plans and freezes one release-candidate SHA for proxy, TLS, fuzz, race, soak, artifact, provenance, independent-review, and profile qualification.
15. Any code-affecting fix during Plan 089 invalidates and reruns the mapped evidence for the new SHA.

Parallel implementation is allowed only where affected modules do not overlap and the merge order preserves these dependencies. Performance work must not begin before semantics and ownership are stable.

## Scope firewall

The corrective roadmap must not add:

- ASGI or WSGI support;
- routing, middleware, sessions, templates, authentication, or application frameworks;
- reverse proxying or caching proxy behavior;
- ACME, certificate renewal, virtual hosts, multi-certificate routing, OCSP, HTTP/2, or HTTP/3;
- per-user or distributed rate limiting;
- a metrics/admin network endpoint;
- remote log shipping or a telemetry platform;
- speculative platform support intended only to broaden marketing claims;
- performance changes without benchmark and correctness evidence.

API breakage is acceptable before 1.0 when required to remove misleading or unimplementable contracts. Compatibility shims must not retain incorrect behavior.

The Rust and Python primitives may remain sufficient for downstream projects to construct clients, application servers, or adapters. Those downstream products are not release deliverables or supported application-serving modes of eggserve.

## Evidence policy

Each plan must produce evidence tied to the exact implementation commit. Evidence must include:

- command or workflow identity;
- source SHA;
- platform and architecture;
- enabled features;
- test or benchmark result;
- artifact identity when testing installed binaries or wheels;
- environment/fixture capabilities for privileged Windows tests;
- known exclusions and their rationale.

A plan is not complete merely because code was written. Its acceptance criteria and invalidated release gates must pass.

Evidence fails closed when it is:

- missing;
- stale;
- produced from another SHA;
- marked skipped where a required platform fixture was unavailable;
- produced from a source tree instead of a required installed artifact;
- inconsistent with support-profile metadata;
- accompanied by an open blocking finding.

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

## Release D exit criteria

Release D closes only when:

- Windows `ResolvedDirectory` retains an owned directory handle;
- child and index resolution remain handle-relative from the retained directory;
- directory enumeration operates from the validated handle and parses variable-length records safely;
- selected entries are reopened and validated relative to the same directory handle;
- all available reparse classes are denied by default;
- Unicode, namespace, 8.3 alias, ACL/sharing, and race matrices pass on dedicated Windows environments;
- zero bytes are served outside the pinned root during race testing;
- handle/task/permit counts remain bounded;
- installed Windows artifacts use the same hardened resolver;
- independent Windows FFI/unsafe review has no unresolved high or critical finding;
- Windows profile status is updated only from exact-SHA evidence.

## Release E exit criteria

Release E closes only when:

- JSON logging is valid JSON Lines derived from one event model;
- text logging is sanitized and library embedding is silent by default;
- persistent listener errors use bounded backoff and cannot spin or flood logs;
- streaming and response errors are categorized without invalid second responses;
- performance changes are supported by reproducible cross-platform measurements;
- buffers/pools are bounded and cannot leak stale cross-request data;
- Caddy and nginx origin tests show no request desynchronization;
- promoted direct HTTPS profiles pass native TLS abuse and resource tests;
- stateful live-socket fuzzing completes the release budget;
- cross-platform filesystem race suites serve zero outside-root bytes;
- long-duration soak shows bounded memory, descriptors/handles, tasks, permits, and shutdown;
- installed binaries and wheels pass critical production-path tests;
- checksums, SBOM, and provenance bind artifacts to the exact release SHA;
- independent final security review has no unresolved high or critical finding;
- every production support profile has an explicit evidence-backed decision.

## Documentation obligations

The following documents must be updated as behavior changes:

- README and support matrix;
- threat model;
- security policy;
- deployment and TLS guides;
- Rust API documentation and examples;
- Python API documentation;
- timeout and request-body policy documentation;
- Windows filesystem ADR and confinement architecture;
- logging/operations and performance methodology;
- release criteria and evidence invalidation mappings;
- migration notes for removed or renamed configuration fields;
- release notes and operator runbook.

Documentation must describe actual behavior, not intended future behavior. “Production ready” must always be qualified by the named support profile.

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

After Plans 075–089 close, the repository should have:

- safe Windows native-string and handle ownership primitives;
- truthful timeout and shutdown behavior;
- a coherent embedded Rust/Python service contract;
- enforceable request-body policies;
- one validated configuration model;
- unified static-file and directory-index semantics;
- corrected GET/HEAD/conditional/range behavior;
- complete Windows handle-relative child and enumeration paths;
- adversarial Windows filesystem evidence and explicit profile decisions;
- truthful structured logging and bounded operational errors;
- measured, bounded streaming performance;
- reverse-proxy and native TLS qualification where claimed;
- stateful fuzz, filesystem race, soak, artifact, provenance, and independent-review evidence;
- a frozen release candidate whose installed artifacts are traceable to the exact source SHA.

The terminal product remains a narrow, read-only HTTP/1.1 static file server and reusable protocol-neutral primitive library. Application servers, clients, ASGI/WSGI adapters, proxies, and edge platforms remain downstream or out of scope.