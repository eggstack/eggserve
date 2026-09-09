# Non-Goals

These are explicit non-goals for eggserve. If a feature appears here, it is out of scope unless this document is updated first.

- **No in-tree ASGI or WSGI adapter** — eggserve is a static file server, not an application server; ASGI/WSGI integration is left to separate projects
- **No general-purpose request handling framework** — the optional handler callback provides a hook for custom responses, but eggserve is not a routing framework or application server
- **No CGI / FastCGI in-tree adapters (Plan 167 no-go)** — legacy subprocess execution and FastCGI gateway state machines live downstream as plain canonical `Service` implementations, not in `eggserve-core`/CLI/wheels. CGI follows the upstream removal (deprecated in Python 3.13, removed in 3.15) with no concrete in-tree consumer; FastCGI was never an `http.server` feature. Both would add process-management and backend-protocol maintenance against the no-broad-dependencies rule, and the anonymity-sensitive profile never enables them.
- **No upload/write support in the initial product** — the server is read-only by design
- **No reverse proxying** — eggserve does not forward requests to upstream servers
- **No automatic ACME** — TLS certificate management and automation are out of scope (native TLS server termination is implemented; see docs/tls.md)
- **No database-backed configuration** — configuration is file/CLI based
- **No generic plugin host** — eggserve has a fixed feature set, not an extensible architecture
- **No templating engine** — directory listings use static HTML, not templates
- **No framework routing** — eggserve maps URLs to files, not to application handlers
- **No middleware stack** — request processing is a fixed pipeline, not composable layers
- **No session, cookie, or auth framework** — except possible later basic-auth opt-in; no auth by default
- **No attempt to compete with nginx/Caddy as a full edge server** — eggserve is a hardened static file server, not an edge platform
- **No attempt to compete with Granian/Uvicorn as app servers** — eggserve does not run Python application code
- **No Windows hardened profile** — Windows has handle-relative confinement (directory-handle retention, child resolution, and directory enumeration via `NtQueryDirectoryFile`) and has been manually qualified for the executed classes. Two open-descendant root-rename cases are skipped because NTFS rejects that external path operation, so Windows remains functional-only and trusted/local-content only. See [security-policy.md](security-policy.md) for the full statement.
- **No HTTP trailers** — Trailers are deferred; the canonical response model does not include trailer support
- **No raw socket response writers** — All responses go through the canonical normalization path
- **No socketserver implementation identity** — The Python `http.server` facade uses Rust-managed listeners, bounded file-like request/response buffers, and event-driven shutdown; raw sockets, `fileno()`, and exact one-request polling are not compatibility promises
- **No unqualified protocol expansion** — HTTP/2 and HTTP/3 are optional,
  separately governed transport work under Plans 183–188. HTTP/1.1 remains
  the minimal/default compatibility baseline, and this plan does not enable a
  second wire protocol. Protocol work does not authorize routing, reverse
  proxying, WebSockets, WebTransport, CONNECT tunnels, server push, ACME,
  middleware, or application-server behavior in-tree. Python
  `http.server`-shaped surfaces remain HTTP/1.1-oriented unless a later
  compatibility decision says otherwise.
- **No WebSocket or generic upgrade support (Plan 176 deferred)** — The runtime
  has no canonical upgrade capability, 101-handshake path, or upgraded-IO
  handoff: `Request` carries head/body/connection/lifecycle only, `Service`
  returns `Response` only, and normalization strips hop-by-hop handshake
  headers. The HTTP/1 driver deliberately uses Hyper's ordinary connection;
  it does not enable `.with_upgrades()`. Downstream WebSocket-class servers
  must not bypass the canonical boundary via raw Hyper types; reopen Plan 176
  only with a concrete upgrade consumer and current-Hyper evidence.
- **No middleware stack in the server module** — The `Service` trait is a single-layer abstraction. Composition via middleware is left to downstream projects.
- **No Python existing-socket support** — Passing an already-bound Python socket to the native `Server` is deferred. Rust supports `from_listener()` for existing `TcpListener` ownership, but the Python bindings do not yet expose this. Ownership transfer semantics differ across platforms and would require careful descriptor/handle duplication. This capability may be added in a future milestone if cross-platform safety can be ensured.
- **No production profile without evidence** — Production profiles require external qualification evidence before hardened status. Production profiles are documented in README.md and `docs/deployment.md`.
- **No server capability expansion (product-surface freeze)** — Do not add server capabilities outside the existing non-goals without a new explicit product decision. Reject routine feature proposals for: routes/middleware, application handler ecosystems, uploads/forms/multipart, content compression, HTTP/3, WebSockets, reverse proxying, ACME, and virtual hosts. The separately governed, feature-gated native Rust HTTP/2 runtime is limited to Plan 185's bounded transport scope and does not expand the Python compatibility surface. Correct HTTP/1.1 behavior, security fixes, platform hardening, and bounded compatibility corrections remain in scope.
- **No Python API expansion** — Retain the documented six-class `http.server` subset. Do not pursue raw `socketserver` internals, `fileno()`, one-request listener mode, arbitrary stream replacement, forking mixins, or async handlers.
- **No crate split without measured benefit** — Do not split crates automatically. A split is authorized only if measurements show all of: default server artifacts or compile graph materially benefit; public compatibility can be preserved or migrated simply before 1.0; workspace/release complexity does not increase disproportionately; the split removes real feature coupling rather than changing directory layout.

> These are non-goals for this repository, not forbidden downstream uses. The
> currently qualified downstream capability is an HTTP-only application-server
> substrate using the public Rust primitives and experimental runtime APIs.
> Separate projects may build ASGI/WSGI/CGI/FastCGI adapters and application
> servers externally, but those projects are not release deliverables or
> supported application-serving modes of eggserve.
