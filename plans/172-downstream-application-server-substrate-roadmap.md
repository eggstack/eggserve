# Plan 172 — Downstream Application-Server Substrate Roadmap

## Status

**PLANNED.**

Prerequisites: Plans 161–171 are implemented/closed. This roadmap is a new, narrow continuation of the downstream-embedding goal already named by Plan 161. It must not reopen the completed production-hardening roadmap wholesale and must not turn EggServe into an ASGI, WSGI, WebSocket, framework, router, middleware, or process-supervision implementation.

## Goal

Prepare `eggserve-core` to serve as a complete, transport-owning HTTP substrate for a separate downstream application-server project.

The reference consumer is an ASGI-class server because ASGI exercises the difficult boundaries—byte-preserving metadata, independently streamed request/response bodies, application/connection cancellation, early responses, long-lived responses, and optional protocol upgrade—but all EggServe changes must remain protocol- and language-neutral wherever practical.

The intended architecture remains:

```text
Python/Rust/other application framework
                 |
       downstream app server
      (ASGI is one consumer)
                 |
       EggServe Service boundary
                 |
 canonical HTTP request/response runtime
                 |
 parsing / framing / limits / timeouts
                 |
       TCP / TLS / caller-owned IO
```

EggServe owns HTTP transport semantics and hardened runtime policy. The downstream app server owns application protocol adaptation, event-loop integration, worker/process strategy, framework loading, application lifespan, routing/framework semantics, and language FFI.

## Why another roadmap is needed

Plan 161 intentionally included “downstream application-server implementations built on EggServe rather than on raw Hyper/socket code” as a supported consumer. Plans 162–170 subsequently landed most of the prerequisites:

- transport-independent canonical requests and responses;
- one-shot bounded streaming request bodies;
- pull/backpressure-driven streaming responses;
- transport-neutral caller-owned HTTP/1 connection driving;
- independent connection/service/file admission;
- parser/header/request-target ceilings;
- keep-alive, hard-lifetime, handler, body, write-progress, and shutdown controls;
- transport-independent connection metadata;
- server lifecycle/readiness/shutdown handles;
- privacy/fingerprint response policy;
- low-level Python service/runtime access;
- qualification and API-boundary closure.

That means the remaining work is not “build a general web server.” It is a small set of application-server seam corrections that are difficult or impossible to implement cleanly above EggServe’s current public boundary.

## Reference protocol requirements

At planning time, ASGI 3.0 with HTTP/WebSocket sub-specification 2.5 is the reference stress case. The downstream adapter—not EggServe—would construct ASGI dictionaries/events, but EggServe must preserve enough information and lifecycle semantics to do so correctly.

Relevant ASGI HTTP requirements include:

- request/response headers are ordered duplicate-preserving byte pairs;
- request body transfer coding is removed by the protocol server and body bytes are delivered incrementally;
- response transfer coding is owned by the protocol server;
- `raw_path` is optional but, when exposed, must reflect the original path bytes;
- `query_string` is byte-oriented;
- long-polling applications need notification when the client disconnects;
- send-side failure may precede receive-side disconnect notification;
- an application may stream request and response activity rather than using a strictly buffer-then-respond model.

ASGI lifespan is explicitly downstream responsibility. EggServe’s ready/shutdown/drain primitives should be sufficient hooks; EggServe must not gain an ASGI-specific startup/shutdown state machine.

HTTP trailers, zero-copy/path-send, early hints, and TLS metadata are useful extension points but are not required to begin a compliant HTTP-only downstream server. Generic support should be added only when it improves EggServe’s transport abstraction independently of ASGI.

WebSockets are a separate optional workstream because they require an HTTP upgrade plus bidirectional framed protocol handling. EggServe should at most provide a generic, transport-neutral upgrade handoff; the WebSocket codec/state machine belongs downstream.

## Current-state findings

### 1. The core Service split is correct

`server::Service` receives canonical `Request` and returns canonical `Response`. Hyper and sockets remain below the application-facing seam. `Server`, `RuntimeState`, caller-owned connection driving, and shutdown controls already provide the right ownership model for a downstream app server.

