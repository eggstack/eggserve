# Non-Goals

These are explicit non-goals for eggserve. If a feature appears here, it is out of scope unless this document is updated first.

- **No in-tree ASGI or WSGI server** — eggserve is a static file server/library, not a maintained application server; production ASGI/WSGI servers live in separate projects. Plan 204 provides an experimental async low-level bridge (`lowlevel.AsyncServer` + test/example ASGI fixture for HTTP/WebSocket qualification only), not a maintained server with lifespan/workers/reload/router.
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
- **No HTTP trailers** — Superseded by Plan 198: canonical request/response trailers and bounded interim 1xx are implemented (H1/H2/H3 adapters + `ResponseStream::with_trailers` + `InterimSender` + Python `validate_trailers`/`validate_interim` and async wire support via `AsyncServer`). The remaining limitation is upstream-library emission scoping (documented in `http-primitives.md`), not absence.
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
- **No WebSocket framing in core (Plan 199 generic tunnel supported)** — The
  runtime provides a safe, generic duplex handoff (`TunnelRequest`/
  `TunnelCapability`/`TunnelIo` on `RequestContext`; H1 `Upgrade`, `CONNECT`,
  H2/H3 Extended `CONNECT`; H3 generic `:protocol` blocked by `h3` 0.0.8),
  not a WebSocket codec. `Service` still returns `Response` only; `accept`
  returns the validated handshake (`101` H1 / `200` otherwise, runtime owns
  framing, no raw socket) plus bounded `TunnelIo` for the downstream codec.
  Ordinary denial stays ordinary HTTP. WebSocket ping/pong, fragmentation,
  close codes, permessage-deflate, and ASGI `websocket.*` events remain
  downstream (see `tunnel_upgrade.rs` echo + `tokio-tungstenite` fixture).
  Downstream servers must not bypass the canonical boundary via raw
  Hyper/h2/h3/Quinn types.
- **No middleware stack in the server module** — The `Service` trait is a single-layer abstraction. Composition via middleware is left to downstream projects.
- **No Python existing-socket support** — Passing an already-bound Python socket to the native `Server` is deferred. Rust supports `from_listener()` / `from_std_listener()` for existing `TcpListener` ownership plus Unix (`from_unix_listener`), systemd (`from_systemd_index`/`from_systemd_name`), and H3 prebound UDP (`http3_socket`) under Plan 201, but the Python bindings do not yet expose these. Ownership transfer semantics differ across platforms and would require careful descriptor/handle duplication. This capability may be added in a future milestone if cross-platform safety can be ensured.
- **No production profile without evidence** — Production profiles require external qualification evidence before hardened status. Production profiles are documented in README.md and `docs/deployment.md`.
- **No unqualified server capability expansion (product-surface freeze)** — Do not add server capabilities outside the existing non-goals without a new explicit product decision. The Python facade remains HTTP/1.1-shaped, and routine feature proposals for routes/middleware, application handler ecosystems, uploads/forms/multipart, content compression, WebSockets, reverse proxying, ACME, and virtual hosts remain out of scope. Native Rust HTTP/2 and HTTP/3 are separately governed, feature-gated experimental transport boundaries limited to Plans 185–188 and their qualification handoffs; neither expands the Python compatibility surface. Correct HTTP/1.1 behavior, security fixes, platform hardening, and bounded compatibility corrections remain in scope.
- **No Python API expansion** — Retain the documented six-class `http.server` subset for sync compatibility. Do not pursue raw `socketserver` internals, `fileno()`, one-request listener mode, arbitrary stream replacement, or forking mixins. Async handlers live only in the experimental `lowlevel.AsyncServer` substrate (Plan 204, H1-only); the sync facade still rejects coroutines.
- **No crate split without measured benefit** — Do not split crates automatically. A split is authorized only if measurements show all of: default server artifacts or compile graph materially benefit; public compatibility can be preserved or migrated simply before 1.0; workspace/release complexity does not increase disproportionately; the split removes real feature coupling rather than changing directory layout.

> These are non-goals for this repository, not forbidden downstream uses. The
> currently qualified downstream capability is an HTTP-only application-server
> substrate using the public Rust primitives and experimental runtime APIs.
> Separate projects may build ASGI/WSGI/CGI/FastCGI adapters and application
> servers externally, but those projects are not release deliverables or
> supported application-serving modes of eggserve.
