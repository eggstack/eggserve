# Phase 47 — Milestone 2A: Canonical HTTP Request Types

## Goal

Introduce transport-independent, stable HTTP request value types for Rust and Python so downstream projects can inspect requests without depending on Hyper internals or lossy Python projections.

This phase establishes the request-side contract only. It must not add request-body streaming, routing, middleware, ASGI/WSGI adapters, or a general application framework.

## Starting state

Eggserve already validates methods, request targets, and body framing, and the Python server exposes a request object. However:

- public request concepts are split across validators and runtime-specific types;
- method handling is primarily string-oriented;
- HTTP version and connection metadata are not represented as a clean public contract;
- Python header mappings lose duplicate fields;
- stable APIs must avoid exposing Hyper types.

## Track A — Public type design

Define a canonical request-head model in `eggserve-core::primitives` or a dedicated public module:

```rust
pub struct RequestHead {
    method: Method,
    target: RequestTarget,
    version: HttpVersion,
    headers: HeaderBlock,
}
```

Requirements:

- owned or safely reference-counted data;
- no public Hyper types;
- immutable inspection after construction;
- validated construction through parser/runtime adapters and explicit constructors;
- stable debug/display behavior only where intentionally documented;
- clear size and allocation behavior.

Decide whether fields are public or accessed through methods. Prefer methods if future representation changes are likely.

## Track B — Method model

Add a validated `Method` type supporting:

- standard methods;
- extension methods without information loss;
- token validation;
- canonical string access;
- case-sensitive method semantics;
- safe/idempotent classification helpers where standards-based and useful;
- `permits_static_resolution()` or equivalent policy helper without conflating method identity with server policy.

Keep the built-in static service restricted to GET and HEAD.

Compatibility requirements:

- preserve existing `ReadOnlyMethod` APIs or provide a documented migration path;
- do not silently coerce invalid methods;
- avoid a non-exhaustive public enum that prevents extension methods.

Tests:

- standard methods;
- valid extension tokens;
- invalid separators/control characters;
- case preservation;
- round-trip string conversion;
- classification helpers.

## Track C — HTTP version model

Add an `HttpVersion` type covering the versions the runtime actually supports:

- HTTP/1.0;
- HTTP/1.1.

Unsupported versions should be rejected at the transport boundary rather than represented as if supported, unless a deliberate `Other` representation is needed for downstream inspection.

Document:

- keep-alive implications belong to the runtime, not this value type;
- no HTTP/2 support claim;
- exact serialization and comparison behavior.

## Track D — Duplicate-preserving header block

Create a canonical ordered `HeaderBlock` and validated `HeaderName`/`HeaderValue` surface.

Requirements:

- preserve duplicate fields;
- preserve deterministic iteration order;
- case-insensitive lookup by field name;
- expose all values for a name;
- distinguish absent, single, and multiple values;
- reject invalid names, control bytes, CR/LF injection, and invalid value bytes according to the chosen HTTP/1 contract;
- define whether original field-name casing is preserved;
- avoid implicit comma joining for fields where joining changes semantics;
- provide explicit single-value convenience methods that return an ambiguity error when duplicates exist.

Recommended methods:

```rust
headers.iter()
headers.get_first(name)
headers.get_all(name)
headers.get_unique(name) -> Result<Option<&HeaderValue>, DuplicateHeaderError>
```

Do not use a map as the normative representation.

## Track E — Request target integration

Unify the existing target validation/path parsing surface with the canonical request model.

Define the relationship among:

- raw request target;
- validated origin-form target;
- path;
- query;
- decoded confined path used for static resolution.

Requirements:

- preserve raw target when safe/useful for logging or downstream parsing;
- do not decode path components twice;
- do not normalize away security-significant distinctions;
- reject absolute-form, authority-form, and asterisk-form where outside the current server contract;
- keep query parsing out of scope beyond safe raw access and optional split.

Add tests for percent encodings, query delimiters, invalid UTF-8/octet policy, dot segments, backslashes, and origin-form enforcement.

## Track F — Connection metadata

Introduce a separate immutable `ConnectionInfo` type:

```rust
pub struct ConnectionInfo {
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
    scheme: Scheme,
    tls: Option<TlsInfo>,
}
```

Requirements:

- values come from the actual transport;
- `Forwarded` and `X-Forwarded-*` remain ordinary untrusted headers;
- TLS metadata is bounded and avoids exposing implementation-specific internals;
- absence of TLS is explicit;
- connection metadata is not mixed into request headers.

Potential TLS fields:

- negotiated protocol/version if available;
- server name if safely available;
- peer certificate information only if already supported and privacy-reviewed.

Do not add proxy-trust configuration in this phase.

## Track G — Runtime adoption

Map Hyper requests into the canonical request types at one boundary.

Requirements:

- conversion is fallible and typed;
- malformed/unsupported input is rejected before handlers;
- built-in static service consumes canonical request types where practical;
- Python callbacks receive the canonical projection;
- no duplicate parsing or inconsistent validation paths;
- benchmark allocation/regression impact.

Avoid a parallel legacy request model persisting indefinitely. If temporary compatibility adapters are needed, mark them internal and add removal criteria.

## Track H — Python projection

Expose Python request types with parity:

- `Method`;
- `HttpVersion`;
- duplicate-preserving headers;
- raw target/path/query access;
- `ConnectionInfo`;
- immutable request object.

Python headers should support:

- iteration as ordered `(name, value)` pairs;
- `get_all(name)`;
- `get_unique(name)` with a typed duplicate error;
- dictionary conversion only as an explicit lossy operation.

Do not silently retain the old flat mapping as the canonical property. If compatibility requires it, deprecate it and name it clearly as lossy.

Add `.pyi`/typing updates and installed-wheel tests.

## Track I — Error taxonomy and stability

Define typed errors for:

- invalid method;
- invalid HTTP version;
- invalid header name;
- invalid header value;
- duplicate unique-header access;
- invalid request target;
- canonical conversion failure.

Update:

- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- Rust docs and Python API docs.

Explicitly classify new APIs as stable or experimental. Prefer experimental during implementation, promoting only after conformance completion in Phase 49.

## Required tests

Rust:

- unit tests for every value type;
- property tests for method/header validation;
- raw-wire mapping tests;
- duplicate-header regression tests;
- request-target corpus tests;
- compile fixtures proving no Hyper dependency in public use;
- serde-free unless serialization is intentionally part of the contract.

Python:

- ordered duplicate preservation;
- immutability;
- typed errors;
- connection metadata;
- installed-wheel imports and signatures;
- Rust/Python parity fixtures.

Run existing raw-wire, production-path, static-serving, Python server, fuzz corpus, and API-stability suites.

## Completion criteria

- downstream Rust code can inspect a request using only public eggserve types;
- no Hyper type appears in the stable request API;
- duplicate headers survive transport-to-handler conversion in Rust and Python;
- request target semantics remain security-preserving;
- connection metadata is trustworthy and separate from forwarding headers;
- built-in static behavior remains unchanged;
- API docs and stability inventory are updated;
- no material allocation/performance regression is unexplained.

## Non-goals

- Request body streaming.
- Query-parameter framework.
- Routing or middleware.
- Proxy-header trust.
- HTTP/2.
- ASGI/WSGI adapters.