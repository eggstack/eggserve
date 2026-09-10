# Plan 193 — HTTP/3 Supported-Tier Promotion Qualification

## Status

**PLANNED — final evidence-led support-tier decision after Plan 192 dependency readiness.**

Prerequisite: Plan 192 must close with `READY FOR PLAN 193` against a frozen H3 dependency/runtime candidate. If Plan 192 is `BLOCKED`, do not execute this plan until its blocker is resolved through a later narrow readiness update.

Baseline when written: `main` at or after the Plan 190 closure and Plans 191–192 planning handoff. Re-read the Plan 192 readiness record, exact dependency set, standards matrix, upstream issues, and current qualification scripts before execution.

This plan attempts to promote native Rust HTTP/3 from **experimental** to **supported, opt-in**. It does **not** default-enable H3, add a Python H3 compatibility API, stabilize the experimental `server` module, or authorize WebTransport/datagrams/extended CONNECT/proxy/edge-server features.

## Purpose

Provide the independent interoperability, adversarial protocol, network impairment, platform runtime, resource, shutdown, and browser evidence that Plans 188 and 190 intentionally lacked.

The target result is one of two explicit outcomes:

1. **SUPPORTED, OPT-IN** — every mandatory gate passes against the frozen Plan 192 candidate; or
2. **EXPERIMENTAL** — one or more mandatory gates remain unavailable or fail, with the exact blocker recorded.

There is no “mostly supported” state. Missing mandatory evidence is not a pass.

## Support contract being qualified

A successful Plan 193 may support only the H3 subset EggServe actually implements:

- HTTP semantics through the canonical EggServe request/service/response boundary;
- H3 over QUIC/UDP with TLS 1.3 and `h3` ALPN;
- GET/HEAD and whatever custom-service methods the canonical Rust service boundary already supports;
- bounded request-body Reject/Buffer/Stream behavior;
- bounded header/field-section and transport resource policy;
- stream-scoped ordinary request failures/cancellation where the selected public APIs permit;
- connection-wide cancellation on unusable connection state and forced shutdown;
- canonical runtime errors and response normalization;
- H3 GOAWAY/graceful drain;
- runtime-owned Alt-Svc advertisement from TCP/H1/H2 responses when configured;
- TCP/H1/H2 fallback if H3 is unavailable to a client.

The supported H3 contract does **not** include:

- 0-RTT application requests;
- WebTransport;
- H3 datagrams;
- extended CONNECT or WebSocket-over-H3;
- server push;
- proxying;
- DNS HTTPS/SVCB publication;
- ACME/certificate automation;
- routing/middleware/application workers;
- Python `http.server` H3 behavior.

## Normative/delegated standards basis

Use the current Plan 192 standards/ownership matrix. At minimum it should cover:

- RFC 9110 — HTTP semantics;
- RFC 9114 — HTTP/3;
- RFC 9000 — QUIC transport;
- RFC 9001 — QUIC TLS usage;
- RFC 9002 — QUIC loss detection/congestion control;
- RFC 9204 — QPACK;
- RFC 7838 — Alt-Svc;
- RFC 7301 — ALPN;
- RFC 9846 or the then-current TLS 1.3 specification.

Do not duplicate Quinn/h3's full QUIC/QPACK implementation tests. Qualify the subset at every point EggServe configures, adapts, limits, advertises, times out, or exposes as a support guarantee.

## Mandatory independent implementation policy

### Two independent non-Quinn H3 stacks

Promotion requires successful interoperability with **at least two HTTP/3 implementation families that do not share EggServe's Quinn/h3 server stack**.

Preferred current candidates when this plan was written:

- ngtcp2 + nghttp3 client tooling;
- aioquic-based H3 client tooling.

Alternatives are acceptable if maintained and genuinely independent, for example a current quiche-based ordinary client. Record the actual protocol library behind each frontend.

Do not count:

- two command-line programs backed by the same H3 library as two implementations;
- an in-tree h3-quinn client as independent evidence;
- browser variants that share the same networking implementation as separate protocol families.

The deterministic in-process Quinn/h3 tests remain valuable regression evidence but do not satisfy this gate.

## Track A — Candidate freeze and evidence inventory

