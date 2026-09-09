# Plan 186 — HTTP/2 Conformance, Interoperability, and Release Closure

## Status

**CLOSED — H2 remains experimental after qualification; no HTTP/3 implementation in this plan.**

Closure record: [`release/plan-186-http2-qualification.md`](../release/plan-186-http2-qualification.md).
The deterministic H2+TLS suite and Linux `curl` wire qualification pass. The
feature is not promoted to supported because this environment lacked a second
independent H2 client and browser/platform runtime coverage, and Hyper's
public server API does not provide a safe per-stream reset hook for the
response-body adapter. Those limitations are documented and are not hidden by
the release surface.

Prerequisites: Plans 183–185 complete and the deterministic HTTP/2 implementation suite green. This plan determines whether EggServe may truthfully document HTTP/2 as supported rather than experimental/incomplete.

## Purpose

Qualify the new HTTP/2 runtime against standards, independent clients, adversarial resource cases, packaging/feature behavior, and the existing HTTP/1 compatibility contract.

The goal is evidence, not additional feature growth. Failures discovered here should produce narrow corrective work in the Plan 185 implementation or a scoped follow-up, not opportunistic addition of server push, WebSockets, proxying, middleware, or unrelated edge-server features.

## Qualification principles

- Test the wire, not only internal adapter functions.
- Exercise multiple streams on one connection; single-request smoke tests are insufficient.
- Distinguish stream-scoped failures from connection-scoped failures.
- Test flow-control/resource exhaustion behavior, not only valid happy paths.
- Keep hostile request data out of unsanitized logs.
- Preserve HTTP/1 behavior through the same release.
- Treat interoperability evidence as release support, not as a replacement for deterministic repository tests.
- Do not make routine CI disproportionately expensive for a small/local-first server; keep broad interoperability and platform qualification as targeted release/manual workflows where appropriate.

## Track A — Standards conformance inventory

### A1. Build an RFC behavior checklist

Map implemented HTTP/2 behavior against the currently applicable HTTP specifications, at minimum:

- RFC 9110 HTTP semantics;
- RFC 9113 HTTP/2;
- relevant TLS/ALPN requirements;
- the project's existing HTTP normalization/privacy invariants.

Record which behaviors are delegated to Hyper/Hyper-Util and which are EggServe-owned. Delegation still requires integration tests where EggServe configures limits or translates errors.

### A2. Explicit unsupported H2 features

Document and test the intended response to unsupported capabilities, including:

- server push (do not originate it; define handling of related peer settings/state through the library);
- extended CONNECT;
- WebSocket-over-H2;
- HTTP/1 Upgrade-based h2c;
- trailers if the canonical model still does not support them.

Unsupported behavior should fail safely and predictably without exposing a bypass around canonical normalization.

### A3. Header and pseudo-header correctness

Qualify:

- pseudo-header ordering/validation as enforced by the H2 library;
- method/path/scheme/authority translation;
- duplicate ordinary header preservation where HTTP permits it;
- lowercase transport representation without changing canonical case-insensitive semantics;
- forbidden connection-specific fields;
- `Content-Length` consistency;
- no `Transfer-Encoding` emission;
- `HEAD` and body-forbidden status handling.

## Track B — Independent-client interoperability

### B1. CLI clients

At minimum test with current maintained builds of:

- `curl` using HTTP/2 over TLS;
- an nghttp2 client such as `nghttp` if available in the qualification environment.

Re-check exact client versions at implementation time and record them in the closure artifact. Do not bake stale tool versions into permanent product requirements.

Exercise:

- TLS H2 negotiation;
- HTTP/1.1 ALPN fallback;
- static GET/HEAD;
- range and conditional requests;
- large streaming file;
- concurrent/multiplexed requests;
- response headers and privacy profile;
- graceful shutdown behavior visible to the client.

### B2. Browser interoperability

Perform a targeted smoke qualification with at least one current Chromium-family and one current non-Chromium implementation where practical. The goal is negotiation/static serving correctness, not browser automation coverage in routine CI.

Record only protocol/result/version evidence; do not make browser-specific behavior part of the stable API.

### B3. Downstream Rust service consumer

Extend or add a small external-consumer fixture that compiles against the public/experimental EggServe runtime and serves a generic canonical `Service` over H2 without importing Hyper types.

Prove the downstream app-server substrate remains transport-independent.

## Track C — Multiplexing and isolation qualification

### C1. Concurrent stream correctness

On one H2 connection, run streams that intentionally differ:

- fast success;
- slow body upload;
- handler timeout;
- rejected request body;
- large file download;
- response producer failure;
- client-cancelled stream.

