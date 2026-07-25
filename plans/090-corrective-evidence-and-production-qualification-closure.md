# Plan 090 — Corrective Evidence and Production Qualification Closure

## Goal

Close the remaining correctness and release-evidence defects after implementation of Plans 075–089 without expanding eggserve beyond its intended scope.

This plan must reconcile the repository's implementation-complete claims with its still-pending qualification evidence, eliminate the remaining panic-capable Windows handle duplication path, make required gates genuinely fail closed, execute Windows and production-profile qualification in environments capable of exercising the required fixtures, and produce one exact-SHA release decision for every support profile.

Completion of this plan means:

- implementation status and qualification status are represented separately and truthfully;
- no required gate can pass through an ignored test, fixture early return, warning-only command, missing artifact, stale SHA, or unexecuted harness;
- Windows hardened qualification is performed on an appropriate NTFS environment with required reparse fixtures;
- reverse-proxy and direct-TLS soak tests exercise their real deployment topologies;
- all promoted profiles are backed by complete exact-SHA evidence;
- profiles lacking complete evidence remain explicitly candidate or functional;
- the final release report contains no unresolved critical or high findings.

This is a corrective closure pass. It must not add framework, proxy, application-server, or edge-platform functionality.

## Preconditions

- Plans 075–089 have been implemented on `main`.
- The current support profile source of truth is `release/support-profiles.toml`.
- The current release gate source of truth is `release/criteria.toml`.
- The current corrective finding registry is `release/corrective-findings.toml`.
- The current generated checklist is `docs/release-checklist.md`.
- Existing candidate statuses must remain unchanged until this plan's evidence gates pass.

Before implementation begins, record the starting SHA and hashes of:

- `Cargo.lock`;
- `release/criteria.toml`;
- `release/support-profiles.toml`;
- `release/corrective-findings.toml`;
- `.github/workflows/ci.yml`;
- the Plan 086 Windows qualification suite;
- the Plan 089 proxy and soak harnesses.

## Scope firewall

Do not add:

- ASGI or WSGI support;
- routing, middleware, templates, sessions, authentication, or application lifecycle;
- reverse-proxy forwarding inside eggserve;
- ACME, certificate renewal, virtual hosting, HTTP/2, HTTP/3, WebSockets, or edge-platform features;
- a custom HTTP parser solely to broaden downstream application-server claims;
- a network metrics or administration endpoint;
- distributed coordination or rate limiting;
- support claims for SMB, non-NTFS, cloud-placeholder, or link-following modes;
- performance changes unrelated to a demonstrated regression or qualification blocker.

The preferred correction for claims that cannot be enforced within eggserve's narrow scope is to narrow and document the claim, not to grow a new subsystem.

## Governing invariants

The implementation must preserve these invariants:

1. A plan may be implementation-complete while release evidence remains pending.
2. A required release gate passes only when executable evidence for the exact candidate SHA exists.
3. A skipped, ignored, blocked, warning-only, unavailable, or stale required gate is not a pass.
4. Human approval gates require an explicit approval record tied to the exact candidate SHA.
5. Windows hardened claims require handle-relative behavior and adversarial tests on local NTFS.
6. No fallible operating-system resource duplication is converted into a process panic.
7. Support-profile documentation must be generated from or validated against `release/support-profiles.toml`.
8. Production soak evidence must exercise the topology named by the profile.
9. Raw-wire guarantees must describe what eggserve can actually observe and enforce.
10. No profile is promoted by changing a status field before all required gates pass.

## Track A — Establish a truthful implementation/evidence state model

Replace the current binary open/closed presentation with separate fields for implementation and evidence.

### Required state dimensions

For every plan and finding, represent at least:

- `implementation_status`: `not-started`, `in-progress`, `implemented`, or `reopened`;
- `evidence_status`: `missing`, `blocked`, `stale`, `partial`, `passed`, or `not-required`;
- `implementation_sha`;
- `evidence_sha`;
- `required_gates`;
- `blocking_reason` when evidence is not passed;
- `profile_impact`;
- `review_status` where independent review is required.

A finding is fully closed only when both implementation and all required evidence are complete. Infrastructure existence alone is not closure evidence.

### Files to update

