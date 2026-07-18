# Plan 078 — Custom-Service Ownership and Real Connection Metadata

## Goal

Repair the embedded custom-service API so supplied services are retained and invoked through the same supervised runtime as the built-in static service, and propagate truthful local, remote, scheme, and TLS connection metadata into Rust and Python request objects.

## Preconditions

- Plans 075–077 are complete or their implementation primitives are available.
- The shared connection supervisor and shutdown model from Plan 077 are the only runtime path used by new work.
- Existing Rust and Python request/connection metadata types are inventoried.

## Non-goals

Do not add:

- routing or middleware;
- ASGI/WSGI adapters;
- proxy-header trust or automatic client-IP rewriting;
- authentication or tenant context;
- a general connection registry;
- HTTP/2 or HTTP/3 metadata;
- unrelated static-serving changes.

## Defect statement

The current builder exposes a service-accepting construction path while the built server does not retain that service. The service is supplied again at start time, making the builder contract misleading and allowing arguments to be silently discarded.

Connection information is also synthesized with placeholder loopback addresses, HTTP scheme, and no TLS state. Downstream logging, access control, rate limiting, diagnostics, and Python callbacks can therefore observe false metadata.

## Track A — Public API inventory and decision record

Inventory:

- server builder types and methods;
- `build`, `build_with_service`, `service`, `start`, and `start_with_service` behavior;
- examples and doctests;
- Python callback/static server construction;
- feature-gated TLS startup;
- trait bounds and clone/send/sync requirements.

Write an ADR selecting one service ownership model.

Preferred model:

- the built server owns its service;
- `Server<S>` or an internal boxed/enum representation stores the service;
- startup consumes or borrows that retained service according to a documented lifecycle;
- a server cannot be built in a custom-service state without a service.

Minimum acceptable fallback:

- remove all builder methods that accept a service;
- require service only at `start_with_service`;
- make examples and types impossible to misread.

Do not preserve a method that accepts and discards a value.

## Track B — Type-state or explicit mode separation

Make built-in static serving and custom serving explicit.

Possible designs:

- `Server<StaticService>` and `Server<S>`;
- `ServerBuilder<NoService>` transitioning to `ServerBuilder<S>`;
- internal `ServiceMode` enum with boxed service erasure.

Selection criteria:

- no silent discard;
- clear trait bounds;
- minimal duplication;
- compatibility with shared supervisor;
- testable ownership and drop semantics;
- support for future TLS wrapping without a second runtime path;
- Python integration does not require application-framework semantics.

## Track C — Service lifecycle and ownership

Define:

- whether the service is cloned per connection, per request, or shared behind `Arc`;
- whether clone failure is possible;
- when the service is dropped;
- whether startup can be repeated;
- how service state survives keep-alive requests;
- what happens on shutdown and panic;
- thread-safety requirements.

Add tests proving:

- the supplied service instance is the one invoked;
- drop occurs exactly once for owned state;
- no service instance leaks after shutdown;
- service state persists according to the documented model;
- static and custom services use the same listener/task supervisor.

## Track D — Capture socket metadata at accept time

Capture from the accepted socket:

- local socket address;
- remote peer socket address;
- transport type;
- connection identifier if one already exists internally.

Failures to retrieve metadata must be represented explicitly. Do not replace missing data with `127.0.0.1:0` or another plausible-looking fake value.

Choose either:

- required fields with connection setup failure when metadata is unavailable; or
- optional fields with documented absence.

For normal TCP listeners, local and remote addresses should be present.

## Track E — Scheme and TLS metadata

Populate:

- `http` for plaintext;
- `https` for TLS;
- TLS-present flag or structured TLS metadata;
- negotiated protocol/version/cipher only where reliably available and already within product scope;
- server-name information only when provided by the TLS layer and safe to expose.

Do not infer scheme from headers. Do not trust `Forwarded` or `X-Forwarded-*` in this plan.

The connection executor should construct immutable per-connection metadata once and attach it to each request.

## Track F — Request propagation

Thread real metadata through:

- Hyper connection service closure;
- canonical Rust request type;
- built-in static service where metadata is observed/logged;
- custom Rust service;
- Python request adapter;
- operational event/logging hooks.

Avoid rebuilding metadata per request when it is connection-stable.

Request-local extensions should not permit user code to mutate the shared connection metadata observed by later requests.

## Track G — Python parity

Expose at minimum:

- `remote_addr`;
- `local_addr` where the API supports it;
- scheme;
- TLS presence or optional TLS metadata.

Define stable Python representations, including IPv6 formatting and optional values.

Installed-wheel tests must verify real loopback ephemeral ports rather than merely checking non-null values.

Do not expose raw Rust socket types or unsafe handle objects to Python.

## Track H — Trusted proxy boundary

Document that connection metadata is transport-peer metadata.

If eggserve is behind a reverse proxy:

- peer address is the proxy address;
- forwarded client identity remains downstream policy;
- eggserve does not automatically trust forwarding headers;
- future trusted-proxy work requires an explicit allowlist and separate plan.

This prevents real transport metadata from being confused with end-client identity.

## Required tests

### Service ownership

- builder/custom service invokes supplied instance;
- no discarded service path remains;
- service state survives multiple requests as documented;
- drop-count instrumentation on normal shutdown, forced shutdown, startup failure, and panic;
- trait-bound compile tests for accepted/rejected service types;
- doctests match actual API.

### Connection metadata

- plaintext IPv4 local/remote values;
- plaintext IPv6 where available;
- TLS scheme and TLS-present state;
- keep-alive requests share stable connection metadata;
- separate connections have distinct remote ephemeral ports;
- metadata failure path does not create placeholders;
- custom Rust service sees same values as runtime logging;
- Python callback sees actual peer/local values from installed wheel.

### Lifecycle integration

- shutdown while custom service active;
- custom service panic/error;
- forced abort and drop behavior;
- no duplicated supervisor path;
- TLS/plaintext feature matrix.

## Documentation and migration

Update:

- Rust builder/start examples;
- API snapshots;
- custom-service lifecycle contract;
- connection metadata reference;
- Python request documentation;
- reverse-proxy deployment note;
- migration notes for removed/renamed builder methods;
- finding registry and release criteria.

## Acceptance criteria

- No public method accepts and discards a service.
- A built custom-service server has an unambiguous service owner and lifecycle.
- Static and custom service modes use the shared Plan 077 supervisor.
- Rust requests receive actual local and remote socket addresses.
- Plaintext and TLS requests receive accurate scheme/TLS state.
- Python requests receive real peer metadata with stable representations.
- Missing metadata is explicit rather than replaced with fake loopback values.
- Forwarded headers are not trusted implicitly.
- Service ownership/drop and metadata tests pass on supported platforms and installed Python artifacts.
- Documentation and examples match the final API.

## Stop conditions

Stop and record a blocking design issue if:

- the selected service ownership model requires duplicating the runtime supervisor;
- TLS metadata cannot be propagated without unsafe lifetime leakage;
- Python parity would require exposing raw internal pointers or handles;
- compatibility requires retaining a silently discarded service argument;
- real socket metadata is unavailable on a claimed supported transport.

## Handoff

Plan 079 uses the corrected service invocation boundary to guarantee that rejected request bodies cannot invoke user code. Plan 080 finalizes configuration ownership after the custom-service API shape is stable.