# Phase 53 — Milestone 3C: Python Runtime Parity, Conformance, and API Closure

## Goal

Close Milestone 3 by projecting the reusable Rust runtime and lifecycle model into Python without duplicating transport logic, then proving CLI/Rust/Python parity through installed-wheel, real-socket, lifecycle, and release-evidence tests.

The Python API must remain a controlled projection of the Rust runtime. Python may provide ergonomic wrappers, but it must not own socket parsing, HTTP framing, file streaming, timeout enforcement, or independent lifecycle state.

This phase also performs the final API and conformance review for the Milestone 3 runtime surface.

## Starting state

Eggserve already has:

- PyO3-backed primitive types;
- Python `Server`, `ServerSecureRoot`, `Request`, and `Response` types;
- subprocess lifecycle helpers;
- context-manager support;
- Rust-owned socket I/O and file streaming;
- installed-wheel test infrastructure;
- canonical request/response parity tests;
- lifecycle exceptions;
- cross-platform wheel jobs.

Phases 51 and 52 should provide:

- a public Rust `Service` boundary;
- reusable `ServerBuilder`/runtime configuration;
- `ServerHandle` lifecycle;
- listener ownership;
- readiness;
- graceful and forced shutdown;
- a reusable `StaticService`.

This phase must map those capabilities into one coherent Python surface and eliminate legacy divergence.

## Track A — Python API design review

Define the Python runtime API before implementation.

Recommended conceptual surface:

```python
from eggserve import Server, ServerConfig, StaticService

service = StaticService("public")
server = Server(service=service, config=ServerConfig(port=0))
server.start()
server.wait_ready()
print(server.addr)
server.shutdown()
server.wait()
```

For callback services:

```python
def handler(request):
    return Response.text(200, "ok")

server = Server(handler=handler)
```

Requirements:

- one `Server` class maps to the Rust runtime;
- static root and callback conveniences become service constructors/adapters rather than separate transport implementations;
- existing APIs remain compatible where reasonable;
- lifecycle methods have deterministic blocking semantics;
- context-manager behavior is explicit;
- no hidden subprocess use in the native `Server` API;
- subprocess helpers remain separately named and documented.

Decide whether to retain names such as `ServerSecureRoot` or replace them with a clearer `StaticService`/`SecureRoot` split. Provide deprecation aliases if needed.

## Track B — Synchronous callback execution model

Keep the first stable Python runtime callback model synchronous unless an async model is explicitly implemented and tested.

Define:

- which thread invokes Python callbacks;
- whether callbacks execute in a bounded worker pool;
- maximum callback concurrency;
- GIL acquisition/release behavior;
- handler timeout semantics;
- cancellation limitations;
- context-variable behavior if relevant;
- what happens when callback execution outlives forced shutdown.

Preferred safety properties:

- Rust I/O threads do not remain blocked indefinitely on Python execution;
- callback concurrency is bounded;
- callback exceptions become sanitized 500 responses;
- no Python traceback leaks to clients;
- file responses retain Rust-owned streaming;
- callback return validation occurs before transport conversion.

Do not implicitly accept coroutine objects. Either reject them with a typed error or introduce a clearly experimental `AsyncServer` in a later milestone.

Tests:

- normal callback;
- callback exception;
- invalid return type;
- callback timeout;
- blocking callback during shutdown;
- concurrent callbacks;
- callback returning bytes/file/range response;
- coroutine returned accidentally;
- GIL released during network/file I/O.

## Track C — Python lifecycle parity

Map the Rust lifecycle state machine directly into Python.

Required behavior:

- `start()` transitions once;
- `wait_ready()` reports startup failure;
- `addr` is available only after bind/readiness according to documented rules;
- `shutdown()` requests graceful shutdown;
- `shutdown(force=True)` or a separate `force_shutdown()` is explicit;
- `wait()` returns or raises the terminal runtime result;
- double start and invalid stop operations raise typed lifecycle exceptions;
- multiple shutdown calls are deterministic;
- context manager starts and shuts down safely;
- object finalization does not silently leak a running server.

