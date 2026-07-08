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
eggserve [OPTIONS] [PORT] [--directory DIR]

# Options:
#   --directory DIR          Root directory to serve (default: .)
#   --addr HOST:PORT         Bind address (default: 127.0.0.1:8000)
#   --bind HOST:PORT         Bind address (alias for --addr)
#   --port PORT              Port to listen on
#   --public                 Bind to all interfaces
#   --directory-listing      Enable directory listing
#   --follow-symlinks        Follow symlinks
#   --allow-dotfiles         Serve dotfiles
#   --log-format FORMAT      text, json, or none (default: text)
#   --quiet                  Suppress startup banner
#   --max-connections N      Max concurrent connections (default: 64)
#   --max-file-streams N     Max concurrent file streams (default: 32)
#   --max-header-bytes N     Max header size (default: 32768)
#   --max-request-target-bytes N  Max request target size (default: 8192)
#   --header-timeout SECS    Header read timeout (default: 10)
#   --idle-timeout SECS      Idle keep-alive timeout (default: 30)
#   --write-timeout SECS     Response write timeout (default: 60)
```

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

**Plan 005 complete.** CLI arguments match the plan 005 spec. Python wheel packaging via maturin is functional. The `python -m eggserve` launcher works. See [plans/](plans/) for the planned milestone sequence.

### Installation

```sh
# From source (requires Rust toolchain)
cargo install --path crates/eggserve-bin

# Via Python wheel (requires Python 3.8+)
pip install eggserve

# Or run directly with pipx
pipx run eggserve
```

## Development

Development is plan-driven. All changes must be backed by a plan in the [plans/](plans/) directory. See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.
