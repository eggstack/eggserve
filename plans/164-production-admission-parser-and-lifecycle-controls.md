# Plan 164 — Production Admission, Parser, and Lifecycle Controls

## Status

**IMPLEMENTED / CLOSED.**

Prerequisites: Plan 161 present. Coordinate with Plans 162/163 because streaming responses and caller-owned connections must participate in the same budgets.

## Goal

Close the still-open production resource/lifecycle work identified historically in Plan 067 and make the generic runtime suitable for high-concurrency upstream use without turning EggServe into a WAF or edge proxy.

The core requirement is that independent resource classes have independent, observable bounds. An idle keep-alive connection must not consume the same budget as a Python/Rust handler actively executing, and parser memory must not be governed only by upstream library defaults.

## Do not duplicate completed work

Plan 067 already established the desired direction and portions have landed: connection/file-stream admission, multiple deadlines, shutdown, body ceilings, counters, and related qualification. This plan is a closure pass for the remaining gaps, not a replacement for that history.

## Phase 0 — Hyper dependency refresh and requalification

The current lockfile uses Hyper 1.10.1. At plan time Hyper 1.11.x contains HTTP/1 correctness and buffer-enforcement fixes relevant to this scope.

Before adding new parser policy:

1. update Hyper/hyper-util to the current semver-compatible versions allowed by the workspace;
2. inspect release notes for HTTP/1 server semantic changes;
3. rerun canonical wire, smuggling/desync, corpus, property/fuzz, TLS, Python facade, and shutdown tests;
4. record any behavior delta before changing EggServe policy.

Do not describe the existing version as vulnerable absent an applicable advisory. This is dependency maintenance plus HTTP/1 requalification.

Reference: https://github.com/hyperium/hyper/releases

## Parser and header resource ceilings

The HTTP/1 builder currently sets the header-read timeout but does not explicitly set all available parser limits. Hyper documents that defaults are not stable.

Add validated EggServe-owned configuration for at least:

- maximum HTTP/1 parser/read buffer size (`max_buf_size`, minimum supported by Hyper is 8192);
- maximum request header field count (`max_headers`);
- maximum post-parse aggregate request-header name/value bytes;
- maximum request-target length before application service work;
- a documented request-line limit model, including any part that Hyper cannot independently configure without replacing the parser.

Set Hyper options explicitly so release upgrades cannot silently widen resource policy.

Reference: https://docs.rs/hyper/latest/hyper/server/conn/http1/struct.Builder.html

### Defaults

Choose defaults from compatibility + measured memory cost, not the smallest possible number. Preserve ordinary browser/proxy compatibility. Document heap-allocation/performance effects of non-default Hyper header counts where relevant.

### Failure behavior

- parser/header excess must fail before service invocation;
- use standards-appropriate 4xx where Hyper/runtime can safely produce it (e.g. 431);
- if parsing has not produced a trustworthy request boundary, close rather than trying to preserve keep-alive;
- counters/events must distinguish header timeout, header count/size, request-target limit, and malformed framing where feasible.

## Separate admission budgets

Introduce distinct runtime admission classes:

1. **open connections** — sockets/streams held by the HTTP runtime;
2. **in-flight service requests** — canonical `Service::call()` executions, independent of idle keep-alive connections;
3. **file streams** — existing budget;
4. **application response streams** — only add a separate budget if Plan 162 evidence shows one is necessary; otherwise account them under in-flight request/connection limits and document why;
5. **Python callback workers** — adapter-specific limit owned by Plan 166, but it must compose with the generic in-flight service limit rather than replace it.

`max_connections` must not be treated as the sole application backpressure control.

### Saturation behavior

- connection admission exhaustion may reject/drop before an HTTP request exists;
- request/service admission exhaustion should produce a deterministic generic 503 where the request is already trustworthy;
- never queue unbounded work waiting for a permit;
- if bounded waiting is supported, make the queue depth/time explicit and default to no hidden queue;
- always recover permits on timeout, cancellation, panic, disconnect, and shutdown.

## Connection/request lifecycle controls

Replace the current production dependence on one mandatory `connection_total_timeout` with independently meaningful lifecycle controls.

Target controls:

- header read deadline — existing;
- request-body read deadline — existing;
- handler/service deadline — existing;
- keep-alive idle timeout — new;
- optional hard maximum connection lifetime — retain as a defense-in-depth knob, but do not make a 60-second lifetime the only way to bound idle clients;
- maximum completed requests per connection — new;
- response write/no-progress timeout — new;
- graceful-shutdown drain deadline — existing.

### Compatibility/migration

`connection_total_timeout` is already public. Do not silently reinterpret it.

Choose one migration strategy and document it:

- retain it as `Option<Duration>`/hard lifetime with old builder method deprecated if necessary; or
- introduce a clearly named `max_connection_lifetime` while keeping the old field for a deprecation window.

