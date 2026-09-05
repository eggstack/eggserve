# Extension Contract

This document is the authoritative contract for downstream consumers of eggserve. It defines what eggserve guarantees, what it does not implement, which APIs are safe to build on, and the rules that preserve eggserve's security properties when consumed as a library.

## Overview

eggserve is a library, not a framework. It provides hardened path validation,
policy enforcement, secure root resolution, canonical HTTP types, and response
planning as composable primitives. A separate downstream project may use the
qualified HTTP-only `Service`/runtime substrate for an application server;
other protocol adapters—including ASGI/WSGI/CGI/FastCGI—remain external
projects and are not implemented or supported modes of eggserve. All consumers
must respect the security boundaries that make eggserve's guarantees
meaningful.

## What eggserve guarantees

- **Path confinement.** Every request path is parsed, decoded, normalized, and validated against a policy before any filesystem access occurs. Traversal, NUL bytes, ambiguous separators, Windows prefixes, reserved device names, and ADS syntax are rejected. The resolved filesystem path is verified to remain within the configured root.
- **Policy enforcement.** `StaticPolicy` defaults deny all optional behaviors: directory listing, symlinks, and dotfiles. Callers must explicitly opt in to any weaker behavior. Policies are enforced before resolution; violations produce 403, not 404.
- **Safe defaults.** The server binds to loopback, accepts only GET and HEAD, rejects request bodies, denies symlinks, denies dotfiles, denies directory listing, and sanitizes logs. These are not advisory — the code rejects non-conforming requests before any filesystem access.
- **Descriptor-relative hardening on Unix.** Under safe defaults, symlink denial is descriptor-relative. Each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`. This prevents TOCTOU symlink-swap attacks during resolution. A symlink swapped into place between the stat and the open is refused rather than followed.
- **Pinned root identity.** The serving root is opened once at server startup and retained for the server lifetime via `PinnedRoot`. Renaming or replacing the configured pathname does not redirect the running server to a different tree. Every static response streams from an already-validated opened file — no file is reopened by path after resolution.

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
- HTTP/2, HTTP/3, WebSocket, CONNECT, or generic upgrade-handoff semantics
  (Plan 176 closed as deferred: no `UpgradeRequest`/`UpgradedIo`/upgrade
  outcome in `primitives`/`server`; 101 cannot be emitted via `Response`)

These are non-goals for this repository, not forbidden downstream uses. The
public Rust substrate is qualified for separate projects to build the HTTP
half of an application server; ASGI/WSGI/CGI/FastCGI protocol adaptation and
all application semantics remain downstream-owned. See [non-goals.md](non-goals.md)
for the full list. Downstream use is explicitly allowed but is not owned by
eggserve, and those projects are not release deliverables or supported
application-serving modes.

## Allowed integration patterns

### Dynamic sites

Dynamic applications (frameworks, CMS backends, API servers) may use `SecureRoot` to serve assets, downloads, and uploaded files. eggserve handles path validation and confinement; the application handles routing, authentication, and business logic.

```python
# Python example: dynamic endpoint + static assets
from eggserve.lowlevel import SecureRoot, StaticPolicy

root = SecureRoot("public", StaticPolicy())
resource = root.resolve_path(request_path)
```

Dynamic endpoints must not bypass eggserve's path resolution. User-provided paths must always flow through `SecureRoot.resolve_path()` or `SecureRoot.resolve()` — never through raw `os.path.join()` or equivalent.

### Downstream application servers (HTTP half)

A downstream event-driven application server is a plain canonical `Service`
plus bounded downstream tasks; it is not an EggServe feature. The supported
shape is: `Stream` body policy, move `RequestBody` into a spawned app task,
retain the `RequestLifecycle` observer, wait only for response-start, return
`ResponseBody::Stream`, and let the app task continue consuming request
chunks while producing bounded response chunks after return. All
cross-task coordination uses bounded channels (capacity 2 in the
qualification fixture); every potentially blocking send also watches
`lifecycle.cancelled()`. EggServe `max_in_flight_requests` bounds
pre-response `Service::call` only; the downstream project owns a separate
bounded application-task budget. Full ownership, timeout-split,
cancellation, shutdown-ordering, and byte-metadata rules are in
[downstream-app-server.md](downstream-app-server.md); the external-consumer
proof is `crates/eggserve-core/tests/app_server_consumer.rs` (Plan 175).

### Test servers

Integration tests may use request validation (`validate_method`, `validate_request_body`) and response planning (`plan_file_response`) to verify server behavior without spinning up a full HTTP listener. The planner produces Hyper-independent value objects that can be inspected, asserted on, or mapped into test fixtures.

### Server primitives

Python applications should use the six-class `eggserve.server` façade for
HTTP serving. Rust owns socket I/O, connection management, response framing,
and file streaming; Python handlers never receive raw sockets. The low-level
native callback engine and responder types are implementation details, while
Rust embedders may use the experimental `eggserve_core::server` service API.

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
from eggserve.lowlevel import SecureRoot, StaticPolicy, validate_method

root = SecureRoot("public", StaticPolicy())
resource = root.resolve_path("/assets/style.css")
plan = resource.file.plan_response("GET")
```

Native primitives provide full path confinement, descriptor-relative hardening (on Unix), and response planning without a subprocess.

### Subprocess API

When native primitives are unavailable, use `ServeConfig` and `ServerProcess` to manage the Rust binary:

```python
from eggserve.subprocess import ServeConfig, ServerProcess

config = ServeConfig(directory="public", port=9000)
proc = ServerProcess(config)
proc.start()
```

See [python-api.md](python-api.md) for the full API reference.

## Which primitives are safe to build on