No replacement service trait, Tower dependency, framework middleware layer, or raw-Hyper escape hatch is warranted.

### 2. Request and response streaming are already sufficient in shape

`RequestBody` supports one-shot incremental consumption and enforces body limits. `ResponseStream` is one-owner, `Send + !Sync` capable, pull/backpressure-driven, and supports known/unknown representation length while EggServe retains framing authority.

A downstream app server can therefore bridge request and response events through bounded channels/tasks without adding an unbounded buffering layer.

### 3. Canonical header values are too text-centric

`HeaderBlock` correctly preserves order and duplicates, but `HeaderValue` currently stores `String`. The Hyper-to-canonical adapter calls `HeaderValue::to_str()` and rejects legal opaque non-UTF-8 header bytes.

That is a mismatch for a general HTTP application-server substrate. The canonical HTTP layer should preserve valid field-value octets and provide text conversion only as an explicit convenience. This affects both inbound request headers and outbound application response headers.

This is the first implementation dependency because it changes a semver-considered primitive and should settle before a downstream consumer freezes assumptions.

### 4. Request-target byte fidelity must be explicitly qualified

`RequestTarget` preserves a raw string plus path/query views, which is enough for ordinary HTTP routing and for ASGI’s mandatory percent-encoded query bytes. ASGI `raw_path` is optional, so lack of a raw-path byte accessor is not itself a blocker.

However, the project currently does not have an explicit public fidelity contract saying which exact wire request-target octets survive Hyper parsing and canonical conversion. Before inventing another target type, qualify the real boundary. If the existing representation is lossless for accepted origin-form HTTP/1 targets, add byte accessors/tests and keep the type. If Hyper necessarily normalizes an accepted input, document that limitation and leave `raw_path` absent in downstream adapters rather than fabricating bytes.

### 5. Service completion is currently too early to represent application completion

The important lifecycle gap is Stream request-body handling. The runtime clones the request-body consumed flag, awaits `Service::call()`, and immediately marks the connection for close if the service returns before body EOF.

That rule is safe for ordinary request handlers: unread HTTP/1 body bytes must not be confused with a subsequent request. It is not sufficient for an event-driven application-server bridge where the service may:

1. spawn/own the application task and request-body receiver;
2. wait only until response-start is available;
3. return an EggServe `ResponseStream` backed by that task; and
4. continue consuming request bytes while response bytes are produced.

EggServe needs a transport-neutral way to distinguish “service returned and abandoned the request body” from “service returned a response but a valid downstream task still owns the request body.” It must also preserve safe HTTP/1 reuse: the connection cannot accept a subsequent request until the prior body is fully consumed or intentionally abandoned/closed.

### 6. No public per-request disconnect signal exists

Body reads can observe transport failure and response-stream drop propagates cancellation, but an application that is not currently reading/writing has no canonical public future/token for peer disconnect or runtime cancellation. Long-polling, SSE, and application cleanup require this independently of body polling.

The signal must be EggServe-owned and transport-neutral. It must not expose Hyper `OnUpgrade`, raw sockets, or Tokio task handles as the public contract.

### 7. Generic upgrades are absent

The connection executor enables Hyper upgrades internally, but there is no canonical service-level mechanism to accept an upgrade and receive the upgraded bidirectional IO. That is acceptable for HTTP-only downstream servers and was an explicit non-goal in Plan 161.

If a downstream server is expected to support WebSockets, implement a generic upgrade handoff as a later, isolated plan. Do not add WebSocket framing or ASGI event semantics to EggServe.

### 8. Trailers are not a prerequisite

Current `ResponseStream` carries byte chunks, not trailer frames. ASGI documents HTTP trailers as an extension. A first downstream HTTP server can omit that extension honestly. Do not complicate the core streaming body model merely to advertise optional ASGI surface area.

If another concrete consumer requires transport-independent trailers later, plan them independently using HTTP semantics rather than ASGI event names.

