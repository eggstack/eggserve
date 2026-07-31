# Plan 098 — TLS, API Scope, and Compatibility Closure

## Goal

Complete the bounded `http.server` compatibility workstream by:

1. adding `HTTPSServer` and `ThreadingHTTPSServer` as thin Python facades over EggServe's existing Rust TLS server path;
2. establishing one clear supported Python API centered on `eggserve.server`;
3. demoting or removing overlapping bespoke and experimental surfaces from the default package;
4. reconciling documentation, examples, tests, and package metadata around the final narrow contract;
5. proving closure through the existing verification and CI paths without adding infrastructure.

This is a closure and simplification phase. It must reduce conceptual surface area.

## Final supported product contract

After this plan, the primary Python API is:

```python
from eggserve.server import (
    HTTPServer,
    ThreadingHTTPServer,
    HTTPSServer,
    ThreadingHTTPSServer,
    BaseHTTPRequestHandler,
    SimpleHTTPRequestHandler,
)
```

The package may retain a small convenience API:

```python
from eggserve import serve_directory
```

Low-level primitives that remain intentionally supported are available through an explicitly advanced namespace:

```python
from eggserve.lowlevel import SecureRoot, StaticPolicy, RequestTarget
```

The default package must no longer present the bespoke native callback `Server` type or the experimental HTTP client as peer primary APIs.

## Scope firewall

This plan must not add:

- ACME;
- certificate generation;
- certificate reload/watch services;
- SNI virtual hosting;
- multiple certificates;
- HTTP/2 or HTTP/3;
- client certificate authentication;
- a general TLS configuration framework;
- ASGI/WSGI;
- routing or middleware;
- authentication/session/cookie frameworks;
- reverse proxying;
- HTTP client features;
- a deprecation framework with registries or generated warnings;
- a new packaging backend;
- a new required platform matrix;
- a new CI job or workflow;
- automatic PyPI or crates.io publication;
- Python-version expansion work beyond correcting metadata that is already inaccurate.

Broadening Python version support is valuable but is not part of this workstream. Keep the currently supported interpreter range unless a compatibility implementation change makes the existing declaration false. Do not add a multi-version matrix here.

## Required file inspection

Before editing, inspect at least:

- completed Plans 095–097 implementation
- `crates/eggserve-bin/src/tls.rs`
- `crates/eggserve-bin/src/main.rs`
- `crates/eggserve-core/src/server/`
- Rust TLS feature definitions in workspace Cargo manifests
- `crates/eggserve-python/Cargo.toml`
- `crates/eggserve-python/pyproject.toml`
- `crates/eggserve-python/src/lib.rs`
- `crates/eggserve-python/src/server.rs`
- `crates/eggserve-python/src/client.rs`
- `crates/eggserve-python/python/eggserve/__init__.py`
- `crates/eggserve-python/python/eggserve/server.py`
- proposed or implemented `eggserve.lowlevel` module
- current subprocess helper modules
- all Python tests and package smoke tests
- README and active docs listed below
- `.github/workflows/ci.yml`
- `scripts/test-python-wheel.sh`
- `scripts/verify.sh`

Inventory the public surface before changes:

```sh
python - <<'PY'
import eggserve
print(sorted(eggserve.__all__))
PY

rg -n "HttpClient|ClientConfig|ClientRequest|ClientResponse|ClientMethod" crates/eggserve-python docs README.md examples
rg -n "from eggserve import Server|eggserve\.Server|ServerSecureRoot|StaticResponder|StaticPolicyWrapper" crates/eggserve-python docs README.md examples
rg -n "ServeConfig|ServerProcess|serve_directory" crates/eggserve-python docs README.md examples
rg -n "HTTPSServer|ThreadingHTTPSServer|tls_cert|tls_key|certfile|keyfile" crates docs
```

Create a temporary public-name disposition table during implementation:

```text
name
current module
current status
used by final server facade
final module
final disposition
retained test
retained documentation
```

Do not create a permanent API registry.

## Track A — Expose existing Rust TLS to the native Python server

### Objective

Add the smallest native configuration path needed for the Python server facade to start the same Rust server runtime with TLS.

### Reuse requirement

The Python path must reuse the existing Rust TLS implementation and certificate/key loading rules. Do not copy certificate parsing logic into Python and do not shell out to the CLI.

