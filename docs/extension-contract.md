# Extension Contract

This document is the authoritative contract for downstream consumers of eggserve. It defines what eggserve guarantees, what it does not implement, which APIs are safe to build on, and the rules that preserve eggserve's security properties when consumed as a library.

## Overview

eggserve is a library, not a framework. It provides hardened path validation, policy enforcement, secure root resolution, and response planning as composable primitives. Downstream projects may build on these primitives for dynamic sites, test harnesses, or protocol adapters — including ASGI/WSGI adapters, application servers, and HTTP clients — but they must respect the security boundaries that make eggserve's guarantees meaningful.

## What eggserve guarantees

- **Path confinement.** Every request path is parsed, decoded, normalized, and validated against a policy before any filesystem access occurs. Traversal, NUL bytes, ambiguous separators, Windows prefixes, reserved device names, and ADS syntax are rejected. The resolved filesystem path is verified to remain within the configured root.
- **Policy enforcement.** `StaticPolicy` defaults deny all optional behaviors: directory listing, symlinks, and dotfiles. Callers must explicitly opt in to any weaker behavior. Policies are enforced before resolution; violations produce 403, not 404.
- **Safe defaults.** The server binds to loopback, accepts only GET and HEAD, rejects request bodies, denies symlinks, denies dotfiles, denies directory listing, and sanitizes logs. These are not advisory — the code rejects non-conforming requests before any filesystem access.
- **Descriptor-relative hardening on Unix.** Under safe defaults, symlink denial is descriptor-relative. Each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`. This prevents TOCTOU symlink-swap attacks during resolution. A symlink swapped into place between the stat and the open is refused rather than followed.

## What eggserve intentionally does not implement

eggserve does not provide:

- ASGI or WSGI runtime interfaces
- Request routing or URL dispatch
- Middleware stacks
- Templating or dynamic content execution
- Cookies, sessions, or authentication
- Reverse proxying
- Compression
- Plugin systems or extensible architectures
- HTTP/2, HTTP/3, WebSocket, or CONNECT semantics

These are non-goals for this repository, not forbidden downstream uses. The primitive API should be strong enough for separate projects to build them externally. See [non-goals.md](non-goals.md) for the full list. Downstream use of primitives for clients or application servers is explicitly allowed but not owned by eggserve. Those projects are not release deliverables or supported application-serving modes.

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

### Server primitives

Python code may use `StaticResponder`, `Server`, and `Response` to build HTTP servers while Rust owns socket I/O, connection management, and file streaming. The server can dispatch to a Python handler callback for dynamic responses, or serve static files via `StaticResponder`. Python never touches sockets directly.

### HTTP client substrate

Downstream projects may use the feature-gated (`client`) client primitives to perform outbound HTTP requests:

```rust
use eggserve_core::primitives::client::{
    HttpClient, ClientConfig, ClientRequest, Method,
};

let client = HttpClient::new(ClientConfig::default());
let request = ClientRequest::builder()
    .method(Method::Get)
    .url("http://localhost:8080/api/data")?
    .build()?;
