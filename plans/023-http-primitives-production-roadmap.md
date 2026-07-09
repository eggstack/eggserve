# Plan 023: HTTP Primitives Production Roadmap

## Summary

This roadmap extends eggserve from a hardened static file server into a lean, security-aware HTTP primitives substrate. The goal is not to turn eggserve into an ASGI server, WSGI server, application framework, reverse proxy, or batteries-included web stack. The goal is to expose correct, auditable HTTP and filesystem primitives so downstream projects can build those higher-level systems on top of eggserve while Rust continues to own protocol parsing, socket I/O, concurrency, timeout enforcement, file streaming, and security-sensitive policy decisions.

The intended end state is a small set of stable Rust and Python APIs that provide the kind of low-level utility Python's `http.server`, `http.client`, and adjacent standard-library modules provide, but with stronger correctness, better filesystem confinement, explicit security policy, resource limits, and Rust-backed async execution. A separate downstream project should be able to use eggserve to build an ASGI implementation, WSGI bridge, static/dynamic hybrid server, HTTP client, or test harness without eggserve carrying those integration layers in-tree.

## Scope boundary

### In scope

- Hardened static-file serving remains first-class.
- Correct HTTP/1.1 request, header, method, target, body-policy, and response-planning primitives.
- Rust-owned accept loop, connection lifecycle, timeouts, limits, and file streaming.
- Python bindings for low-level HTTP request/response primitives.
- Python bindings for safe static resolution and resolver-opened file streaming.
- Python-facing server lifecycle primitives that allow controlled Python dispatch while keeping socket I/O and backpressure in Rust.
- Low-level HTTP client primitives suitable for building higher-level clients later.
- Documentation that explains how downstream projects may build ASGI/WSGI/adapters externally.
- Tests proving that the primitive APIs preserve the same policy guarantees as the CLI.

### Out of scope

- In-tree ASGI adapter.
- In-tree WSGI adapter.
- Routing framework.
- Middleware framework.
- Template rendering.
- Cookie/session/auth framework.
- Reverse proxying.
- HTTP cache layer.
- Plugin system.
- Dynamic Python execution in the static server path.
- HTTP/2 or HTTP/3 before HTTP/1.1 primitives are stable and production-reviewed.

Downstream adapters are explicitly enabled. They are not implemented here.

## Design principles

### Capabilities over reconstructed paths

A resolved file is a capability. A string path is not. Python and Rust consumers must not need to reconstruct filesystem paths from safe components and reopen files manually. Any API that would encourage reopening a path after policy enforcement is a security regression.

### Rust owns the dangerous parts

Rust should own:

- TCP listener and accepted sockets.
- HTTP parser/serializer integration.
- Connection concurrency and backpressure.
- Header read timeout.
- Write timeout.
- Graceful shutdown.
- Request body size policy.
- Static file resolution.
- File descriptor/file handle lifetime.
- Byte-range file streaming.
- TLS, where enabled.

Python may choose request handling policy and construct response values, but Python should not be responsible for raw HTTP serialization, timeout enforcement, static file reopening, or socket lifecycle.

### Low-level primitives before convenience APIs

The first stable APIs should be boring and explicit: request target, method, headers, body policy, response plan, static root, resolved file, resolved directory, body source, server config, client config, and structured errors. Higher-level convenience should be built only after these pieces are stable.

### Strict defaults with explicit weakening

Safe defaults remain non-negotiable. Public bind, symlink following, dotfile serving, directory listing, large request bodies, redirect following, insecure TLS, and other weaker modes require explicit opt-in and must be visible in configuration and logs.

### One security policy across CLI, Rust, and Python

The CLI, Rust primitives, and Python primitives must share the same enforcement path. No Python-only implementation should approximate filesystem confinement or HTTP body validation independently if the Rust implementation exists.

## Target architecture

### Layer 1: static server CLI

The existing CLI remains the most constrained product surface. It serves static files from a root under safe defaults. It should continue to reject unsupported methods, request bodies for GET/HEAD, malformed request targets, path traversal, dotfiles, and symlink traversal under safe defaults.

This layer should consume the same primitive APIs exposed to external callers. If the CLI requires private shortcuts that cannot be reproduced by the public primitive layer, that is a sign the primitive layer is incomplete.

### Layer 2: Rust HTTP primitive library

The Rust primitive layer should expose stable, documented types for:

- `HttpMethod` or a constrained method validation API.
- `RequestTarget` / `ConfinedPath` parsing.
- `HeaderMapPlan` / structured header views.
- Request body metadata policy.
- Static response planning.
- Conditional request evaluation.
- Range request evaluation.
- Response body sources: empty, bytes, static file, static range.
- Safe static root and resolved resources.
- Server lifecycle configuration and policy summaries.
- Structured request/response/server/client errors.

