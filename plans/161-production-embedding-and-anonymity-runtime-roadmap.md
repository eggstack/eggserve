# Plan 161 — Production, Embedding, and Anonymity-Sensitive Runtime Roadmap

## Status

**IMPLEMENTED IN SUBSTANCE; FINAL ROADMAP CLOSURE PENDING PLAN 170.**

This roadmap records the revised product target after Plan 160. It does not reopen completed hardening work wholesale; it identifies the remaining delta required to make EggServe a production-oriented HTTP substrate in addition to a hardened `http.server`-shaped static server.

## Goal

Evolve EggServe into a small production-oriented HTTP/1 server runtime with four supported consumers:

1. the Python `http.server`-shaped facade;
2. native Rust services;
3. downstream application-server implementations built on EggServe rather than on raw Hyper/socket code;
4. an embedded anonymity-sensitive origin, including an I2P router that supplies an established bidirectional stream and expects a separate WAF/rate-limiting layer.

EggServe remains an HTTP runtime/static server, not an application framework.

## Product boundary

The intended architecture is:

```text
Python http.server facade       downstream app server       static service
          \                            |                         /
           +---------------- canonical Service ----------------+
                                |
                    canonical HTTP/1 connection driver
                                |
                  framing / limits / timeouts / policy
                                |
          +---------------------+---------------------+
          |                     |                     |
         TCP                   TLS            caller-owned stream
                                                   (e.g. I2P)
```

The canonical request/response and service boundary remains the application-facing seam. Hyper is an implementation dependency behind that seam.

## Current baseline

Do not reimplement capabilities already landed by Plans 067, 077, 078, 088, 104, 107, 109, 112, 113, 119, 123, 124, 136, 137, and subsequent corrective plans.

The current repository already provides:

- canonical transport-independent request types;
- one-shot bounded request-body buffering/streaming;
- canonical response normalization and runtime-owned framing;
- generic Rust `Service` dispatch;
- static service reuse across CLI/Python/Rust;
- connection and file-stream admission;
- header/body/handler/TLS/connection/shutdown timeouts;
- panic containment and generic client-facing error responses;
- default suppression of the `Server` response header;
- pre-bound TCP listener support;
- a substantial Python native-primitives surface;
- conformance, fuzz/corpus, race, soak, and performance qualification infrastructure.

The remaining architectural gaps are response streaming, transport-neutral canonical connection driving, finer production admission/lifecycle controls, final-boundary privacy policy, Python runtime/service construction APIs, and profile-specific qualification.

## Workstreams and dependency order

Implementation order is mandatory unless a plan explicitly documents why it can proceed independently:

1. **Plan 162 — transport-independent streaming responses.**
2. **Plan 163 — transport-neutral canonical connection driver and connection metadata evolution.**
3. **Plan 164 — production admission, parser, and lifecycle resource controls.**
4. **Plan 165 — response privacy and fingerprint-minimization policy.**
5. **Plan 166 — Python low-level runtime/service substrate.**
6. **Plan 167 — optional CGI/FastCGI adapters and go/no-go gate.**
7. **Plan 168 — production, embedding, privacy, and performance qualification.**

Plans 162 and 163 may be implemented in either order internally if the final public contracts are reviewed together. Plan 166 must not invent a Python-only transport or response-streaming model before 162/163 settle the Rust ownership model.

## Deployment profiles

All implementation and documentation must distinguish three profiles rather than making a single vague “production ready” claim.

### Reverse-proxy production

Preferred conventional deployment. EggServe runs behind a mature edge/reverse proxy. EggServe still enforces its own parser, body, concurrency, timeout, and filesystem bounds; the proxy is not a substitute for internal resource safety.

### Direct TLS

Retain the existing deliberately narrow native-TLS origin profile. Do not expand this roadmap into certificate automation, HTTP/2/3, OCSP, edge caching, or a Caddy/nginx replacement.

### Embedded anonymity-sensitive origin

A caller such as an I2P router supplies an already-established byte stream. EggServe owns HTTP parsing/framing and local resource safety; the router/WAF owns peer identity, network admission, rate limiting, reputation, tunnel policy, and anonymity-network semantics.

This profile must minimize gratuitous origin fingerprinting but must not claim that the server is un-fingerprintable or safe for arbitrary bare-Internet deployment without a WAF.

