# Plan 188 — HTTP/3 Interoperability and Multi-Protocol Release Closure

## Status

**CLOSED — H3 remains experimental; the protocol program is closed with explicit evidence gaps.**

Prerequisites: Plan 187 deterministic HTTP/3 implementation tests green; Plan 186 has already established the HTTP/2 support tier and qualification method.

## Purpose

Determine whether EggServe's HTTP/3 implementation is safe and interoperable enough to call supported, and close the overall HTTP/1+HTTP/2+HTTP/3 program with truthful feature, packaging, platform, security, and documentation claims.

This plan is intentionally qualification-heavy. It should not add new HTTP/3 extensions. Findings may cause narrow corrections to Plan 187 behavior/configuration, but WebTransport, datagrams, extended CONNECT, 0-RTT application semantics, server push, proxying, ACME, routing, and middleware remain out of scope.

## Qualification principles

HTTP/3 needs evidence beyond a local happy-path client because QUIC adds transport state that HTTP/1/2 do not exercise:

- UDP reachability and listener behavior;
- TLS 1.3/QUIC handshake state;
- connection and stream flow control;
- QPACK state;
- stream cancellation/reset;
- packet loss/reordering;
- idle/path behavior;
- client discovery/fallback;
- per-connection and pending-handshake resource pressure.

Qualification should test those dimensions without turning routine CI into a full Internet/network-emulation lab.

## Track A — Standards and implementation ownership review

### A1. Build the H3/QUIC standards checklist

Map supported behavior against the currently applicable standards, at minimum:

- RFC 9110 HTTP semantics;
- RFC 9114 HTTP/3;
- QUIC transport RFCs used by the selected implementation;
- QPACK requirements used by the selected H3 implementation;
- TLS 1.3/QUIC requirements;
- Alt-Svc semantics where used for H3 discovery.

Re-check current specifications/errata at implementation time. Record which behavior is delegated to the H3/Quinn stack and which is EggServe-owned.

### A2. Confirm explicit non-features

Verify and document that initial EggServe H3 does not provide:

- 0-RTT application requests;
- WebTransport;
- H3 datagrams;
- CONNECT tunnels;
- WebSocket-over-H3;
- server push;
- DNS HTTPS/SVCB management;
- reverse proxying.

The H3 dependency stack may contain support for some of these internally; that does not make them EggServe capabilities.

## Track B — Independent HTTP/3 client interoperability

### B1. Select current maintained clients

At qualification time, select at least two independent current client implementations. Candidates may include:

- a current `curl` build with HTTP/3 support;
- an ngtcp2/nghttp3 client;
- a quiche-based client;
- a browser implementation.

Do not make stale binary/package versions permanent requirements. Record the exact versions/build features used in the qualification result.

At least one client should not share Quinn/H3 implementation code with the server, otherwise a shared bug can masquerade as interoperability.

### B2. Direct H3 requests

Exercise direct H3 connectivity for:

- GET/HEAD;
- redirect;
- conditional GET;
- range response;
- large static file;
- generic buffered response;
- generic streaming response;
- request body Buffer/Stream if the test service allows bodies;
- graceful close/drain.

Verify response semantic headers against H1/H2 equivalents, excluding protocol-specific framing.

### B3. Alt-Svc discovery

Using a client that supports Alt-Svc discovery, prove the actual origin flow:

1. client connects over TCP H1/H2;
2. EggServe advertises the active H3 endpoint when policy enables it;
3. client subsequently establishes H3 over UDP;
4. disabling/suppressing advertisement prevents EggServe from adding the value;
5. an alternate H3 port advertises the actual port.

Do not claim general Internet reachability from loopback tests; this proves server advertisement semantics only.

### B4. Fallback behavior

Confirm that an H3-capable origin still serves H1/H2 when UDP/H3 is unavailable, because TCP and H3 listeners are independent. EggServe itself does not control client fallback, but its TCP listener and Alt-Svc behavior must not make fallback impossible.

## Track C — Multiplexing, cancellation, and isolation

### C1. Mixed stream outcomes

