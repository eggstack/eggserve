# Phase 73 — Independent Audit and Production-Release Closure

## Goal

Perform an implementation-independent security and release review, remediate or explicitly bound every finding, verify all production-profile evidence on the final source SHA, and authorize only the narrowly defined profiles that satisfy every required gate.

This is the terminal closure phase for the production-grade internet deployment and Windows hardening roadmap. It is not a feature phase.

## Preconditions

- Plans 060–072 are implemented or have explicit evidence-backed exclusions.
- Release criteria identify every profile and required gate.
- Windows profiles are promoted only if dedicated NTFS evidence exists.
- Direct-TLS profiles are promoted only if TLS artifact and abuse evidence exists.
- Soak, installed-artifact, SBOM, checksum, and provenance evidence is available.

## Non-goals

Do not add:

- new protocol features;
- ASGI/WSGI, routing, middleware, or application serving;
- proxying, ACME, HTTP/2, HTTP/3, or virtual hosting;
- new filesystem support merely to avoid documenting a limitation;
- performance optimization unrelated to an audit finding;
- waivers for unresolved high-severity security defects;
- broad branding claims unsupported by profile-specific evidence.

## Audit independence requirement

The review must be performed by a person or agent that did not author the implementation under review and is instructed to challenge both code and evidence.

Independence may be satisfied by:

- external security reviewer;
- separate internal reviewer with no implementation ownership;
- independent review agent with fresh context and explicit adversarial mandate;
- multiple reviewers split by subsystem.

The implementation author may answer questions and implement fixes but should not be the sole party declaring closure.

## Track A — Review scope and evidence package

Prepare a review package containing:

- source SHA and branch/tag;
- roadmap and Plans 060–073;
- architecture documents;
- threat model and security policy;
- production profile matrix;
- release criteria;
- API stability inventory;
- dependency policy;
- test/fuzz corpora;
- race and soak evidence;
- proxy and TLS evidence;
- Windows dedicated-runner evidence;
- artifact checksums/SBOM/provenance;
- prior security findings and corrective commits;
- known limitations.

The reviewer must be able to reproduce critical gates from a clean checkout or installed artifact.

## Track B — Unix confinement review

Review:

- pinned root descriptor lifetime;
- descriptor-relative traversal;
- `statat`/`openat` ordering and flags;
- intermediate and final symlink denial;
- root rename/replacement behavior;
- opened-file identity through metadata/range/streaming;
- index lookup and directory listing;
- safe relative path exposure;
- error cleanup and descriptor leaks;
- mutation/race harness validity.

Attempt to identify:

- validate/reopen gaps;
- alternate serving paths;
- canonicalization fallbacks entering hardened mode;
- race windows;
- path reconstruction by Python or downstream wrappers;
- filesystem class assumptions not represented in support claims.

## Track C — Windows confinement review

Review:

- root handle opening and filesystem classification;
- relative open API and flags;
- reparse suppression and tag inspection;
- deny-all semantics;
- final handle identity and volume checks;
- directory enumeration;
- index lookup;
- range/file streaming conversion;
- unsafe FFI safety comments;
- handle ownership and double-close risks;
- namespace, ADS, reserved-name, trailing-dot/space, case, and short-name behavior;
- dedicated-runner test completeness;
- fallback separation.

Attempt bypasses using:

- file/directory symlinks;
- junctions and mount points;
- unknown reparse tags;
- namespace/device paths;
- concurrent replacement;
- unsupported filesystem roots;
- installed wheel/binary behavior.

Any path-based reopen in the hardened Windows pipeline is a blocking finding.

## Track D — HTTP framing and connection review

Review:

- TE+CL rejection;
- duplicate Content-Length rejection;
- header grammar and limits;
- request-target validation;
- body policy and one-shot ownership;
- drain/close sequencing;
- keep-alive reuse;
- HTTP/1.0 versus HTTP/1.1 persistence;
- response normalization;
- HEAD and body-forbidden statuses;
- range and conditional precedence;
- connection closure after ambiguity;
- proxy desynchronization corpus and origin invocation instrumentation;
- stateful fuzz model/oracles.

