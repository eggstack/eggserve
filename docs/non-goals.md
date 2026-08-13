# Non-Goals

These are explicit non-goals for eggserve. If a feature appears here, it is out of scope unless this document is updated first.

- **No in-tree ASGI or WSGI adapter** — eggserve is a static file server, not an application server; ASGI/WSGI integration is left to separate projects
- **No general-purpose request handling framework** — the optional handler callback provides a hook for custom responses, but eggserve is not a routing framework or application server
- **No CGI** — legacy dynamic content execution is not supported
- **No upload/write support in the initial product** — the server is read-only by design
- **No reverse proxying** — eggserve does not forward requests to upstream servers
- **No automatic ACME** — TLS certificate management and automation are out of scope (native TLS server termination and TLS client verification are implemented; see docs/tls.md)
- **The experimental HTTP client is not an HTTPX/requests replacement** — it supports basic low-level requests but has no connection pooling, redirects, cookies, proxy support, or streaming
- **No database-backed configuration** — configuration is file/CLI based
- **No generic plugin host** — eggserve has a fixed feature set, not an extensible architecture
- **No templating engine** — directory listings use static HTML, not templates
- **No framework routing** — eggserve maps URLs to files, not to application handlers
- **No middleware stack** — request processing is a fixed pipeline, not composable layers
- **No session, cookie, or auth framework** — except possible later basic-auth opt-in; no auth by default
- **No attempt to compete with nginx/Caddy as a full edge server** — eggserve is a hardened static file server, not an edge platform
- **No attempt to compete with Granian/Uvicorn as app servers** — eggserve does not run Python application code
- **No Windows hardened profile** — Plans 084–086 have implemented handle-relative confinement (directory-handle retention, child resolution, directory enumeration via `NtQueryDirectoryFile`) and established the adversarial filesystem qualification test scaffold (114 tests) covering reparse-point denial, namespace normalization, race harness, and more. Independent adversarial review is incomplete. Windows remains functional-only until that review is completed. See [security-policy.md](security-policy.md) for the full statement.
- **No HTTP trailers** — Trailers are deferred; the canonical response model does not include trailer support
- **No raw socket response writers** — All responses go through the canonical normalization path
- **No socketserver implementation identity** — The Python `http.server` facade uses Rust-managed listeners, bounded file-like request/response buffers, and event-driven shutdown; raw sockets, `fileno()`, and exact one-request polling are not compatibility promises
- **No HTTP/2** — The runtime supports HTTP/1.1 only. HTTP/2 is out of scope.
- **No WebSocket or upgrade support** — The runtime does not support protocol upgrades.
- **No middleware stack in the server module** — The `Service` trait is a single-layer abstraction. Composition via middleware is left to downstream projects.
- **No Python existing-socket support** — Passing an already-bound Python socket to the native `Server` is deferred. Rust supports `from_listener()` for existing `TcpListener` ownership, but the Python bindings do not yet expose this. Ownership transfer semantics differ across platforms and would require careful descriptor/handle duplication. This capability may be added in a future milestone if cross-platform safety can be ensured.
- **No production profile without evidence** — Production profiles require external qualification evidence before hardened status. Production profiles are documented in README.md and `docs/deployment.md`.
- **No server capability expansion (product-surface freeze)** — Do not add server capabilities outside the existing non-goals without a new explicit product decision. Reject routine feature proposals for: routes/middleware, application handler ecosystems, uploads/forms/multipart, content compression, HTTP/2/3, WebSockets, reverse proxying, ACME, and virtual hosts. Correct HTTP/1.1 behavior, security fixes, platform hardening, and bounded compatibility corrections remain in scope.
- **No client feature expansion** — The existing client feature may remain for primitive completeness, but: do not expose a new primary Python client; do not pursue `httpx`/`requests` parity; do not add pools, redirects, cookies, proxies, authentication helpers, decompression, or HTTP/2; do not let client requirements enlarge the default server dependency graph. The client is documented as low-level/experimental.
- **No Python API expansion** — Retain the documented six-class `http.server` subset. Do not pursue raw `socketserver` internals, `fileno()`, one-request listener mode, arbitrary stream replacement, forking mixins, or async handlers.
- **No crate split without measured benefit** — Do not move client code to a new crate automatically. A split is authorized only if measurements show all of: default server artifacts or compile graph materially benefit; public compatibility can be preserved or migrated simply before 1.0; workspace/release complexity does not increase disproportionately; the split removes real feature coupling rather than changing directory layout. Otherwise retain feature-gated client code and freeze it.

> These are non-goals for this repository, not forbidden downstream uses. The primitive API should be strong enough for separate projects to build ASGI/WSGI adapters, application servers, and HTTP clients externally. Those downstream projects are not release deliverables or supported application-serving modes of eggserve.
