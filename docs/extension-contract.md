# Extension Contract

## Overview

eggserve is a library, not a framework. It provides hardened path validation, policy enforcement, secure root resolution, and response planning as composable primitives. Downstream projects may build on these primitives for dynamic sites, test harnesses, or protocol adapters — but they must respect the security boundaries that make eggserve's guarantees meaningful.

## Allowed integration patterns

### Dynamic sites

Dynamic applications (frameworks, CMS backends, API servers) may use `SecureRoot` to serve assets, downloads, and uploaded files. eggserve handles path validation and confinement; the application handles routing, authentication, and business logic.

```python
# Python example: dynamic endpoint + static assets
from eggserve import SecureRoot, StaticPolicy

root = SecureRoot("public", StaticPolicy())
resource = root.resolve_path(request_path)
```

Dynamic endpoints must not bypass eggserve's path resolution. User-provided paths must always flow through `SecureRoot.resolve_path()` or `SecureRoot.resolve()` — never through raw `os.path.join()` or equivalent.

### Test servers

Integration tests may use request validation (`validate_method`, `validate_request_body`) and response planning (`plan_file_response`) to verify server behavior without spinning up a full HTTP listener. The planner produces Hyper-independent value objects that can be inspected, asserted on, or mapped into test fixtures.

### ASGI/WSGI adapters

Out-of-tree adapters may map ASGI/WSGI request objects into eggserve's primitives and translate response plans back into framework responses. The adapter is responsible for:

1. Extracting the request path and method from the framework's request object.
2. Calling `SecureRoot.resolve_path()` or `ConfinedPath::parse()` + `SecureRoot::resolve()`.
3. Calling `plan_file_response()` or `plan_directory_listing()` with the resolved resource.
4. Mapping `StaticResponsePlan` fields into the framework's response API.

eggserve does not provide ASGI/WSGI interfaces directly (see [non-goals.md](non-goals.md)).

## Security boundary rules

### What downstream must do

- Route all request paths through eggserve's resolution layer (`SecureRoot`, `ConfinedPath`).
- Preserve safe defaults unless the user explicitly opts in via `StaticPolicy` fields.
- Use the file handle returned by `ResolvedFile::into_std_file()` (Rust) or `resource.file` (Python) directly — do not reconstruct paths for reopening.

### What downstream must not do

- **Must not claim descriptor-relative hardening** if it extracts paths from `safe_relative_components()` and reopens them manually. Descriptor-relative TOCTOU hardening applies only when files are opened during resolution via `openat(O_NOFOLLOW)`. Reopening by path — even a relative path reconstructed from components — bypasses the guarantee.
- **Must not join user input to filesystem paths** and serve the result directly. This defeats path confinement.
- **Must not cache resolved file handles across requests** without understanding that `RootGuard` is created per resolution call. Caching introduces staleness and potential TOCTOU issues.
- **Must not modify the `StaticPolicy` defaults silently.** If downstream enables directory listing, symlinks, or dotfiles, the user must explicitly request it.

## Python integration patterns

### Native primitives (preferred)

When `eggserve.NATIVE_AVAILABLE is True`, use the Rust-backed primitives directly:

```python
from eggserve import SecureRoot, StaticPolicy, validate_method

root = SecureRoot("public", StaticPolicy())
resource = root.resolve_path("/assets/style.css")
plan = resource.file.plan_response("GET")
```

Native primitives provide full path confinement, descriptor-relative hardening (on Unix), and response planning without a subprocess.

### Subprocess API

When native primitives are unavailable, use `ServeConfig` and `ServerProcess` to manage the Rust binary:

```python
from eggserve import ServeConfig, ServerProcess

config = ServeConfig(directory="public", port=9000)
proc = ServerProcess(config)
proc.start()
```

See [python-api.md](python-api.md) for the full API reference.

## Rust integration patterns

Use the `primitives` module:

```rust
use eggserve_core::primitives::{
    SecureRoot, StaticPolicy, ConfinedPath, PathPolicy,
    http::{validate_method, validate_request_body, ReadOnlyMethod},
    planner::plan_file_response,
};

let root = SecureRoot::new(".", StaticPolicy::safe_default())?;
let resource = root.resolve_uri("/src/lib.rs")?;
```

See [public-api-boundary.md](public-api-boundary.md) for the stable API surface and [secure-root.md](secure-root.md) for resolution details.

## Non-goals reminder

eggserve deliberately does not provide:

- ASGI/WSGI compatibility (adapters are downstream responsibility)
- Request routing or middleware
- Authentication or authorization
- Templating or dynamic content execution
- Plugin systems or extensible architectures

If your project needs these, build them on top of eggserve's primitives. See [non-goals.md](non-goals.md) for the full list.