Define destructor behavior conservatively. Do not depend on `__del__` for correctness. Emit a warning or perform a bounded emergency shutdown only if safe and documented.

Add `.pyi` signatures and exception hierarchy updates.

## Track D — Listener and port ownership in Python

Expose the common safe cases:

- bind host/port;
- port zero;
- query actual bound address;
- optionally accept an existing Python socket if ownership transfer can be made safe and portable.

If existing-socket support is added:

- require a TCP socket;
- duplicate the descriptor/handle or document ownership transfer precisely;
- normalize blocking mode;
- prevent double close;
- support Unix and Windows semantics explicitly;
- reject unsupported socket states with typed errors.

It is acceptable to defer Python existing-socket support while Rust supports it, provided the capability matrix states the difference.

## Track E — Static service and custom service parity

Expose Python constructors for:

- hardened static service;
- callback service;
- optional static fallback/composition only if already implemented in Rust.

Static service must preserve:

- no symlinks by default;
- no dotfiles by default;
- no directory listing by default;
- GET/HEAD-only behavior;
- no request bodies;
- range and conditional semantics;
- Rust-owned file streaming;
- Unix descriptor-relative confinement;
- Windows limitation statements.

Test that Python static service and CLI produce equivalent wire responses for the same root/configuration.

## Track F — Configuration parity

Create or refine a typed Python configuration object matching Rust runtime semantics.

Fields should include only supported runtime controls:

- bind host/port or address;
- connection limit;
- file-stream limit;
- header timeout;
- handler timeout;
- response-write timeout;
- graceful-shutdown timeout;
- keep-alive policy if public;
- TLS settings if included in the wheel/runtime build;
- callback concurrency if Python-specific.

Requirements:

- validation occurs before start;
- defaults match Rust/CLI defaults unless intentionally Python-specific;
- unknown or unsupported options fail clearly;
- duration units are unambiguous;
- config objects are immutable or safely snapshot at start;
- support claims account for wheels that exclude TLS.

Add contract-consistency checks comparing documented defaults across Rust, Python, CLI help, and release metadata.

## Track G — Real-socket parity matrix

Build a shared lifecycle/runtime conformance matrix executed against:

1. CLI static server;
2. embedded Rust static service;
3. native Python static service;
4. embedded Rust custom service;
5. Python callback service.

Cover:

- startup/readiness;
- port zero;
- GET/HEAD;
- duplicate response headers;
- byte bodies;
- full-file bodies;
- range bodies;
- 204/304 suppression;
- malformed request rejection;
- connection metadata;
- keep-alive;
- connection-limit saturation;
- handler timeout;
- graceful shutdown;
- forced shutdown;
- shutdown during slow headers;
- shutdown during slow response read;
- shutdown during file stream;
- callback exception;
- terminal wait result.

Use common fixture descriptions where feasible so semantics cannot drift among frontends.

## Track H — Cross-platform installed-wheel validation

Run all Python runtime tests from installed wheels with:

- `PYTHONPATH` unset;
- working directory outside the repository;
- native extension loaded from the installed wheel;
- bundled CLI resolved from the wheel;
- clean virtual environment.

Platforms:

- Linux x86_64;
- macOS runner;
- Windows x86_64.

Required tests:

- public imports and stubs;
- callback runtime;
- static service;
- lifecycle state errors;
- readiness and port zero;
- graceful/forced shutdown;
- CLI/native parity;
- file streaming;
- connection limits;
- packaging metadata.

Continue to classify Windows as functional/parser-hardened rather than Unix-level filesystem-hardened.

## Track I — Runtime API compatibility and migration

Review old and new Python surfaces:

- native `Server`;
- `ServerSecureRoot`;
- `StaticResponder`;
- `ServerProcess`;
- `ServeConfig`;
- `serve_directory()`;
- new runtime configuration/service types.