Within one H3 connection, run concurrent request streams with:

- normal success;
- slow upload;
- request-body rejection;
- handler timeout;
- response flow-control stall;
- response producer failure;
- client cancellation/reset;
- large file streaming.

Ordinary stream failure must remain isolated from siblings unless the H3/QUIC stack reports a connection-scoped protocol failure.

### C2. Deferred request-body lifecycle

Prove a deferred streaming body can outlive `Service::call()` return exactly as documented, and that body timeout/reset wakes that request lifecycle without incorrectly cancelling sibling requests.

### C3. Exactly-once release

After every cancellation/reset/timeout path, verify:

- service permits return;
- file-stream permits return;
- active request/stream counters return;
- producer tasks terminate;
- no body lifecycle remains falsely Active;
- connection permit is retained or released according to actual connection state.

## Track D — QUIC handshake and connection pressure

### D1. Pending handshake pressure

Exercise many incomplete/slow QUIC handshakes up to and beyond configured pending-handshake limits. Measure memory/task behavior and verify state is reclaimed after timeout.

### D2. Active connection pressure

Open QUIC connections up to and beyond the configured shared/per-protocol connection budget. Confirm the policy selected in Plan 187 is actually enforced and does not silently double resources relative to TCP.

### D3. Address spoofing/amplification posture

Using a safe local/test environment, review the selected Quinn anti-amplification and retry behavior. If stateless retry is enabled/configurable, qualify both resource benefit and normal-client interoperability.

Do not implement bespoke QUIC cookies/tokens in EggServe if the transport library already owns that mechanism.

### D4. Connection ID/reset state

Review library defaults and retained-state limits related to QUIC connection IDs/stateless reset where configurable. EggServe should not override transport defaults without a concrete hardening reason, but any exposed operator setting must be bounded.

## Track E — Stream/QPACK/header resource pressure

### E1. Bidirectional request stream pressure

Open streams rapidly to configured limits and verify bounded task/memory behavior. A peer must not create unbounded application tasks before stream/admission limits take effect.

### E2. Unidirectional/control stream pressure

Exercise peer-created unidirectional streams and protocol control/QPACK stream rules. The configured uni-stream limit must be high enough for valid H3 operation but low enough to prevent unbounded state.

### E3. Header/QPACK pressure

Exercise:

- large field sections;
- many fields;
- compressed fields that expand toward the decoded limit;
- dynamic-table/blocked-stream pressure up to configured bounds;
- oversized authority/path after decode.

Verify transport/H3 limits reject before excessive memory growth and EggServe's canonical aggregate limits still run before service invocation.

Do not write a second QPACK parser/fuzzer in EggServe; fuzz/test the adapter inputs and rely on the selected H3 library's parser plus dependency/security process for wire QPACK correctness.

## Track F — Flow control, slow peers, and no-progress timeout

### F1. Response flow-control stall

Stop increasing the receive window for one H3 response while sibling streams continue. Acceptance requires the stalled stream's no-progress timeout to fire independently.

### F2. Request flow-control/body timeout

Send request DATA slowly or stop mid-body. `body_read_timeout`/lifecycle policy must terminate the request stream within bounds without keeping service/application permits indefinitely.

### F3. Large file backpressure

Stream a large file to a slow but progressing client. Steady forward progress must not trigger the no-progress timeout, and the server must not buffer the entire file in memory.

### F4. Connection idle

With no active streams, verify QUIC/H3 connection idle policy closes within documented bounds. With a legitimate active stream, connection idle must not fire prematurely.

## Track G — Loss, reordering, and network impairment

HTTP/3's value depends on QUIC handling network behavior through the transport implementation. EggServe should qualify that its own timeout/lifecycle decisions do not fight normal QUIC recovery.

### G1. Controlled impairment environment

Where available, use a disposable Linux network namespace/VM or equivalent safe local harness to introduce bounded packet:

- loss;
- delay;
- jitter;
- reordering.

Use standard OS network-emulation facilities rather than custom packet mangling in EggServe.

### G2. Recovery behavior