Attempt to construct hidden second requests through both qualified proxies and directly.

## Track E — Runtime resource and shutdown review

Review:

- accepted socket admission;
- TLS handshake budget;
- connection semaphore;
- header/body/write/idle deadlines;
- requests-per-connection;
- file/listing/callback budgets;
- task and permit ownership;
- graceful deadline;
- forced shutdown;
- client disconnect behavior;
- repeated start/stop;
- soak trend analysis.

Look for:

- unbounded queues;
- timeout reset abuse;
- detached tasks;
- permits retained after cancellation;
- deadlocks during shutdown initiated from callbacks;
- log amplification;
- memory/handle growth hidden by short tests.

## Track F — TLS review

For direct-TLS profiles, review:

- rustls configuration and protocol versions;
- certificate/key startup validation;
- handshake admission and deadline;
- malformed/stalled handshake behavior;
- plaintext-to-TLS port behavior;
- TLS truncation;
- established HTTP parity;
- shutdown;
- artifact feature set;
- restart-only rotation documentation;
- key-file diagnostics.

Confirm no accidental claims of ACME, virtual hosting, HTTP/2, OCSP, client authentication, or hot reload.

## Track G — Python and FFI review

Review:

- native primitive parity;
- raw pointer/handle ownership;
- GIL release and callback invocation;
- callback timeout semantics;
- unkillable Python execution handling;
- body iterator cancellation/backpressure;
- interpreter shutdown;
- repeated server lifecycle;
- exception mapping;
- installed-wheel behavior;
- source identity of extension and bundled binary;
- public API scope.

Confirm the Python API remains low-level and protocol-neutral. It may enable downstream application servers or clients but must not include ASGI/WSGI or framework semantics.

## Track H — Supply-chain and release review

Review:

- dependency audit and license policy;
- lockfile/toolchain capture;
- SBOM completeness;
- artifact checksums;
- provenance/attestation;
- source/artifact SHA identity;
- wheel bundled-binary freshness;
- installed-artifact tests;
- release workflow permissions;
- evidence maximum age and invalidation;
- release tag/commit selection;
- reproducibility documentation.

Attempt negative cases against the evidence aggregator:

- stale evidence;
- wrong SHA;
- missing platform artifact;
- mismatched feature set;
- skipped dedicated Windows tests;
- missing TLS qualification;
- documentation claiming an unsupported profile.

## Track I — Finding taxonomy

Classify findings:

### Critical/high

Examples:

- root escape;
- request smuggling/desynchronization;
- arbitrary file disclosure;
- unsafe FFI memory/handle ownership;
- unbounded remotely triggerable resource exhaustion;
- release artifact/source mismatch;
- evidence system falsely promoting unsupported profile.

These block release and cannot be waived.

### Medium

Examples:

- bounded denial of service with practical exploitability;
- incomplete platform limitation;
- significant log disclosure/injection;
- shutdown/resource leak requiring sustained attack;
- incorrect status/connection behavior with security implications.

Correct before release or narrow the affected profile with explicit evidence and approval. Prefer correction.

### Low/informational

Examples:

- documentation clarity;
- non-security protocol edge behavior;
- test maintainability;
- conservative hardening suggestion.

May be deferred only with issue/plan reference and no contradiction of current claims.

## Track J — Corrective closure loop

For each finding:

1. Assign identifier, severity, subsystem, and affected profiles.
2. Create deterministic reproduction or explain why external evidence is required.
3. Implement the smallest scope-preserving correction.
4. Add regression test/corpus seed.
5. Rerun invalidated gates.
6. Update threat model/docs if semantics changed.
7. Obtain independent reviewer confirmation.
8. Record closure commit and evidence.

Corrections must not introduce application-server or edge-server scope.

## Track K — Final profile authorization

Evaluate each profile independently.

### Unix reverse-proxy origin

Requires:

- contract/root identity;
- Caddy/nginx interoperability;
- desynchronization qualification;
- lifecycle limits;
- HTTP stateful fuzzing;
- Unix race/fault evidence;
- soak/artifact/provenance;
- audit closure.

### Unix direct HTTPS

Requires Unix reverse-proxy profile plus all direct-TLS gates and TLS artifact evidence.

### Windows reverse-proxy origin

Requires Unix reverse-proxy runtime gates plus Windows local-NTFS confinement, Windows race/fault, installed artifact, and audit closure.

### Windows direct HTTPS

Requires all roadmap gates.

A profile may remain `candidate` while another is released as `supported-hardened`. Do not delay a fully qualified narrow profile solely to avoid publishing an honest support matrix.

## Track L — Release documentation

Publish/update:

- final support matrix;
- exact platform/architecture/filesystem support;
- hardened versus functional-only modes;
- reverse-proxy and direct-TLS deployment guides;
- root replacement/content deployment semantics;
- Windows reparse policy;
- native TLS limitations;
- Python primitive scope;
- non-goals;
- security reporting process;
- patch/backport policy;
- known limitations;
- evidence summary for the release SHA.

Avoid unqualified phrases such as “production ready on all platforms.” Name the profiles.

## Track M — Maintenance and invalidation policy

Define security maintenance after release:

- vulnerability intake and acknowledgement target;
- supported release branches;
- patch/backport criteria;
- dependency update cadence;
- proxy-version requalification;
- Windows OS/filesystem requalification;
- scheduled fuzz and corpus replay;
- scheduled short soaks;
- full pre-release soaks;
- artifact provenance retention;
- evidence expiration.

Map code paths to invalidated gates:

- path parser/filesystem changes → confinement/race gates;
- Windows FFI changes → dedicated Windows gates/audit;
- Hyper/body/connection changes → direct/proxy framing and stateful fuzz;
- rustls changes → direct-TLS gates;
- PyO3 changes → Python lifecycle/installed wheel;
- packaging/workflow changes → artifact/provenance gates;
- docs/profile metadata changes → contract consistency.

## Required final verification

On the exact proposed release SHA:

- run full release validation;
- aggregate evidence fail-closed;
- verify every required artifact hash;
- verify no required job is skipped;
- verify dedicated Windows evidence freshness;
- verify proxy and TLS version evidence;
- verify 24-hour soak evidence;
- verify independent review closure;
- install artifacts in clean environments and run final smoke;
- inspect generated release checklist manually;
- compare public documentation with machine-readable support status.

## Acceptance criteria

- No unresolved critical/high findings.
- Every medium finding is corrected or narrowly dispositioned without overstating support.
- Independent reviewer confirms closure.
- Every authorized profile has complete current evidence on the final SHA.
- Unsupported profiles remain clearly functional-only, candidate, or unsupported.
- Published artifacts match source, feature set, checksums, SBOM, and provenance.
- Maintenance and evidence-invalidation policy is operational.
- Scope firewall remains intact.

## Stop conditions

Do not release or promote an affected profile if:

- any high-severity finding remains;
- required evidence is missing, stale, skipped, or tied to another SHA;
- Windows privileged/reparse evidence is absent for a Windows claim;
- direct-TLS artifact/abuse evidence is absent for a direct-TLS claim;
- proxy desynchronization evidence is incomplete;
- soak shows unresolved growth/degradation;
- artifacts cannot be traced to the reviewed source;
- documentation claims more than the gates authorize.

## Final handoff state

When this phase closes, the repository should contain:

- a profile-specific production support contract;
- hardened Unix and, where evidence permits, Windows static confinement;
- qualified reverse-proxy and optional direct-TLS deployment paths;
- bounded connection/resource lifecycle;
- fixed and fuzzed HTTP/1 conformance evidence;
- filesystem race/fault evidence;
- installed-artifact and provenance evidence;
- independent audit closure;
- explicit maintenance policy.

No downstream application server, HTTP client product, ASGI/WSGI adapter, reverse proxy, or framework implementation is required for roadmap completion.