The following types are in the stable tier. They are safe to build on; patch
releases preserve their source compatibility. Before 1.0, intentional
breaking changes require an explicit minor-version transition with release
notes and migration guidance:

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

## Which modules are internal and must not be depended on

The following modules are internal implementation details. They may change without notice and must not be imported by downstream code:

- `fs` — filesystem resolution internals (`RootGuard`, `ResolvedResource` internals, platform-specific traversal)
- `path` — path parsing internals (decoding, normalization, component validation, platform checks)
- `response` — response construction internals (file streaming, directory listing HTML, error responses)
- MIME type detection (`mime` module) — extension-to-type mapping, `octet-stream` fallback

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

The root itself is a capability object. `PinnedRoot` is opened once at startup and retained for the server lifetime. Requests resolve relative to this persistent root. Renaming or replacing the configured pathname does not redirect the running server.

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

ASGI/WSGI/CGI/FastCGI adapters should live downstream (Plan 167 closed as
no-go for in-tree CGI/FastCGI: no concrete consumer, upstream CGI removal,
and process/backend maintenance cost outweigh an in-tree adapter).

eggserve provides the primitives that adapters need: path resolution, request validation, response planning, resolved file handles, and server primitives (`StaticResponder`, `Server`, `Response`). The adapter is responsible for:

1. Extracting the request path and method from the framework's request object.
2. Calling `SecureRoot.resolve_path()` or `ConfinedPath::parse()` + `SecureRoot::resolve()`.
3. Calling `plan_file_response()` or `plan_directory_listing()` with the resolved resource.
4. Mapping `StaticResponsePlan` fields into the framework's response API.
5. For server-based adapters: returning `Response` objects from the responder callback.

eggserve does not provide ASGI/WSGI/CGI/FastCGI interfaces directly (see [non-goals.md](non-goals.md)). A downstream CGI executor or FastCGI Responder is a plain canonical `Service`: map the request into `Response`/`ResponseBody::Stream`, let the runtime own framing/normalization and the Plan 165 privacy boundary, and enforce its own subprocess/backend bounds (concurrency, env/PARAMS caps, stdout header scan, STDERR cap, deadlines, kill/abort and reaping on timeout/disconnect/shutdown/drop).

## Downstream adapter boundary

> eggserve may expose primitives sufficient for an external ASGI, WSGI, CGI, FastCGI, or application server adapter. eggserve does not provide those adapters in-tree. Those downstream projects are not release deliverables. Any new API added for adapter authors must remain protocol- and framework-neutral.

## Adapter support matrix

| Adapter | In-tree status | Downstream path |
|---------|---------------|-----------------|
| ASGI bridge | Not provided (non-goal) | Canonical `Service` + `Response`/`ResponseBody::Stream`; runtime owns framing/normalization and the Plan 165 privacy boundary |
| WSGI bridge | Not provided (non-goal) | Same `Service` seam; synchronous response mapping only |
| CGI executor (`CGIHTTPRequestHandler`/`--cgi` parity) | Not provided — Plan 167 closed as no-go (upstream 3.13 deprecation / 3.15 removal, no concrete consumer, subprocess-maintenance cost vs no-broad-dependencies) | Plain `Service`: bounded child concurrency, env/input sanitization, stdout/stderr caps, deadlines with kill/reap on timeout/disconnect/shutdown, generic 502/504 mapping, no shell/request injection |
| FastCGI gateway | Not provided — Plan 167 closed as no-go (never `http.server`, no concrete consumer) | Plain `Service`: fragmented-record corpus handling, Responder request/response mapping, streaming STDIN/STDOUT backpressure, STDERR caps, backend timeout/disconnect/abort, no cross-request contamination, connection/resource recovery |
| Generic HTTP upgrade handoff (WebSocket-class) | Not provided — Plan 176 closed as deferred (no concrete upgrade consumer; HTTP-only contract closed by Plans 172–175) | Not currently buildable on the canonical boundary; do not bypass via raw Hyper `OnUpgrade`/`Upgraded`. Reopen Plan 176 with a concrete consumer before designing the handoff |
| Custom subprocess/pipe backends | Not provided | Same bounds as CGI: the adapter enforces its own limits; core never inherits backend responsibilities |

Core release claims never depend on an optional adapter's behavior or
benchmarks. Adapter failures are qualified separately and never mixed into
core HTTP claims.

## Security boundary rules

### What downstream must do

- Route all request paths through eggserve's resolution layer (`SecureRoot`, `ConfinedPath`).
- Preserve safe defaults unless the user explicitly opts in via `StaticPolicy` fields.
- Use the file handle returned by `ResolvedFile::into_std_file()` (Rust) or `resource.file` (Python) directly — do not reconstruct paths for reopening.
- Respect root pinning: the root is opened once at startup and retained for the server lifetime. To serve a different root tree, construct a new `SecureRoot` or restart the server.

### What downstream must not do

- **Must not claim descriptor-relative hardening** if it extracts paths from `safe_relative_components()` and reopens them manually. Descriptor-relative TOCTOU hardening applies only when files are opened during resolution via `openat(O_NOFOLLOW)`. Reopening by path — even a relative path reconstructed from components — bypasses the guarantee. The root is pinned at startup; once resolved, files must be accessed through their opened handles.
- **Must not join user input to filesystem paths** and serve the result directly. This defeats path confinement.
- **Must not cache resolved file handles across requests** without understanding that `RootGuard` is created per resolution call. Caching introduces staleness and potential TOCTOU issues.
- **Must not modify the `StaticPolicy` defaults silently.** If downstream enables directory listing, symlinks, or dotfiles, the user must explicitly request it.
