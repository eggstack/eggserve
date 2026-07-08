# Non-Goals

These are explicit non-goals for eggserve. If a feature appears here, it is out of scope unless this document is updated first.

- **No ASGI or WSGI runtime** — eggserve is a static file server, not an application server
- **No dynamic Python callbacks in the initial server path** — no hook into request handling via Python
- **No CGI** — legacy dynamic content execution is not supported
- **No upload/write support in the initial product** — the server is read-only by design
- **No reverse proxying** — eggserve does not forward requests to upstream servers
- **No automatic ACME** — TLS certificate management is out of scope (TLS itself is deferred)
- **No database-backed configuration** — configuration is file/CLI based
- **No plugin system** — eggserve has a fixed feature set, not an extensible architecture
- **No templating engine** — directory listings use static HTML, not templates
- **No authentication system** — except possible later basic-auth opt-in; no auth by default
- **No attempt to compete with nginx/Caddy as a full edge server** — eggserve is a development/utility tool
- **No attempt to compete with Granian/Uvicorn as app servers** — eggserve does not run Python application code