Before external testing, record:

- Plan 192 readiness result and candidate commit;
- exact h3/h3-quinn/Quinn/rustls versions;
- Rust stable and MSRV;
- H3/QUIC feature flags;
- certificate setup and trust path;
- two independent H3 implementation families and versions;
- adversarial H3 client/tool version;
- browser/version used for Alt-Svc;
- operating systems/architectures;
- network impairment environment/tooling;
- exact default and stress-limit configurations.

If the H3 dependencies change after Plan 192, return to the relevant Plan 192 gates before continuing. Do not qualify one stack and release another.

## Track B — Direct independent-client semantic matrix

Run the following against **both required independent H3 implementations**.

### B1. Basic HTTP semantics

Verify:

- QUIC/TLS handshake and `h3` ALPN;
- certificate verification enabled;
- GET 200;
- HEAD parity/no body;
- 404;
- 405 and `Allow` semantics;
- conditional response / 304;
- single-range 206;
- unsatisfiable range 416;
- large static file;
- canonical response headers without H1 hop-by-hop/framing leakage;
- repeated requests on one connection;
- several concurrent request streams.

### B2. Canonical custom-service behavior

Using a small test service, verify through each independent client:

- bounded request body Buffer;
- request body Stream;
- Reject with DATA and no `Content-Length` where the client can construct it;
- normal streaming response;
- generic 4xx/5xx runtime/service errors;
- lifecycle/cancellation after client reset/disconnect;
- sibling request success during one failed request.

The goal is to qualify the reusable service substrate, not only the built-in static handler.

### B3. Connection reuse and cleanup

Verify after ordinary request completion, rejected requests, client cancellation, and service errors that:

- connection remains reusable when protocol semantics permit;
- request/file/service permits return to baseline;
- closed streams are not retained indefinitely;
- counters do not double-decrement or leak.

## Track C — Browser Alt-Svc qualification

At least one current mainstream browser implementation must demonstrate the real discovery path.

Preferred candidates: current Chromium/Chrome and/or Firefox. Use whichever offers trustworthy protocol instrumentation in the test environment.

Qualification sequence:

1. access EggServe over HTTPS/TCP;
2. verify the response contains the runtime-owned, correct-port H3 Alt-Svc advertisement when enabled;
3. allow the browser to cache/discover H3 according to its policy;
4. verify a subsequent request actually uses HTTP/3/QUIC rather than merely succeeding over H2/H1;
5. make UDP/H3 unavailable while leaving TCP reachable;
6. verify the browser falls back without making the origin unusable;
7. re-enable H3 and verify future discovery/use remains functional within normal cache behavior.

Also verify:

- Alt-Svc is absent when advertisement is disabled;
- privacy/header denylist suppresses it;
- dynamic port `:0` advertises the actual resolved endpoint;
- application response headers cannot override the runtime-owned value.

If reliable browser H3 instrumentation cannot be obtained, H3 remains experimental.

## Track D — Adversarial HTTP/3 frame/state qualification

Use a maintained independent frame-level H3 exerciser such as Cloudflare `h3i` or an equivalent tool that can intentionally manipulate H3 stream/frame ordering and QUIC stream resets.

Exercise representative cases from the current RFC 9114/QPACK ownership matrix:

- duplicate control streams;
- duplicate QPACK encoder stream;
- duplicate QPACK decoder stream;
- missing/closed critical streams;
- unknown unidirectional stream types;
- invalid SETTINGS placement/duplication/value combinations;
- request-only frames on control streams and control-only frames on request streams;
- illegal pseudo-header ordering/content;
- connection-specific HTTP fields;
- invalid `TE` values;
- oversized field sections;
- malformed or invalid request message sequences;
- request RESET_STREAM;
- STOP_SENDING;
- reset/stop races during body receive and response send;
- GOAWAY sequencing and new-stream races.

For every case record whether RFC/dependency semantics require:

- stream error;
- connection error;
- ignored extension behavior;
- HTTP response before commitment.

Acceptance requires EggServe not to turn a dependency-defined stream error into unnecessary connection-wide failure, and not to keep a connection alive when the protocol requires a connection error.

