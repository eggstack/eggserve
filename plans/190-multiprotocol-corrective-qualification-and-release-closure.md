# Plan 190 — Multiprotocol Corrective Qualification and Release Closure

## Status

**PLANNED — qualification and documentation closure after Plan 189.**

Prerequisite: Plan 189 implementation is complete and all deterministic H1/H2/H3 regression tests are green on the candidate commit.

This plan does not add protocol capability. Its job is to prove that the Plan 189 corrections actually close the identified semantic gaps, update evidence records, and leave support/release claims truthful.

## Purpose

Close the post-Plan-188 corrective pass with evidence rather than implementation assertions.

The plan must answer five questions:

1. Does request-body Reject now work correctly on H2/H3 when DATA is present without `Content-Length`?
2. Are runtime-generated errors semantically identical across H1/H2/H3 except for protocol-specific framing fields?
3. Does the public `RequestLifecycle` contract work for H3 connection and stream termination as it already does for H1/H2?
4. Are H2 response-progress claims limited to what the public Hyper/h2 API can actually guarantee?
5. Do all product, capability, plan-status, versioning, and release documents agree on the post-correction state?

The expected support tier after this plan remains:

- HTTP/1.1: supported default/baseline;
- native HTTP/2: experimental unless new evidence independently satisfies its outstanding Plan 186 gaps;
- native HTTP/3: experimental unless new evidence independently satisfies its outstanding Plan 188 gaps;
- Python compatibility facade: HTTP/1.1-shaped.

Passing this corrective plan alone is not sufficient to promote H2 or H3.

## Qualification principles

### 1. Reproduce the bug classes directly

Do not infer closure from general test-suite success. The exact previously incorrect cases must have targeted regression evidence.

### 2. Separate deterministic correctness from independent interoperability

In-tree protocol clients are appropriate for deterministic state/lifecycle tests. Independent clients are still required for claims about real-world interoperability. Keep those evidence classes distinct.

### 3. Do not weaken qualification because support remains experimental

Experimental status permits known evidence gaps; it does not permit known deterministic correctness bugs. The Plan 189 bug classes must be closed before this plan can finish.

### 4. Do not promote based on absence of failure

A successful compile, local startup, or same-stack client is not evidence that an experimental protocol is generally supported.

### 5. Keep routine CI proportionate

The focused regressions belong in the existing H2/H3 feature jobs. External H3 clients, browsers, and network impairment remain release/manual qualification rather than PR-CI requirements.

## Track A — Establish the tested candidate and change inventory

Record:

- exact commit SHA;
- Rust stable version;
- MSRV version;
- Hyper/h2 versions;
- h3/h3-quinn/Quinn/rustls versions;
- enabled feature combinations;
- OS/architecture;
- available independent clients and versions.

Compare the candidate against the Plan 188 baseline and confirm the correction is narrow. Expected changed areas are primarily:

- H1/H2 request pipeline/body policy;
- H3 adapter body handling;
- canonical runtime error construction;
- request lifecycle registry/cancellation;
- H2 activity/progress naming or implementation;
- focused tests;
- protocol/roadmap/release documentation.

Unexpected routing, middleware, proxy, WebSocket, upload, application-server, Python compatibility, or broad dependency changes require explicit review before qualification proceeds.

## Track B — H2 Reject-body regression qualification

### B1. DATA without Content-Length

Using the deterministic H2 integration harness, send a request that carries DATA without `Content-Length` to a service whose effective request policy is Reject.

Verify:

- response status matches the canonical body-rejection policy;
- service invocation count is zero for that request;
- no H1-only framing/hop-by-hop field appears;
- request stream is cancelled/ended safely;
- sibling streams continue;
- connection remains usable when protocol state allows;
- admission/resource counters return to baseline.

### B2. Bodyless request without Content-Length

Send the same request shape with END_STREAM/no DATA.

Verify:

- service is invoked exactly once;
- canonical request body is empty/complete;
- no false body rejection occurs.

### B3. Content-Length edge cases

Exercise where constructible:

- `Content-Length: 0` plus DATA;
- positive Content-Length with no DATA/premature EOF;
- DATA exceeding declared length;
- DATA exceeding EggServe body limit;
- duplicate Content-Length rejected by dependency stack or EggServe before service invocation.

Record whether Hyper/h2 or EggServe owns each rejection. Do not duplicate protocol-parser logic merely to move ownership into EggServe.

### B4. Multiplexing isolation

Run rejected-body traffic concurrently with valid GET/HEAD or custom-service requests and confirm one rejected stream cannot poison unrelated streams.

