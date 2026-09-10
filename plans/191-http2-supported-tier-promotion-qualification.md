# Plan 191 — HTTP/2 Supported-Tier Promotion Qualification

## Status

**CLOSED — promotion attempt executed 2026-09-10; H2 remains experimental.**

Outcome: two independent H2 implementation families (curl/libnghttp2,
python-h2), h2spec classification, multiplexing/header/flow-control/GOAWAY,
and h2load evidence were collected, and `scripts/qualify-http2.sh` now fails
closed on missing promotion evidence. Browser evidence and macOS/Windows
runtime evidence were unavailable in the execution environment, so per the
decision rule the tier is retained as experimental with blockers isolated for
future work. Full evidence in
[`release/plan-191-http2-supported-tier-qualification.md`](../release/plan-191-http2-supported-tier-qualification.md).

Original plan below (historical):

Baseline: `main` at or after the Plan 190 closure (`dbac1b70fd77b1d10984c2af697e3722a27a2156` when this plan was written). Re-read the current implementation, dependency versions, open upstream issues, and release documentation before execution.

Prerequisites:

- Plans 183–190 remain implemented/closed at their recorded tiers;
- the deterministic H2 correctness regressions introduced by Plans 185, 186, 189, and 190 are green;
- no new known correctness or security regression has appeared in Hyper/h2 since the baseline.

This plan attempts to promote the existing opt-in Rust `http2` feature from **experimental** to **supported, opt-in**. It does **not** default-enable HTTP/2, stabilize the experimental `server` API, change the Python `http.server` compatibility facade, or add another protocol feature.

## Purpose

Close the evidence gaps that kept HTTP/2 experimental after Plans 186 and 190.

The implementation itself is already structurally sound. The remaining work is predominantly qualification, harnessing, documentation, and release-contract work. Source changes are permitted only when qualification finds an actual defect or when a small testing/observability seam is required to exercise an existing guarantee.

The target support contract is intentionally narrower than “perfect stream-local transport control”:

- TLS HTTP/2 is negotiated through `h2` ALPN;
- cleartext HTTP/2 is supported only through prior knowledge, not Upgrade-based `h2c`;
- request parsing, canonical request/response semantics, body limits, service admission, resource ceilings, cancellation, GOAWAY/drain, and H1 fallback are maintained as supported EggServe behavior;
- response production is bounded according to the currently documented H2 producer/poll-progress model;
- bytes already accepted by Hyper remain subject to explicit Hyper send-buffer bounds and EggServe's hard connection-lifetime fallback;
- EggServe does not promise a safe per-stream wire-progress timer or stream-local reset after response commitment unless a maintained public Hyper/h2 API actually provides one.

A supported label means EggServe is willing to maintain this documented behavior when the feature is enabled. It does not mean the feature becomes part of the default dependency graph.

## Normative and delegated standards basis

At execution time, build a short requirement/delegation matrix using the then-current standards. The minimum baseline when this plan was written is:

- RFC 9110 — HTTP semantics;
- RFC 9113 — HTTP/2;
- RFC 7541 — HPACK;
- RFC 7301 — ALPN;
- the current TLS specification and Rustls security policy used by the selected feature set. RFC 9846 is the current TLS 1.3 specification as of this plan and obsoletes RFC 8446.

Do not reimplement or independently prove all HPACK/frame-parser logic owned by h2/Hyper. Instead classify each requirement as:

1. **EggServe-owned** — adaptation, policy, limits, lifecycle, timeout, admission, response finalization, service behavior;
2. **dependency-owned but EggServe-configured** — stream/window/header/reset limits, ALPN integration, graceful shutdown hooks;
3. **dependency-owned and pass-through** — low-level HPACK/frame parsing and transport state machines.

Every EggServe-owned or EggServe-configured boundary must have direct qualification evidence. Dependency-owned pass-through behavior requires dependency/version review plus representative adversarial tests, not a duplicate protocol implementation.

## Support-tier decision rule

HTTP/2 may be promoted only if all mandatory gates below pass on the candidate commit.

If a gate cannot be executed because tooling is unavailable, the correct result is **remain experimental**, not “passed by inspection.”