Do not add unsupported H3 extensions merely because the adversarial tool can generate them.

## Track E — QPACK/header pressure

Using the independent/adversarial clients, exercise:

- many fields;
- large individual fields;
- compressed field sets that expand near the configured decoded field-section limit;
- repeated field sets to exercise dynamic compression state;
- QPACK blocked-stream pressure up to the selected dependency bounds;
- control/QPACK stream reset behavior;
- authority/path lengths near canonical EggServe ceilings.

Verify:

- decoded field limits are enforced before service invocation;
- EggServe canonical aggregate limits remain effective after H3 decode;
- dependency-managed QPACK state remains within the Plan 192 documented envelope;
- one oversized request cannot cause unbounded process growth;
- sibling streams survive stream-local errors;
- no QPACK state leak persists after connection close.

Do not write a QPACK codec/fuzzer in EggServe.

## Track F — Stream/concurrency/resource pressure

Exercise the default `max_concurrent_bidi_streams = 100` and nearby boundary cases.

Verify:

- 100 simultaneous request streams can be admitted at the QUIC/H3 layer when higher-level service admission permits;
- attempts beyond advertised transport limits are bounded by Quinn/H3 behavior rather than queued unboundedly in EggServe;
- `max_in_flight_requests` remains a separate global application/service budget;
- `max_file_streams` remains a separate global file budget;
- repeated stream open/reset cycles do not grow retained state without bound;
- connection-level receive/send windows limit aggregate buffering;
- per-stream receive and response-send buffers behave near documented limits;
- global `max_connections` is shared correctly between TCP and QUIC;
- `max_pending_handshakes` bounds H3 handshakes without bypassing global connection admission.

Measure memory/task/counter state at idle, near limits, after reset churn, and after all clients disconnect.

## Track G — Response flow-control and slow-client qualification

### G1. One stalled response, healthy sibling

Create two or more concurrent H3 responses. Withhold receive credit/consumption for one while allowing another to progress.

Verify:

- the stalled stream's send operation reaches EggServe's documented no-progress timeout behavior;
- the affected send side is terminated at stream scope when the selected APIs support it;
- healthy siblings continue;
- sibling progress does not refresh the stalled stream's deadline;
- memory stays bounded by the configured QUIC/H3/application send windows/buffers.

### G2. Slow but progressing response

Consume a large file/stream slowly while continuing to grant enough flow-control credit for progress.

Verify steady progress does not cause a false timeout and the entire response is not buffered in memory.

### G3. Application producer stall

Exercise a canonical streaming response producer that stops yielding data after response commitment.

Verify behavior matches the Plan 192 timeout/lifetime decision and cannot pin resources indefinitely outside the documented contract.

## Track H — Request body flow control and cancellation

Exercise:

- body DATA sent slowly but within the deadline;
- body DATA stopped mid-request;
- peer reset during body read;
- peer connection close during body read;
- body exceeding runtime limit;
- declared-length underrun/overrun where constructible;
- Reject with positive Content-Length;
- Reject with absent/zero Content-Length plus DATA;
- Buffer and Stream policy cancellation.

Verify:

- body timeout/cancellation releases request/service resources;
- only the affected request lifecycle is cancelled for stream-local failures;
- connection-level loss wakes all remaining request lifecycles;
- service invocation remains suppressed for rejected bodies;
- sibling streams continue after ordinary stream-local body failures.

## Track I — GOAWAY, graceful shutdown, and rolling-drain behavior

Qualify H3 shutdown under:

- idle connection;
- one active request;
- many concurrent requests;
- request body in progress;
- response body in progress;
- one flow-control-stalled response;
- new requests racing with GOAWAY;
- `max_requests_per_connection` threshold;
- grace-period expiry;
- peer connection close during server drain.

Verify:

- GOAWAY is issued according to H3 semantics;
- already accepted requests can complete within the configured grace period;
- inappropriate new requests are refused/drained rather than silently accepted indefinitely;
- forced shutdown cancels remaining lifecycle waiters before task abort;
- connection/request/file/service resources return to baseline exactly once.

Use independent clients where possible rather than only the same h3-quinn test client.

## Track J — Network impairment qualification