let response = client.send(request)?;
```

The client is a transport substrate — downstream projects build higher-level clients (cookie management, retries, redirects, auth) on top. The client enforces timeouts, verifies TLS by default, and provides structured errors. It does not provide convenience features that should be decided by the application layer.

## How downstream projects should consume the Rust primitives

Use the `primitives` module. It is the stable public boundary for embedding consumers.

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

## How downstream projects should consume the Python primitives

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

## Which primitives are safe to build on

The following types are in the stable tier. They are safe to build on; breaking changes to these types bump the major version (pre-1.0, minor versions may break):

| Type | Source |
|------|--------|
| `SecureRoot` | `primitives::secure_root` |
| `resolve_and_plan` | `primitives::secure_root` |
| `ResolvedResource` | `primitives::secure_root` |
| `ResolvedFile` | `primitives::secure_root` |
| `ResolvedDirectory` | `primitives::secure_root` |
| `StaticPolicy` | `primitives` (re-export of `policy`) |
| `PathPolicy` | `primitives` (re-export of `path`) |
| `ConfinedPath` | `primitives` (re-export of `path`) |
| `PathRejection` | `primitives` (re-export of `path`) |
| `validate_method` | `primitives::http` |
| `validate_request_body` | `primitives::http` |
| `validate_request_target` | `primitives::http` |
| `ReadOnlyMethod` | `primitives::http` |
| `RequestValidationError` | `primitives::http` |
| `plan_file_response` | `primitives::planner` |
| `plan_directory_listing` | `primitives::planner` |
| `evaluate_conditional_headers` | `primitives::planner` |
| `evaluate_if_none_match` | `primitives::planner` |
| `evaluate_if_range` | `primitives::planner` |
| `evaluate_range_header` | `primitives::planner` |
| `generate_etag` | `primitives::planner` |
| `StaticResponsePlan` | `primitives::response` |
| `HeaderMapPlan` | `primitives::response` |
| `ResponseHeader` | `primitives::response` |
| `BodyPlan` | `primitives::response` |
| `ResponseStatus` | `primitives::response` |
| `FileRange` | `primitives::response` |
| `ConditionalRequestOutcome` | `primitives::response` |
| `RangeRequestOutcome` | `primitives::response` |
| `BodySource` | `primitives::body` |
| `BodyKind` | `primitives::body` |
| `BodySourceError` | `primitives::body` |
| `HttpClient` | `primitives::client` (feature-gated: `client`) |
| `ClientConfig` | `primitives::client` (feature-gated: `client`) |
| `ClientRequest` | `primitives::client` (feature-gated: `client`) |
| `ClientRequestBuilder` | `primitives::client` (feature-gated: `client`) |
| `ClientResponse` | `primitives::client` (feature-gated: `client`) |
| `ClientError` | `primitives::client` (feature-gated: `client`) |

## Which modules are internal and must not be depended on

The following modules are internal implementation details. They may change without notice and must not be imported by downstream code:

- `fs` — filesystem resolution internals (`RootGuard`, `ResolvedResource` internals, platform-specific traversal)
- `path` — path parsing internals (decoding, normalization, component validation, platform checks)
- `response` — response construction internals (file streaming, directory listing HTML, error responses)
- MIME type detection (`mime` module) — extension-to-type mapping, `octet-stream` fallback
- Error taxonomy (`error` module) — `Config`, `Bind`, `Runtime`, `RequestRejected`, `Io` variants

The only public path into these types is through the `primitives` facade. If a type is not re-exported in `primitives`, it is not part of the stable contract.

## How policy preservation works across CLI, Rust, and Python

Safe defaults are shared across all three interfaces:

| Interface | Default policy |
|-----------|---------------|
| CLI (`eggserve-bin`) | `StaticPolicy::safe_default()` via flags |
| Rust primitives | `StaticPolicy::safe_default()` or `StaticPolicy::default()` |
| Python primitives | `StaticPolicy()` (constructors use safe defaults) |

All three enforce the same `StaticPolicy` shape: directory listing disabled, symlinks denied, dotfiles denied. Weakening any default requires an explicit opt-in (CLI flag, Rust struct field, Python constructor argument). Downstream projects must not silently override these defaults.

## The capability rule

Use resolved resources and body sources, not reconstructed paths.

`ResolvedFile` is a capability object. It holds the open file handle, metadata, content type, and ETag. It has no public constructor — it is obtained only through `SecureRoot::resolve()`.

Downstream code must:

- Use the `File` handle returned by `ResolvedFile` (Rust) or the `file` attribute on the resolved resource (Python) directly.
- Plan responses with `plan_file_response()` or `ResolvedFile::plan_response()` using the resolved resource.
- Convert resolved files to `BodySource` objects via `into_body(&plan)` (Rust) or `body_for_plan(plan)` (Python) for streaming, rather than reopening paths.
- Never extract a path from a resolved resource and reopen it. Descriptor-relative hardening applies only when files are opened during resolution via `openat(O_NOFOLLOW)`. Reopening by path — even a relative path reconstructed from components — bypasses the guarantee.

## The concurrency rule

Rust owns sockets, timeouts, and file streaming for Python server APIs.

When Python code is used to build a server (via the `Server` primitive or the subprocess API):

- Socket I/O, connection acceptance, and timeout enforcement are handled by the Rust runtime.
- File streaming is handled by the Rust runtime; file bodies never pass through Python memory.
- Python code returns explicit `Response` values; it does not drive socket I/O directly.
- The GIL is released during I/O operations, allowing other Python threads to run.
- Callback-induced latency or errors must not prevent Rust from enforcing connection-level policy.

This separation ensures that Python application code cannot bypass timeout limits, connection caps, or file-stream quotas.

## The adapter rule

ASGI/WSGI adapters should live downstream.

eggserve provides the primitives that adapters need: path resolution, request validation, response planning, resolved file handles, and server primitives (`StaticResponder`, `Server`, `Response`). The adapter is responsible for:

1. Extracting the request path and method from the framework's request object.
2. Calling `SecureRoot.resolve_path()` or `ConfinedPath::parse()` + `SecureRoot::resolve()`.
3. Calling `plan_file_response()` or `plan_directory_listing()` with the resolved resource.
4. Mapping `StaticResponsePlan` fields into the framework's response API.
5. For server-based adapters: returning `Response` objects from the responder callback.

eggserve does not provide ASGI/WSGI interfaces directly (see [non-goals.md](non-goals.md)).

## Downstream adapter boundary

> eggserve may expose primitives sufficient for an external ASGI, WSGI, or application server adapter. eggserve does not provide those adapters in-tree. Those downstream projects are not release deliverables. Any new API added for adapter authors must remain protocol- and framework-neutral.

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