Verify one stream's ordinary failure does not cancel siblings.

### C2. Admission layering

Test combinations of:

- per-connection H2 max concurrent streams;
- server-wide `max_connections`;
- server-wide `max_in_flight_requests`;
- `max_file_streams`;
- body limits.

The observed rejection/fairness behavior should match the documented layer ownership. No idle peer should reserve unbounded service/file permits.

### C3. Flow-control isolation

Create a client that stops advancing the receive window for one response stream while allowing another stream to continue.

Acceptance requires:

- the stalled stream hits the intended stream-level progress policy;
- sibling traffic does not refresh its timeout;
- sibling streams continue successfully where the H2 connection remains valid;
- permits/lifecycle state are released exactly once after reset/cancellation.

If this cannot be demonstrated with the chosen Hyper public API, HTTP/2 must remain experimental and the limitation must be documented rather than hidden.

## Track D — Adversarial protocol/resource cases

### D1. Stream creation pressure

Exercise rapid stream opens up to and beyond configured limits. Verify bounded memory/task growth and predictable refusal behavior.

### D2. Header pressure

Exercise:

- many small headers;
- large decoded header lists;
- headers near both H2 library and EggServe aggregate limits;
- oversized authority/path;
- invalid/forbidden connection headers.

Ensure no service invocation occurs for pre-service rejection cases.

### D3. Reset/cancellation pressure

Exercise repeated client stream resets and any library-supported local-error/reset accounting protections. Verify limits are not disabled casually and that reset storms do not create unbounded retained stream state.

### D4. Slow and stalled peers

Test:

- connection established but no usable request progress;
- slow request DATA;
- stalled response flow control;
- graceful-drain peer that stops reading;
- repeated partial requests/stream cancellation.

All should release resources within documented bounds.

### D5. Error/privacy behavior

Malformed/protocol errors and service failures must produce sanitized events. Client responses must never contain Hyper error text, panic payloads, filesystem details, certificate paths, or internal protocol state.

## Track E — Graceful shutdown and GOAWAY qualification

Test graceful shutdown under:

- no active streams;
- one active response;
- multiple active streams;
- one deferred request body;
- new stream attempts racing with drain;
- drain deadline expiration.

Verify:

- new work is refused after drain begins according to H2 semantics;
- accepted work can complete within the configured grace;
- late/unfinished work is cancelled after the bound;
- `RequestLifecycle` cancellation reasons remain coherent;
- server task/connection permits return to zero.

Also test `max_requests_per_connection` as a GOAWAY/drain trigger under concurrent arrivals, including boundary races.

## Track F — TLS/ALPN and deployment qualification

### F1. ALPN matrix

Qualify at least:

| Server mode | Client offer | Expected result |
|---|---|---|
| H1 only | `http/1.1` | H1 success |
| H1 only | `h2` only | handshake/protocol failure per configured TLS behavior |
| H1+H2 | `h2,http/1.1` | H2 |
| H1+H2 | `http/1.1` | H1 |
| H1+H2 | unsupported token only | no accidental protocol coercion |

Confirm protocol metadata reports the negotiated result, not the client's first offer.

### F2. Reverse-proxy deployment documentation

Update deployment docs to describe that an external proxy may terminate H2 while EggServe remains H1, or EggServe may terminate H2 natively when enabled. Do not add proxy functionality.

### F3. Cleartext policy

If prior-knowledge cleartext H2 is supported, document exact invocation/client requirements and test it. If it is not supported/recommended, state that clearly. Do not imply the removed Upgrade path exists.

## Track G — Performance and resource regression

This plan is not a benchmark competition, but protocol expansion must not create obvious regressions.

### G1. Establish reproducible baselines

Measure representative HTTP/1 and HTTP/2 cases on the same host/build:

- small static file throughput/latency;
- large file throughput;
- many concurrent small requests;
- memory under configured maximum concurrent streams;
- binary size and dependency graph with/without H2 feature if feature-gated.

Record methodology and relative changes. Do not set arbitrary absolute RPS gates without baseline evidence.

### G2. Investigate material regressions

A large unexplained H1 regression caused by moving to an auto builder, or unexpectedly high per-H2-stream memory, blocks release until understood or explicitly accepted/documented.

### G3. Preserve local-first footprint

If enabling H2 by default materially increases binary size/compile time for local static serving, use measurements to decide whether H2 remains opt-in. Do not default-enable solely because implementation exists.

## Track H — CI and release-test placement

### H1. Routine CI

Keep deterministic H2 unit/integration tests in ordinary CI if runtime remains reasonable. At minimum routine CI should compile/test the supported H2 feature combination on Linux.