If TLS setup currently lives only in the binary crate, move only the reusable certificate/key loading and acceptor construction logic needed by both the CLI and Python runtime into `eggserve-core` behind the existing TLS feature or a narrowly named shared module.

Do not make `eggserve-core` depend on the binary crate.

### Native configuration

Add internal native server inputs equivalent to:

- certificate chain path;
- private key path;
- optional password argument only if the current Rust TLS stack already supports encrypted keys safely;
- ALPN list limited to `http/1.1` for this product.

Prefer path inputs over accepting arbitrary Python SSL contexts. EggServe uses rustls, not CPython's `ssl.SSLContext` implementation.

The native server should validate TLS configuration before reporting readiness:

- both certificate and key required;
- files readable;
- certificate chain nonempty;
- supported private key parsed;
- no plaintext fallback when TLS configuration fails;
- startup error returned as a clear Python exception;
- key material never logged.

### Feature configuration

The Python wheel must enable only the server TLS feature required by these classes.

Review current features carefully. The Python crate currently enables experimental client functionality. Remove that feature coupling under Track D rather than adding a second broad feature set.

Do not enable HTTP/2.

### Tests

At Rust/native level:

- valid local test certificate starts TLS server;
- missing cert or key fails;
- invalid cert fails;
- invalid key fails;
- plaintext request to TLS port fails safely;
- HTTPS GET succeeds;
- shutdown works over TLS;
- no key material in error/log output.

Reuse existing test certificate fixtures and TLS tests. Do not add certificate-generation dependencies if fixtures already exist.

## Track B — `HTTPSServer` and `ThreadingHTTPSServer`

### Constructors

Support the Python 3.14-shaped constructor:

```python
HTTPSServer(
    server_address,
    RequestHandlerClass,
    bind_and_activate=True,
    *,
    certfile,
    keyfile=None,
    password=None,
    alpn_protocols=None,
)
```

`ThreadingHTTPSServer` has the same TLS parameters plus the same bounded concurrency extension used by `ThreadingHTTPServer`.

### Required semantics

- `certfile` is required;
- `keyfile=None` may mean the key is in the same PEM file only if the existing rustls loader supports this cleanly;
- unsupported encrypted-key passwords raise a clear error rather than being ignored;
- `alpn_protocols=None` defaults to `['http/1.1']`;
- any requested protocol other than `http/1.1` is rejected because HTTP/2 is out of scope;
- server lifecycle and handler behavior are inherited from the plaintext classes;
- `server_address`, `server_name`, and `server_port` describe the actual bound listener;
- handler request metadata reports `https` scheme;
- file streaming and static policy behavior remain identical under TLS;
- TLS startup failures occur before `serve_forever()` claims readiness.

### Compatibility boundary

Do not accept a CPython `SSLContext` or expose a raw wrapped socket.

Document that:

- TLS is provided by rustls;
- only HTTP/1.1 ALPN is supported;
- no SNI multi-certificate selection;
- no client certificates;
- no automatic certificate management;
- `password` support follows actual key-loader capabilities and may be unsupported.

### Tests

Installed-wheel tests must cover:

- imports;
- HTTPS custom BaseHTTPRequestHandler response;
- HTTPS SimpleHTTPRequestHandler full file and HEAD;
- range response over TLS;
- invalid certificate startup;
- invalid key startup;
- missing key behavior;
- unsupported ALPN rejection;
- scheme metadata is `https`;
- ThreadingHTTPSServer bounded concurrent handlers;
- shutdown and context manager behavior.

Do not create a separate TLS test matrix; add focused cases to existing TLS/native and compatibility modules.

## Track C — Establish the final module layout

### `eggserve.server`

This is the canonical user-facing server module.

Its explicit `__all__` should include only:

- `HTTPServer`
- `ThreadingHTTPServer`
- `HTTPSServer`
- `ThreadingHTTPSServer`
- `BaseHTTPRequestHandler`
- `SimpleHTTPRequestHandler`

Subprocess helpers may live elsewhere.

Avoid exporting internal FFI classes, body tokens, response staging objects, or native callback adapters.

### `eggserve.lowlevel`

Create one advanced namespace for the small set of low-level primitives that remain useful and coherent.

Candidate retained types:

