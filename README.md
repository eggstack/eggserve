# eggserve

> eggserve is a hardened, Rust-backed replacement for the common `python -m http.server` static-serving workflow and a small foundation library for safe HTTP/static-serving primitives.

**eggserve is not a general web server, framework, ASGI/WSGI runtime, or Granian replacement.** It serves static files from a directory with secure-by-default behavior. That is all.

## What is eggserve?

eggserve provides a single-purpose CLI and a Rust library for serving static files over HTTP with security as the default, not an afterthought. It targets the common developer workflow of "I need to serve this directory locally" while rejecting the unsafe behaviors baked into Python's `http.server` module.

## Why not Python http.server?

`python -m http.server` is convenient but unsafe by default:

- Binds to all interfaces (0.0.0.0) unless explicitly told otherwise
- Follows symlinks without restriction
- Serves dotfiles
- Enables directory listing
- Uses a slow, single-threaded Python implementation

eggserve fixes these by making the safe choice the only default. Every unsafe behavior is available but requires explicit opt-in.

## Scope and non-goals

eggserve is deliberately narrow. For the full list of non-goals, see [docs/non-goals.md](docs/non-goals.md).

**This is not:** an ASGI/WSGI runtime, a reverse proxy, a web framework, a template engine, a plugin host, a dynamic request execution environment, or a replacement for nginx/Caddy.

**This is:** a hardened static file server with safe defaults, a small reusable library for path confinement and policy enforcement, and a Python-packaged tool that feels like `python -m http.server`.

## Expected CLI shape

```sh
eggserve [DIR] [--bind HOST:PORT] [--port PORT] [--public] [--directory-listing] [--follow-symlinks] [--serve-dotfiles]
         [--max-connections N] [--max-file-streams N] [--max-header-bytes N] [--max-request-target-bytes N]
         [--header-timeout SECS] [--idle-timeout SECS] [--write-timeout SECS]
```

- `DIR` — directory to serve (default: current directory)
- `--bind HOST:PORT` — address to bind (default: `127.0.0.1:8000`)
- `--port PORT` — port to listen on (default: `8000`)
- `--public` — bind to all interfaces (overrides loopback default)
- `--directory-listing` — enable directory listing (disabled by default)
- `--follow-symlinks` — follow symlinks (denied by default)
- `--serve-dotfiles` — serve dotfiles (denied by default)
- `--max-connections N` — max concurrent connections (default: `64`)
- `--max-file-streams N` — max concurrent file streams (default: `32`)
- `--max-header-bytes N` — max header size in bytes (default: `32768`)
- `--max-request-target-bytes N` — max request target size in bytes (default: `8192`)
- `--header-timeout SECS` — header read timeout in seconds (default: `10`)
- `--idle-timeout SECS` — idle keep-alive timeout in seconds (default: `30`)
- `--write-timeout SECS` — response write timeout in seconds (default: `60`)

## Security defaults

eggserve ships with secure defaults. Every option that weakens security requires explicit CLI flags. The full security policy is documented in [docs/security-policy.md](docs/security-policy.md).

Key defaults:

- **Loopback only** — binds to 127.0.0.1 unless `--public` is passed
- **GET and HEAD only** — all other methods are rejected
- **No request bodies** — incoming request bodies are discarded
- **No symlink following** — denied unless `--follow-symlinks` is passed
- **No dotfiles served** — hidden files are excluded
- **No directory listing** — unless `--directory-listing` is passed
- **Unknown MIME as application/octet-stream** — safe fallback
- **Malformed request targets rejected** — invalid paths are not resolved
- **Logs sanitized** — paths/headers are sanitized before logging
- **Resource limits enabled** — connection and request limits are active

## Project status

**Plan 004 complete.** Resource limits and operational hardening are enforced. Connection limits, file-stream limits, header timeouts, response write timeouts, and request body rejection are active. Startup output displays all enforced limits. See [plans/](plans/) for the planned milestone sequence.

## Development

Development is plan-driven. All changes must be backed by a plan in the [plans/](plans/) directory. See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.