At minimum:

- `release/corrective-findings.toml`;
- `release/corrective-status.md`;
- the release-status/report generator;
- contract-consistency checks;
- release documentation that currently states Plans 075–089 are wholly closed.

Correct closure SHAs for Plans 084–089 must reference the actual implementation commits rather than the earlier Plan 075 baseline commit.

COR-017 must be reopened or marked `implementation_status = "implemented"` and `evidence_status = "partial"` until proxy, TLS, soak, installed-artifact, provenance, review, and profile-decision evidence is complete.

### Acceptance criteria

- The dashboard cannot say "all corrective work closed" while required gates are pending.
- Every finding has an implementation SHA corresponding to the commit that actually corrected it.
- Every finding requiring qualification has an evidence state independent of implementation state.
- Generated status output distinguishes implementation completion from production qualification.
- Contract-consistency tests fail when a finding is marked fully closed but a required gate is missing, stale, blocked, or failed.
- Documentation contains no blanket "Plans 000–089 are all complete" statement unless qualified as implementation-only.

## Track B — Remove the remaining panic-capable pinned-root clone path

`OwnedHandle::try_clone()` is fallible, but `PinnedRoot::clone()` currently calls it through `expect(...)`. Remove this contradiction.

### Required design

Prefer one of these narrow designs, in order:

1. Remove `Clone` from `PinnedRoot` and share it through `Arc<PinnedRoot>` wherever shared ownership is needed.
2. Add an explicit `PinnedRoot::try_clone() -> io::Result<PinnedRoot>` and update all callers to propagate failure.
3. If an infallible clone is demonstrably required by a public trait, redesign the owning type so operating-system handle duplication does not occur in that trait implementation.

Do not retain a panic, abort, silent fallback to pathname reopening, or invalid handle sentinel on duplication failure.

Audit Unix descriptor duplication in the same path. A fallible `try_clone()` result must not be converted to panic there either.

### Required tests

- Windows handle-duplication failure returns a typed error.
- Unix descriptor-duplication failure returns a typed error where practically injectable.
- No root handle or descriptor is closed twice.
- The original pinned root remains usable after a failed duplication attempt.
- Server construction/startup reports the error without entering a partially running lifecycle state.
- No fallback path reopens the configured root pathname after duplication failure.

### Acceptance criteria

- No `expect`, `unwrap`, or panic remains in pinned-root descriptor/handle duplication paths.
- `PinnedRoot` ownership and duplication semantics are documented accurately.
- Failure injection demonstrates clean error propagation and resource return to baseline.
- Windows and Unix hardened paths remain handle/descriptor relative.

## Track C — Make required evidence aggregation fail closed

Upgrade the evidence aggregator from artifact collection to profile-aware release validation.

### Evidence record requirements

Every gate evidence record must include:

- gate ID;
- status: `passed`, `failed`, `blocked`, or `skipped`;
- source SHA;
- workflow run ID and job ID;
- platform and architecture;
- feature set;
- command identity;
- start/end timestamps;
- artifact identity where applicable;
- exit status;
- fixture capability metadata;
- evidence producer version/schema;
- reason for any non-pass state.

### Aggregator behavior

Given a candidate SHA and one or more profiles, the aggregator must:

1. load each profile's required gates from `release/support-profiles.toml`;
2. require exactly matching evidence for the candidate SHA;
3. reject stale or unknown-schema evidence;
4. reject `blocked`, `skipped`, warning-only, missing, or failed required gates;
5. reject source-tree evidence where installed-artifact evidence is required;
6. reject evidence for the wrong feature set, platform, architecture, or artifact hash;
7. reject open critical/high findings;
8. require explicit approval records for human gates;
9. emit a deterministic machine-readable candidate report;
10. exit nonzero when any requested profile is not promotable.

Warnings must not transform a failing required command into a successful job.

### Required tooling

Provide commands equivalent to:

```sh
python3 scripts/release-status.py candidate \
  --sha "$CANDIDATE_SHA" \
  --profile unix-reverse-proxy \
  --evidence-dir evidence-artifacts
```

and:

```sh
python3 scripts/release-status.py validate-all \
  --sha "$CANDIDATE_SHA" \
  --evidence-dir evidence-artifacts
```

