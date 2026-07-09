# Plan 024: Production Contract and API Boundary

## Purpose

Define the production contract for eggserve's next stage: a hardened, correct HTTP/static-serving primitive library with Python bindings that downstream projects can use to build application servers, clients, and adapters. This plan is deliberately documentation- and boundary-first. It prevents scope drift before implementation begins.

The key rule is that eggserve should expose primitives that make ASGI/WSGI/app-server projects possible, but eggserve should not implement ASGI, WSGI, routing, middleware, or framework behavior in this repository.

## Current context

The repo already describes eggserve as a hardened static file server and primitive library. It also already documents many non-goals and safe defaults. The next stage needs to refine that into an extension contract suitable for downstream consumers.

Current useful anchors:

- `README.md` defines CLI static server plus primitive library.
- `docs/non-goals.md` defines broad exclusions.
- `docs/security-policy.md` defines safe defaults and weaker options.
- `docs/threat-model.md` exists and should be updated for downstream primitive consumers.
- `docs/extension-contract.md` exists and should become the central downstream-consumer contract.
- `architecture/primitives-api.md` describes the current primitive boundary.
- `crates/eggserve-core/src/lib.rs` classifies stable-ish, experimental, and internal APIs.
- `crates/eggserve-core/src/primitives/` is the intended Rust public facade.
- `docs/python-api.md` describes the current Python primitive and subprocess APIs.

## Goals

- Make the production target precise.
- Define which APIs are intended for downstream framework/server/client authors.
- Make it explicit that ASGI/WSGI are downstream use cases, not in-tree deliverables.
- Separate stable, provisional, and internal APIs in docs.
- Define the invariant policy for all public primitives.
- Ensure Python primitives preserve the same security posture as Rust primitives and CLI.
- Add tests or doc checks where feasible so public claims do not silently drift.

## Non-goals

- Do not implement new runtime behavior in this pass unless needed for doc/test consistency.
- Do not add ASGI/WSGI adapters.
- Do not add routing, middleware, templates, cookies, sessions, auth, proxying, or compression.
- Do not freeze the entire current public surface prematurely.
- Do not promise HTTP/2, HTTP/3, WebSocket, CONNECT, proxy, or application server semantics.

## Required documentation changes

### 1. Update `docs/extension-contract.md`

Make this document the authoritative contract for downstream users. It should include:

- What eggserve guarantees.
- What eggserve intentionally does not implement.
- How downstream projects should consume the Rust primitives.
- How downstream projects should consume the Python primitives.
- Which primitives are safe to build on.
- Which modules are internal and must not be depended on.
- How policy preservation works across CLI/Rust/Python.
- The capability rule: use resolved resources and body sources, not reconstructed paths.
- The concurrency rule: Rust owns sockets, timeouts, and file streaming for Python server APIs.
- The adapter rule: ASGI/WSGI adapters should live downstream.

Include a section titled `Downstream adapter boundary` with this normative language:

> eggserve may expose primitives sufficient for an external ASGI, WSGI, or framework adapter. eggserve does not provide those adapters in-tree. Any new API added for adapter authors must remain protocol- and framework-neutral.

### 2. Update `docs/non-goals.md`

Ensure the following are explicitly listed as non-goals:

- In-tree ASGI adapter.
- In-tree WSGI adapter.
- Framework routing.
- Middleware stack.
- Template engine.
- Session/cookie/auth framework.
- Reverse proxy.
- Generic plugin host.
- Dynamic Python code execution inside the static server path.

Add a clarifying paragraph:

> These are non-goals for this repository, not forbidden downstream uses. The primitive API should be strong enough for separate projects to build them externally.

### 3. Update `docs/threat-model.md`

Extend the threat model beyond the CLI to cover primitive consumers.

Add sections for:

- Rust embedding consumers.
- Python primitive consumers.
- Python server callback consumers.
- Downstream adapter authors.
- Unsafe path reconstruction risk.
- Request-body policy risk.
- Header spoofing/normalization risk.
- Response serialization risk.
- Callback-induced latency and backpressure risk.
- Trust boundary between Rust runtime and Python user code.

The threat model should explicitly state that Python callbacks may be untrusted from a latency/resource perspective but are not sandboxed. Rust should enforce connection and I/O policy around them, but eggserve does not make Python application code safe.

