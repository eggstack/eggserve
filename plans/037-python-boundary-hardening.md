# Phase 37 — Python Boundary Hardening

**Status:** Complete.

## Goal

Ensure the Python API cannot violate Rust-side HTTP, filesystem, lifecycle, concurrency, or body-capability invariants. The Python surface should remain thin, explicit, and auditable.

## Track A — Response validation boundary

Create one Rust-owned validation path for every Python-produced response before serialization.

Validate:
- status is a valid supported HTTP status;
- 1xx responses are rejected unless explicitly supported;
- 204 and 304 cannot carry a body;
- HEAD suppresses body bytes while preserving representation headers;
- header names and values are valid;
- CR/LF injection is impossible;
- hop-by-hop headers are rejected or controlled;
- `Transfer-Encoding` cannot be injected;
- explicit `Content-Length` agrees with the body;
- duplicate headers are preserved according to the release contract;
- consumed body sources cannot be reused.

Invalid callback results must produce a generic 500 without malformed wire output or internal details.

## Track B — Handler return and exception contract

Keep the handler return contract narrow: EggServe `Response` only unless phase 31 explicitly standardized another type.

Test:
- wrong return type;
- `None`;
- exception subclasses;
- exception after constructing a response;
- invalid status/header/body;
- reference cycles;
- handler object deletion while server runs.

Client-visible 500 responses must not include traceback, exception text, filesystem paths, or Python object representations.

## Track C — GIL and lock audit

Audit every path entering Python:
- callback semaphore acquired before GIL acquisition;
- no Tokio runtime thread performs blocking Python work directly;
- no Rust mutex or write lock is held across arbitrary Python execution;
- socket and file I/O occur without the GIL;
- `start`, `stop`, context-manager exit, and destructor paths release the GIL around blocking joins;
- queued callback tasks cannot deadlock interpreter shutdown.

Add regression tests for the phase 32 GIL deadlock and any newly found lock-ordering issue.

## Track D — Lifecycle and ownership

Define and test:
- repeated `start()`;
- repeated `stop()`;
- `stop()` before `start()`;
- context-manager auto-start and exit;
- partial bind/start failure;
- object deletion while running;
- interpreter shutdown with active server;
- active callback during stop;
- file-backed response during stop;
- callback references and root lifetime.

Use explicit errors rather than silent no-ops where state misuse matters.

## Track E — File-backed response capability

Verify:
- Python cannot construct a trusted resolved file through ordinary public APIs;
- bridge-only constructors remain feature-gated and native-internal;
- `StaticResponder` and secure-root resolution preserve open handles;
- handler-returned file/range bodies do not reopen paths;
- body sources are single-consumption capabilities;
- bounded read helpers cannot bypass range limits;
- file-stream permit lifetime covers the full body stream.

## Track F — Request representation

Review Python `Request` fidelity:
- method;
- raw/path/query distinctions;
- HTTP version;
- remote address;
- body-presence metadata;
- ordered duplicate-preserving headers;
- non-UTF-8 header handling according to the declared contract.

Do not expose normalized mappings as the only representation if they lose protocol-relevant duplicates.

## Track G — Exception hierarchy

Define public exception classes for:
- lifecycle/configuration errors;
- request validation errors;
- policy/path errors;
- filesystem resolution errors where exposed;
- response construction errors;
- client transport/protocol/TLS/timeout/body-limit errors.

Preserve stable base classes while allowing specific subclasses. Avoid leaking internal Rust enum formatting as the API contract.

## Track H — Python API consistency

Audit:
- constructor signatures;
- default values;
- keyword-only behavior where appropriate;
- frozen/immutable classes;
- `repr` safety;
- type annotations/stubs if provided;
- `__all__` and documentation;
- native module versus package-level names.

## Tests

Add installed-wheel and source-tree tests for:
- malicious response headers;
- duplicate headers;
- invalid status/body combinations;
- wrong handler return types;
- exception leakage;
- lifecycle misuse;
- stop/start races;
- active callback shutdown;
- file capability reuse;
- GIL regression;
- public exception classes.

## Acceptance criteria

- Python cannot emit malformed HTTP through EggServe response APIs.
- Handler failures are generic and non-leaking.
- No lock is held across arbitrary Python execution.
- Blocking lifecycle operations release the GIL.
- File-backed responses preserve resolver-opened handles.
- Request headers retain required fidelity.
- Exception taxonomy is documented and tested.
- Installed-wheel behavior matches source-tree behavior.

## Non-goals

- No async Python handler API.
- No ASGI/WSGI adapter.
- No routing or middleware layer.
- No generic Python file-object streaming API that weakens capability guarantees.