The exact CLI may differ, but it must support deterministic local and CI use.

### Required tests

Fixtures must cover:

- all gates passed;
- one missing gate;
- one blocked fixture;
- one ignored/skipped gate;
- one wrong SHA;
- one wrong artifact hash;
- one wrong platform;
- one stale record;
- one required human approval missing;
- one open high finding;
- an optional gate missing;
- a candidate profile retained without promotion.

### Acceptance criteria

- The aggregate job fails when any required gate is not a verified pass.
- `|| echo warning`, `continue-on-error`, or equivalent cannot satisfy a required gate.
- The generated candidate report lists every required gate and its evidence file.
- Evidence from a prior code SHA is rejected after any invalidating source change.
- The generated release checklist/report can be populated from evidence without manually editing status cells.

## Track D — Correct Windows qualification fixture semantics

Separate ordinary cross-platform CI coverage from hardened Windows qualification.

### Required execution modes

Define two explicit modes:

1. **Standard Windows CI** — compile, unit, parser, ordinary handle-relative, wheel, and non-privileged tests.
2. **Windows qualification** — dedicated ephemeral or resettable Windows x86_64 VM/runner on local NTFS with Developer Mode and privileges required to create symlinks, junctions, mount-point/reparse fixtures, sharing conflicts, and race scenarios.

The qualification mode must be machine-detectable, for example through an environment variable and capability preflight.

### Capability preflight

Before tests begin, verify and record:

- Windows version/build;
- NTFS filesystem;
- local volume, not SMB/network/cloud-placeholder;
- Developer Mode or symlink privilege;
- junction creation capability;
- reparse-point query capability;
- ability to run installed binary/wheel tests;
- process handle-count observation;
- sufficient privileges for required fixtures;
- clean/resettable test root.

If a required capability is unavailable, the qualification gate must be `blocked` and fail profile qualification. It must not report a test pass.

### Test behavior changes

- Replace required `#[ignore]` cases with qualification-mode execution.
- Replace early `return` on fixture failure with a typed `blocked-fixture` test failure or evidence result.
- Ordinary CI may exclude privileged tests, but it must not emit passing evidence for the corresponding qualification gates.
- Each gate must map to a concrete test selection rather than the whole scaffold merely compiling.

### Required Windows gate coverage

At minimum:

- file and directory symlinks;
- junctions and mount-point reparse tags;
- dangling and unknown reparse tags where safely constructible;
- intermediate, final, index, and listing reparse behavior;
- root rename and pathname replacement;
- component and parent mutation races;
- 8.3 aliases where enabled;
- ADS and reserved namespaces;
- trailing dot/space normalization;
- case-insensitive Unicode names;
- validator identity under replacement;
- sharing violations and permission changes;
- directory enumeration buffer edge cases;
- handle/resource stability;
- installed binary and installed wheel paths.

### Acceptance criteria

- Required Windows qualification tests run on a dedicated NTFS environment.
- No required Windows gate is satisfied by a skipped, ignored, or early-return test.
- Blocked fixtures are visible as blocked evidence and prevent profile promotion.
- Zero outside-root or reparse-target bytes are served across the adversarial matrix.
- Handles return to baseline after success, denial, failure, race, and shutdown cases.
- Windows profile promotion remains impossible until independent review and profile-decision records are also present.

## Track E — Restore nginx as a real blocking interoperability gate

`proxy.nginx-interop` is currently required by the Unix reverse-proxy profile but the workflow converts its failure to a warning. Correct this inconsistency.

### Required harness behavior

Run nginx without relying on systemd:

- use a temporary prefix and generated configuration;
- use explicit pid, error log, access log, and temporary directories;
- run `nginx -t` before launch;
- run foreground or controlled daemon mode;
- poll the listener and process health deterministically;
- capture complete startup and shutdown diagnostics;
- pin or record the tested nginx package/version;
- terminate and reap the process reliably.

The Caddy and nginx tests must exercise equivalent origin behavior and the same core desynchronization corpus where applicable.

### Failure policy