- `SecureRoot`
- one `StaticPolicy` type
- `PathPolicy`
- `RequestTarget`
- `ResolvedResource`
- `ResolvedFile`
- `ResolvedDirectory`
- `BodySource` only if it remains necessary for external safe embedding and has a complete one-shot contract
- canonical `Method`, `HttpVersion`, `HeaderBlock`, and response types only if they remain genuinely supported rather than historical artifacts.

The implementation agent must reduce duplication:

- one public static policy name;
- one documented method type per use case;
- one documented header block;
- no parallel `ServerSecureRoot` and `SecureRoot` public concepts;
- no parallel `StaticPolicy` and `StaticPolicyWrapper` public concepts;
- no two public body-source types unless an unavoidable transport ownership distinction is clearly documented.

Prefer Python wrappers or aliases over changing stable Rust names unnecessarily. The goal is a coherent Python namespace, not a broad Rust refactor.

### Top-level `eggserve`

Keep the top level small.

Recommended explicit exports:

- `__version__`
- `serve_directory`
- the six primary server/handler classes, if convenient re-exports improve discoverability without ambiguity.

The canonical import in docs remains `eggserve.server`.

Do not re-export the complete low-level namespace at top level.

### Subprocess convenience

Move or retain subprocess helpers under a clear module such as:

```python
from eggserve.subprocess import ServeConfig, ServerProcess, serve_directory
```

Keep top-level `serve_directory` as a simple convenience if desired.

Do not maintain two separate `StaticPolicy` dataclasses with the same public name. Subprocess configuration should use the same public policy object where practical or a clearly named subprocess config field.

Avoid a large deprecation layer. Because the package is still alpha, direct cleanup is preferable to years of aliases. Retain a compatibility alias only when it is trivial, unambiguous, and does not preserve the old architecture as a second recommended API.

## Track D — Remove default experimental client exposure

### Objective

Prevent EggServe's default Python package from presenting a partial HTTP client as part of the static-server product.

### Required implementation

1. Remove these names from top-level exports:
   - `HttpClient`
   - `ClientConfig`
   - `ClientRequest`
   - `ClientResponse`
   - `ClientError`
   - `ClientMethod`
2. Stop compiling the Python extension with the core `client` feature unless another final server component genuinely requires it.
3. Remove or disable the PyO3 client module from the default extension build.
4. Remove client examples and active API documentation from the default package docs.
5. Remove Python client tests from the required wheel suite once the surface is no longer shipped.
6. Do not expand or redesign the Rust experimental client in this plan.

### Rust client disposition

The existing Rust experimental client may remain in `eggserve-core` behind an opt-in feature if removing it would broaden this closure pass. It must not be described as part of the main EggServe product or enabled by the Python wheel.

If it is completely unused and deletion is small, deletion is permitted, but not required. Do not turn client removal into a separate architectural rewrite.

### Acceptance

The installed default wheel must not expose client names or pull in client-only dependencies/features.

## Track E — Demote the bespoke native callback server

### Objective

Ensure users see one primary server programming model.

The native PyO3 `Server` implementation may remain as an internal engine used by `HTTPServer`, but it must not be a top-level supported peer API.

Required disposition:

- import internally as `_NativeServer` or equivalent;
- remove `Server` from top-level `eggserve.__all__`;
- remove `ServerSecureRoot`, `StaticResponder`, `StaticPolicyWrapper`, and `ServerBodySource` from the primary namespace unless retained under `eggserve.lowlevel` with a specific advanced contract;
- update examples to use handler classes;
- remove active documentation recommending `Server(handler=...)`;
- remove API stability snapshots that require obsolete names;
- retain focused native tests only for behavior still required by the compatibility facade.

Do not delete the internal engine if doing so would force a rewrite of the Rust/Python bridge. Internal reuse is the intended outcome.

### Callback semantics

The handler-class facade still uses the callback engine internally. Ensure:

- duplicate headers remain preserved;
- file body tokens from SimpleHTTPRequestHandler remain supported;
- base handler byte responses remain bounded;
- handler timeout behavior remains documented;
- internal types are not discoverable as ordinary supported imports beyond `_native` implementation details.

## Track F — Test-suite consolidation

### Objective

End with a smaller, contract-focused test suite rather than retaining every historical surface test plus new compatibility tests.

### Required retained Python test groups

1. Package/import smoke tests.
2. Base `http.server` compatibility tests from Plan 096.
3. `SimpleHTTPRequestHandler` compatibility tests from Plan 097.
4. Focused TLS compatibility tests from this plan.
5. Low-level primitive tests only for names retained in `eggserve.lowlevel`.
6. A small subprocess convenience test if `ServerProcess` remains supported.
7. Representative Python boundary-hardening tests not already fully proven at Rust wire level.

