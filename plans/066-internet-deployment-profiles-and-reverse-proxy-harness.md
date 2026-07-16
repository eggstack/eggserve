# Phase 66 — Internet Deployment Profiles and Reverse-Proxy Harness

## Goal

Turn the documented reverse-proxy recommendation into a precise, tested production-origin profile. Provide safe Caddy and nginx reference deployments, build an edge/origin integration harness, and ensure eggserve does not accidentally acquire reverse-proxy or trusted-forwarding responsibilities.

## Preconditions

- Plan 060 defines production profiles and scope boundaries.
- Plan 061 pins root identity and opened-resource ownership.
- Existing HTTP framing and static-service tests pass.

## Non-goals

Do not add:

- reverse proxying to eggserve;
- automatic proxy discovery;
- implicit trust of `Forwarded` or `X-Forwarded-*` headers;
- virtual hosts or Host-based root selection;
- ACME, HTTP/2, HTTP/3, or edge certificate management;
- authentication, rate-limiting policy engines, WAF behavior, or access-control frameworks;
- application routing or dynamic upstream dispatch.

## Production deployment contract

The preferred public profile is:

- edge listens on public interfaces;
- edge terminates TLS;
- edge may negotiate HTTP/2 or HTTP/3 externally;
- edge speaks HTTP/1.1 to eggserve;
- eggserve binds to loopback or an explicitly private interface;
- origin port is not publicly reachable;
- eggserve applies its own request parsing, limits, and static policy regardless of edge validation;
- edge logs own client attribution;
- eggserve treats forwarding headers as ordinary untrusted header values.

## Track A — Reference Caddy deployment

Add a production-oriented Caddy example covering:

- public HTTPS listener;
- automatic certificate management at Caddy, not eggserve;
- reverse proxy to loopback eggserve;
- explicit origin transport protocol;
- bounded dial, response-header, idle, and request-body behavior where supported;
- preservation of Host only where it does not alter eggserve root selection;
- prevention of accidental direct exposure of eggserve;
- optional static security headers at the edge without relying on them for core confinement;
- service startup ordering and health behavior.

Provide:

- minimal configuration;
- hardened annotated configuration;
- systemd or container composition example;
- verification commands;
- expected failure cases.

Do not imply that Caddy configuration is mandatory for local/internal deployments.

## Track B — Reference nginx deployment

Add an equivalent nginx configuration covering:

- TLS termination;
- loopback/private upstream;
- HTTP/1.1 origin connection;
- request and response timeout guidance;
- buffering implications for large static files;
- request-size limits;
- header handling;
- origin keep-alive behavior;
- no public origin exposure;
- error-page behavior that does not mask origin protocol failures during tests.

Document differences between buffered and streaming proxy behavior, especially for slow readers and response-write timeout interpretation.

## Track C — Forwarding-header policy

Adopt an explicit initial policy:

- `Forwarded`, `X-Forwarded-For`, `X-Real-IP`, `X-Forwarded-Proto`, and `X-Forwarded-Host` are not trusted;
- connection metadata reports the direct transport peer;
- logs do not label forwarded values as authenticated client identity;
- static file selection never depends on forwarded host/path metadata;
- no trusted-proxy CIDR configuration is added in this phase.

Add tests showing spoofed forwarding headers cannot:

- alter root selection;
- alter scheme-sensitive behavior;
- influence path resolution;
- inject logs;
- produce privileged client attribution.

If structured logs include these headers at all, sanitize and label them as untrusted. Prefer not logging them by default.

## Track D — Edge/origin integration harness

Create a reproducible harness that starts:

- eggserve on an ephemeral loopback port;
- Caddy or nginx in a container/process;
- a raw TCP and ordinary HTTP client;
- optional TLS client tooling.

The harness must support:

- injecting raw HTTP/1 requests at the edge;
- issuing multiple requests on one edge connection where possible;
- collecting edge and origin logs;
- detecting whether the origin handler was invoked;
- observing connection closure/reuse at both layers;
- deterministic teardown;
- CI execution without public network access after dependencies/images are available.