## Track C — H3 Reject-body regression qualification

### C1. DATA without Content-Length

Using the in-process H3 client/harness, construct the previously incorrect case.

Verify:

- DATA is detected without relying on Content-Length;
- service is not invoked;
- canonical rejection response is emitted when response state permits;
- request receive direction is stopped/reset at stream scope;
- sibling streams remain usable;
- no unbounded buffering occurs while deciding body presence.

### C2. Immediate end-of-stream

Send an H3 request with no Content-Length and no DATA.

Verify:

- service invocation occurs normally;
- body lifecycle begins/ends in the expected complete state;
- no body timeout is unnecessarily armed after a proven empty request.

### C3. Presence-probe timeout/error

If Plan 189 uses a bounded first-read/presence probe, qualify:

- peer sends headers and then stalls without ending the request;
- body timeout fires within the configured bound;
- service is never invoked;
- receive stream is cancelled;
- request task and any admission/lifecycle state are released;
- sibling streams remain healthy.

### C4. Declared-length exactness

Repeat the H2 declared-length edge cases where supported by the H3 client API and record dependency-vs-EggServe ownership.

## Track D — Cross-protocol runtime error parity

### D1. Build a semantic comparison table

For each representative runtime-generated status, record H1, H2, and H3:

- wire status;
- body bytes under `Minimal`;
- body bytes under `Empty`;
- `Content-Type` presence;
- `Content-Length` semantics where applicable;
- HEAD behavior;
- `Allow` where applicable;
- Date/Server finalization;
- stripped-header/privacy policy behavior;
- protocol-forbidden fields.

At minimum cover:

- 400;
- 405;
- 408;
- 413;
- 414;
- 431;
- 500;
- 503;
- one valid unassigned status code.

### D2. Acceptance rule

For the same canonical error condition, H1/H2/H3 must agree on application semantics. Differences are allowed only for protocol framing/hop-by-hop metadata.

A status/body mismatch such as `408` with an “internal server error” representation is a failure.

### D3. Post-commit failures

Qualify at least one response-stream producer failure after headers are committed on H2 and H3.

Expected behavior:

- no second status response is attempted;
- affected stream/request is terminated according to transport capability;
- logs remain sanitized;
- sibling multiplexed work remains unaffected where the stack exposes stream-local reset;
- lifecycle cancellation matches the documented contract.

## Track E — H3 RequestLifecycle parity qualification

### E1. Peer disconnect

Use a service that clones `RequestLifecycle`, then waits independently of request-body and response I/O.

Close the QUIC connection from the peer side.

Verify:

- lifecycle waiter resolves promptly;
- reason is `PeerDisconnected` or the specifically documented transport-failure fallback permitted by implementation evidence;
- request task/resource ownership terminates cleanly;
- no sibling/other connection is affected.

### E2. Server shutdown

Start a long-lived accepted H3 request and trigger server shutdown.

Test both:

- request finishes within grace -> no premature cancellation solely because drain began;
- request exceeds grace -> lifecycle resolves with `ServerShutdown` before/with forced termination.

### E3. Stream-local transport failure

Cause one request stream to become unusable without closing the whole QUIC connection if the client API permits.

Verify only that request's lifecycle is cancelled.

### E4. Body timeout

Stall a streaming request body until `body_read_timeout` expires.

Verify:

- lifecycle resolves;
- current public reason remains `ConnectionTimeout` under the existing coarse taxonomy;
- only the request stream is cancelled;
- downstream service permit and body ownership release exactly once.

### E5. Normal completion

A normally completed H3 request on a still-live connection must not receive a false cancellation signal.

## Track F — H2 response-progress evidence and wording

### F1. Determine implemented capability

Record whether Plan 189 found a safe public stream-local H2 send-progress/reset API.

If yes, capture direct flow-control stall evidence.

If no, explicitly state that EggServe observes application body producer/poll progress while it owns response production, while bytes already handed to Hyper are bounded by configured H2 send buffers and the hard connection lifetime.

### F2. Sibling-mask regression

Retain a deterministic test in which one application response body stops producing while another H2 stream remains active.

Verify sibling activity does not refresh the stalled producer's deadline.

### F3. Buffered-after-EOS limitation

If true wire progress is still unavailable, do not write a test that pretends the condition is solved. Instead:

- document the limitation in `architecture/http2.md` and timeout/capability docs;
- keep H2 experimental;
- verify the configured H2 per-stream send buffer and connection-total timeout remain explicit bounded fallback controls.

## Track G — Re-run external H2 qualification

Run the existing `scripts/qualify-http2.sh` on a representative environment.

