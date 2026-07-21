# Plan 089 — Production Soak, Internet, Artifact, and Release Closure

## Goal

Perform the final integrated qualification required to call eggserve production grade within its narrowly defined static-serving profiles. This plan combines long-duration resource testing, reverse-proxy desynchronization testing, native TLS qualification, stateful live-socket fuzzing, installed-artifact and provenance verification, independent security review, and profile-specific release decisions.

This plan closes Release E and the production-readiness roadmap. It does not expand eggserve into an edge server, proxy, application server, certificate platform, or framework.

## Preconditions

- Plans 075–088 are closed with exact-SHA evidence.
- Windows hardened-profile work in Plans 084–086 is complete or Windows profiles remain explicitly unpromoted.
- Operational events/counters are truthful and bounded.
- Performance baselines and resource thresholds are defined.
- The release aggregator fails closed for stale, skipped, missing, or wrong-artifact evidence.

## Non-goals

Do not add:

- ASGI/WSGI hosting;
- middleware, routing, sessions, authentication, or application lifecycle;
- reverse proxy forwarding;
- ACME, certificate renewal, virtual hosts, multi-certificate routing, OCSP, HTTP/2, or HTTP/3;
- a metrics/admin endpoint;
- distributed rate limiting;
- speculative platform support;
- production claims outside the explicitly qualified profiles.

## Track A — Freeze the release-candidate source and evidence identity

Select one release-candidate commit and freeze it for qualification.

Record:

- source SHA;
- version;
- Cargo.lock hash;
- Rust toolchain;
- Python and maturin/build toolchain;
- generated-file state;
- support-profile metadata hash;
- release-criteria hash;
- expected artifact matrix;
- any permitted documentation-only follow-up rule.

No code change may reuse evidence from the prior SHA. If a code-affecting change lands, restart every invalidated qualification class.

Create a release-candidate status file showing:

- each profile;
- required gates;
- evidence SHA;
- artifact identities;
- open findings;
- independent reviewer status;
- promotion decision.

## Track B — Reverse-proxy origin interoperability

Qualify the primary public deployment profile behind at least Caddy and nginx. A third proxy or managed load balancer may be included if reproducible.

Test topology:

```text
hostile client -> proxy TLS/public listener -> HTTP/1.1 loopback origin -> eggserve
```

Requirements:

- origin binds only to loopback/private address;
- proxy terminates public TLS;
- origin protocol and connection reuse are explicit;
- no dependency on `Forwarded` or `X-Forwarded-*` trust inside eggserve;
- client identity comes from edge logs, not implicit origin parsing;
- proxy and origin timeouts are documented;
- origin port exposure is tested/checked.

### Desynchronization corpus

Send through each proxy:

- Transfer-Encoding plus Content-Length;
- duplicate identical and conflicting Content-Length;
- comma-combined lengths;
- malformed chunk sizes and terminators;
- chunk extensions and trailers according to supported policy;
- obsolete folding;
- whitespace before colon/field values;
- bare LF/CR;
- oversized headers;
- invalid method and target forms;
- hidden second requests after malformed bodies;
- pipelined valid/malformed/valid sequences;
- premature EOF;
- body-forbidden methods carrying bodies.

Observe not only the client status but also:

- whether eggserve service code was invoked;
- bytes received at origin;
- connection reuse/closure at proxy and origin;
- whether a hidden trailing request was processed;
- correlation IDs across proxy-origin attempts.

Any frontend/backend disagreement that permits request smuggling or cross-request confusion blocks the profile.

## Track C — Direct native TLS qualification

Qualify the intentionally limited direct HTTPS profile separately.

Requirements:

- TLS 1.2/1.3 policy documented;
- separate handshake concurrency and timeout where implemented;
- incomplete/stalled handshake tests;
- handshake flood under connection limits;
- malformed TLS records;
- certificate/key mismatch startup failure;
- empty/malformed chain failure;
- unsupported encrypted key failure;
- certificate rotation documented as restart-required;
- shutdown during handshake and established response;
- client abort/truncation;
- large-file and range streaming over TLS;
- no plaintext accepted on the TLS listener;
- ALPN behavior explicit and does not imply HTTP/2.

Native TLS remains a single-origin HTTP/1.1 feature. Missing ACME, virtual hosting, OCSP, client certificates, and HTTP/2 are documented non-goals, not release defects.

## Track D — Stateful live-socket fuzzing

Run a state-machine fuzzer against a real server process or embedded runtime.

Generated actions should include:

- open connection;
- send request fragments;
- pause beyond timeouts;
- send malformed and valid requests in sequence;
- pipeline requests;
- half-close;
- disconnect during read/write;
- consume part of a request body;
- trigger service return with incomplete body;
- request ranges and conditionals;
- initiate shutdown;
- reconnect;
- TLS handshake fragments where applicable.

Assertions:

- no panic or abort;
- no response splitting;
- no cross-request body contamination;
- no handler invocation for rejected requests;
- no parsing of a second request after ambiguous framing;
- no connection reuse after failed drain/framing;
- no permit/task/handle leak;
- no unbounded allocation;
- bounded shutdown.

Persist minimized failing sequences as regression corpus entries.

Run deterministic corpus replay in regular CI and larger fuzz budgets on schedule and before release.

## Track E — Cross-platform filesystem race qualification

Run the final race suites on:

- Linux supported local filesystem;
- macOS supported local filesystem;
- Windows local NTFS when the profile is under consideration.

Use stable identity and content digest allowlists. Exercise:

- file ↔ symlink/reparse replacement;
- directory ↔ symlink/junction replacement;
- parent replacement;
- root pathname replacement;
- index replacement;
- listing churn;
- file truncation/replacement during full/range streaming;
- permission changes;
- deletion and recreation.

Acceptance:

- zero outside-root bytes served;
- zero denied-object bytes served;
- safe opened-version or documented error only;
- no mixed response body from two identities;
- no path leakage;
- resources return to baseline.

## Track F — Long-duration mixed-traffic soak

Run at least a 24-hour release-candidate soak per primary production environment. Longer scheduled runs are desirable.

### Traffic mix

- small/medium/large files;
- full and range GET;
- HEAD;
- conditionals and 304;
- missing and denied paths;
- directory index;
- optional listing within qualified limits;
- keep-alive reuse;
- connection churn;
- slow headers;
- slow readers;
- malformed framing;
- TLS handshakes where applicable;
- Python callback/body paths for profiles that claim them;
- periodic graceful restart/shutdown cycles.

### Metrics

- RSS/working set;
- allocator metrics where available;
- file descriptor/handle count;
- tasks/threads;
- active connections;
- file/listing/body/callback permits;
- listener errors;
- timeout categories;
- bytes sent;
- request latency percentiles;
- CPU;
- socket states;
- log queue/drop counts;
- shutdown duration and forced count.

Define baseline-relative pass criteria before the run. No monotonic resource growth, unexplained task accumulation, permit loss, or shutdown deadline violation is acceptable.

## Track G — Fault injection and degraded environments

Exercise:

- file descriptor/handle exhaustion;
- memory pressure within safe test limits;
- log sink failure;
- read-only/unreadable roots;
- file read errors after response start;
- listener persistent errors;
- TLS handshake errors;
- blocking-worker saturation;
- Python callback exception/stall;
- forced shutdown under saturation;
- full or unavailable artifact/evidence destination where tooling applies.

Required behavior:

- no panic;
- no tight loop;
- errors categorized and rate-limited;
- future healthy requests recover where possible;
- fatal conditions terminate with a truthful result;
- process does not claim stopped while owned work remains.

## Track H — Installed artifact matrix

Build and test the actual release artifacts:

### Rust

- `eggserve-core` package dry run;
- `eggserve-bin` package dry run;
- standalone binaries for claimed targets where published;
- default and TLS-enabled artifact distinction.

### Python

- wheels for each claimed CPython/platform target;
- clean-environment install;
- CLI invocation;
- `python -m eggserve`;
- native primitives;
- in-process server;
- subprocess server;
- lifecycle and shutdown;
- static confinement and critical wire subset;
- uninstallation/upgrade smoke where relevant.

For each artifact capture:

- SHA-256;
- source SHA embedded or associated;
- target triple/platform tag;
- feature set;
- toolchain;
- test environment;
- signature/attestation where used.

Source-tree success does not substitute for installed-artifact evidence.

## Track I — Supply chain, SBOM, and provenance

Produce:

- cargo-audit result;
- cargo-deny/license policy result;
- Python dependency/build inventory;
- SBOM for release artifacts;
- checksums;
- GitHub artifact attestation or equivalent provenance;
- source archive identity;
- reproducibility notes;
- release workflow dry run.

The provenance record must bind artifact hashes to the exact release-candidate SHA. Missing or mismatched provenance blocks release.

## Track J — Independent security review

Require independent review of the final candidate, not only earlier component reviews.

Review scope:

- Unix descriptor-relative confinement and pinned root;
- Windows handle-relative confinement if promoted;
- HTTP framing and request smuggling;
- body rejection/drain/connection reuse;
- canonical response normalization, GET/HEAD/range/conditional behavior;
- timeout and shutdown ownership;
- TLS configuration/admission;
- Python FFI, callbacks, iterators, and lifecycle;
- logging privacy/injection;
- artifact/provenance/release gates.

Findings policy:

- critical/high: must be fixed and invalidated gates rerun;
- medium: fix or narrow the affected profile with explicit documentation;
- low: may defer with owner and rationale;
- no finding may be hidden by changing test expectations without protocol/security justification.

Store reviewer identity, scope, source SHA, findings, dispositions, and closure evidence.

## Track K — Support-profile decisions

Decide each profile independently.

### `unix-reverse-proxy`

Promote/retain `supported-hardened` only if proxy interop, filesystem races, soak, artifacts, and review pass on the exact SHA.

### `unix-direct-https`

Promote from `candidate` only if native TLS, direct internet runtime, soak, artifacts, and review pass. Otherwise remain candidate while reverse-proxy remains the recommended public deployment.

### `windows-reverse-proxy`

Promote only if Plan 086 and the common proxy/soak/artifact/review gates pass.

### `windows-direct-https`

Promote only if both Windows hardened and native TLS/direct internet qualifications pass.

### `local-development`

Retain support based on ordinary cross-platform gates; do not let this broad low-risk profile imply public internet hardening.

### Functional/compatibility profiles

Keep SMB/non-NTFS/cloud and link-following configurations explicitly outside hardened claims.

`release/support-profiles.toml` is the source of truth. Generated documentation must match it.

## Track L — Release operator runbook

Create a concise runbook covering:

- freeze candidate;
- execute required workflows;
- verify exact-SHA evidence;
- inspect soak/fuzz/race artifacts;
- confirm findings closed;
- verify artifact hashes/provenance;
- update support profiles;
- human approval;
- tag/release/publish order;
- post-release smoke tests;
- rollback/yank procedure;
- security advisory process.

No manual step may silently mark a missing required gate as passed.

## Required release gates

At minimum include grouped gates for:

- proxy Caddy interop;
- proxy nginx interop;
- proxy desynchronization corpus;
- native TLS abuse/limits;
- stateful live-socket fuzz replay and release budget;
- Unix filesystem race;
- Windows filesystem race where promoted;
- 24-hour soak per promoted production profile;
- fault injection;
- installed binaries;
- installed wheels;
- SBOM/checksums/provenance;
- independent review;
- profile decision;
- human approval.

The aggregator must fail closed if evidence is:

- missing;
- stale;
- from another SHA;
- from a source build instead of required artifact;
- skipped due to unavailable fixtures;
- incomplete for a promoted platform;
- accompanied by open blocking findings.

## Documentation changes

Update atomically:

- README status/profile table;
- deployment guide;
- TLS guide;
- security policy;
- threat model;
- release contract;
- operations/logging and performance docs;
- Windows ADR and platform limitations;
- Python/Rust support statements;
- release checklist;
- support profiles;
- changelog/release notes;
- security reporting policy.

Use qualified language. Do not say simply “production ready” without naming the supported profile boundary.

## Acceptance criteria

- One frozen release-candidate SHA has complete evidence.
- Caddy and nginx origin tests show no request desynchronization.
- Native TLS passes abuse/resource tests for any promoted direct profile.
- Stateful fuzzing completes the release budget with no unresolved failure.
- Cross-platform race suites serve zero outside-root bytes.
- Long-duration soak shows bounded resources and truthful shutdown.
- Fault injection produces no panic, spin, or false lifecycle completion.
- Installed artifacts pass the critical production-path suites.
- Checksums, SBOM, and provenance bind artifacts to source SHA.
- Independent review has no unresolved high/critical findings.
- Every support-profile status is evidence-backed and machine-readable.
- Scope non-goals remain intact.

## Stop conditions

Do not release or promote an affected profile if:

- CI/evidence cannot prove exact-SHA identity;
- a required platform or privileged fixture is unavailable;
- soak shows monotonic resource growth;
- proxy/origin parsers disagree on framing;
- a race serves outside-root content;
- native TLS admission is unbounded;
- installed artifact behavior differs from source;
- independent review is incomplete;
- any high/critical finding remains open.

## Final handoff state

When this plan closes, eggserve should be releasable as a production-grade read-only HTTP/1.1 static file server only for the profiles explicitly marked `supported-hardened`. The project remains deliberately out of scope for application serving, proxying, certificate automation, HTTP/2+, and framework responsibilities.