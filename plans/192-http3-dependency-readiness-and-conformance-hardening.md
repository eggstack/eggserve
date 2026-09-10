# Plan 192 — HTTP/3 Dependency Readiness and Conformance Hardening

## Status

**PLANNED — prerequisite readiness gate before any HTTP/3 supported-tier promotion attempt.**

Baseline: `main` at or after Plan 190 (`dbac1b70fd77b1d10984c2af697e3722a27a2156` when this plan was written). Re-read current code, dependency versions, upstream issue state, and standards before implementation because the HTTP/3 ecosystem is still moving quickly.

Prerequisite: the Plan 189 deterministic H3 correctness fixes and Plan 190 qualification record remain valid on the execution candidate.

This plan does **not** promote HTTP/3. Its purpose is to decide whether EggServe's selected `h3` / `h3-quinn` / Quinn stack is mature enough, in the exact subset EggServe uses, to justify the broader promotion campaign in Plan 193.

HTTP/3 remains **experimental** throughout Plan 192.

## Purpose

The current H3 implementation is functionally sound in deterministic in-process tests, but the dependency and wire-level evidence is not yet strong enough for a supported claim.

The required end state is:

- current HTTP/3, QUIC, QPACK, TLS, and Alt-Svc responsibilities are mapped explicitly to EggServe versus its dependencies;
- every known upstream issue plausibly affecting EggServe's supported subset is triaged;
- relevant upstream defects are either fixed by moving to a maintained release, proven inapplicable, or mitigated narrowly with a regression test;
- EggServe's H3 cancellation, body, response, flow-control, QPACK/header, shutdown, and resource boundaries are testable with independent/adversarial clients;
- the selected stack exposes enough bounded configuration or documented bounded defaults for EggServe to make a support promise;
- no production fork/private dependency patch is introduced merely to force a supported label;
- Plan 193 receives a frozen, auditable candidate stack or an explicit reason to leave H3 experimental.

## Why this gate is separate from promotion

The current `h3` ecosystem is not equivalent to the relatively mature Hyper/h2 stack. A promotion plan that simultaneously changes dependencies, discovers upstream protocol defects, builds adversarial harnesses, and declares support would blur implementation correctness with evidence.

Plan 192 therefore owns **dependency readiness and conformance hardening**. Plan 193 owns **independent-client, network, platform, and final support-tier qualification**.

If Plan 192 concludes that the selected stack cannot satisfy EggServe's boundedness/cancellation/correctness contract without fragile patches, stop and leave H3 experimental. That is a successful readiness decision, not a failed project.

## Current dependency concerns that must be re-evaluated

At the time this plan was written, EggServe's lockfile used the following H3 stack family:

- `h3 0.0.8`;
- `h3-quinn 0.0.10`;
- `quinn 0.11.x`;
- rustls 0.23.x.

Two upstream `hyperium/h3` issues are specifically relevant enough to require explicit disposition:

### Upstream issue #338 — buffered H3 data lost on same-batch connection error

`hyperium/h3#338` reports that `FrameStream::poll_next` / `poll_data` can discard already-buffered bytes when QUIC delivers stream data and a connection-level error in the same receive batch. The report targets the same `h3 0.0.8` / `h3-quinn 0.0.10` family and notes platform/kernel sensitivity with Quinn's batched receive path.

This could affect EggServe if the analogous server-side receive path can misclassify a request or body that arrived immediately before peer/connection close.

Required disposition:

- re-check issue/PR/release status;
- inspect whether EggServe's server-side code traverses the affected path;
- reproduce against the current candidate if feasible;
- if fixed upstream, move only to the first suitable maintained release after normal compatibility/MSRV/security review;
- if inapplicable, record why with code-path evidence;
- if applicable and unfixed, H3 cannot be promoted until there is a robust public fix or a narrow maintainable mitigation.

Do not vendor/fork `h3` as the default answer.

### Upstream issue #262 — unfinished RequestStream drop/reset behavior