### Required retained Rust test groups

- planner and canonical response semantics;
- raw wire correctness;
- filesystem confinement and race-oriented tests;
- server lifecycle and limits;
- static streaming and permit lifetime;
- TLS behavior;
- public Rust API tests for the actual retained Rust contract.

### Candidates for removal or consolidation

- Python client tests after client export removal;
- tests whose only purpose is preserving top-level imports for demoted native names;
- duplicate tests for both `StaticPolicy` and `StaticPolicyWrapper`;
- duplicate tests for both `SecureRoot` and `ServerSecureRoot` where one is internal;
- bespoke `Server(handler=...)` documentation tests replaced by handler facade tests;
- repeated canonical cases already exhaustively covered in Rust and not needed to validate a Python binding;
- historical profile/evidence tests not part of current CI policy;
- API snapshot tests that assert a large namespace instead of an explicit small supported list.

### Reduction rule

For every deleted test, identify one of:

- covered by retained Rust invariant test;
- covered through final public Python API;
- tests a removed API;
- duplicates another deterministic test;
- tests historical infrastructure no longer in force.

Do not remove unique filesystem, wire, lifecycle, packaging, or TLS safety coverage merely to reduce a count.

### No new infrastructure

- keep `unittest`;
- keep the installed-wheel harness;
- keep current Rust test commands;
- no pytest, tox, nox, or custom compatibility runner;
- no test-count gate;
- no generated disposition registry;
- no new CI job.

## Track G — Documentation and examples reconciliation

Review and update active documents:

- `README.md`
- `docs/python-api.md`
- `docs/python-http-server-compatibility.md`
- `docs/compatibility.md`
- `docs/http-primitives.md`
- `docs/security-policy.md`
- `docs/security-review.md`
- `docs/non-goals.md`
- `docs/tls.md`
- `docs/deployment.md`
- `docs/library-capability-matrix.md`
- `architecture/eggserve-python.md`
- `architecture/overview.md`
- `AGENTS.md`
- `.opencode/skills/eggserve-dev/SKILL.md`
- examples under `examples/`

### Required active documentation state

The README should have one primary Python example and one CLI example.

Python docs must state:

- canonical `eggserve.server` imports;
- standard-library compatibility boundary;
- safe-default divergences;
- bounded handler bodies;
- no raw socket access;
- synchronous handler methods only;
- Rust-owned framing and streaming;
- TLS constructor and limitations;
- low-level namespace purpose;
- subprocess convenience location;
- client non-goal;
- platform security qualifications.

Remove stale claims that:

- the bespoke native `Server` is the primary stable Python API;
- the default Python package is an HTTP client library;
- status codes through 999 are valid;
- weak ETags satisfy `If-Range`;
- conflicting constructor examples are supported.

Historical plans remain historical. Do not rewrite all plans.

### Examples

Retain a small set:

1. basic `SimpleHTTPRequestHandler` server;
2. custom `BaseHTTPRequestHandler` health endpoint;
3. optional HTTPS example using local certificate paths;
4. `serve_directory()` convenience if retained.

Delete redundant examples based on obsolete primary APIs.

## Track H — Packaging and import verification

### Installed-wheel contract

The wheel test must verify:

```python
from eggserve.server import (
    HTTPServer,
    ThreadingHTTPServer,
    HTTPSServer,
    ThreadingHTTPSServer,
    BaseHTTPRequestHandler,
    SimpleHTTPRequestHandler,
)
```

It must also verify that primary removed names are absent from top-level `eggserve.__all__`.

Do not assert that private `_native` implementation names are inaccessible; private extension internals may remain importable. The supported contract is determined by documented modules and `__all__`.

### Dependency/feature check

Confirm:

- Python extension no longer enables client feature by default;
- TLS dependencies are only those already required by the Rust TLS implementation;
- no Python runtime dependency was added unless indispensable;
- wheel still bundles the CLI as currently designed;
- package metadata accurately describes static server and `http.server` compatibility;
- no release workflow changes.

### Python version metadata

Do not expand support in this plan. Keep the current version declaration if tests and wheel build still target it.

