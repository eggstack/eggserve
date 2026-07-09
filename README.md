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
#   --bind HOST              Bind host (host:port or bare host)
#   --port PORT              Port to listen on
#   --public                 Bind to all interfaces (required for 0.0.0.0)
#   --directory-listing      Enable directory listing
#   --follow-symlinks        Follow symlinks
#   --allow-dotfiles         Serve dotfiles
#   --log-format FORMAT      text, json, or none (default: text)
#   --quiet                  Suppress startup banner
#   --max-connections N      Max concurrent connections (default: 64)
#   --max-file-streams N     Max concurrent file streams (default: 32)
#   --header-timeout SECS    Header read timeout, also bounds TLS handshake (default: 10)
#   --write-timeout SECS     Response write timeout (default: 60)

# TLS options (requires tls feature):
#   --tls-cert PATH          PEM certificate chain (requires --tls-key)
#   --tls-key PATH           PEM private key (requires --tls-cert)
```

## Security defaults

eggserve ships with secure defaults. Every option that weakens security requires explicit CLI flags. The full security policy is documented in [docs/security-policy.md](docs/security-policy.md).

Key defaults:

- **Loopback only** — binds to 127.0.0.1 unless `--public` is passed
- **GET and HEAD only** — all other methods are rejected
- **No request bodies** — `Content-Length > 0`, invalid `Content-Length`, and any `Transfer-Encoding` on GET/HEAD are rejected (413 for body-size limits, 400 for malformed framing)
- **No symlink following** — final and intermediate symlinks are denied unless `--follow-symlinks` is passed. On Unix, descriptor-relative traversal uses `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)` to prevent symlink detection bypass and to refuse to follow a symlink swapped into the path between the two calls. Even with `--follow-symlinks`, symlinks whose final canonical target escapes the root are denied. **Follow-symlinks mode is weaker and is not covered by the descriptor-relative hardening guarantee.**
- **No dotfiles served** — hidden files are excluded
- **No directory listing** — unless `--directory-listing` is passed
- **Unknown MIME as application/octet-stream** — safe fallback
- **Malformed request targets rejected** — invalid paths are not resolved
- **Logs sanitized** — paths/headers are sanitized before logging
- **Resource limits enabled** — connection and request limits are active

## Project status

**Plan 017 complete (SecureRoot public API).** The public `SecureRoot` API exposes audited filesystem resolution as capability-oriented primitives. Callers can resolve request-derived paths under the same descriptor-relative hardening (Unix safe defaults) without touching internal types. `ResolvedFile` wraps an already-opened file handle with safe metadata methods; `ResolvedDirectory` supports child resolution and policy-filtered listing. Weaker modes (follow-symlinks, non-Unix) are documented accurately in [docs/secure-root.md](docs/secure-root.md). See [plans/](plans/) for the full sequence.

## Supported platforms

| Platform | Status |
|----------|--------|
| Linux x86_64 | Supported, tested in CI — hardened (`openat(O_NOFOLLOW)`) |
| Linux aarch64 | Supported, tested in CI — hardened (`openat(O_NOFOLLOW)`) |
| macOS arm64 (Apple Silicon) | Supported, tested in CI — hardened (`openat(O_NOFOLLOW)`) |
| macOS x86_64 | Supported, tested in CI — hardened (`openat(O_NOFOLLOW)`) |
| Windows x86_64 | Supported; parser-level checks only, reparse-point hardening deferred |

Windows is functional but filesystem hardening (reparse-point/NTFS junction handling) is not yet complete. Do not use with untrusted public content on Windows.

## Python API

eggserve provides a minimal Python API for programmatic static file serving:

```python
from eggserve import serve_directory

# Serve current directory (blocking, safe defaults)
serve_directory(".")
```

For lifecycle control (tests, embedding):

```python
from eggserve import ServeConfig, ServerProcess

config = ServeConfig(directory="public", port=9000)
proc = ServerProcess(config)
proc.start()
proc.wait()
proc.stop()
```

Full API reference: [docs/python-api.md](docs/python-api.md)

**This is NOT an ASGI/WSGI server or a web framework.** It is a hardened static-serving primitive.

### Installation

```sh
# From source (requires Rust toolchain)
cargo install --path crates/eggserve-bin

# Via Python wheel (requires Python 3.8+)
pip install eggserve

# Or run directly with pipx
pipx run eggserve
```

## Local validation

Before pushing, run the full validation sequence:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check
```

Python tests and packaging smoke:

```sh
# Python API unit tests (no wheel build needed)
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_server -v

# Python packaging smoke test
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import ServeConfig, StaticPolicy, ServerProcess, serve_directory
print(ServeConfig())
PY
```

## Development

Development is plan-driven. All changes must be backed by a plan in the [plans/](plans/) directory. See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.
