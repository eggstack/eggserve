# Plan 020: Examples, documentation, and invariant tests

## Status

Planned. This is the closing handoff plan for the Python-accessible hardened HTTP primitives track.

## Objective

Turn the primitive API work from Plans 016-019 into an understandable and auditable developer surface. Add examples that demonstrate composition without turning eggserve into a framework, document the extension contract, and build an invariant-test matrix that proves Rust and Python APIs preserve the same security properties.

## Scope

In scope:

- Documentation for Rust primitives.
- Documentation for Python primitives.
- Small examples showing safe composition.
- Invariant tests across Rust and Python APIs.
- Release-readiness review for the primitive track.

Out of scope:

- In-tree ASGI adapter.
- In-tree WSGI adapter.
- Router framework.
- Middleware framework.
- Template engine.
- Reverse proxy.
- Full HTTP client.
- New compatibility mode unless separately planned.

## Design constraints

Examples must reinforce eggserve's scope. They should show how to use primitives to build small dynamic behavior or safer static/download handling, but they must not imply that eggserve is an application framework.

Documentation must be explicit about security invariants, weaker modes, and what downstream code remains responsible for.

## Documentation work

### 1. Update README positioning

Update `README.md` to describe two layers:

1. CLI static server: hardened replacement for `python -m http.server`.
2. Primitive library: hardened Rust/Python building blocks for request-target parsing, path confinement, secure static resolution, and static response planning.

Keep the non-goal statement prominent: eggserve is not ASGI/WSGI, not a web framework, not a reverse proxy, and not a template engine.

### 2. Add primitive API documentation

Add or update:

- `docs/public-api-boundary.md`
- `docs/secure-root.md`
- `docs/http-response-planning.md`
- `docs/python-api.md`
- `docs/extension-contract.md`

`docs/extension-contract.md` should explain how downstream projects may build on eggserve:

- Dynamic sites may use `SecureRoot` for assets and downloads.
- Test servers may use request validation and response planning.
- Out-of-tree ASGI/WSGI adapters may map framework requests into eggserve primitives.
- Downstream code must not claim descriptor-relative hardening if it extracts paths and reopens them manually.
- Downstream code must preserve safe defaults unless it clearly exposes explicit opt-ins.

### 3. Add API reference examples

Add short documented snippets for:

- Rust request-target parsing.
- Rust secure root resolution.
- Rust response planning.
- Python request-target parsing.
- Python secure root resolution.
- Python static response planning.
- Python subprocess lifecycle server.

Avoid large example frameworks.

## Example programs

### 1. Python dynamic health plus static assets example

Add `examples/python_dynamic_static.py`.

Purpose:

- demonstrate a tiny dynamic endpoint such as `/health`;
- delegate `/static/...` to `SecureRoot`;
- map `StaticResponsePlan` into a minimal Python standard-library server or a small illustrative response function;
- show that request paths are never converted into raw filesystem paths for serving.

Constraints:

- Keep code small.
- No ASGI/WSGI imports.
- No routing framework.
- Include comments explaining what eggserve handles and what the example handles.

### 2. Python safe download handler example

Add `examples/python_safe_download.py`.

Purpose:

- show how user-provided download names can be resolved through `SecureRoot`;
- distinguish not-found from denied;
- avoid raw path joins;
- produce response metadata through the planner.

### 3. Rust primitive embedding example

Add `examples/rust_primitives.rs` or a crate example under `crates/eggserve-core/examples/`.

Purpose:

- parse request target;
- resolve under `SecureRoot`;
- plan a GET or HEAD response;
- print status/headers/body plan;
- avoid Hyper server setup unless already trivial.

### 4. Optional CLI comparison example

Add a docs snippet comparing:

```sh
python -m http.server
```

with:

```sh
eggserve
```

Keep this comparison factual: safe defaults, loopback bind, symlink behavior, dotfile behavior, directory listing behavior, request-body policy, and resource limits.

## Invariant test matrix

Add `docs/invariants.md` or include the table in `docs/security-review.md`.

Required invariant categories:

### Request target invariants