Under moderate impairment, verify:

- valid requests still complete when QUIC recovers within configured timeouts;
- EggServe no-progress/idle budgets do not fire solely because of small transient loss periods inconsistent with the configured bound;
- severe sustained loss eventually releases resources;
- sibling stream isolation remains intact.

Do not tune QUIC congestion control or loss recovery in EggServe unless measurements show the library defaults violate the project's explicit resource/security policy.

### G3. MTU/path behavior

Run at least a smoke test under constrained MTU where practical. Treat detailed path-MTU/congestion tuning as Quinn ownership unless EggServe explicitly overrides it.

## Track H — Graceful shutdown/GOAWAY qualification

Qualify H3 drain under:

- no active streams;
- several active streams;
- active response flow control;
- deferred request body;
- new requests racing with GOAWAY;
- drain deadline expiration.

Verify new request acceptance stops according to H3 semantics while already accepted streams can complete within the grace period.

Qualify `max_requests_per_connection` as an H3 drain trigger under concurrent request races.

## Track I — Security/privacy review

### I1. Transport metadata leakage

Review logs/events/startup summaries for accidental exposure of:

- QUIC connection IDs;
- stateless-reset tokens;
- TLS secrets/key material;
- packet payloads;
- untrusted raw authority/path fields;
- internal H3/QPACK error details.

Keep default logs sanitized and low-cardinality.

### I2. Alt-Svc privacy

Verify privacy/minimal-fingerprint modes suppress H3 advertisement according to policy and that application services cannot override the runtime-owned decision.

### I3. Error responses

Application/service errors must remain generic and semantically consistent with H1/H2. QUIC/H3 transport errors after commitment should reset/close at the correct scope without attempting an invalid HTTP error body.

### I4. Dependency review

Re-run:

- `cargo audit`;
- `cargo deny check`;
- license review;
- feature tree inspection.

Record whether H3 substantially expands cryptographic/system dependency footprint and whether all new dependencies are justified by the optional protocol feature.

## Track J — Performance and memory qualification

### J1. Comparable protocol benchmark

On one host/build/content set, measure representative H1, H2, and H3 cases:

- small static request latency/throughput;
- many concurrent small requests;
- large static transfer;
- memory at configured connection/stream limits;
- CPU under handshakes and steady traffic.

Use results to catch pathological implementation mistakes, not to claim EggServe beats specialized edge servers.

### J2. H3 worst-case memory accounting

Estimate/measure memory from:

- active QUIC connection state;
- concurrent request streams;
- configured stream receive windows;
- connection receive window;
- send buffering;
- QPACK table/blocked-stream state;
- EggServe request/response buffers.

Tune defaults if the product of configured limits creates unreasonable memory exposure for the project's local/SBC deployment profile.

### J3. Binary size and compile graph

Compare minimal, H2, and H3 builds. H3 is expected to add dependencies; confirm the feature gate actually protects minimal users from that cost.

Do not default-enable H3 merely to simplify documentation if it materially harms the minimal binary/compile profile.

## Track K — Platform and packaging qualification

### K1. Rust platform builds

Ensure H3 feature builds on the release-supported Rust targets where the selected QUIC stack supports them. At minimum verify the project's primary Linux targets and representative macOS/Windows builds.

### K2. Runtime platform evidence

Run direct H3 runtime smoke tests on representative platforms available to the project. If UDP/QUIC runtime qualification is incomplete on a target, mark H3 platform-limited rather than extrapolating from compilation.

### K3. Python wheel policy

The Python six-class compatibility facade remains H1.1-shaped. Decide whether release wheels compile H3 code at all:

- preferred default is no H3 dependency in compatibility wheels unless `eggserve.lowlevel` explicitly exposes and supports it;
- if wheel-native CLI shares the same compiled core and H3 is included, measure wheel growth and prove compatibility classes still advertise/serve only their documented protocol behavior.

No new Python API is required to close this program.

### K4. CLI binary policy

Decide from measurements whether release CLI binaries include H2/H3 features by default or expose separate optional source-build features. Make the install/release documentation match actual artifacts.

