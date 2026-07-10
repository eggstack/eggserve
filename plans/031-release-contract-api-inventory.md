# Phase 31 — Release Contract and Public API Inventory

## Goal

Define the exact product and compatibility surface intended for EggServe’s first public release. Correct or remove weak public APIs before they become de facto contracts.

This phase is an audit and stabilization pass. It must not add framework features, ASGI/WSGI adapters, routing, middleware, redirects, retries, cookies, pooling, HTTP/2, or HTTP/3.

## Starting state

EggServe exposes:

- a CLI and binary static server;
- Rust path, policy, secure-resolution, response-planning, and body primitives;
- Python equivalents and a Rust-owned callback server;
- experimental Rust/Python HTTP client primitives;
- internal Python-binding bridge APIs behind `python-bindings-internal`.

The implementation is functional, but the repository does not yet have one normative compatibility inventory covering every exported name and behavioral guarantee.

## Deliverables

1. `docs/release-contract.md`
2. `docs/api-stability.md`
3. A generated or manually maintained Rust export inventory.
4. A Python public export inventory.
5. Code/doc corrections needed to make exports match their classification.
6. Tests proving internal APIs are absent from default-feature/public package surfaces.

## Workstream A — Define the release product

Document the supported release artifacts:

- `eggserve` binary;
- `eggserve-core` Rust crate, if published;
- Python wheel/package;
- optional client and TLS features.

Define supported behavior:

- HTTP/1.1 only unless another version is explicitly implemented;
- supported request methods and request-target form;
- static directory behavior;
- conditional requests and ranges;
- safe bind defaults and public-bind acknowledgement;
- callback server behavior;
- buffered client behavior;
- supported platforms and security level per platform.

Separate normative guarantees from implementation notes.

## Workstream B — Rust API inventory

Inventory every `pub` item reachable from documented crate roots. At minimum review:

- path: `ConfinedPath`, `PathPolicy`, `PathRejection`;
- policy: `StaticPolicy` and policy enums;
- secure root: `SecureRoot`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory`;
- response planning: `StaticResponsePlan`, `ResponseStatus`, header/body plan types;
- body: `BodySource`, `BodyKind`, `BodySourceError`;
- convenience facade: `resolve_and_plan` and validation helpers;
- client: config, method, request builder/request, response, error, parsed URL exposure if any.

For each item assign one classification:

- stable for the first public release;
- experimental and explicitly exempt from normal compatibility promises;
- internal bridge;
- deprecated before release;
- remove before release.

Audit constructors and public fields. Any type whose invariants can be bypassed through public fields or unchecked constructors must be corrected before classification as stable.

## Workstream C — Python API inventory

Inventory `eggserve.__all__`, native module exports, documented classes/functions, and exception classes.

Review:

- path/policy wrappers;
- secure root and resolved-resource wrappers;
- response-plan and body-source wrappers;
- `Server`, `Request`, `Response`, `StaticResponder`, `ServerSecureRoot`;
- client config/request/response/client/error types;
- subprocess/CLI lifecycle helpers.

Ensure:

- names in `__all__` exist;
- documented imports work from an installed wheel;
- internal bridge names are not exported;
- native-only implementation names do not accidentally become public;
- exceptions have a coherent hierarchy rather than generic `ValueError` mapping.

## Workstream D — Header representation decision

Make an explicit decision for request and response headers.

A plain mapping loses duplicate fields such as `Set-Cookie` and can alter HTTP semantics. Choose and document one of:

- ordered list/tuple of `(name, value)` pairs as the canonical representation;
- a dedicated multi-value header type;
- a narrow mapping API plus explicit duplicate-preserving raw access.

The callback and client surfaces must preserve duplicates where HTTP requires them. Do not silently collapse duplicate response headers.

## Workstream E — Response contract decision

Define exactly what a Python handler may return. Preferred initial contract:

- only an EggServe `Response` object;
- invalid return types produce a generic 500;
- invalid status/header/body combinations are rejected before serialization;
- file-backed response capabilities remain opaque and Rust-owned.

Specify behavior for:

- HEAD;
- 204 and 304;
- informational statuses, if unsupported;
- explicit `Content-Length`;
- hop-by-hop headers;
- `Transfer-Encoding`;
- duplicate headers;
- consumed body sources.

## Workstream F — Client stability decision

Decide whether the client is:

- experimental in the first release; or
- part of the stable contract.

Given its narrow buffered, one-connection-per-request behavior, the default recommendation is experimental. Document that it provides no pooling, redirects, cookies, proxies, retries, decompression, or streaming response API.

## Workstream G — Internal bridge isolation

Verify `python-bindings-internal` is:

- disabled by default;
- undocumented as a normal user feature;
- used only by the Python crate;
- incapable of leaking through default Rust docs or package examples.

Add compile/build checks proving default and client builds do not expose bridge-only constructors/extractors.

## Tests

Add tests or CI checks for:

- Rust default-feature public API compile sample;
- client-feature compile sample;
- Python installed-wheel public import list;
- absence of internal names from `eggserve` and `_native` where practical;
- duplicate-header round trips;
- callback response validation;
- documentation examples compiling/running.

Consider `cargo public-api` or an equivalent snapshot mechanism, but do not introduce fragile tooling without documenting its maintenance model.

## Documentation updates

Update:

- README project-status and stability claims;
- `architecture/primitives-api.md`;
- `architecture/eggserve-python.md`;
- client documentation;
- Python API documentation;
- non-goals and compatibility policy.

Use consistent labels: stable, experimental, internal.

## Acceptance criteria

- Every exported Rust and Python item is classified.
- No undocumented public item remains.
- Header duplicate semantics are resolved and tested.
- Callback response rules are normative and tested.
- Internal bridge APIs are unavailable under normal features.
- Client stability is explicitly declared.
- Public docs and actual exports agree.
- Any API removals or renames occur before the release candidate phase.

## Non-goals

- No new application protocol adapters.
- No framework abstractions.
- No client convenience ecosystem expansion.
- No compatibility freeze before the audit is complete.
