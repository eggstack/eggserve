# Plan 015: Python-accessible hardened HTTP primitives roadmap

## Status

Planned. This is a roadmap track for turning eggserve from a hardened static-serving CLI plus alpha Rust core into a composable, audited HTTP/static-serving substrate that is directly useful from both Rust and Python.

## Context

eggserve already has a strong narrow core: a safe-default static file server, loopback binding by default, explicit public exposure acknowledgement, no dotfiles by default, no directory listing by default, no symlink following by default, descriptor-relative Unix traversal under safe defaults, pre-opened file handles, resource limits, and a small Python subprocess API.

That makes eggserve a practical replacement for `python -m http.server` as a command-line static server. The next step is to make it a better library substrate than Python's `http.server` without turning eggserve into a web framework, ASGI runtime, WSGI runtime, reverse proxy, or dynamic application server.

Python's standard `http.server` is useful because users can compose request parsing, handlers, response writing, and file-serving helpers. It is often avoided for serious use because those extension points are not paired with strong path confinement, symlink policy, request-target validation, response correctness, or a modern security posture. eggserve should keep the composability but invert the safety model: expose typed primitives whose construction enforces audited invariants.

## Goal

Make eggserve a hardened replacement for Python's `http.server` at two layers:

1. A safe CLI for serving a directory.
2. A composable primitive library that Rust and Python code can use to build dynamic sites, test servers, asset handlers, download handlers, and out-of-tree ASGI/WSGI adapters without reimplementing path confinement, symlink policy, request-target parsing, static response metadata, or HTTP conditional/range semantics.

The project must remain below framework scope. It should expose safe primitives and reference examples, not routing stacks, middleware systems, template engines, application lifecycle systems, or in-tree ASGI/WSGI adapters.

## Non-goals

This track must not introduce:

- An ASGI adapter in this repository.
- A WSGI adapter in this repository.
- A Python request callback server as the first deliverable.
- A routing framework.
- Middleware abstractions.
- Template rendering.
- Dynamic code execution in the static-serving path.
- A reverse proxy.
- A full HTTP client stack.
- Compatibility behavior that weakens root confinement or symlink escape prevention.

It is acceptable and intended for third parties or future sibling projects to build ASGI/WSGI adapters or dynamic frameworks on top of the primitives once the primitive contract is stable.

## Design principle: capability-oriented primitives

The public API should expose decisions and capabilities, not mutable internals. Callers should not be encouraged to validate a path and then reopen the returned absolute path manually. Instead, they should receive objects whose construction proves a property:

- `RequestTarget` proves request-target syntax was parsed and accepted.
- `ConfinedPath` proves normalized path components were checked for traversal and platform ambiguity.
- `SecureRoot` proves a root was established under a filesystem policy.
- `ResolvedFile` proves a file was resolved under the root using the configured policy and, on Unix safe defaults, opened through descriptor-relative no-follow traversal.
- `ResolvedDirectory` proves a directory was resolved under the root and can be listed or used for child lookup under policy.
- `StaticResponsePlan` proves HTTP response metadata was derived from a resolved resource and request headers according to central eggserve logic.

Diagnostic paths may exist, but serving should not require them. If an absolute path is exposed, it must be documented as diagnostic only and not as the recommended I/O path.

## Track milestones

### Milestone A: Public API boundary and crate reshaping

Define the stable and experimental public primitive surface before exposing more internals. The current `eggserve-core` internals are useful but should not be made public wholesale.

Deliverables:

- `docs/public-api-boundary.md` describing public, experimental, and internal surfaces.
- Rust public wrapper types for the primitive API.
- Clear naming that avoids leaking implementation detail.
- Explicit invariants for every public type.
- A migration note explaining how this differs from the current alpha `service::handle_request` exposure.

### Milestone B: Path and policy primitive stabilization

Make request-target and path-policy behavior reusable without filesystem access.

Deliverables:

- Public request-target parser.
- Public path policy and static policy types.
- Public confined path object.
- Structured parse/validation errors.
- Tests for traversal, encoding, NUL, dotfile, backslash, Windows prefix, ADS, and reserved-name behavior.
- Python binding plan for these types.

### Milestone C: Secure root and resolution capabilities

Expose secure filesystem resolution as a capability-oriented API.

