# Plan 207 — Cross-Protocol Application-Server Conformance

## Status

**PLANNED — qualification phase after Plans 197–206.**

## Purpose

Create one protocol-neutral application-server contract suite that proves EggServe exposes the same application semantics across HTTP/1.1, HTTP/2, HTTP/3, TLS/plaintext where applicable, caller/prebound transports, native Rust/Tower consumers, and the async Python bridge.

The repository already has extensive per-feature testing. This plan does not replace it. Its purpose is to catch semantic drift between adapters: a reusable server base is only coherent if downstream applications can reason about one request/response/lifecycle contract instead of learning separate H1/H2/H3 behavior.

## Core qualification principle

Define semantic scenarios once, then run each scenario through every applicable transport/consumer combination.

Do not assert byte-identical framing across protocols. Assert canonical application-visible behavior and protocol-correct lifecycle outcomes.

## Track A — Define the normative application-server contract

Create a machine-readable or strongly typed scenario inventory covering:

### Request metadata
- method and extension methods;
- target/path/query raw-byte fidelity;
- authority and scheme;
- ordered duplicate/opaque header values;
- HTTP version;
- immediate peer/local endpoint;
- trusted proxy/effective metadata provenance;
- TLS/SNI/client-auth metadata.

### Request body
- empty body;
- fixed known-length body;
- chunked/streamed transfer where protocol applicable;
- no declared length with stream data under H2/H3;
- incremental consumption;
- `read_all` bounded path;
- request trailers;
- partial consumption/abandonment;
- over-limit body;
- slow/stalled body;
- peer reset/disconnect.

### Response
- empty/bytes/known-length stream/unknown-length stream;
- duplicate/opaque headers;
- HEAD and body-forbidden status behavior;
- response trailers;
- informational responses;
- producer error before/after commitment;
- stalled producer/no-progress timeout;
- application panic/exception;
- length mismatch/framing attempts.

### Full duplex/lifecycle
- early response while upload continues;
- body completes after response-start and connection/stream becomes safely reusable;
- body abandoned after response-start;
- long-poll response with no active body IO and peer disconnect;
- graceful shutdown at each request phase;
- forced shutdown after drain deadline;
- connection hard timeout;
- application admission saturation and recovery.

### Tunnels
- ordinary denial;
- H1 upgrade success;
- H2/H3 Extended CONNECT success where implemented;
- duplex backpressure;
- peer/local close/reset;
- shutdown with active tunnel;
- sibling multiplexed request remains healthy after one tunnel reset.

## Track B — Transport matrix

Run applicable scenarios against:

```text
H1 cleartext TCP
H1 TLS
H1 prebound TCP
H1 Unix socket (Unix only)
H2 cleartext prior knowledge
H2 TLS/ALPN
H2 prebound TCP/TLS
H3 QUIC/TLS
caller-owned duplex/H1 test transport where semantics apply
```

Do not force impossible combinations (for example H3 over Unix stream). Each skip must be capability-driven and documented rather than silently omitted.

If H2/H3 remain experimental at execution time, the suite is still required; protocol support-tier promotion is Plan 208's decision.

## Track C — Consumer matrix

At minimum exercise:

1. native EggServe `Service` reference consumer;
2. `http`/`http-body` adapter consumer;
3. Tower service consumer;
4. async Python low-level handler;
5. test/example ASGI adapter from Plan 204.

Not every low-level transport scenario needs every consumer if doing so creates redundant combinatorics. Build a coverage matrix showing which semantic boundary each consumer proves.

The native service is the normative baseline. Adapter differences must be documented as intentional representation limits, never accidental behavior drift.

## Track D — Multiplexing and isolation

H2/H3-specific qualification must prove stream isolation under concurrency:

- one oversized/rejected request does not kill healthy siblings unless connection state is invalid;
- one response producer timeout/reset does not refresh or terminate unrelated streams incorrectly;
- one tunnel reset does not end sibling HTTP traffic;
- server-wide application admission is shared correctly across connections/streams;
- protocol stream limits bound hostile idle/open streams;
- graceful GOAWAY/drain stops new work while allowing eligible in-flight streams to complete;
- request/response IDs and observability events remain distinct.

Use deterministic synchronization primitives rather than sleep-heavy timing races.

## Track E — Security corpus replay through real drivers

Feed representative hostile cases through each protocol path where semantically relevant:

- H1 request-smuggling/framing ambiguity corpus;
- oversized headers/targets/trailers;
- invalid pseudo-header combinations delegated to H2/H3 parser libraries plus EggServe adapter limits;
- forbidden connection-specific response fields;
- invalid trailer fields;
- spoofed proxy headers/untrusted PROXY preamble;
- slowloris header/body/TLS/PROXY preamble behavior;
- invalid upgrade/Extended CONNECT attempts;
- malformed service output/length mismatch;
- cancellation/resource leak cycles.

Do not duplicate the protocol library's full conformance suite. Test EggServe's adapter/security boundary plus a curated regression corpus for past bugs.

## Track F — Resource and leak qualification

For each transport/consumer family, repeatedly exercise:

- connection admission exhaustion/recovery;
- in-flight service admission;
- file-stream admission where static service is involved;
- active tunnel limit;
- Python application-task admission;
- request/response body cancellation;
- TLS handshake limit/timeout;
- shutdown churn.

Assert counters/semaphores/task registries return to baseline. Include long-running soak as manual/release evidence rather than every-PR CI where expensive.

Use memory-growth observations to detect unbounded queues; avoid fragile absolute-RSS thresholds in routine CI.

## Track G — Interoperability clients

Use at least two independent client/protocol implementations per promoted modern protocol where practical.

H1 can use raw sockets plus a conventional HTTP client. H2 qualification should retain independent clients from Plans 186/191 and add application-feature scenarios (trailers/Extended CONNECT only where client support exists). H3 qualification should retain independent current H3 tooling/clients and record missing feature support explicitly.

Browser evidence is useful for supported-tier claims involving browser-facing features but should not replace deterministic protocol-client tests.

For WebSocket-class tunnel qualification, use at least one independent downstream codec/client against the generic tunnel fixture; do not count EggServe talking to itself as interoperability evidence.

## Track H — Python event-loop stress

Plan 204 qualification must include:

- concurrent requests on H2/H3;
- slow Python request consumer causing transport backpressure;
- slow Python response producer;
- Python task cancellation before/after commitment;
- event-loop shutdown while requests active;
- repeated start/stop cycles;
- GIL contention with CPU-light handlers;
- bounded queues under a client that sends faster than Python consumes.

Assert no deadlocks/orphan tasks and deterministic exceptions on closed/disconnected streams.

## Track I — Performance sanity evidence

Record same-machine comparisons for major paths:

- native H1/H2/H3 trivial response;
- native streaming response;
- Tower adapter overhead;
- async Python bridge overhead;
- static file path regression relative to pre-roadmap baseline.

The goal is to identify accidental orders-of-magnitude regressions or serialization, not establish absolute benchmark gates. Store environment and raw results under `benchmarks/` if the repository's existing benchmark evidence format fits.

## Track J — CI/release partition

Keep routine CI focused:

- deterministic semantic contract subset on Linux;
- all default/unit/API/fuzz corpus tests;
- feature compile/test for H2/H3/TLS;
- installed Python wheel tests.

Use manual/release workflows for expensive multi-platform browsers, external H3 tooling, soak, and full interoperability matrices. Fail release qualification closed when mandatory evidence is unavailable; do not silently reinterpret missing tools as passing.

## Documentation/artifacts

Produce a current capability matrix that states for each feature:

- native Rust support;
- Tower/http adapter support;
- Python low-level support;
- H1/H2/H3 support;
- platform qualification;
- stable versus experimental status;
- known limitations.

Prefer generated/checkable matrix data where existing repository tooling can validate documentation consistency.

## Acceptance criteria

- [ ] one normative semantic scenario inventory drives cross-protocol application-server qualification;
- [ ] H1/H2/H3 application-visible metadata/body/response/lifecycle behavior is consistent where semantics overlap;
- [ ] protocol-specific differences are explicit and capability-gated rather than accidental;
- [ ] native, standard-HTTP/Tower, async Python, and ASGI fixture consumers all pass their assigned contract coverage;
- [ ] request/response trailers, interim responses, early/full-duplex responses, disconnects, and tunnels are qualified through real drivers;
- [ ] multiplexed failures remain stream-local where protocol correctness permits;
- [ ] admission/resources recover to baseline after repeated error/cancellation/shutdown cycles;
- [ ] proxy/TLS trusted metadata cannot be spoofed through application headers;
- [ ] interoperability evidence uses independent clients rather than only internal loopback stacks;
- [ ] routine CI remains bounded while release qualification fails closed on missing mandatory external evidence;
- [ ] performance evidence shows no accidental global serialization/unbounded buffering introduced by adapters.

## Handoff

Plan 208 consumes this evidence to decide which APIs and protocols can be promoted from experimental status. Do not mark a feature supported merely because its implementation exists; support tier follows the qualification gates.