### 4. Update `architecture/primitives-api.md`

Document the desired primitive groups:

- Request primitives.
- Header primitives.
- Body policy primitives.
- Response planning primitives.
- Body source/streaming primitives.
- Static filesystem primitives.
- Server lifecycle primitives.
- Client primitives, future.

For each group, mark status as one of:

- Implemented and stable-ish.
- Implemented but provisional.
- Missing, planned.
- Internal only.

### 5. Update `docs/python-api.md`

Add a section titled `Adapter-building posture`.

It should say:

- Python primitives are intended to allow downstream projects to build app servers and adapters.
- eggserve does not implement ASGI/WSGI.
- The safe design is to let Rust own socket I/O and let Python return explicit response values.
- Reopening paths in Python is outside the security guarantee.
- Response/body streaming primitives are planned before Python dynamic-server use should be considered production-grade.

## Required source-level review

Review these files and ensure comments/docs match the new contract:

- `crates/eggserve-core/src/lib.rs`
- `crates/eggserve-core/src/primitives/mod.rs`
- `crates/eggserve-core/src/primitives/secure_root.rs`
- `crates/eggserve-core/src/primitives/http.rs`
- `crates/eggserve-core/src/primitives/planner.rs`
- `crates/eggserve-core/src/primitives/response.rs`
- `crates/eggserve-python/src/lib.rs`
- `crates/eggserve-python/python/eggserve/__init__.py`
- `crates/eggserve-python/python/eggserve/server.py`

Do not expose new types during this pass unless documentation reveals a naming conflict or a public API accidental export.

## Public API classification task

Create or update a table in `architecture/primitives-api.md` with columns:

- API item.
- Language: Rust, Python, or both.
- Status: stable-ish, provisional, internal, planned.
- Security invariant.
- Downstream use case.

Minimum rows:

- `StaticPolicy`.
- `PathPolicy`.
- `ConfinedPath` / `RequestTarget`.
- `SecureRoot`.
- `ResolvedResource`.
- `ResolvedFile`.
- `ResolvedDirectory`.
- `ResponsePlan` / `StaticResponsePlan`.
- `HeaderMapPlan`.
- `validate_method`.
- `validate_request_body`.
- `validate_request_target`.
- Planned `BodySource`.
- Planned Python server primitive.
- Planned HTTP client primitive.

## Invariant checklist

The docs should assert these invariants:

- Safe defaults are shared across CLI, Rust primitives, and Python primitives.
- Path parsing rejects traversal, NUL, ambiguous separators, Windows prefixes, reserved device names, and ADS syntax according to current policy.
- Static filesystem resolution must not serve outside root.
- Under Unix safe defaults, symlink denial is descriptor-relative.
- `--follow-symlinks` is weaker and outside the descriptor-relative guarantee.
- Python consumers must not reconstruct and reopen paths for static serving.
- Future Python server APIs must keep socket I/O, timeout enforcement, and file streaming in Rust.
- Future client APIs must verify TLS by default.
- Unsupported behavior fails closed or is explicitly out of contract.

## Tests and checks

This pass is primarily documentation, but add lightweight tests if they are low-noise:

- A doc-link or markdown lint is not required unless already present.
- Add Rust doc tests only if examples are stable and compile under current APIs.
- Add Python docs examples only if they already work.
- Do not add broad tooling dependencies for doc validation.

## Acceptance criteria

- `docs/extension-contract.md` clearly explains how downstream ASGI/WSGI/app-server/client projects can build on eggserve without adding those adapters in-tree.
- `docs/non-goals.md` distinguishes repository non-goals from downstream use cases.
- `docs/threat-model.md` covers primitive consumers, not only CLI users.
- `architecture/primitives-api.md` classifies public, provisional, internal, and planned APIs.
- `docs/python-api.md` warns against unsafe path reconstruction and explains the adapter-building posture.
- No new framework or adapter code is added.
- No new broad dependency is added.
- All current validation commands pass.

## Validation commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python smoke:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_server -v
```

Run native primitive tests if the local environment has a built extension:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
```

## Handoff notes

Keep the language normative and boring. The purpose of this plan is to prevent future contributors from interpreting "HTTP primitives" as permission to build an in-tree framework. Downstream enablement is the product; in-tree adapter ownership is not.