- Only origin-form request targets accepted.
- Absolute-form rejected.
- Authority-form rejected.
- Asterisk-form rejected.
- Malformed percent encoding rejected.
- Percent-encoded traversal rejected.
- NUL rejected.
- Backslash rejected by default.
- Windows drive prefixes rejected.
- Windows ADS syntax rejected.
- Windows reserved names rejected.

### Policy invariants

- Dotfiles denied by default.
- Dotfiles allowed only through explicit policy.
- Directory listing disabled by default.
- Directory listing enabled only through explicit policy.
- Symlinks denied by default.
- Follow-symlinks mode explicit and documented weaker.

### Filesystem invariants

- Normal file resolves.
- Normal directory resolves.
- Missing path is not found.
- Final symlink denied under safe defaults on Unix.
- Intermediate symlink denied under safe defaults on Unix.
- Symlink swap test or equivalent no-follow kernel behavior test remains present on Unix.
- Follow-symlinks internal target allowed if policy permits.
- Follow-symlinks outside-root target denied.
- Directory listings hide dotfiles and symlinks under safe defaults.
- File serving path does not reopen by absolute path under Unix safe defaults.

### HTTP validation invariants

- Only GET/HEAD accepted for static serving.
- Other methods map to 405-equivalent result.
- Positive `Content-Length` rejected under zero-body policy.
- Invalid `Content-Length` rejected.
- `Transfer-Encoding` rejected for GET/HEAD.
- Conflicting `Content-Length` and `Transfer-Encoding` rejected.

### Response planning invariants

- GET file plan includes status, content length, content type, validators, and nosniff.
- HEAD file plan has matching headers and empty body.
- Matching ETag conditional returns 304.
- Matching Last-Modified conditional returns 304 when appropriate.
- Satisfiable range returns 206 with correct content range.
- Unsatisfiable range returns 416 with correct content range.
- Directory listing HTML escapes visible names.
- Directory listing hrefs percent-encode path segments.
- Directory listing response includes CSP and referrer policy.

### Python binding invariants

- Python behavior mirrors Rust behavior for all above categories where platform permits.
- Python cannot directly construct `ResolvedFile` or `ResolvedDirectory` from arbitrary paths.
- Python exceptions expose stable machine-readable codes.
- Python response plans expose plain status/header/body-plan values, not Hyper internals.

## Test implementation strategy

Add Rust integration tests for public primitives if not already done in Plans 016-018.

Add Python tests:

- `eggserve.test_primitives_path`
- `eggserve.test_primitives_root`
- `eggserve.test_primitives_response`
- or one `eggserve.test_primitives` module if smaller.

Where feasible, use table-driven cases so Rust and Python test data can be kept visually aligned.

For platform-specific behavior:

- gate Unix symlink tests appropriately;
- include explicit skip messages in Python tests on non-Unix;
- do not claim Windows reparse-point hardening until implemented.

## Release-readiness review

After examples and tests land, perform a final track review:

- Are all new public APIs documented?
- Are all public constructors safe?
- Are capability objects impossible to forge from Python?
- Does any example use raw path joins for request-derived paths?
- Are weaker modes documented?
- Does the CLI still behave as before?
- Do Python subprocess APIs still work?
- Does the README avoid framework claims?
- Does packaging include both binary and native extension if required?
- Are feature flags and workspace membership coherent?

## Validation

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
```

After native primitive tests land, also run:

```sh
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_primitives -v
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import ServeConfig, ServerProcess, serve_directory
print(ServeConfig())
try:
    from eggserve import SecureRoot, RequestTarget, StaticPolicy
    print(SecureRoot, RequestTarget, StaticPolicy)
except ImportError as exc:
    raise SystemExit(f"native primitive import failed: {exc}")
PY
```

If available:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This plan is complete when:

- README and docs explain the CLI layer and primitive layer accurately.
- Examples demonstrate safe composition without framework scope creep.
- Rust and Python invariant tests cover request target parsing, policy, resolution, HTTP validation, response planning, and binding safety.
- Existing subprocess API and CLI behavior remain intact.
- The project has a clear extension contract for out-of-tree dynamic frameworks and ASGI/WSGI adapters.