- Remove warning-only success behavior.
- A failed nginx launch, failed interop assertion, unavailable binary, or incomplete test is a failed/blocked required gate.
- If maintainers intentionally stop requiring nginx, remove it atomically from the profile, release criteria, README, deployment documentation, and Plan 089 claims. Do not leave it both required and non-blocking.

The preferred outcome is a reliable blocking nginx gate.

### Acceptance criteria

- `proxy.nginx-interop` exits nonzero on any failure.
- The workflow job cannot pass after the nginx command fails.
- Startup diagnostics identify config, process, port, and log state.
- Caddy and nginx both pass normal static, HEAD, range, conditional, keep-alive, body rejection, and desynchronization cases.
- The reverse-proxy profile cannot be promoted with either required proxy gate missing.

## Track F — Make soak tests exercise the named production topology

The existing soak script labels profiles but starts only direct plaintext eggserve. Split it into actual profile-specific topologies.

### `unix-reverse-proxy` topology

Run:

```text
mixed/hostile clients -> Caddy or nginx edge -> loopback HTTP/1.1 eggserve origin
```

Requirements:

- edge and origin processes monitored independently;
- origin not directly exposed beyond the test namespace/loopback;
- proxy connection reuse and timeout policy recorded;
- TLS termination at the edge where the production profile requires it;
- proxy and origin resource metrics captured;
- proxy restart and origin restart exercised separately;
- malformed/desync corpus periodically replayed without contaminating subsequent requests.

### `unix-direct-https` topology

Run:

```text
mixed/hostile TLS clients -> eggserve native rustls listener
```

Requirements:

- actual certificate/key pair generated or provisioned;
- TLS 1.2 and 1.3 behavior recorded according to policy;
- handshake stalls, malformed records, plaintext-on-TLS, and client aborts included;
- file/range/conditional/HEAD traffic occurs over TLS;
- TLS listener shutdown and restart included.

### Duration and scheduling

- Full qualification duration: at least 24 uninterrupted hours per promoted production profile.
- Provide a short smoke mode for ordinary CI that does not satisfy the 24-hour gate.
- Provide `workflow_dispatch` inputs for candidate SHA/profile/duration.
- Provide a scheduled requalification workflow or documented operator-triggered release workflow.
- Evidence must identify whether a run is smoke or qualification. Smoke evidence cannot satisfy a soak gate.

### Metrics and thresholds

Define thresholds before the run for:

- error count/rate;
- process crashes;
- RSS/working-set growth;
- file descriptor/handle growth;
- task/thread growth;
- connection and permit leakage;
- latency percentiles;
- CPU saturation;
- socket-state accumulation;
- forced shutdown count and duration;
- log sink drops/failures;
- proxy-origin disagreement.

Do not use a heuristic such as "final RSS is less than half of maximum" as the sole leak criterion. Use baseline windows, trend/slope, restart baselines, and explicit tolerances.

### Acceptance criteria

- Each soak gate exercises the actual topology named by its profile.
- A plaintext direct server run cannot satisfy either production-profile soak gate.
- Qualification runs last at least 24 hours and are tied to the exact candidate SHA.
- No unexplained monotonic resource growth or permit/task/handle leak occurs.
- Every periodic restart returns resources to the defined baseline tolerance.
- Zero successful hidden requests occur after malformed/desynchronizing traffic.
- All threshold decisions are stored in machine-readable evidence.

## Track G — Reconcile the custom-service TE+CL contract

The documentation currently states a global strict `Transfer-Encoding` plus `Content-Length` rejection guarantee, while Hyper may normalize or remove headers before the custom-service path observes them.

Do not add a new raw HTTP parser solely to broaden custom application-serving guarantees.

### Required corrective decision

Audit raw-wire behavior for:

- built-in static service;
- custom service with `Reject` body policy;
- custom service with `Buffer` body policy;
- custom service with `Stream` body policy;
- direct listener and proxy-origin paths.

Then implement the smallest truthful contract:

- retain strict rejection guarantees for the built-in static service and every case eggserve can deterministically observe;
- ensure ambiguous framing never invokes custom service code when detectable;
- ensure connection closure prevents a hidden trailing request after rejection;
- explicitly document parser-owned behavior where the original raw header combination is not exposed to eggserve;
- narrow downstream application-server claims rather than inventing a new parser subsystem.