Pin proxy versions for qualification and record them in evidence.

## Track E — Baseline interoperability matrix

Test normal behavior through each proxy:

- GET and HEAD;
- 404, 403, 405;
- conditional requests;
- ranges;
- large-file streaming;
- keep-alive reuse;
- client disconnect;
- slow reader;
- graceful origin shutdown;
- origin unavailable;
- edge restart;
- directory listing disabled/enabled;
- unknown MIME fallback;
- sanitized errors.

Compare direct-origin and proxied raw-wire semantics where the proxy legitimately transforms framing. Assert semantic equivalence rather than requiring byte identity across the edge.

## Track F — Deployment process guidance

Document safe operational patterns:

- dedicated unprivileged service account;
- root directory not writable by the service account where practical;
- explicit working directory;
- read-only filesystem/container mount;
- loopback/private bind;
- firewall denial of origin port;
- service restart policy;
- file descriptor limits;
- log destination and rotation;
- graceful termination signal;
- health check using a known static file or TCP readiness rather than adding an admin endpoint;
- atomic content deployment compatible with pinned-root semantics.

Explain that replacing the configured root pathname does not retarget a running server after Plan 061. Operators should update files within the pinned tree according to documented mutation semantics or restart with a new root.

## Track G — Container guidance

Provide an optional minimal container example without making containers required.

Requirements:

- non-root user;
- read-only root filesystem where feasible;
- read-only content mount;
- no unnecessary capabilities;
- explicit exposed proxy port only;
- eggserve origin network isolated from the host/public network;
- signal propagation and graceful shutdown;
- pinned image/toolchain versions for examples;
- no embedded certificate automation in eggserve container.

Avoid building a general orchestration system.

## Track H — Release criteria

Add gates for:

- Caddy normal interoperability;
- nginx normal interoperability;
- origin bind isolation checks;
- forwarding-header non-trust;
- proxied range and conditional behavior;
- large-stream and disconnect behavior;
- configuration syntax validation;
- installed eggserve artifact in the harness;
- proxy-version evidence.

Invalidation paths include:

- connection handling;
- request parsing;
- response framing;
- logging/connection metadata;
- CLI binding options;
- deployment configurations;
- proxy harness scripts.

## Required tests

- edge can reach origin, public test client cannot reach origin port in isolated topology;
- spoofed forwarding headers have no authority;
- edge and origin do not disagree on ordinary request completion;
- origin shutdown produces bounded edge failure;
- large range and full-file streams do not buffer entirely in eggserve;
- edge client disconnect releases origin permits;
- malformed requests reserved for Plan 068 can be injected by the harness;
- harness cleanup leaves no processes, containers, sockets, or temporary certificates.

## Documentation deliverables

- `docs/deployment.md` profile expansion;
- Caddy example;
- nginx example;
- container/service-manager examples;
- origin network isolation checklist;
- forwarding-header trust statement;
- proxy harness README;
- troubleshooting guide for timeout layering.

## Acceptance criteria

- A user can deploy eggserve safely behind Caddy or nginx without exposing the origin port.
- Reference configurations are syntax-tested and integration-tested.
- Forwarding headers remain untrusted and cannot alter static serving.
- The harness can inject raw requests and observe edge/origin behavior.
- Installed artifacts are used in at least one integration path.
- No reverse-proxy feature is added to eggserve itself.

## Stop conditions

Stop and document rather than add scope if:

- a desired deployment behavior requires eggserve to proxy requests;
- client identity requires trusted-proxy policy not already designed;
- virtual hosting or Host-based root selection becomes necessary;
- an edge feature is better handled by the selected proxy;
- tests cannot distinguish edge rejection from origin invocation.

## Handoff

Plan 067 hardens connection and resource lifecycle using the deployment assumptions established here. Plan 068 extends this harness with request-smuggling and desynchronization cases.
