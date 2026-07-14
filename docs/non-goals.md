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
- **No attempt to compete with nginx/Caddy as a full edge server** — eggserve is a development/utility tool
- **No attempt to compete with Granian/Uvicorn as app servers** — eggserve does not run Python application code
- **No Windows reparse-point/NTFS junction hardening** — Windows is supported functionally with parser-level checks only (reserved names, ADS, drive prefixes, backslash). Filesystem-level hardening against reparse points and NTFS junctions is deferred. Windows is explicitly a trusted/local-use platform, not hardened for untrusted mutable public roots. See [security-policy.md](security-policy.md) for the full statement.
- **No HTTP trailers** — Trailers are deferred; the canonical response model does not include trailer support
- **No raw socket response writers** — All responses go through the canonical normalization path
- **No request body streaming** — Request body streaming belongs to Milestone 4
- **No HTTP/2** — The runtime supports HTTP/1.1 only. HTTP/2 is out of scope.
- **No WebSocket or upgrade support** — The runtime does not support protocol upgrades.
- **No middleware stack in the server module** — The `Service` trait is a single-layer abstraction. Composition via middleware is left to downstream projects.
- **No Python existing-socket support** — Passing an already-bound Python socket to the native `Server` is deferred. Rust supports `from_listener()` for existing `TcpListener` ownership, but the Python bindings do not yet expose this. Ownership transfer semantics differ across platforms and would require careful descriptor/handle duplication. This capability may be added in a future milestone if cross-platform safety can be ensured.

> These are non-goals for this repository, not forbidden downstream uses. The primitive API should be strong enough for separate projects to build them externally.