Deliverables:

- `SecureRoot` public wrapper.
- `ResolvedResource` public enum or equivalent typed result.
- `ResolvedFile` and `ResolvedDirectory` capability wrappers.
- Directory listing through resolver policy, not direct filesystem walking.
- Unix safe-default fd-relative traversal preserved behind the public wrapper.
- Non-Unix and follow-symlinks caveats documented without weakening default invariants.

### Milestone D: HTTP validation and response planning

Centralize request validation and static response metadata construction so dynamic users do not need to reimplement edge cases.

Deliverables:

- Method/body/framing validation primitives.
- Response metadata builder for files and directory listings.
- Conditional request support: `If-None-Match`, `If-Modified-Since`, and correct `304` behavior.
- Range request support: `Range`, `If-Range`, `206`, `416`, and HEAD parity.
- Tests against malformed and edge-case headers.

### Milestone E: Native Python primitive bindings

Extend the Python package beyond subprocess lifecycle management by adding native bindings for the primitive layer.

Deliverables:

- PyO3 module exposing safe primitive objects.
- Python objects with no unsafe direct constructors for capability types.
- Python exceptions mirroring structured Rust errors.
- Python docs and examples for primitive usage.
- Python tests mirroring Rust invariant tests.

### Milestone F: Examples, docs, and audit harness

Show how to compose the primitives without implying that eggserve is a framework.

Deliverables:

- Small Python dynamic example serving `/health` dynamically and delegating `/static/...` safely.
- Rust embedding example using primitive response planning.
- Python download-handler example using `SecureRoot` rather than raw paths.
- Security invariant test matrix.
- Documentation of out-of-tree adapter expectations.

## Proposed detailed plan files

This roadmap is split into these detailed handoff plans:

- `016_public_api_boundary_and_core_primitives.md`
- `017_secure_root_and_resolution_capabilities.md`
- `018_http_validation_and_response_planning.md`
- `019_python_native_bindings_for_primitives.md`
- `020_examples_docs_and_invariant_tests.md`

## API shape sketch

The exact names should be finalized during Plan 016, but the intended Python usage should look roughly like this:

```python
from eggserve import PathPolicy, RequestTarget, SecureRoot, StaticPolicy

root = SecureRoot("public", policy=StaticPolicy())
target = RequestTarget.parse("/assets/app.css", policy=PathPolicy())
resource = root.resolve(target)

if resource.is_file:
    plan = resource.file.plan_response(method="GET", headers=request_headers)
    # The caller can map the plan to its chosen server implementation.
```

The important property is that `resource.file` is not constructed from an arbitrary Python path. It is created only by resolving a validated request target under a secure root.

## Stability model

During this track, use three stability labels:

- Stable-ish: policy value objects and parse result types once tests are complete.
- Experimental: response planning and Python native bindings until HTTP semantics and binding ergonomics settle.
- Internal: raw filesystem traversal, Hyper body types, concrete `openat` implementation details, low-level response body streaming, and platform-specific resolver internals.

Before a 1.0 API freeze, every exposed primitive must have an invariant statement and tests demonstrating that the Python binding cannot weaken the Rust-side contract.

## Validation requirements

Each implementation phase should run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
```

After native bindings are introduced, add the appropriate maturin build and Python binding tests to the required sequence.

## Risk register

The main risks are scope creep, unsafe constructor leakage, premature API freezing, Python binding drift from Rust invariants, and HTTP correctness bugs in conditional/range support.

Mitigations:

- Keep ASGI/WSGI/framework work out of tree.
- Expose capability wrappers, not internals.
- Require invariant tests for every public primitive.
- Mirror Rust tests in Python.
- Keep response planning pure and testable before connecting it to streaming.
- Document weaker modes explicitly: follow-symlinks and Windows reparse-point handling must not be described as equivalent to Unix safe defaults.

## Completion criteria for the track

This track is complete when a Python or Rust user can safely:

1. Parse and validate a request target.
2. Resolve it under a secure static root without raw path reopening.
3. Inspect structured allowed/denied/not-found outcomes.
4. Generate static response metadata with conditional and range semantics.
5. Use the primitives in a small dynamic example without relying on ASGI/WSGI or a framework abstraction.
6. Trust that the same security invariants are tested through Rust and Python APIs.