Reverse-proxy production defaults should permit healthy long-lived keep-alive connections while bounding idle/stalled clients. The stdlib compatibility facade may retain conservative compatibility defaults if changing them would be observable.

## Keep-alive idle timeout

The timeout must reset based on actual request/connection activity according to a documented definition; it must not merely be another total lifetime.

Test:

- idle after response;
- slow but still within header deadline;
- repeated valid keep-alive requests;
- shutdown while idle;
- proxy-like persistent connections.

## Maximum requests per connection

After the configured count:

- complete the current response correctly;
- signal/perform connection close without corrupting framing;
- count HEAD/error responses consistently;
- document whether rejected requests count.

Default may be high or disabled if measurements show no meaningful benefit, but the control must exist for anonymity-sensitive/resource-constrained profiles if implementation is cheap and correct.

## Response write/no-progress timeout

This is a required design spike before implementation because timing only body production does not protect against a client that stops reading and fills socket/transport buffers.

Evaluate two approaches:

1. wrap the transport `AsyncWrite` with progress/deadline instrumentation;
2. integrate a per-connection response-progress signal around Hyper's write path without exposing Hyper publicly.

Requirements:

- timeout means no forward write progress for the configured interval, not a fixed maximum duration for legitimate large responses;
- works for files, buffered bodies, and Plan 162 streams;
- works for TLS and caller-owned transports;
- on expiry, cancel producer/file work and close the connection;
- no secondary response after partial commitment;
- no unbounded buffering to avoid the timeout.

If Hyper makes a precise no-progress timer impractical without invasive custom I/O wrapping, implement the wrapper at Plan 163's transport boundary and document it as runtime I/O instrumentation.

## Per-profile defaults

Document defaults for:

### Reverse-proxy production

Favor persistent connections, bounded parser memory, meaningful service concurrency, and idle/write-stall defense.

### Direct TLS

Same core bounds plus TLS handshake budget already present.

### Embedded anonymity-sensitive

Allow stricter open-connection, header, keep-alive, request-count, and write-stall defaults suitable for resource-constrained direct origins. This is still not rate limiting: all clients share generic resource budgets.

## Observability

Extend structured counters/events for:

- parser buffer/header-count/header-byte/request-target rejection;
- connection admission saturation;
- service admission saturation;
- keep-alive idle expiry;
- max-requests close;
- hard connection lifetime expiry;
- write no-progress expiry;
- permit recovery/invariant failures if instrumentable.

Do not log full hostile headers/request targets by default.

## Tests and hostile cases

Add deterministic tests for:

- slowloris header delivery;
- excessive header count;
- aggregate header bytes and parser buffer saturation;
- long request target;
- duplicate/framing adversarial cases after dependency refresh;
- many idle keep-alive connections plus active requests;
- service saturation while connection capacity remains;
- stalled client reader on large file and streaming response;
- long healthy download with continuous progress;
- max requests per connection;
- optional hard lifetime;
- all timeout/saturation cancellation paths returning permits;
- shutdown under each blocked phase;
- TLS and caller-owned stream parity.

Use soak/resource-trend tests where existing Plan 072/088 infrastructure supports them.

## Non-goals

Do not add:

- per-IP/client/user token buckets;
- authentication-based quotas;
- SYN protection, connection reputation, CAPTCHA/bot logic;
- request routing/middleware;
- reverse-proxy features;
- custom HTTP parser replacement solely to expose a more exact request-line knob;
- kernel-bypass/io_uring complexity without benchmark evidence.

## Acceptance criteria

- [ ] Hyper HTTP/1 dependencies are current and the existing hostile-wire corpus is requalified.
- [ ] Parser buffer and header-count policy are explicit EggServe configuration, not inherited defaults.
- [ ] Aggregate header/request-target work is bounded before application service execution.
- [ ] Open connections and in-flight service calls have separate limits.
- [ ] Keep-alive idle time, optional hard lifetime, and request count have documented independent semantics.
- [ ] A stalled response writer cannot pin a connection/producer indefinitely.
- [ ] Saturation/timeout paths recover every permit and remain observable.
- [ ] Profile-specific defaults and migration from `connection_total_timeout` are documented.
- [ ] No WAF/rate-limiting responsibility has entered the core.

## Handoff

Plan 165 composes a stricter privacy profile from these generic controls. Plan 166 adds Python callback admission on top of the generic service budget. Plan 168 supplies high-concurrency and slow-client evidence.

## Closure record

Implemented in commit `888e24f9c636b765a8bf808b142a71da5b73d687`.

The implementation retains the documented Hyper 1.11.1 parser behavior and
independent admission/lifecycle controls. Remaining broad performance
evidence is tracked by Plan 170.