## Workstreams and dependency order

Implementation order is:

1. **Plan 173 — octet-preserving canonical HTTP metadata.**
   Settle byte-preserving header values and qualify request-target fidelity before downstream adapters bind to the current text-only assumptions.

2. **Plan 174 — deferred request-body ownership and request lifecycle signaling.**
   Make early-response/full-duplex application bridging safe and expose a canonical disconnect/cancellation observation primitive without weakening HTTP/1 reuse safety.

3. **Plan 175 — downstream application-server consumer contract and qualification.**
   Prove the public API with an external/reference Rust consumer that implements the hard bridge shape using bounded channels/tasks, including early response, ongoing request consumption, streamed response, disconnect, shutdown, and byte metadata. This is a consumer test, not an ASGI implementation.

4. **Plan 176 — optional generic HTTP upgrade handoff.**
   Only required if the desired downstream application server must support WebSockets/upgraded protocols. Keep it independent so HTTP-only consumers do not inherit upgrade API complexity.

Plans 173 and the design portion of 174 may proceed in parallel, but Plan 175 must validate their final public contracts. Plan 176 should start only after the HTTP consumer contract is stable enough that upgrade support cannot distort it.

## API design constraints

Across all child plans:

- keep `Service`, canonical request/response types, and caller-owned connection APIs free of direct Hyper requirements;
- do not add ASGI/WSGI/Python event names to `eggserve-core`;
- keep Tokio/Hyper implementation details behind EggServe-owned types where a public lifecycle token or IO wrapper is needed;
- preserve the one-owner `ResponseStream: Send + !Sync` model from Plans 169/171;
- use bounded channels only in qualification/reference consumers; never hide an unbounded queue in EggServe;
- retain EggServe framing authority for `Content-Length`, transfer coding, HEAD/body-forbidden statuses, and connection reuse;
- retain body/policy limits even when request-body ownership outlives `Service::call()`;
- cancellation, timeout, panic, disconnect, and shutdown paths must release all permits/guards;
- avoid requiring an application server to import internal `fs`, `response`, Hyper adapters, or Python low-level facade types.

## Timeout and admission semantics

A downstream event-driven server changes what “handler completion” means. The implementation must keep the following concepts distinct:

- time until `Service::call()` produces the response head/body object;
- request-body read progress/lifetime;
- application response-body production;
- socket write progress;
- overall optional hard connection lifetime;
- application-server-owned execution/admission after response-start.

Do not silently extend the existing in-flight-service permit from `Service::call()` across a response stream unless measurements and semantics justify it. A downstream app server may own a separate application-task semaphore. EggServe should expose enough lifecycle information to build that correctly, not dictate application-worker policy.

Likewise, do not reinterpret `handler_timeout` as a total application-coroutine deadline. The downstream server can choose a runtime profile with an appropriate response-start deadline and can enforce its own application lifecycle after the response becomes streaming.

## Compatibility and release policy

Plans 173/174 touch semver-considered primitives and experimental server APIs. Follow Plan 171’s release rule:

- stable/pre-1.0 primitive breaks require an explicit minor-version transition, migration notes, and API snapshots;
- prefer additive compatibility where it does not perpetuate an incorrect HTTP representation;
- experimental server APIs may evolve more freely but still require examples/docs and external-consumer tests because this roadmap’s purpose is downstream use.

Do not preserve a text-only `HeaderValue` contract solely to avoid a pre-1.0 migration if doing so makes the canonical HTTP model incorrect for valid input.

## Documentation target

When the child plans land, current-state docs must state a precise product boundary:

> EggServe is not an application server. It is a hardened HTTP/static-serving runtime with a public canonical service/connection substrate suitable for downstream application-server implementations.

Update at minimum:

- `README.md`;
- `plans/ROADMAP.md` current positioning;
- `docs/public-api-boundary.md`;
- `docs/api-stability.md`;
- `docs/library-capability-matrix.md`;
- runtime/embedding architecture documentation;
- timeout and lifecycle reference docs;
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` if invariants change.

Historical plans remain historical; do not rewrite their non-goals as though they had always included this follow-up.

## Verification strategy

Child plans should use the existing focused verification style. The roadmap does not justify a broad new CI matrix.

Required classes of evidence by closure:

- canonical metadata round-trip/wire tests for legal opaque header bytes and duplicate ordering;
- external-consumer compilation using only public APIs;
- real HTTP/1 early-response test while request-body ownership continues downstream;
- proof that the connection is reusable only after body completion and is closed after abandonment;
- peer-disconnect tests when the application is idle, reading, and streaming a response;
- shutdown/timeout tests for downstream-owned body/application tasks;
- TLS and caller-owned transport parity for lifecycle signaling;
- leak/permit recovery checks under cancellation;
- optional upgrade tests only under Plan 176.

Do not add performance-number CI gates. A small benchmark may be recorded if lifecycle changes introduce extra Arc/atomic/channel overhead, but correctness and bounded ownership are the acceptance gates.

## Acceptance criteria

This roadmap is complete when:

- [ ] valid inbound/outbound HTTP header field values can cross the canonical application-facing boundary without mandatory UTF-8 conversion;
- [ ] duplicate header order remains preserved;
- [ ] request-target byte-fidelity limits are measured/documented, with a public byte view where truthful;
- [ ] a service can return response-start/stream ownership while a downstream task legitimately continues consuming the request body;
- [ ] HTTP/1 connection reuse occurs only after the prior request body is complete, while abandonment still forces a safe close;
- [ ] downstream code can observe peer disconnect/runtime cancellation without polling raw transport types;
- [ ] a reference external consumer demonstrates bounded bidirectional application bridging using only public EggServe APIs;
- [ ] long-polling/streamed response cancellation and graceful shutdown are deterministic;
- [ ] current docs explicitly describe downstream app-server support without claiming EggServe itself is an application server;
- [ ] if WebSocket-class support is desired, Plan 176 provides a generic upgrade handoff without WebSocket/ASGI logic in core;
- [ ] no framework, router, middleware, worker supervisor, Python event-loop, ASGI/WSGI protocol implementation, or WebSocket codec has entered `eggserve-core`.

## Non-goals

Do not add as part of this roadmap:

- ASGI or WSGI protocol/event implementations;
- PyO3 asyncio integration for an app server;
- Gunicorn-style worker/process management;
- application routing or middleware;
- HTTP/2 or HTTP/3 solely for ASGI feature breadth;
- WebSocket framing, ping/pong, fragmentation, close-code policy, or compression;
- HTTP trailers solely to advertise an optional ASGI extension;
- arbitrary raw-socket access from `Service`;
- a Tower compatibility layer without a separate demonstrated consumer need;
- unbounded request/response queues;
- weakening parser/body/timeout/admission security policy for framework compatibility.

## External references

Implementation should re-check current specifications at execution time. At plan time:

- ASGI HTTP/WebSocket specification 2.5: https://asgi.readthedocs.io/en/latest/specs/www.html
- ASGI lifespan specification: https://asgi.readthedocs.io/en/latest/specs/lifespan.html
- ASGI common extensions: https://asgi.readthedocs.io/en/latest/extensions.html
- RFC 9110 HTTP semantics: https://www.rfc-editor.org/rfc/rfc9110.html
- Hyper/http `HeaderValue` byte-oriented contract: https://docs.rs/http/latest/http/header/struct.HeaderValue.html

These references define consumer requirements and HTTP semantics; they do not define EggServe’s internal architecture.

## Handoff

Implement Plans 173–175 as the HTTP application-server-readiness sequence. Treat Plan 176 as optional unless the downstream project explicitly requires WebSockets/upgraded protocols.

The critical design rule is that an application-server adapter must not need to bypass EggServe’s canonical boundary to achieve correct concurrency. If implementation discovers that correct early-response/body-consumption behavior requires raw Hyper access, revise the EggServe-owned lifecycle abstraction instead of exposing Hyper to the downstream server.