The existing `primitives` facade is the right place to grow this, but the boundary must be deliberately reviewed and stabilized before 1.0.

### Layer 3: Python primitive bindings

The Python package should expose:

- Immutable policy objects.
- Request target parsing.
- Header and method validation.
- Static root resolution.
- Resolved file/directory capabilities.
- Safe streaming from resolver-opened file handles.
- Response plan objects.
- Response body objects backed by Rust.
- Server configuration and lifecycle control.
- Low-level Python dispatch hooks for downstream frameworks.
- Later, HTTP client primitives.

The Python API should be familiar to users of `http.server`/`http.client`, but not replicate unsafe defaults or insecure path handling.

### Layer 4: downstream integrations

Downstream projects may build:

- ASGI protocol adapters.
- WSGI protocol adapters.
- Microframeworks.
- Static/dynamic hybrid servers.
- Testing harnesses.
- HTTP clients.
- Local development servers.

The eggserve repo should provide documentation and examples showing where the extension points are, but not vendor these adapters.

## Roadmap tracks

### Track A: production contract and API boundary

Clarify the product contract, non-goals, public API stability model, threat model, and downstream extension contract. The immediate output should be docs and tests that prevent scope creep.

Detailed handoff: `plans/024-production-contract-api-boundary.md`.

### Track B: HTTP correctness primitive closure

Turn the current static response behavior into a formal HTTP/1.1 subset contract and expose reusable primitives for downstream servers. This includes method/body validation, header normalization, conditional requests, ranges, HEAD/GET parity, status/header contracts, and live integration tests.

Detailed handoff: `plans/025-http-correctness-primitives.md`.

### Track C: safe body and file streaming primitives

Expose body sources and streaming abstractions so Rust and Python callers can safely send bytes, empty bodies, full static files, and static ranges without reconstructing paths or implementing HTTP framing themselves.

Detailed handoff: `plans/026-safe-body-streaming-primitives.md`.

### Track D: Rust-owned Python server primitives

Expose a Python-facing server primitive that lets Python choose responses while Rust owns the accept loop, HTTP parser integration, concurrency, timeouts, backpressure, and static file streaming. This enables downstream ASGI/WSGI projects without implementing ASGI/WSGI here.

Detailed handoff: `plans/027-python-server-primitives.md`.

### Track E: HTTP client primitive substrate

Add low-level Rust-backed Python HTTP client primitives only after server/body primitives are clean. Keep this smaller than `requests` or `httpx`: request construction, timeout policy, TLS verification, response streaming, and structured errors.

Detailed handoff: `plans/028-http-client-primitives.md`.

### Later tracks

After the first five detailed plans land, follow-up plans should cover:

- Filesystem adversarial hardening expansion.
- Windows reparse-point production decision or implementation.
- Network exhaustion and slow-client integration tests.
- Python wheel matrix and installed-wheel tests.
- API freeze and semver review.
- Security review and fuzzing corpus expansion.
- Performance benchmarks against Python `http.server` and reference servers.

## Milestones

### Milestone A: primitive contract is explicit

The repo should have a precise contract for what eggserve provides and what downstream projects can build on top. Docs should explicitly say that ASGI/WSGI adapters are external consumers, not in-tree features.

### Milestone B: HTTP/1.1 static subset is formally tested

Every supported request/response case should have pure primitive tests and live HTTP integration tests. Unsupported behavior should fail safely and predictably.

### Milestone C: safe body streaming is exposed

Downstream callers, including Python, should be able to return a static file or file range without reopening a path and without manually serializing HTTP.

### Milestone D: Python can build a small dynamic server on eggserve

A Python user should be able to create a minimal server that returns static files, bytes/text responses, and health-check responses, while Rust owns I/O and concurrency. No ASGI/WSGI adapter should be included.

### Milestone E: Python can build a low-level client on eggserve

A Python user should be able to issue basic GET/HEAD requests with Rust-backed networking, TLS verification, timeouts, and response streaming.

## Acceptance criteria for the roadmap

- The CLI remains a hardened static server with safe defaults.
- No ASGI/WSGI code is added to the repository.
- Public primitive APIs are documented as downstream extension points.
- Python APIs do not require unsafe path reconstruction.
- Rust owns connection, timeout, concurrency, and file streaming mechanics.
- Tests verify policy preservation across CLI, Rust primitives, and Python primitives.
- Documentation gives downstream adapter authors enough information to build on eggserve without expanding eggserve scope.

## Validation commands

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python validation should include:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
```

The native primitive tests may require the wheel or `maturin develop` depending on local environment.