## Track L — MSRV decision

Re-run the MSRV check for:

- minimal build;
- H2 feature;
- H3 feature.

If the selected maintained QUIC/H3 stack requires a Rust version newer than the current project MSRV, choose explicitly:

- raise the project MSRV as part of the planned 0.2 API/protocol transition; or
- document H3 as requiring a higher compiler if Cargo/packaging policy can support that clearly.

Prefer one truthful project-wide MSRV over confusing per-feature promises unless there is a compelling compatibility reason.

## Track M — Routine CI versus release qualification

### M1. Routine CI

Keep deterministic local H3 integration tests in CI if reliable and reasonably fast. At minimum compile/test the intended supported feature combination on Linux.

Do not require external Internet access, browsers, or privileged network emulation in routine CI.

### M2. Release qualification script/checklist

Create a reproducible documented qualification path for external H3 clients and optional network impairment. It may be manual or workflow-dispatched if infrastructure permits.

Record commands, versions, expected results, and environment assumptions. Do not pretend a manual test is routine CI.

### M3. Avoid matrix explosion

Do not create all combinations of H1/H2/H3 × TLS × Python × every OS in normal PR CI. Choose a small set of compile/test representatives and retain broader release qualification separately.

## Track N — Final documentation synchronization

After evidence is complete, update all live statements including:

- `README.md`;
- `plans/ROADMAP.md` if milestone/support language changed;
- `docs/non-goals.md`;
- `docs/library-capability-matrix.md`;
- `docs/api-stability.md`;
- `docs/deployment.md`;
- `docs/downstream-app-server.md`;
- `docs/security-policy.md` where transport threat assumptions change;
- `architecture/tls.md`;
- protocol/configuration documentation;
- release notes/migration guide for the 0.2 API transition.

Documentation must clearly separate:

- minimal/default protocol support;
- opt-in features;
- TLS/ALPN behavior;
- H3 UDP/Alt-Svc behavior;
- resource defaults;
- platform qualification;
- Python compatibility behavior;
- intentionally unsupported protocol extensions.

Search for stale `HTTP/1.1 only` and `No HTTP/2` claims and retain them only where they still describe a narrow surface such as Python compatibility.

## Track O — Final support-tier and release decision

Classify HTTP/3 as:

1. **supported** — deterministic, independent-client, adversarial/resource, shutdown, and representative platform evidence pass;
2. **experimental** — functional but meaningful hardening/interoperability/platform evidence remains incomplete;
3. **disabled/deferred** — the chosen stack cannot satisfy required safety/resource/lifecycle invariants.

Also restate H2's Plan 186 tier and the final default-feature decision.

The closure record should include:

- tested commit;
- dependency versions;
- client implementations/versions;
- platforms;
- feature flags;
- major resource defaults;
- known limitations;
- benchmark methodology/summary;
- support tier.

## Verification