For each, determine:

- stable role;
- experimental role;
- deprecation alias;
- subprocess versus native behavior;
- removal timeline.

Avoid two classes named `Server` with materially different execution models.

Update migration documentation with concrete before/after examples.

Use API snapshot tests to catch accidental export/signature drift.

## Track J — Runtime observability hooks for tests

Expose only the minimal safe runtime state needed for lifecycle verification:

- bound address;
- current lifecycle state;
- optional active connection count;
- optional active request/file-stream counts;
- terminal error/result.

Do not expose Tokio task handles, sockets, raw file descriptors, or mutable internal registries.

If counters are not intended as stable public API, provide test-only/internal instrumentation and document that decision.

## Track K — Performance and resource qualification

Measure Python runtime overhead relative to embedded Rust service:

- callback dispatch latency;
- GIL acquisition cost;
- byte response throughput;
- file response throughput;
- memory per active callback;
- shutdown latency;
- worker-pool saturation behavior.

The goal is not equivalence with pure Rust callbacks. The goal is bounded, explainable overhead with no transport regression for file serving.

Add soak tests for:

- repeated server lifecycle cycles;
- callback exceptions under concurrency;
- callback timeouts;
- many idle connections;
- file streams during shutdown;
- no thread/file-descriptor growth.

## Track L — API stability promotion review

At Milestone 3 closure, classify the new runtime APIs.

Promotion requirements:

- production path uses them;
- CLI and Python project onto the same implementation;
- real-socket parity matrix passes;
- installed-wheel matrix passes;
- lifecycle stress tests pass;
- public consumer fixtures exist;
- documentation and migration guide are complete;
- no unresolved cancellation or ownership ambiguity remains.

It is acceptable to keep the runtime experimental for one release. Do not promote merely because implementation is complete.

## Track M — Release criteria and evidence

Add release gates for:

- Rust runtime consumer fixtures;
- runtime lifecycle tests;
- listener ownership tests;
- graceful/forced shutdown stress;
- CLI/runtime parity;
- Python callback runtime;
- Python lifecycle parity;
- cross-platform installed-wheel runtime tests;
- resource-leak qualification;
- migration/API snapshot checks.

Run a final main-branch evidence workflow and record:

- evaluated SHA;
- gate results;
- platform wheel results;
- resource qualification artifact;
- benchmark artifact;
- known limitations;
- API stability decision.

## Required validation

Rust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test runtime_conformance
cargo test -p eggserve-core --test lifecycle_conformance
cargo test -p eggserve-bin --test production_path
cargo test -p eggserve-core --test public_api_consumers
```

Use actual final test names if implementation chooses different files.

Python:

- build release wheel with matching bundled CLI;
- install in clean venv;
- run runtime, callback, lifecycle, static-service, API-stability, typing, parity, and resource tests with `PYTHONPATH` unset;
- run on Linux, macOS, and Windows.

Release infrastructure:

```sh
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
python3 -m unittest scripts.test_release_criteria -v
python3 -m unittest scripts.test_release_safety -v
bash scripts/release-validate.sh metadata
```

## Completion criteria

Milestone 3 is complete only when:

- Rust and Python use the same runtime and lifecycle implementation;
- Python callbacks execute under a bounded, documented model;
- readiness, graceful shutdown, forced shutdown, and terminal results are parity-tested;
- static service behavior matches CLI behavior;
- installed-wheel tests pass on every advertised OS family;
- no transport or file streaming is reimplemented in Python;
- lifecycle/resource stress shows no leaks;
- old/new API roles and migration are explicit;
- release criteria and final-SHA evidence are complete;
- downstream projects can build custom servers using only public runtime APIs.

## Non-goals

- Async Python coroutine handlers.
- ASGI or WSGI implementation.
- Routing or middleware frameworks.
- Request-body streaming.
- HTTP/2, WebSockets, and upgrades.
- Hot reload or configuration reload.
- Python ownership of raw sockets after runtime start.