If strict custom-service TE+CL rejection cannot be proven at eggserve's observable boundary, remove the unconditional global `400` claim and replace it with profile/mode-specific language.

### Required tests

- raw TE+CL against built-in static service;
- raw TE+CL against each custom body policy;
- duplicate and comma-combined Content-Length;
- malformed chunking and trailers;
- valid/malformed/valid pipelined sequences;
- handler invocation counter;
- hidden trailing request counter;
- connection reuse/closure assertion;
- proxy-origin differential behavior.

### Acceptance criteria

- Documentation and tests agree on observable TE+CL behavior.
- Built-in static production profiles retain a strict no-request-body framing posture.
- No rejected ambiguous request invokes user code.
- No hidden second request is processed after ambiguous framing.
- No release claim depends on raw header information unavailable after the chosen parser boundary.

## Track H — Close known independent-review findings or narrow the contract

The prior independent review recorded two high findings and several medium limitations, including:

- latent `StaticService::call` response-header loss;
- dual validation architecture between built-in and custom-service paths;
- HEAD body suppression concerns in body-error responses;
- Python duplicate-header representation limitations;
- file-backed handler behavior and related test history.

Re-audit these findings against the current tree rather than assuming they remain latent or already fixed.

### Required disposition policy

For each finding:

1. reproduce on the current SHA;
2. assign severity and affected profiles;
3. fix it, or narrow the affected API/support claim;
4. add regression coverage;
5. record implementation and evidence SHAs;
6. obtain independent confirmation for critical/high findings.

No high finding may remain open for a promoted profile. A high finding may remain for an explicitly experimental embedding API only if the production profiles do not use that path and the limitation is prominent in the API contract.

### Acceptance criteria

- Every prior independent-review finding has a current disposition.
- No critical/high finding affects a promoted support profile.
- HEAD error responses transmit no body and preserve correct headers in every runtime path.
- File-backed handler behavior is either fully supported and tested or explicitly unsupported without hanging/skipped tests.
- Public API documentation accurately states duplicate-header limitations.

## Track I — Execute installed-artifact and provenance qualification

Run tests against the actual release artifacts, not only workspace source trees.

### Required artifacts

- `eggserve-core` crate package dry run;
- `eggserve-bin` crate package dry run;
- standalone binaries for claimed targets;
- Python wheels for claimed CPython/platform targets;
- default and TLS-enabled distinctions where published;
- source archive;
- SBOM;
- checksums;
- provenance/attestation records.

### Required installed tests

For each claimed platform/artifact:

- clean-environment install;
- CLI help/version;
- static serving;
- safe defaults;
- critical path confinement subset;
- HEAD/range/conditional subset;
- lifecycle/shutdown;
- Python primitives;
- Python in-process and subprocess server paths;
- installed-artifact logging;
- artifact/source SHA association.

Windows installed-artifact tests must run in the same qualified environment where practical.

### Acceptance criteria

- Artifact hashes are bound to the exact candidate SHA.
- Installed binary and wheel evidence cannot be replaced by source-tree tests.
- SBOM and provenance are generated for the final artifact set.
- Package contents contain no unintended test roots, evidence secrets, private paths, or build debris.
- Clean uninstall/upgrade smoke passes where supported.

## Track J — Freeze one final candidate and rerun invalidated gates

After all code and test corrections land, select one final candidate SHA.

### Freeze record

Create a machine-readable record containing:

- candidate SHA;
- version;
- tree status;
- toolchain versions;
- Cargo.lock hash;
- criteria/profile/finding registry hashes;
- expected artifacts and hashes;
- required gates per profile;
- evidence expiration policy;
- independent reviewer identity/status;
- allowed documentation-only follow-up policy.

Any code, build, workflow, release-criteria, support-profile, or artifact-producing change after freeze invalidates applicable evidence and requires a new candidate SHA.

### Required rerun matrix

At minimum rerun:

- full Rust matrix on Linux, macOS, and Windows;
- all feature combinations claimed by profiles;
- canonical and raw-wire suites;
- body-policy suites;
- lifecycle and shutdown suites;
- Unix filesystem races;
- Windows qualification suite;
- Caddy and nginx interoperability/desync suites;
- native TLS abuse suite;
- stateful fuzz replay and release budget;
- fault injection;
- 24-hour profile-specific soaks;
- installed binaries and wheels;
- supply-chain audit/deny;
- SBOM/provenance;
- independent security review;
- profile decisions and human approval.