Where available, include:

- curl with HTTP/2;
- nghttp/nghttp2 client;
- browser smoke.

The corrective minimum is to ensure body/error/lifecycle changes did not regress:

- ALPN selection;
- H1 fallback;
- cleartext prior knowledge;
- no Upgrade-based h2c;
- static GET/HEAD;
- range/conditional behavior;
- multiplexing.

If only one client is available, retain H2 experimental and record that the prior Plan 186 evidence gap remains.

## Track H — Re-run external H3 qualification where possible

Run:

```bash
bash scripts/qualify-http3.sh
EGGSERVE_REQUIRE_H3_CLIENTS=1 bash scripts/qualify-http3.sh
EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1 bash scripts/qualify-http3.sh
```

Interpret results honestly:

- the non-required command may still pass deterministic startup/fallback checks without a direct H3 client;
- the required-client variants must continue to fail when evidence is unavailable;
- do not weaken those scripts to make the corrective plan appear complete.

If independent H3 clients are available, add direct checks for:

- bodyless request;
- request DATA without Content-Length;
- normal GET/HEAD;
- range/conditional response;
- concurrent streams;
- response/error semantics;
- Alt-Svc discovery/fallback;
- graceful shutdown where the client supports observing it.

Absence of external H3 evidence does not block deterministic bug closure, but it continues to block a supported-tier claim.

## Track I — Resource and race qualification

### I1. Permit accounting

For H2 and H3 correction cases verify exactly-once release of:

- global service permits;
- file-stream permits if exercised;
- connection permits;
- pending H3 handshake permits;
- active-service/request counters;
- deferred-body accounting.

### I2. Cancellation races

Exercise races between:

- body timeout and peer disconnect;
- server shutdown and body timeout;
- response producer failure and peer disconnect;
- H3 stream cancellation and QUIC connection close.

The first public lifecycle reason wins; counters must not underflow/double-count; no task may remain indefinitely blocked.

### I3. Memory behavior

Confirm the new body-presence checks do not introduce whole-body buffering under Reject.

For H3, inspect the maximum temporary data retained during presence detection. It must remain bounded by transport/chunk configuration rather than peer-declared length.

## Track J — Security and privacy review

Review new code for:

- reflection of service/internal error messages into responses;
- raw body/path/authority values in logs;
- QUIC connection IDs/tokens/key material in logs;
- request-data logging during body-presence checks;
- connection-wide cancellation caused by an ordinary stream-level error;
- accidental acceptance of HTTP/1 framing headers on H2/H3;
- new H3/QUIC dependencies in the default graph.

Re-run `cargo audit` and `cargo deny check` as part of release qualification where those tools are available.

## Track K — Documentation synchronization

Update all live documents affected by the corrective findings, including as applicable:

- `README.md`;
- `plans/ROADMAP.md`;
- Plan 183 status;
- Plans 185–188 execution/limitation notes if they contain stale claims;
- `architecture/http2.md`;
- `architecture/http3.md`;
- `docs/timeout-reference.md`;
- `docs/downstream-app-server.md`;
- `docs/library-capability-matrix.md`;
- `docs/api-stability.md`;
- `docs/non-goals.md` only if wording is stale, not to expand scope;
- `docs/migration-guide.md`;
- `release/plan-186-http2-qualification.md`;
- `release/plan-188-http3-qualification.md`.

Create a concise new closure record, preferably:

`release/plan-190-multiprotocol-corrective-qualification.md`

The record should state:

- tested commit;
- exact deterministic regressions closed;
- external clients actually available;
- lifecycle evidence;
- H2 progress guarantee/limitation;
- H2/H3 final support tiers;
- remaining independent-client/platform/adversarial evidence gaps;
- release-version boundary.

Do not erase historical Plan 186/188 records; append or cross-link corrections so the change trace remains auditable.

## Track L — Version and release boundary

### L1. Stable API transition

Confirm that the current stable Rust API changes documented for the pre-1.0 transition are not published as `0.1.x`.

If this plan is executed as part of release preparation:

- synchronize workspace/crates/Python metadata using the Plan 182 ownership path;
- release as `0.2.0` or later;
- update migration/release notes in the same change;
- run the existing release metadata check before builds.

If this plan is executed before an actual release, it is acceptable to leave repository development metadata unchanged, but the closure record must explicitly say that the current line is not patch-release compatible with 0.1.x.

### L2. No protocol-driven Python API bump

Do not add H2/H3 compatibility classes or Python protocol-selection API as part of version synchronization.

## Track M — Routine CI placement

Keep the newly fixed deterministic regressions in existing jobs:

- default workspace H1 regression suite;
- `http2,tls` representative;
- `http3,tls` representative;
- existing Python job;
- existing supply-chain job.

Do not add external clients, browsers, or privileged network namespaces to every PR.

If a focused test is slow/flaky, fix the harness before making it routine CI; do not silently remove the invariant.

## Verification

Minimum deterministic verification:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo +1.88 check --workspace --all-targets --features http2,tls
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
bash scripts/test-python-wheel.sh
```

Release/manual qualification where available:

```bash
bash scripts/qualify-http2.sh
bash scripts/qualify-http3.sh
EGGSERVE_REQUIRE_H3_CLIENTS=1 bash scripts/qualify-http3.sh
EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1 bash scripts/qualify-http3.sh
cargo audit
cargo deny check
```

Also run direct focused tests for the exact Plan 189 regressions rather than relying only on aggregate suite output.

## Acceptance criteria

- [ ] candidate commit and dependency/client/platform inventory are recorded.
- [ ] H2 DATA without Content-Length under Reject is deterministically rejected with zero service invocation.
- [ ] bodyless H2 request without Content-Length still invokes the service.
- [ ] H2 rejection is stream-scoped and sibling traffic survives.
- [ ] H3 DATA without Content-Length under Reject is deterministically rejected with zero service invocation.
- [ ] bodyless H3 request without Content-Length still invokes the service.
- [ ] H3 body-presence timeout/error path is bounded and releases resources.
- [ ] declared-length mismatch behavior is tested/owned for H2 and H3.
- [ ] H1/H2/H3 runtime-generated error semantics match for the representative status matrix.
- [ ] no H3-local misleading fallback body remains for statuses such as 408.
- [ ] H3 peer disconnect wakes detached lifecycle waiters.
- [ ] H3 forced server shutdown wakes remaining lifecycle waiters with the documented reason.
- [ ] H3 stream-local failure/body timeout cancels only the affected request lifecycle.
- [ ] normal H3 completion does not spuriously cancel lifecycle.
- [ ] permit/counter accounting is exactly once under the correction paths and cancellation races.
- [ ] H2 response-progress documentation and tests match the actual safe public Hyper guarantee.
- [ ] if true H2 wire progress/reset remains unavailable, that limitation is explicit and H2 remains experimental.
- [ ] existing H2 ALPN/prior-knowledge/H1 fallback/static/multiplexing qualification still passes where clients are available.
- [ ] H3 external-client scripts remain evidence-sensitive and are not weakened when clients are unavailable.
- [ ] minimal dependency graph remains free of H3/QUIC dependencies.
- [ ] Python compatibility and wheel behavior remain H1.1-shaped.
- [ ] Plan 183 and roadmap status text no longer contradict the implemented protocol gate.
- [ ] Plans 189–190 are represented as corrective follow-up, not expanded scope.
- [ ] migration/release docs prevent the breaking stable API line from being represented as a `0.1.x` patch release.
- [ ] a Plan 190 closure record captures exact evidence and remaining gaps.
- [ ] final support tiers are evidence-based and do not promote H2/H3 merely because Plan 189 bugs are fixed.
- [ ] no new edge-server/framework/protocol-extension feature entered scope.

## Suggested execution order

1. Freeze candidate SHA and inventory changed files/dependencies.
2. Run exact H2/H3 body-without-Content-Length regression cases.
3. Run bodyless and declared-length edge cases.
4. Run cross-protocol error semantic matrix.
5. Run H3 lifecycle peer/shutdown/stream/body-timeout tests.
6. Run permit/counter race tests.
7. Record the actual H2 progress capability and run its focused stall test.
8. Run full deterministic/MSRV/Python CI-equivalent verification.
9. Run H2 external qualification.
10. Run H3 external qualification with evidence-required modes where clients exist.
11. Re-run security/dependency checks and minimal feature-tree inspection.
12. Synchronize roadmap/plan/protocol/timeout/capability/migration/release docs.
13. Write `release/plan-190-multiprotocol-corrective-qualification.md`.
14. State final H1/H2/H3 tiers and close the corrective pass.

## Handoff

Plans 189–190 are complete when the known post-Plan-188 deterministic correctness gaps are fixed, directly reproduced as passing regressions, and reflected truthfully in the release/capability documentation.

If independent-client, adversarial-network, or platform evidence remains absent, leave H2/H3 experimental and record the gap. Do not create more protocol implementation work merely to manufacture a supported label.

Any future promotion of H2 or H3 should be a narrow evidence/qualification plan against the then-current protocol stack, not another broad architectural expansion.