Run all deterministic H1/H2/H3/TLS/Python checks and supply-chain gates. Expected core shape:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features http2,http3,tls
cargo test -p eggserve-bin --features http2,http3,tls
cargo audit
cargo deny check
bash scripts/test-python-wheel.sh
```

Run the selected MSRV commands and feature-tree/binary-size measurements.

Run the documented external HTTP/3 client qualification and, where available, the safe network-impairment qualification environment.

## Acceptance criteria

- [ ] H3 supported behavior is mapped against current HTTP/3/QUIC/QPACK/TLS standards and library ownership is explicit.
- [ ] at least two independent current H3 client implementations interoperate with the supported core behavior, with at least one not sharing the server's Quinn/H3 stack.
- [ ] Alt-Svc discovery uses the real active endpoint and direct/fallback TCP serving remains functional.
- [ ] mixed H3 stream failures/cancellation/timeouts are isolated correctly and all permits/lifecycles release exactly once.
- [ ] pending handshake, active connection, stream, header/QPACK, body, service, file, and response resources remain bounded under adversarial pressure.
- [ ] response and request flow-control stalls time out at the correct stream scope and cannot be masked by sibling traffic.
- [ ] controlled loss/reordering does not expose an obvious conflict between EggServe timeouts and ordinary QUIC recovery.
- [ ] graceful H3 GOAWAY/drain behavior is qualified under concurrent work.
- [ ] privacy/logging review finds no transport-secret/high-cardinality sensitive leakage and Alt-Svc obeys suppression policy.
- [ ] H3 dependency/security/license review passes and the minimal build remains free of QUIC/H3 dependencies when disabled.
- [ ] memory, CPU, binary-size, and compile-graph impact are measured and defaults are reasonable for local/SBC use.
- [ ] representative platform build/runtime status is documented truthfully.
- [ ] Python compatibility behavior remains narrow and documented; no accidental H2/H3 `http.server` semantics are introduced.
- [ ] project MSRV is truthful for the released feature policy.
- [ ] routine CI remains proportionate; broader H3 qualification is reproducibly documented.
- [ ] README, capability/stability/security/deployment/TLS/downstream docs and migration notes agree on the final H1/H2/H3 support tiers and feature defaults.
- [ ] the final closure record labels H3 supported, experimental, or deferred based on evidence rather than implementation existence.
- [ ] no 0-RTT application semantics, WebTransport, datagrams, CONNECT/WebSocket, server push, reverse proxy, ACME, routing, middleware, or DNS automation has entered scope.

## Suggested implementation order

1. Build the standards/delegation checklist and select independent H3 clients.
2. Run direct H3 and Alt-Svc interoperability.
3. Run concurrent stream/cancellation/lifecycle qualification.
4. Exercise handshake/connection/stream/QPACK/header pressure.
5. Exercise request/response flow control and slow-peer behavior.
6. Run safe loss/delay/reordering/MTU smoke qualification where infrastructure allows.
7. Qualify GOAWAY/graceful shutdown/request-threshold behavior.
8. Perform security/privacy and dependency review.
9. Measure H1/H2/H3 resource/performance/binary impacts and tune bounded defaults if necessary.
10. Perform representative platform/package qualification and make H3/default-feature/MSRV decisions.
11. Place deterministic tests in routine CI and document broader release qualification.
12. Synchronize all documentation and migration guidance.
13. Record final H2/H3 support tiers and close the protocol program.

## Handoff

Plans 183–188 are complete only when protocol support is represented as one canonical EggServe service/runtime model with protocol-specific transport adapters and evidence-backed support tiers. Any remaining gap should be recorded as a narrow follow-up plan rather than reopening broad server scope.

## Execution and closure decision

Plan 188 was executed on Linux x86_64 on 2026-09-09. The result is a
qualification closure, not a promotion of HTTP/3 to the supported tier:

- HTTP/3 remains **experimental**. The deterministic feature suite, bounded
  Quinn configuration, same-port startup, runtime-owned Alt-Svc policy,
  canonical request/response adapter, QUIC context metadata, deferred request
  body timeout cancellation, and H1 fallback checks are implemented and tested.
- H2 remains **experimental** under the Plan 186 decision. H1 remains the
  minimal/default protocol and the Python compatibility facade remains
  HTTP/1.1-shaped.
- The available host has curl 8.5.0 with HTTP/2 but without HTTP/3 support;
  nghttp3-client, quiche-client, browsers, and network-emulation tooling were
  unavailable. Consequently no independent direct-H3 client evidence or
  impairment/platform runtime evidence is claimed. The release record and
  [`scripts/qualify-http3.sh`](../scripts/qualify-http3.sh) make that gap
  reproducible and fail when required client evidence is requested.
- No H3 extension was added. 0-RTT application requests, WebTransport,
  datagrams, CONNECT/WebSocket tunnels, server push, DNS HTTPS/SVCB
  management, proxying, ACME, routing, middleware, and application-server
  behavior remain out of scope.

The detailed evidence, dependency versions, standards ownership, commands,
measurements, platform limits, and follow-up criteria are recorded in
[`release/plan-188-http3-qualification.md`](../release/plan-188-http3-qualification.md).