This is a **mandatory H3 promotion gate**.

Use a disposable Linux network namespace/VM/container host or equivalent safe environment with `tc netem` or another standard network emulator. Do not add custom packet-manipulation code to EggServe.

Exercise bounded combinations of:

- packet loss;
- latency;
- jitter;
- reordering;
- constrained MTU;
- UDP blackhole while TCP remains reachable.

### J1. Moderate recoverable impairment

Verify valid H3 requests still complete when Quinn recovers within configured timeout budgets.

### J2. Sustained/severe impairment

Verify connections/streams eventually time out and all EggServe resources release.

### J3. Sibling isolation

Under impairment plus concurrent streams, verify one stalled/retransmitting stream does not incorrectly refresh EggServe's per-stream application deadlines or corrupt sibling request state.

### J4. MTU smoke

Run at least one constrained-MTU scenario around QUIC's practical minimum datagram behavior. Treat detailed PMTU discovery/congestion control as Quinn ownership unless EggServe explicitly configures it.

### J5. UDP fallback path

Block UDP/QUIC while TCP remains healthy. Verify direct H3-only clients fail as expected while Alt-Svc-capable/browser clients can fall back to H2/H1 according to their behavior. EggServe must not make the TCP origin unhealthy merely because UDP is impaired after startup.

## Track K — QUIC transport interoperability evidence

Where practical, integrate EggServe into the current QUIC Interop Runner or equivalent cross-implementation harness.

At minimum exercise relevant tests for:

- handshake/version negotiation as supported by the selected stack;
- HTTP/3 transaction;
- address validation/anti-amplification behavior where the harness can observe it;
- transfer under loss/reordering;
- graceful close.

This does not replace EggServe-specific H3 tests. It is additional evidence that the Quinn integration remains interoperable outside ordinary happy-path clients.

If runner integration is impractical, document the reason and provide equivalent independent-client + network-emulation evidence. Promotion still requires network impairment and two independent H3 implementation families.

## Track L — Platform runtime qualification

Mandatory supported-tier runtime targets:

- Linux x86-64;
- macOS on a currently supported Apple architecture;
- Windows x86-64.

Strongly recommended:

- Linux aarch64 on representative SBC/server hardware.

Every mandatory platform must run actual UDP/QUIC/H3 traffic, not compile-only checks.

Minimum per-platform runtime smoke:

- H3 startup with TLS identity;
- direct H3 GET/HEAD using an independent client available on that platform or remote test client;
- large response;
- concurrent streams;
- one cancellation/reset/body case;
- graceful shutdown;
- H1/H2 TCP fallback remains healthy;
- no resource leak after client close.

Platform-specific UDP/socket behavior must not be inferred from Linux.

If one release-supported OS cannot run H3 reliably, either keep H3 experimental globally or define an explicit narrower supported H3 platform set in the release contract. Do not silently claim parity.

## Track M — Security/privacy review

Review qualification and runtime output for:

- QUIC connection IDs;
- stateless reset tokens;
- TLS key material/secrets;
- raw packet payloads;
- raw authority/path/body values;
- dependency-internal error strings reflected to clients;
- high-cardinality untrusted transport fields in default logs.

Verify:

- canonical errors remain generic;
- Alt-Svc obeys privacy/denylist policy;
- malformed H3 traffic cannot turn on verbose sensitive logging by default;
- qualification-only packet capture/logging is clearly opt-in and excluded from product defaults.

Re-run current advisories/open-issue review from Plan 192 immediately before final promotion.

## Track N — Performance and memory characterization

Measure H1/H2/H3 on the same host/content/build where practical:

- small static request latency/throughput;
- many concurrent small requests;
- large static transfer;
- handshake-heavy short connections;
- memory at configured stream/window limits;
- CPU during steady H3 traffic;
- reset/cancellation churn.

There is no arbitrary throughput threshold. Promotion requires:

- no unexplained pathological implementation regression;
- no unbounded memory/task/resource growth;
- defaults remain reasonable for local/SBC use;
- H3 feature cost remains isolated from minimal/default builds.

Record measurements as characterization, not marketing claims.

## Track O — Supply chain, MSRV, and feature graph

Run:

```bash
cargo audit
cargo deny check
cargo tree -p eggserve-core -e features
cargo +1.88 check --workspace --all-targets --features http3,tls
```

Confirm the frozen Plan 192 dependency set is what is actually tested/released.

If an upstream security/correctness fix requires a dependency or MSRV change during Plan 193, return to Plan 192 readiness rather than silently changing the candidate under qualification.

Minimal/default builds must remain free of `h3`, `h3-quinn`, and Quinn.

## Track P — Qualification harness and reproducibility

Harden `scripts/qualify-http3.sh` and companion documentation so supported-tier mode fails unless all mandatory evidence classes are supplied.

The promotion path should be able to require:

- two independent H3 implementation families;
- one adversarial H3 client;
- browser Alt-Svc evidence;
- network impairment evidence;
- Linux/macOS/Windows runtime evidence;
- Plan 192 readiness candidate identity.

Do not encode fake success flags. External/manual evidence may be supplied by recorded artifacts/logs from separate hosts, but the closure record must identify exactly what ran where.

Routine CI should retain deterministic same-stack tests and compile representatives; browsers, privileged netem, multi-OS H3 runtime, and heavy external-client matrices may remain workflow-dispatched/manual release qualification.

## Track Q — Final documentation transition

Only after every mandatory gate passes, update the live documentation together:

- `README.md`;
- `plans/ROADMAP.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- `docs/api-stability.md`;
- `docs/deployment.md`;
- `docs/downstream-app-server.md`;
- `docs/security-policy.md` / security review where transport assumptions are stated;
- `docs/timeout-reference.md`;
- `architecture/http3.md`;
- `architecture/tls.md`;
- Plan 188/190/192 qualification cross-links;
- release/migration notes for the next actual release.

Supported wording must be narrow and explicit:

- native H3 is **supported when the opt-in `http3` feature is enabled and an H3 TLS identity is configured**;
- H3 remains disabled by default;
- Python compatibility remains HTTP/1.1-shaped;
- Alt-Svc advertisement remains opt-in/runtime-owned;
- unsupported extensions remain unsupported;
- any platform restriction is named explicitly;
- dependency limitations accepted during Plan 192 remain documented.

If any mandatory gate is incomplete or fails, do not partially promote documentation. Leave H3 experimental and write the blocker in the closure record.

## Track R — Closure record

Create:

`release/plan-193-http3-supported-tier-qualification.md`

Record:

- Plan 192 readiness record and candidate SHA;
- exact dependency versions;
- standards matrix date;
- independent H3 implementation families/versions;
- adversarial tool/version;
- browser/version and Alt-Svc evidence;
- operating systems/architectures;
- network emulation parameters;
- QUIC interop harness evidence if used;
- semantic/adversarial/resource/shutdown results;
- memory/performance characterization;
- advisory/upstream issue re-check;
- known limitations;
- final tier: `supported, opt-in` or `experimental`.

## Verification

Run the deterministic project matrix before and after any defect fix:

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

Then run the evidence-required external H3 matrix. Existing strict modes such as `EGGSERVE_REQUIRE_H3_CLIENTS` / `EGGSERVE_REQUIRE_TWO_H3_CLIENTS` may be extended or replaced as needed, but supported-tier mode must fail closed for all mandatory evidence classes.

## Acceptance criteria

- [ ] Plan 192 closed `READY FOR PLAN 193` against the exact dependency candidate being tested.
- [ ] at least two independent non-Quinn H3 implementation families successfully interoperate with EggServe.
- [ ] direct independent clients pass GET/HEAD/error/conditional/range/large-response/reuse/multiplexing semantics.
- [ ] custom-service request-body and streaming-response behavior is exercised outside the in-tree Quinn client.
- [ ] a current mainstream browser discovers H3 through EggServe's real Alt-Svc path and demonstrably uses QUIC/H3.
- [ ] browser/client fallback remains functional when UDP/H3 becomes unavailable while TCP remains reachable.
- [ ] Alt-Svc port-0, privacy suppression, disabled state, and runtime ownership remain correct.
- [ ] an independent adversarial H3 tool exercises critical/control/QPACK/SETTINGS/frame-placement/message/reset/GOAWAY cases.
- [ ] adversarial cases fail at the RFC-required stream or connection scope without unbounded resource retention.
- [ ] QPACK/header pressure remains within the Plan 192 documented resource envelope.
- [ ] 100 default concurrent request streams and near-boundary behavior are bounded; global service/file/connection budgets remain distinct and leak-free.
- [ ] request body slow/stall/reset/limit/rejection cases cancel and release resources correctly while healthy siblings survive stream-local failures.
- [ ] response flow-control stall affects the correct stream scope where supported and cannot grow memory without bound.
- [ ] slow-but-progressing large responses complete without false no-progress timeout.
- [ ] application response-producer stalls obey the documented Plan 192 timeout/lifetime contract.
- [ ] H3 GOAWAY/graceful drain/max-request/forced-shutdown races are qualified under concurrent work.
- [ ] controlled packet loss, latency, jitter, reordering, and constrained-MTU smoke tests do not reveal conflicts between EggServe timeouts and ordinary QUIC recovery.
- [ ] sustained impairment eventually releases all resources.
- [ ] UDP blackhole behavior leaves TCP/H1/H2 fallback origin service healthy.
- [ ] QUIC interoperability harness evidence is recorded where practical, or equivalent independent/network evidence is explicitly documented.
- [ ] Linux x86-64, macOS, and Windows run actual H3 traffic successfully; Linux aarch64 evidence is recorded when available.
- [ ] privacy/logging review finds no default leakage of QUIC/TLS secrets, IDs, packets, or raw untrusted request data.
- [ ] current upstream issues/advisories are re-checked and no Plan 192-relevant blocker reappears.
- [ ] memory/performance characterization shows no unbounded growth or unexplained pathological regression.
- [ ] supply-chain, MSRV, deterministic H1/H2/H3, and Python gates remain green.
- [ ] minimal/default dependency graph remains free of H3/QUIC dependencies.
- [ ] routine CI remains proportionate and broader promotion evidence is reproducible outside normal PR CI.
- [ ] Python `http.server` compatibility remains HTTP/1.1-shaped.
- [ ] H3 remains disabled by default even if promoted.
- [ ] if and only if every mandatory gate passes, live docs promote H3 to **supported, opt-in**; otherwise they retain experimental status.
- [ ] final closure record captures exact evidence, limitations, and support tier.
- [ ] no 0-RTT application requests, WebTransport, datagrams, extended CONNECT/WebSocket, push, proxy, DNS automation, routing, middleware, uploads, ACME, or app-server behavior enters scope.

## Suggested execution order

1. Confirm Plan 192 READY result and freeze exact candidate/dependencies.
2. Install/record two independent H3 implementation families plus adversarial tooling.
3. Run direct semantic matrix with both independent clients.
4. Run browser Alt-Svc discovery/use/fallback qualification.
5. Run adversarial H3 frame/control/QPACK/reset/GOAWAY cases.
6. Run QPACK/header and stream/resource pressure.
7. Run request-body and response flow-control/cancellation tests.
8. Run graceful drain/forced shutdown/max-request races.
9. Run controlled network impairment and UDP-blackhole tests.
10. Run QUIC interoperability harness where practical.
11. Run mandatory Linux/macOS/Windows H3 runtime smoke; add Linux aarch64 when available.
12. Run memory/performance characterization.
13. Re-check upstream issues/advisories and run supply-chain/MSRV/deterministic/Python gates.
14. Write the Plan 193 closure record.
15. Promote documentation only if every mandatory gate is satisfied; otherwise leave H3 experimental with a precise blocker list.

## Handoff

Plan 193 is the final H3 support-tier decision, not another feature implementation program.

Do not change protocol scope to make the qualification easier. Do not interpret the upstream `h3` crate's experimental/stable label as either automatic disqualification or automatic approval. The evidence must show that EggServe's exact used subset is interoperable, bounded, cancellation-safe, platform-qualified, and maintainable.

A successful result is **supported, opt-in HTTP/3** while H1 remains the default and Python compatibility remains HTTP/1.1-shaped. Any incomplete mandatory gate leaves H3 **experimental** with no shame and no fabricated support claim.