If qualification exposes a deterministic defect, fix it narrowly, add a regression test, rerun the full gate, and record the correction in the Plan 191 closure record. Do not create a new architecture program unless the defect proves the current design unsound.

## Track A — Freeze candidate and dependency inventory

Record:

- candidate commit SHA;
- Rust stable and MSRV versions;
- Hyper, h2, hyper-util, rustls, tokio-rustls versions;
- target operating systems/architectures;
- exact independent clients and versions;
- browser versions;
- whether testing uses TLS, prior knowledge, or both;
- feature flags and certificate setup;
- configured H2 limits used by each pressure test.

Review current Hyper/h2 release notes and open issues for anything affecting:

- SETTINGS handling;
- HPACK/header-list accounting;
- flow control;
- stream reset retention;
- GOAWAY;
- request/response body cancellation;
- server graceful shutdown;
- panics or unbounded allocation under malformed input.

Do not upgrade dependencies merely because newer versions exist. Upgrade only for a relevant fix, security advisory, MSRV requirement, or clear maintenance reason; then rerun all deterministic and external qualification against the new versions.

## Track B — Independent-client interoperability matrix

### B1. Count implementations, not command-line frontends

The support gate requires **at least two independent HTTP/2 protocol implementations** interoperating with EggServe.

Do not count curl/libnghttp2 and `nghttp`/libnghttp2 as two implementations. They are useful separate clients but share the same underlying H2 stack.

Preferred evidence set:

- one nghttp2-based client such as curl or `nghttp`;
- one current browser implementation such as Chromium/Chrome or Firefox;
- optionally one additional programmatic implementation from a distinct stack.

At least one client must use a protocol implementation that is not libnghttp2.

### B2. Core semantic matrix

For every independent implementation, verify at minimum:

- TLS ALPN negotiation selects `h2`;
- certificate verification is enabled for the real-client run;
- GET and HEAD;
- static 200 response;
- 404/405 representative errors;
- conditional GET / 304;
- single-range 206 and unsatisfiable 416;
- canonical `Content-Length` behavior;
- large streamed/file response;
- repeated requests on one H2 connection;
- multiplexed concurrent requests;
- request cancellation/reset followed by continued sibling use;
- H1 fallback when `h2` is not negotiated.

A browser run should verify actual HTTP/2 in developer/network instrumentation or an equivalent trustworthy protocol signal; successful HTTPS alone is insufficient.

### B3. Cleartext prior knowledge

If cleartext H2 remains part of EggServe's supported Rust surface, verify with an independent client capable of prior-knowledge H2:

- valid H2 preface selects H2;
- ordinary H1 remains H1;
- partial/invalid H2 prefaces do not create ambiguous parser behavior;
- Upgrade-based `h2c` is not accepted as a supported negotiation path.

RFC 9113 deprecates the older `h2c` Upgrade mechanism; EggServe should continue to support only prior knowledge for cleartext H2 unless a future product plan explicitly changes that decision.

## Track C — Standards/conformance exercisers

### C1. h2spec as a diagnostic suite, not certification authority

Run the current usable `h2spec` suite in strict mode where practical. Record its version and the RFC baseline it implements.

Because h2spec historically targets RFC 7540/7541 while RFC 9113 now defines HTTP/2, classify each failure or skipped case against the RFC 9113 requirement matrix. Do not blindly change EggServe to satisfy behavior that RFC 9113 deprecated or changed.

Expected output:

- machine-readable or captured command output;
- explicit list of failures/skips;
- ownership classification: EggServe, Hyper/h2, obsolete test expectation, or unsupported non-goal;
- zero unresolved EggServe-owned failures before promotion.

### C2. Focused raw/adversarial cases

Where h2spec does not cover current semantics sufficiently, add a small external or test-only raw H2 exerciser rather than an in-production parser.

Exercise representative:

- invalid/duplicate SETTINGS sequences;
- SETTINGS ACK misuse;
- invalid PING framing;
- malformed stream-state transitions;
- oversized/fragmented header blocks and CONTINUATION sequences;
- stream reset churn;
- GOAWAY races;
- frames on closed streams;
- connection errors versus stream errors.

The goal is to prove EggServe remains bounded and delegates errors at the correct scope, not to implement another H2 stack in the repository.