`hyperium/h3#262` records that dropping a request stream does not automatically perform the RFC 9114-recommended reset/abort behavior for unfinished directions.

EggServe already performs explicit `stop_sending` / `stop_stream` actions in several H3 error and cancellation paths, so this issue may be mitigated for the subset EggServe exposes. That must be demonstrated rather than assumed.

Required disposition:

- enumerate every EggServe path that drops an H3 request stream before both directions are terminal;
- verify the adapter explicitly resets/aborts the appropriate directions first;
- add tests for timeout, rejected body, handler cancellation, response failure, peer stop/reset, forced shutdown, and task abort;
- use packet/protocol instrumentation where needed to confirm a dropped task does not silently leave a live QUIC stream until idle timeout;
- if a path cannot be made reliable through public APIs, leave H3 experimental.

### Other upstream issues

At execution time, query current open issues and release notes for `hyperium/h3`, `h3-quinn`, Quinn, Quinn-proto, rustls, and relevant UDP/platform layers.

Prioritize issues involving:

- buffered-data loss;
- request reset/STOP_SENDING observability;
- stream drop semantics;
- flow-control deadlock or starvation;
- QPACK state growth/blocking;
- control-stream handling;
- GOAWAY/drain;
- connection close ordering;
- batched UDP receive behavior;
- panics or memory growth under malformed frames;
- stateless retry/address validation;
- TLS/ALPN interoperability;
- platform-specific UDP behavior.

WebTransport/datagram/extended-CONNECT issues are out of scope unless they reveal a bug in shared H3/QUIC machinery used by ordinary requests.

## Normative standards and ownership matrix

Build a dated matrix from the then-current standards. Minimum baseline when written:

- RFC 9110 — HTTP semantics;
- RFC 9114 — HTTP/3;
- RFC 9000 — QUIC transport;
- RFC 9001 — QUIC TLS usage;
- RFC 9002 — QUIC loss detection/congestion control;
- RFC 9204 — QPACK;
- RFC 7838 — Alt-Svc;
- RFC 7301 — ALPN;
- RFC 9846 — current TLS 1.3 specification at the time of this plan.

Classify requirements into:

1. **EggServe-owned** — canonical adaptation, body policy, service admission, final response policy, Alt-Svc generation, timeouts, lifecycle, resource limits exposed by EggServe, dual-listener lifecycle;
2. **dependency-owned but EggServe-configured** — QUIC stream/window/idle/handshake limits, H3 field-section limit, send buffering, stateless retry, TLS 1.3/ALPN setup;
3. **dependency-owned/pass-through** — QUIC packet number/loss recovery/congestion state, QPACK codec correctness, H3 frame parser correctness, transport encryption.

Every category 1 or 2 item requires direct EggServe qualification. Category 3 requires dependency review and representative independent/adversarial evidence, not a second H3/QUIC implementation in EggServe.

## Track A — Candidate stack selection

### A1. Inventory current stack

Record exact versions, feature flags, MSRV, default features, license/security status, and relevant transitive native/system dependencies.

### A2. Compare maintained upgrade candidates

Inspect available stable releases/commits for `h3`, `h3-quinn`, and Quinn. Evaluate upgrades against:

- fixes for relevant upstream issues;
- API stability/maintenance activity;
- MSRV compatibility;
- dependency graph impact;
- TLS/crypto backend changes;
- runtime behavior on Linux/macOS/Windows;
- compile/binary-size impact;
- breaking adapter changes.

Prefer released crates over Git revisions. Avoid unbounded “track master” dependencies.

### A3. Freeze one qualification candidate

Plan 192 must end with one of:

- **READY CANDIDATE** — exact released dependency set selected for Plan 193;
- **CURRENT STACK ACCEPTED** — existing versions retained after issue triage;
- **BLOCKED** — one or more relevant upstream issues prevent a support attempt.

Record the reasoning in a release/readiness record.

## Track B — QPACK and HTTP/3 resource ownership audit

The current EggServe `Http3Config` explicitly controls:

- max concurrent bidirectional streams;
- max concurrent unidirectional streams;
- per-stream receive window;
- connection receive window;
- QUIC send window;
- idle timeout;
- pending handshakes;
- H3 field-section size;
- per-response send buffer;
- stateless retry;
- Alt-Svc advertisement.

Audit the selected H3 library's handling of additional QPACK/H3 state including:

- dynamic table capacity;
- blocked-stream count;
- encoder/decoder stream state;
- control stream uniqueness;
- retained reset/error state;
- maximum encoded/decompressed field behavior.

For each resource, choose one:

1. EggServe configures it explicitly through a maintained public API;
2. the dependency uses a fixed/bounded default that EggServe documents and pins through dependency versioning;
3. the resource is effectively unbounded or unsuitable, which blocks promotion until corrected.

Do not expose knobs merely because they exist. Add public/experimental configuration only when it is required to state and enforce EggServe's resource envelope.

## Track C — Stream termination and cancellation audit

Create a table for every H3 request termination path:

- normal request+response completion;
- body rejected before service invocation;
- body presence probe timeout;
- body read timeout;
- body size/declared-length failure;
- service rejection/error;
- service panic;
- handler timeout;
- response producer error;
- response send timeout;
- peer RESET_STREAM;
- peer STOP_SENDING;
- peer connection close;
- H3 connection error;
- server graceful shutdown;
- graceful-drain deadline expiration;
- task abort;
- max-requests-per-connection drain.

For each direction, record:

- HTTP response possible before commitment?;
- send side terminal action;
- receive side terminal action;
- `RequestLifecycle` reason;
- whether sibling streams remain alive;
- permit/counter release owner;
- dependency API used.

Acceptance requires no ordinary stream-level failure to leak an unfinished stream until idle timeout solely because a task/future was dropped.

## Track D — Upstream issue reproduction harnesses

Add test-only/release qualification harnesses for any relevant current upstream issue.

For #338 or its successor issue:

- reproduce data+connection-close coalescing or the closest deterministic simulation available;
- cover server request headers and request DATA paths relevant to EggServe;
- verify buffered valid application data is not silently discarded before the connection error is surfaced;
- if exact kernel batching cannot be reproduced deterministically, retain a targeted platform run in Plan 193 and document the limitation.

For #262 or its successor:

- deliberately terminate EggServe request tasks in each unfinished-stream state;
- observe peer-visible reset/stop behavior with an independent/adversarial client;
- ensure stream state and connection resources return to baseline promptly.

Keep these harnesses outside the runtime dependency graph.

## Track E — H3 protocol/control-stream conformance harness

Prepare the adapter for adversarial qualification with a tool such as Cloudflare `h3i` or another maintained independent H3 frame-level client.

The harness must be able to exercise:

- duplicate/invalid control streams;
- duplicate QPACK encoder/decoder streams;
- unknown unidirectional stream types;
- malformed/forbidden SETTINGS sequences;
- illegal frame placement;
- request stream reset/stop races;
- malformed pseudo-header/message sequences;
- oversized field sections;
- invalid content-length/body relationships where the independent stack permits construction;
- GOAWAY sequencing/races.

Do not put h3i/quiche in EggServe's runtime dependencies. Prefer a release script/container/dev-tool recipe.

## Track F — Flow control and no-progress readiness

Audit current H3 send behavior against the support contract.

The H3 adapter currently wraps response header/data/finish send operations in `response_write_timeout`. Confirm under real QUIC flow control that:

- a peer withholding receive credit causes the relevant send future to stop making progress;
- timeout terminates only the affected response stream where the public API permits;
- sibling streams remain usable;
- response buffers remain bounded;
- a slow but steadily progressing peer does not spuriously time out.

Also audit application producer stalls. If a canonical `ResponseStream` can wait indefinitely before yielding the next chunk without any H3 no-progress guard or hard connection ceiling, decide explicitly whether:

- existing handler/stream semantics already bound it;
- H3 needs a narrow producer no-progress deadline consistent with H1/H2 semantics;
- or the behavior is documented as application-owned and still bounded by shutdown/lifecycle.