Avoid multiplying every existing job by every feature permutation. Use a small feature matrix or targeted commands in the Rust job.

### H2. Release/manual qualification

Keep broad client/browser/platform interoperability outside the routine quick regression screen if it requires external binaries or significant setup. Document exact commands in a qualification script/checklist where reproducible.

### H3. Platform coverage

At minimum ensure H2/TLS code compiles on the project's supported release targets. Perform direct runtime qualification on representative Linux and at least one macOS/Windows environment before calling native H2 generally supported.

If platform evidence differs, document a platform-limited status rather than generalizing Linux results.

## Track I — Documentation/API closure

Update, after evidence is complete:

- `README.md`;
- `docs/library-capability-matrix.md`;
- `docs/api-stability.md`;
- `docs/deployment.md`;
- `docs/downstream-app-server.md`;
- `architecture/tls.md`;
- protocol/configuration reference docs;
- examples using the Rust runtime/CLI.

State explicitly:

- H2 feature/default status;
- ALPN behavior;
- cleartext policy;
- resource defaults;
- Python compatibility limitation;
- no Upgrade/WebSocket/server-push support;
- current platform qualification.

Do not update docs to imply HTTP/3 support; that remains Plan 187+.

## Track J — Release decision

At closure, classify H2 as one of:

1. **supported** — deterministic suite plus interoperability/resource evidence pass;
2. **experimental** — functional but one or more hardening/interoperability gaps remain;
3. **disabled/deferred** — implementation cannot satisfy required lifecycle/resource invariants safely.

Record the decision and evidence in a concise closure document/plan status update. Do not let mere feature availability imply tier 1.

## Verification

Run all Plan 185 deterministic checks, plus reproducible external qualification commands. Expected core commands include:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features http2,tls
cargo test -p eggserve-bin --features http2,tls
cargo audit
cargo deny check
bash scripts/test-python-wheel.sh
```

Run the declared MSRV check from Plan 184 for the selected H2 feature combination if H2 is intended to support that MSRV.

Capture `cargo tree`/binary size measurements for the minimal and H2 configurations when deciding default feature policy.

## Acceptance criteria

- [x] H2 behavior is mapped against the applicable RFC semantics and unsupported features are explicit.
- [ ] At least two independent current H2 client implementations successfully exercise the supported core behavior; only curl was available in this environment.
- [ ] A downstream Rust `Service` consumer serves H2 without importing Hyper protocol types; the existing consumer remains HTTP-only and this is an explicit follow-up gap.
- [x] The deterministic multiplexed suite covers stream-scoped body rejection and sibling survival.
- [x] Stream-keyed response progress cannot be refreshed by sibling traffic; the public reset limitation remains documented.
- [x] H2 stream, header, body, service, and file-stream budgets are explicit and validated; broad pressure/flood testing remains manual release work.
- [ ] Graceful shutdown/GOAWAY and max-request drain behavior are covered by the shared deterministic kernel, but concurrent independent-client qualification remains open.
- [x] TLS ALPN selection/fallback is qualified locally and protocol metadata is truthful.
- [x] H1 behavior through the post-H2 driver remains green with no unexplained material regression.
- [x] Dependency/binary-size impact is measured; H2 remains opt-in.
- [x] Routine CI remains proportionate and covers the H2+TLS build and MSRV check.
- [x] Platform qualification status is truthful.
- [x] Documentation states H2's experimental tier, configuration, limitations, and Python compatibility boundary.
- [x] HTTP/3 is not claimed or implemented by this plan.

## Suggested implementation order

1. Build the RFC/implementation ownership checklist.
2. Run independent-client TLS/ALPN/static-service interoperability.
3. Run multiplexing/isolation and flow-control-stall qualification.
4. Run adversarial stream/header/reset/slow-peer cases.
5. Qualify GOAWAY/graceful shutdown/request-threshold behavior.
6. Measure H1/H2 performance, memory, binary size, and dependency impact.
7. Place deterministic tests in routine CI and broader tests in the release qualification path.
8. Perform representative platform runtime qualification.
9. Update documentation/capability matrices from evidence.
10. Record H2 as supported, experimental, or deferred.
11. Start Plan 187 only after the H2 lifecycle/configuration architecture is considered stable enough to reuse.

## Handoff

Plan 186 closes the TCP/TLS multiplexed-protocol phase. Plan 187 may reuse its canonical metadata, stream lifecycle, admission, observability, and qualification patterns, but must not assume QUIC behaves like TCP or that an H2 adapter can be mechanically reused as an H3 adapter.