If existing metadata conflicts internally—for example classifiers claim versions outside `requires-python`—correct only the inconsistency. A future plan can broaden ABI/version support without coupling it to API closure.

## Track I — Closure verification

### Targeted verification

Run focused suites for each final contract:

```sh
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test canonical_wire_interop
cargo test -p eggserve-core --test server_integration
cargo test -p eggserve-core --test streaming_buffer_qualification
cargo test -p eggserve-core --features tls
cargo test -p eggserve-bin --features tls
bash scripts/test-python-wheel.sh
```

Adjust feature names to the actual workspace. Do not invent a second TLS implementation solely to satisfy these example commands.

### Standard verification

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

### Same-commit hosted result

Both existing routine CI jobs must pass on the final implementation commit.

No additional hosted job is required for closure. Platform-specific manual wheel building remains governed by the current release policy.

### Handoff report

Add one concise closure note only if the repository's existing plan convention requires implementation summaries. Do not create an evidence directory or generated checklist.

The handoff must report:

- final supported Python exports;
- final low-level exports;
- removed/demoted names;
- TLS limitations;
- test modules removed and retained;
- dependency/feature changes;
- local verification results;
- final CI result and commit SHA;
- known platform qualifications that remain.

## Suggested commit sequence

1. `refactor: share TLS server setup with Python runtime`
2. `feat: add HTTPSServer compatibility facades`
3. `refactor: establish eggserve.server and lowlevel namespaces`
4. `refactor: remove default Python client exposure`
5. `refactor: demote native callback server surface`
6. `test: consolidate Python suite around final API`
7. `docs: reconcile http.server-compatible product contract`
8. `chore: close plans 094-098 on verified final tree`

The exact count may be reduced if several namespace edits are inseparable. Avoid one monolithic commit that mixes TLS implementation with test deletion and documentation.

## Acceptance criteria

Plan 098 and the full roadmap are complete only when all of the following are true on one final commit:

### TLS

- `HTTPSServer` and `ThreadingHTTPSServer` import from an installed wheel.
- HTTPS uses the existing Rust TLS/runtime path.
- Valid certificate/key configuration serves requests.
- Invalid TLS configuration fails before readiness.
- Only HTTP/1.1 ALPN is supported.
- No plaintext downgrade occurs.
- Static files, HEAD, ranges, and handler responses behave the same under TLS.
- TLS key material is not logged.

### API shape

- `eggserve.server` exposes exactly the six documented server/handler classes, aside from deliberately documented module constants or helpers.
- Top-level `eggserve` is small and unambiguous.
- Low-level primitives live under `eggserve.lowlevel`.
- There is one public static policy concept in Python.
- The bespoke native `Server` is not a top-level recommended API.
- `ServerSecureRoot`, `StaticPolicyWrapper`, and duplicate body/policy concepts are internal or removed from supported documentation.
- The default wheel does not expose the experimental HTTP client.
- The Python extension no longer enables client-only features without need.

### Behavior

- Base handler and simple handler tests from Plans 096–097 remain green.
- Duplicate request and response headers remain supported.
- Handler framing remains runtime-owned.
- File-backed static responses remain streamed by Rust.
- Safe defaults remain unchanged.
- TLS and plaintext share the same handler and static semantics.

### Simplification

- Redundant Python tests for removed APIs are deleted.
- Unique wire, filesystem, lifecycle, stream, packaging, and TLS coverage remains.
- The final Python test organization maps directly to supported modules.
- No new testing framework, registry, workflow, or matrix was introduced.
- Active documentation contains no conflicting old primary API examples.
- Examples are limited to the small final set.

### Verification

- `./scripts/verify.sh fast` passes.
- `./scripts/verify.sh full` passes.
- Both existing routine CI jobs pass on the final commit.
- No release was published as part of implementation.
- Manual release policy remains unchanged.

## Explicit non-goals after closure

Completion of this roadmap does not authorize follow-on expansion into:

- ASGI or WSGI;
- application routing;
- middleware;
- proxying;
- uploads;
- CGI;
- authentication;
- sessions or cookies;
- WebSockets;
- HTTP/2 or HTTP/3;
- compression;
- multipart ranges;
- ACME;
- virtual hosting;
- general HTTP client development;
- broad Python-version matrix work;
- edge-server competition with nginx or Caddy.

Any future work in those areas requires a separate scope decision and must not be treated as an implied continuation of Plans 094–098.