## Stable API policy

`primitives` are currently treated as stable even though the crate is pre-1.0. Plans 162/163/165 touch existing stable contracts (`ResponseBody`, `ConnectionInfo`, Date finalization). Therefore:

- prefer additive evolution;
- where a field/type must change, provide an explicit migration path and update API snapshots/examples/docs in the same phase;
- do not silently change stdlib-facing Python semantics in order to improve the Rust API;
- keep Hyper/Tokio transport details out of stable application-facing types;
- document which server/runtime APIs remain experimental until Plan 168 closes qualification.

## Explicit non-goals

This roadmap does not add:

- a web framework, router, middleware stack, dependency-injection system, or template system;
- Gunicorn-style process supervision or worker lifecycle;
- an ASGI or WSGI runtime in `eggserve-core`;
- per-user/IP rate limiting, bot detection, reputation, firewall rules, or other WAF behavior;
- I2P Destination/LeaseSet/tunnel types in EggServe;
- a reverse proxy;
- HTTP/2, HTTP/3, WebSockets/upgrades, or trailers as a prerequisite;
- automatic compression, decompression, or application content transformation;
- claims of bare-edge DoS resilience beyond explicitly qualified resource bounds.

Downstream projects may implement application protocols or process models using the core service/connection APIs.

## External reference points

Implementation should re-check current upstream versions during execution. At plan time:

- RFC 9110 is the normative HTTP semantics reference, including Date requirements: https://www.rfc-editor.org/rfc/rfc9110.html
- Hyper server HTTP/1 builder exposes parser/resource controls such as `max_headers` and `max_buf_size`; defaults are explicitly not stable: https://docs.rs/hyper/latest/hyper/server/conn/http1/struct.Builder.html
- Hyper 1.11 includes HTTP/1 correctness/buffer-enforcement fixes relevant to requalification: https://github.com/hyperium/hyper/releases
- CPython 3.14 documents CGI deprecation/removal in 3.15 and its security caveat: https://docs.python.org/3.14/library/http.server.html
- I2P documents privacy-motivated HTTP header stripping in I2PTunnel and strict router clock-skew expectations: https://www.i2p.net/en/docs/api/i2ptunnel/ and https://www.i2p.net/en/docs/specs/ntcp2/

These references inform boundaries; EggServe should not copy their architecture mechanically.

## Documentation changes required across the roadmap

As each dependent plan lands, keep current-state docs authoritative:

- update `README.md` product positioning;
- update `docs/architecture/*` runtime/service/transport ownership;
- update Python compatibility and low-level API documentation;
- update timeout/resource-limit references;
- add a deployment-profile/threat-model document for the anonymity-sensitive profile;
- update `.opencode/skills/eggserve-dev/SKILL.md` and `AGENTS.md` when stable invariants change;
- keep historical plan text historical rather than rewriting old implemented plans.

## Completion criteria

This roadmap closes only when:

- [ ] a downstream Rust service can return an unknown-length streaming response without importing Hyper;
- [ ] a downstream caller can drive canonical EggServe HTTP over a non-TCP `AsyncRead + AsyncWrite` transport;
- [ ] open connections, in-flight services, parser memory/header count, keep-alive lifetime, request count, and stalled response writes have explicit bounded policies or documented reasons for exclusion;
- [ ] final response metadata can be controlled by a generic privacy/fingerprint policy without weakening framing ownership;
- [ ] Python exposes enough low-level runtime/service functionality for a separate app-server package to be built on top without using the stdlib facade;
- [ ] CGI/FastCGI scope has a documented go/no-go result and any implementation remains optional/default-off;
- [ ] Plan 168 provides reproducible correctness, resource, privacy, and performance evidence for each claimed deployment profile;
- [ ] no core layer has acquired framework, WAF, I2P protocol, or process-manager responsibilities.

## Handoff

Implement Plans 162–168 as separate reviewable phases. If an implementation discovers that a lower layer cannot support a requirement without exposing Hyper/raw sockets, stop and revise the canonical boundary rather than adding a frontend-specific escape hatch.

## Closure record

Plans 162–168 landed in substance. Their implementation, qualification, and
no-go records are retained in the individual plan closure sections and in the
implementation history. Final performance-evidence and claims closure remains
tracked by Plan 170.