### Acceptance criteria

- Every required evidence record references the exact frozen SHA.
- No required gate is stale, missing, blocked, skipped, or warning-only.
- Evidence aggregation exits zero only for profiles whose complete gate set passes.
- The final report can be reproduced from checked-in criteria/profile metadata plus archived evidence.

## Track K — Independent final review

Commission an independent review of the final candidate after all corrective code changes, not merely the earlier baseline.

### Review scope

- pinned-root ownership and fallible duplication;
- Unix descriptor-relative confinement;
- Windows handle-relative traversal/enumeration and FFI safety;
- reparse/namespace/race qualification;
- HTTP framing and parser-boundary claims;
- body rejection and connection reuse;
- lifecycle, timeout, shutdown, and task ownership;
- canonical response/HEAD/range/conditional behavior;
- TLS admission and direct-HTTPS topology;
- proxy-origin desynchronization;
- Python FFI, callbacks, file-backed responses, and lifecycle;
- logging privacy/injection;
- evidence aggregator fail-closed behavior;
- artifacts, SBOM, and provenance.

### Findings policy

- Critical/high: fix and rerun invalidated evidence.
- Medium: fix or narrow the affected support profile/API contract.
- Low: may defer with owner, rationale, and non-blocking profile impact.
- Test weakening is not an acceptable disposition without protocol/security justification.

### Acceptance criteria

- Reviewer identity, scope, candidate SHA, findings, and dispositions are archived.
- No unresolved critical/high finding affects a promoted profile.
- Every fixed finding has regression coverage and rerun evidence.

## Track L — Make profile decisions from evidence

Decide each profile independently from the final aggregate report.

### `unix-reverse-proxy`

Promote only if:

- Caddy and nginx interop pass as blocking gates;
- proxy desynchronization corpus passes;
- Unix race and fault-injection gates pass;
- real reverse-proxy 24-hour soak passes;
- installed artifacts, SBOM/provenance, and review pass.

Otherwise retain `candidate`.

### `unix-direct-https`

Promote only if:

- native TLS abuse/admission gates pass;
- real native-rustls 24-hour soak passes;
- installed artifacts, SBOM/provenance, and review pass.

Otherwise retain `candidate`.

### `windows-reverse-proxy`

Promote only if:

- dedicated Windows qualification passes on local NTFS;
- installed Windows artifacts pass;
- Windows independent safety review passes;
- profile decision is explicitly approved;
- common proxy/profile gates required by metadata pass.

Otherwise retain `candidate`.

### `windows-direct-https`

Promote only if both Windows hardened qualification and direct native-TLS qualification pass. Otherwise retain `functional`.

### `local-development`

Retain its existing narrow support claim based on ordinary cross-platform gates. Do not use it to imply public-internet qualification.

### Functional/compatibility profiles

Keep SMB, non-NTFS, cloud-placeholder, and link-following configurations outside hardened claims.

### Acceptance criteria

- `release/support-profiles.toml` is updated only after aggregate evidence passes.
- Generated README/deployment/release documentation matches the profile file exactly.
- Every profile decision includes candidate SHA, required-gate result, reviewer result, and approver.
- An unpromoted profile remains usable under its existing lower claim without being described as production hardened.

## Track M — Final closure report and repository hygiene

Create `release/plan-090-closure-report.md` containing:

- starting and final SHAs;
- implementation commits by track;
- reopened and closed findings;
- exact evidence manifest identity;
- platform/feature/artifact matrix;
- Windows qualification environment and result;
- Caddy/nginx results;
- TLS result;
- soak results and threshold analysis;
- fuzz/race/fault results;
- installed-artifact hashes;
- SBOM/provenance identities;
- independent-review result;
- final support-profile decisions;
- deferred low findings with owners;
- release/no-release recommendation.

Update:

- `release/corrective-status.md`;
- `release/corrective-findings.toml`;
- `release/support-profiles.toml`;
- `docs/release-checklist.md` or generated candidate report;
- README;
- deployment and TLS documentation;
- threat model and security policy;
- Windows ADR;
- runtime and API contracts;
- release runbook;
- AGENTS/skill documentation where plan status is recorded.

Remove or correct stale claims, warning-only workarounds, ignored required tests, temporary diagnostics, dead test scaffolding, and obsolete plan-status language.

### Acceptance criteria

- Repository status files agree with machine-readable evidence.
- No document claims full qualification while required evidence is pending.
- No required release gate uses warning-only failure handling.
- No required qualification test is silently ignored or early-returned.
- Generated files are clean and reproducible.
- The final closure report identifies one exact candidate SHA.

## Required implementation order

Execute tracks in this order:

1. Track A — truthful state model.
2. Track B — pinned-root panic correction.
3. Track C — fail-closed evidence aggregator.
4. Tracks D, E, F, and G may proceed in parallel after Track C contracts are stable.
5. Track H — current-tree review finding closure.
6. Track I — installed artifacts and provenance.
7. Track J — freeze final candidate and rerun all invalidated gates.
8. Track K — independent final review.
9. Track L — profile decisions.
10. Track M — final report and documentation reconciliation.

Do not freeze the final candidate before all code-affecting tracks are merged.

## Minimum test commands

The implementation agent must adapt commands to the final test names, but the closure evidence must include equivalents of:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
cargo test -p eggserve-core --features client
cargo test -p eggserve-core --features client-tls
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test request_body_wire
cargo test -p eggserve-core --test lifecycle_integration
cargo test -p eggserve-core --test filesystem_race_qualification
cargo test -p eggserve-core --test stateful_fuzz_replay
cargo test -p eggserve-core --test fault_injection
cargo test -p eggserve-bin --features tls --test tls_abuse
python3 scripts/check-contract-consistency.py
python3 scripts/release-status.py validate-all --sha "$CANDIDATE_SHA" --evidence-dir evidence-artifacts
```

Windows qualification, proxy interoperability, soak, wheel, package, and provenance commands must also be captured exactly in their evidence records.

## Explicit plan acceptance criteria

Plan 090 is complete only when all of the following are true:

1. Implementation and evidence states are separate throughout the corrective registry and dashboard.
2. Plans 084–089 reference their real implementation SHAs.
3. COR-017 is not fully closed until final qualification evidence exists.
4. `PinnedRoot` handle/descriptor duplication has no panic-capable path.
5. Required evidence aggregation fails on missing, stale, skipped, blocked, warning-only, wrong-SHA, wrong-platform, or wrong-artifact evidence.
6. Windows hardened tests execute on a capable dedicated local-NTFS environment.
7. Required Windows fixtures cannot report success through early returns or ignored tests.
8. nginx interoperability is either a real blocking pass or atomically removed from the claimed profile; warning-only required gates do not exist.
9. Reverse-proxy soak uses an actual proxy topology.
10. Direct-HTTPS soak uses actual native rustls.
11. Each promoted production-profile soak runs for at least 24 hours on the final candidate SHA.
12. TE+CL and parser-boundary documentation matches actual custom-service and static-service behavior.
13. No rejected ambiguous request invokes user code or permits a hidden trailing request.
14. Prior independent-review findings have current dispositions and regression coverage.
15. Installed binary and wheel evidence exists for every claimed target.
16. SBOM, checksums, and provenance bind artifacts to the final candidate SHA.
17. An independent final review has no unresolved critical/high findings affecting promoted profiles.
18. Every profile decision is derived from the fail-closed aggregate report.
19. Candidate or functional profiles remain unpromoted when any required evidence is absent.
20. One final closure report records the exact SHA, evidence manifest, artifact identities, and release recommendation.

## Final handoff state

A successful implementation leaves the repository in one of two truthful states:

### Qualified release

One or more named profiles have complete exact-SHA evidence and are promoted in `release/support-profiles.toml`. Other profiles remain candidate/functional as appropriate.

### Correctly unpromoted release candidate

The implementation is complete, but one or more required qualification gates remain blocked or failed. The repository clearly reports those blockers, no unsupported profile is promoted, and no blanket production-grade claim is made.

Either outcome is acceptable. False closure is not.