A supported H3 tier may not contain an undocumented indefinitely pinned response task that bypasses all relevant runtime limits.

## Track G — Connection lifetime and idle semantics

Reconcile H3 with the public/runtime timeout documentation.

The TCP/H1/H2 driver has a `connection_total_timeout` hard ceiling, while QUIC/H3 also has `max_idle_timeout`. Determine the intended supported H3 contract:

- apply the common total-lifetime ceiling to H3 as well; or
- explicitly document H3 as using QUIC idle plus per-operation/request deadlines instead of the TCP total-lifetime rule.

Do not leave a public configuration field documented as protocol-neutral if H3 silently ignores it.

If implementation changes are needed, keep them narrow and add cross-protocol documentation/tests.

## Track H — Alt-Svc and endpoint correctness readiness

Before browser promotion tests, re-verify:

- dynamic port `:0` advertises the actual bound UDP/TCP port;
- application responses cannot override runtime-owned Alt-Svc policy;
- privacy/header denylist suppresses advertisement as documented;
- H3-disabled or missing-identity states do not advertise an unusable H3 endpoint;
- TCP/H1/H2 remains healthy if UDP bind/H3 setup is intentionally disabled;
- H3 startup failure follows documented startup semantics and does not leave misleading TCP advertisement.

No DNS HTTPS/SVCB automation is authorized.

## Track I — TLS/QUIC security configuration

Review the selected Quinn/rustls integration against RFC 9001 and current TLS 1.3 requirements.

Verify:

- TLS 1.3 only for QUIC;
- ALPN includes exactly the H3 identifiers intentionally supported;
- 0-RTT application requests remain disabled;
- no weak/legacy TLS mode is enabled for H3 compatibility;
- certificate/key handling matches existing EggServe TLS ownership;
- transport secrets, connection IDs, tokens, and raw packets are not logged by default;
- stateless retry remains explicit policy rather than an accidental dependency default.

Do not add ECH, PQ TLS, client auth, certificate automation, or unrelated TLS feature work in this plan.

## Track J — Dependency/security/MSRV/footprint gates

Run:

```bash
cargo audit
cargo deny check
cargo tree -p eggserve-core -e features
cargo +1.88 check --workspace --all-targets --features http3,tls
```

If dependency upgrades raise the MSRV, make an explicit project decision rather than silently breaking the documented Rust 1.88 floor. Prefer preserving the current MSRV unless a relevant maintained H3/QUIC fix requires otherwise.

Measure minimal versus H3 feature graph/binary impact before and after any upgrade. H3 remains optional and must not enter the default build.

## Track K — Test/qualification tooling changes

Extend `scripts/qualify-http3.sh` or add small companion scripts so the later promotion plan can require named evidence classes.

The harness should be able to fail closed for:

- no direct H3 client;
- fewer than two independent H3 implementation families;
- missing adversarial H3 client;
- missing browser Alt-Svc evidence;
- missing network-impairment evidence;
- missing mandatory platform evidence.

Do not mark these as Plan 192 failures if they belong to Plan 193 execution; Plan 192's requirement is that the harness can represent them accurately and never mistake “tool unavailable” for “passed.”

## Track L — Documentation during readiness

Update documentation only for factual implementation/dependency corrections discovered here. Keep the public support tier experimental throughout.

If the candidate stack changes, synchronize:

- `architecture/http3.md`;
- `architecture/tls.md`;
- `docs/dependency-policy.md`;
- `docs/timeout-reference.md` if semantics change;
- `docs/release-contract.md` only for dependency/limitation truth;
- Plan 188/190 cross-links;
- qualification scripts/docs.

Do not pre-write “supported HTTP/3” language before Plan 193 passes.

## Track M — Readiness record

Create:

`release/plan-192-http3-dependency-readiness.md`

Record:

- candidate commit;
- exact dependency set;
- current upstream issue inventory and dispositions;
- #338 and #262 disposition or successor issue references;
- standards/ownership matrix;
- QPACK/resource ownership table;
- stream termination/cancellation table;
- timeout/lifetime decision;
- security/MSRV/feature-graph results;
- any narrow source changes and tests;
- final result: `READY FOR PLAN 193` or `BLOCKED`.

## Verification

Minimum deterministic verification after any source/dependency change:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
bash scripts/test-python-wheel.sh
cargo audit
cargo deny check
```

Also run the focused upstream-issue/cancellation/flow-control tests added by this plan.

## Acceptance criteria

- [ ] current H3/QUIC/QPACK/Alt-Svc/TLS standards responsibility matrix is documented.
- [ ] current `h3`, `h3-quinn`, Quinn, rustls versions and upgrade candidates are reviewed.
- [ ] relevant current upstream issues are triaged rather than relying on crate maturity labels alone.
- [ ] `hyperium/h3#338` is fixed upstream, proven inapplicable to EggServe's used server path, or otherwise blocks promotion; no silent assumption remains.
- [ ] `hyperium/h3#262` stream-drop/reset behavior is covered by explicit EggServe termination actions and tests or otherwise blocks promotion.
- [ ] every EggServe H3 early-drop/error/timeout/shutdown path has explicit send/receive stream termination ownership.
- [ ] ordinary stream errors remain stream-scoped and connection-wide lifecycle cancellation is reserved for unusable connection state/forced shutdown.
- [ ] QPACK/control/header/resource state is either explicitly bounded by EggServe configuration or documented bounded dependency defaults; no relevant unbounded state remains unexamined.
- [ ] H3 response flow-control stall behavior is bounded and testable independently.
- [ ] H3 application producer/no-progress behavior cannot silently pin resources indefinitely outside the documented timeout model.
- [ ] `connection_total_timeout` versus QUIC idle/per-operation semantics are reconciled in implementation/docs.
- [ ] Alt-Svc/port-0/privacy/startup behavior remains truthful and deterministic.
- [ ] QUIC uses the intended TLS 1.3/H3 ALPN policy and 0-RTT application requests remain disabled.
- [ ] no relevant security advisory or unresolved upstream correctness issue is knowingly hidden by the support plan.
- [ ] any dependency update preserves or explicitly revises the MSRV through normal project policy.
- [ ] minimal/default build remains free of H3/QUIC dependencies.
- [ ] qualification tooling can distinguish missing evidence from passing evidence and fail closed for Plan 193 requirements.
- [ ] routine deterministic H1/H2/Python behavior remains green after any H3 stack change.
- [ ] H3 remains marked experimental throughout this plan.
- [ ] readiness record ends with `READY FOR PLAN 193` or an explicit blocker list.
- [ ] no WebTransport, datagram, extended CONNECT, server push, proxy, routing, middleware, ACME, DNS automation, 0-RTT application behavior, or Python H3 API enters scope.

## Suggested implementation order

1. Freeze baseline and current dependency/upstream issue inventory.
2. Build standards/delegation and stream-termination matrices.
3. Reproduce/triage h3 #338 and #262 against the exact EggServe path.
4. Audit QPACK/control-stream/resource bounds.
5. Audit response flow-control and producer no-progress semantics.
6. Reconcile H3 connection lifetime versus QUIC idle semantics.
7. Select current stack versus a maintained released upgrade.
8. Apply only narrow required dependency/source changes and add regressions.
9. Harden H3 qualification tooling to represent Plan 193 evidence classes.
10. Re-run deterministic/MSRV/Python/supply-chain gates.
11. Write the Plan 192 readiness record and make a binary READY/BLOCKED decision.

## Handoff

Plan 192 should not optimize for producing a supported label. It should optimize for knowing whether EggServe can responsibly make one.

If the selected stack has an unresolved relevant correctness/cancellation/resource bug without a maintainable public fix, leave H3 experimental and stop. If the exact EggServe-used subset is bounded, regression-covered, and dependency-ready, hand the frozen candidate to Plan 193 for independent-client, adversarial-network, platform, and final support-tier qualification.