## Track D — Multiplexing and isolation qualification

Exercise the configured default `max_concurrent_streams` and nearby boundary values.

Verify:

- 100 concurrent request streams work at the documented default when application admission is available;
- stream 101+ behavior follows advertised/protocol constraints without unbounded queuing;
- a rejected body does not kill healthy siblings;
- a body timeout does not refresh from sibling traffic;
- a service panic/timeout affects only the request semantics except where the documented response-stall fallback requires connection shutdown;
- one reset stream does not leak service/file/request permits;
- repeated resets do not grow retained state beyond configured Hyper/h2 bounds;
- service admission exhaustion produces bounded 503 behavior rather than hidden queues;
- connection/request/file counters return to baseline after stress.

Include concurrent GET/HEAD/range/static and custom-service traffic so the canonical adapter rather than one synthetic handler is exercised.

## Track E — Header/HPACK/resource pressure

Exercise the interaction among:

- Hyper/h2 maximum header list size;
- EggServe aggregate canonical header ceiling;
- header field count behavior;
- HPACK dynamic compression;
- many small headers;
- one large header;
- repeated compressed header sets;
- header expansion near and beyond limits;
- request-target/authority validation after decode.

Acceptance requires:

- oversized requests are rejected before service invocation;
- decoded header pressure does not produce unbounded EggServe memory use;
- errors stay at the proper stream/connection scope as determined by RFC 9113 and the selected stack;
- sibling traffic remains usable for stream-local failures;
- no H1-only hop-by-hop/framing fields leak through canonical requests or responses.

Do not write an HPACK implementation or HPACK fuzzer in EggServe.

## Track F — Flow control and response-stall contract

### F1. Preserve the truthful Plan 190 guarantee

The baseline contract is:

- EggServe tracks H2 application-body producer/poll progress per response;
- sibling traffic cannot refresh another response's producer timestamp;
- Hyper owns stream-level wire send and flow control after accepting frames;
- configured per-stream send buffering bounds queued bytes;
- when EggServe detects a stalled producer, the safe fallback may terminate the bounded connection rather than one stream.

This limitation does not automatically block supported status if documentation and tests match it.

### F2. Re-check public Hyper/h2 capabilities

At execution time, inspect the current public API for a maintained stream-local server send-progress/reset facility.

If one now exists and integration is small and robust, a narrow improvement may be made, but it is not required to promote H2 unless the published support contract promises stream-local write-stall termination.

Do not use private Hyper internals, dependency forks, unsafe downcasts, or version-fragile hooks solely to strengthen the label.

### F3. Test the documented failure mode

Create a client that withholds/limits response flow control while other streams continue.

Verify:

- memory remains bounded by configured transport/application buffers;
- sibling progress does not falsely count as producer progress;
- the connection-level hard fallback eventually releases all resources under the documented timeout/lifetime policy;
- no deadlock or unbounded task remains after client disconnect or server shutdown.

If the real behavior cannot satisfy the published boundedness claim, H2 remains experimental until corrected.

## Track G — GOAWAY, drain, and connection lifecycle

Qualify graceful shutdown under:

- idle H2 connection;
- active single request;
- multiple concurrent requests;
- active request body;
- active response body;
- flow-control pressure;
- new request racing with shutdown;
- `max_requests_per_connection` drain;
- grace-period expiry.

Verify:

- GOAWAY semantics do not falsely signal unprocessed streams as processed or vice versa;
- accepted requests may finish during the grace period according to current policy;
- new requests stop being accepted according to the driver's graceful shutdown semantics;
- forced termination cancels all remaining request lifecycles;
- all permits/counters return to baseline exactly once.

## Track H — TLS and negotiation qualification

For TLS H2:

- verify `h2` ALPN selection with at least two client implementations;
- verify H1 fallback when client offers only `http/1.1`;
- verify the server never selects `h2c` through TLS;
- verify malformed/failed TLS handshakes release connection permits;
- verify certificate identity/verification in the external-client test environment;
- review the current rustls/TLS configuration against the project's TLS documentation and current TLS standard.

Do not add legacy protocol/cipher support merely to increase client compatibility.

## Track I — Platform runtime qualification

Mandatory supported-tier runtime targets:

- Linux x86-64;
- macOS on a currently supported Apple architecture;
- Windows x86-64.

Strongly recommended:

- Linux aarch64, preferably representative of the project's SBC deployment profile.

On each mandatory target run actual network tests, not compile-only checks:

- H1 smoke/fallback;
- H2 TLS ALPN GET/HEAD;
- multiplexed requests;
- one body rejection/cancellation case;
- graceful shutdown;
- large/streamed response;
- no permit/counter leak after close.

If a supported release platform cannot run H2 reliably, either keep H2 experimental globally or explicitly scope the supported H2 platform set. Do not infer runtime support from another OS.

## Track J — Load, memory, and resource characterization

Use `h2load` or equivalent nghttp2 tooling for concurrency/resource characterization.

Measure representative:

- small static response throughput/latency;
- many concurrent streams on a small number of connections;
- large file transfer;
- service-admission exhaustion;
- reset churn;
- memory near configured stream/header/send-buffer limits.

There is no arbitrary requests-per-second promotion threshold. Acceptance is:

- no pathological regression from H1 or previous H2 baselines without explanation;
- no unbounded memory/task/permit growth;
- configured resource ceilings behave approximately as documented;
- local/SBC-oriented defaults remain reasonable.

Record methodology and host characteristics so future releases can compare trends.

## Track K — Security and supply-chain review

Run:

```bash
cargo audit
cargo deny check
cargo tree -p eggserve-core -e features
```

Review current advisories and relevant open issues in Hyper, h2, rustls, and transitive H2 dependencies.

Also verify:

- no raw request/header values are introduced into operational logs by new qualification seams;
- protocol errors do not expose dependency internals to clients;
- no new default dependency is added merely for qualification tooling;
- external tools such as h2spec/nghttp/h2load remain release/developer tools, not runtime dependencies.

## Track L — CI versus release qualification

### L1. Routine CI

Keep existing deterministic H2 tests in normal CI. Add only cheap, reliable regressions discovered during this effort.

Do not put browsers, h2spec, h2load, or large stress runs in every PR unless they are demonstrably stable and cheap.

### L2. Reproducible promotion/release harness

Extend or replace `scripts/qualify-http2.sh` so it can express evidence requirements explicitly. Preferred environment gates include concepts such as:

- require one independent H2 client;
- require two independent implementation families;
- require browser evidence;
- require platform/runtime evidence supplied by the caller/environment.

The script must fail closed when a mandatory promotion client is absent. “Tool not installed” must never be reported as a passing supported-tier gate.

Record tool versions and evidence in a release qualification document.

## Track M — Documentation and support-tier transition

If every mandatory gate passes, update all live support claims together, including as applicable:

- `README.md`;
- `plans/ROADMAP.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- `docs/api-stability.md`;
- `docs/deployment.md`;
- `docs/downstream-app-server.md`;
- `docs/timeout-reference.md`;
- `architecture/http2.md`;
- `architecture/tls.md`;
- Plan 186/190 qualification cross-links;
- release/migration notes for the next actual release.

Use precise language:

- H2 is **supported when the opt-in `http2` feature is enabled**;
- H1 remains the default/minimal protocol;
- Python compatibility remains HTTP/1.1-shaped;
- H2 does not become default-enabled merely because support is promoted;
- producer/poll-progress and connection-fallback semantics remain explicit unless a real public stream-progress API has replaced them.

If one or more mandatory gates fail, keep H2 experimental and document only the remaining blocker. Do not partially promote documentation.

## Track N — Closure record

Create:

`release/plan-191-http2-supported-tier-qualification.md`

Record:

- candidate SHA;
- dependency versions;
- standards matrix version/date;
- independent client implementation families and versions;
- browser(s);
- platforms/architectures;
- h2spec result and RFC 9113 discrepancy classification;
- multiplexing/reset/header/flow-control/GOAWAY results;
- load/resource measurements;
- supply-chain results;
- remaining known limitations;
- final tier: `supported, opt-in` or `experimental`.

## Verification

Run the normal deterministic project matrix first. Minimum expected shape:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets
cargo +1.88 check --workspace --all-targets --features http2,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
bash scripts/test-python-wheel.sh
cargo audit
cargo deny check
```

Then run the external promotion matrix, including:

- `scripts/qualify-http2.sh` in evidence-required mode;
- at least two independent implementation families;
- browser qualification;
- h2spec/current standards classification;
- h2load/resource characterization;
- mandatory platform runtime runs.

Exact commands may vary by tool version and platform and should be recorded in the closure record rather than frozen here.

## Acceptance criteria

- [ ] current RFC 9113/HTTP semantics/HPACK/ALPN/TLS responsibility matrix is documented.
- [ ] current Hyper/h2 release notes, advisories, and relevant open issues are triaged.
- [ ] at least two independent H2 implementation families interoperate with EggServe; two libnghttp2 frontends do not satisfy this alone.
- [ ] at least one current browser successfully negotiates and uses H2 with EggServe.
- [ ] TLS ALPN `h2`, H1 fallback, and cleartext prior knowledge behave as documented.
- [ ] Upgrade-based `h2c` remains unsupported and is not accidentally reintroduced.
- [ ] GET/HEAD/error/conditional/range/large-stream behavior is correct through independent clients.
- [ ] multiplexing at and around the configured stream limit is bounded and sibling isolation holds.
- [ ] body rejection, body timeout, service error, reset, and disconnect paths release permits/lifecycles exactly once.
- [ ] header/HPACK pressure respects decoded limits without unbounded memory growth.
- [ ] representative malformed frame/state cases produce correct stream/connection scope or are explicitly delegated to Hyper/h2.
- [ ] h2spec results are recorded and interpreted against RFC 9113 rather than treated as an obsolete certification oracle.
- [ ] GOAWAY/graceful shutdown/max-request drain is qualified under concurrent work.
- [ ] H2 response-stall documentation matches the actual public Hyper capability; unsupported stream-local wire-progress claims are absent.
- [ ] response flow-control stalls remain bounded by explicit send buffers and connection fallback/lifetime policy.
- [ ] Linux x86-64, macOS, and Windows receive actual H2 runtime qualification; Linux aarch64 is recorded when available.
- [ ] h2load/resource characterization shows no unbounded memory/task/permit growth or unexplained pathological regression.
- [ ] `cargo audit`, `cargo deny check`, and feature/dependency review pass or have documented accepted exceptions consistent with project policy.
- [ ] routine CI remains proportionate; external promotion evidence is reproducible but not forced into every PR.
- [ ] Python compatibility remains HTTP/1.1-shaped and no H2 Python compatibility API is added.
- [ ] default/minimal builds remain HTTP/1-only and do not gain H2 dependencies by default.
- [ ] if and only if all mandatory gates pass, live docs promote native H2 to **supported, opt-in**.
- [ ] closure record captures exact evidence and final support tier.
- [ ] no WebSocket, extended CONNECT, server push, proxy, routing, middleware, upload, ACME, app-server, or unrelated feature enters scope.

## Suggested execution order

1. Freeze candidate/dependency/tool/platform inventory and current standards matrix.
2. Triage current Hyper/h2 issues and advisories.
3. Harden `qualify-http2.sh` so missing mandatory evidence fails closed.
4. Establish two independent client families plus a browser.
5. Run TLS ALPN/H1 fallback/prior-knowledge semantic matrix.
6. Run h2spec and classify results against RFC 9113.
7. Exercise multiplexing, resets, body policy, admission, and sibling isolation.
8. Exercise HPACK/header/resource boundaries.
9. Exercise response flow-control stalls and validate the documented fallback contract.
10. Exercise GOAWAY/drain/max-request/shutdown races.
11. Run load/resource characterization.
12. Run mandatory platform runtime qualification.
13. Re-run deterministic/MSRV/Python/supply-chain gates.
14. Write the Plan 191 closure record.
15. Promote documentation only if every mandatory supported-tier gate passes; otherwise retain experimental with a concise blocker record.

## Handoff

This is intentionally a qualification-led plan. Do not rewrite the H2 runtime merely to produce more code. The current architecture should survive the promotion effort mostly unchanged.

A successful Plan 191 result is **supported, opt-in HTTP/2** with H1 still the default and with the response-stall limitation stated truthfully. A failed/incomplete result is simply a better-documented experimental H2 tier with the remaining evidence blocker isolated for future work.