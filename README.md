# eggserve

> eggserve is a hardened, Rust-backed replacement for the common `python -m http.server` static-serving workflow and a small foundation library for safe HTTP/static-serving primitives.

**eggserve is not a general web server, framework, ASGI/WSGI runtime, or Granian replacement.** It serves static files from a directory with secure-by-default behavior. That is all.

## What is eggserve?

eggserve provides two layers:

1. **CLI static server** — a hardened replacement for `python -m http.server` that serves static files with secure-by-default behavior. Bind to loopback, deny symlinks, reject dotfiles, disable directory listing — all unless explicitly opted in.

2. **Primitive library** — hardened Rust/Python building blocks for request-target parsing, path confinement, secure static resolution, and response planning. Use these to build custom serving logic without launching the binary.

Both layers share the same security policy and path confinement engine. The CLI is the simplest path; the primitives are for integration.

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

**Plans 000–046 complete; Plans 000–040 built the core; Plan 041 closes final release gates; Plans 042–045 establish the release evidence infrastructure; Plan 043 reconciles the product contract, capability matrix, and stability policy.** eggserve ships as a hardened CLI static server, a primitive library, and Python server primitives. The primitive library exposes path parsing, policy enforcement, secure root resolution, and response planning to both Rust and Python. Server primitives allow Python code to build HTTP servers while Rust owns socket I/O, HTTP parsing, file streaming, and timeout enforcement. CI gate names are normalized to match `release/criteria.toml` gate IDs, and evidence aggregation runs after all gate jobs. See [plans/](plans/) for the full sequence and [docs/release-checklist.md](docs/release-checklist.md) for evidence-backed release status.

Release gates are defined in [release/criteria.toml](release/criteria.toml) and validated by [scripts/release_criteria.py](scripts/release_criteria.py). See [docs/release-process.md](docs/release-process.md) for the release operator guide.

**Property testing and fuzzing:** Nine fuzz targets cover path parsing, URL parsing, range/conditional headers, platform checks, and request validation. Deterministic property tests (proptest) run in normal CI. Scheduled fuzz runs run weekly. See [docs/fuzzing.md](docs/fuzzing.md).

**API stability:** Every exported Rust and Python item is classified as stable, experimental, or internal. See [docs/api-stability.md](docs/api-stability.md) for the full inventory and [docs/release-contract.md](docs/release-contract.md) for behavioral guarantees.

## Supported platforms

| Platform | Status |
|----------|--------|
| Linux x86_64 | Supported; hardened (`openat(O_NOFOLLOW)`), validated by the release matrix |
| Linux aarch64 | Release target; hardened (`openat(O_NOFOLLOW)`), release evidence required |
| macOS arm64 (Apple Silicon) | Supported; hardened (`openat(O_NOFOLLOW)`), validated by the release matrix |
| macOS x86_64 | Release target; hardened (`openat(O_NOFOLLOW)`), release evidence required |
| Windows x86_64 | Functional wheel/binary support; parser-level checks only, reparse-point hardening deferred |

Windows is functional but filesystem hardening (reparse-point/NTFS junction handling) is not yet complete. Do not use with untrusted public content on Windows.

## Python API

eggserve provides a Python API with two layers:

**Native primitives** (PyO3-backed Rust bindings) — path parsing, policy enforcement, secure root resolution, and response planning without launching the binary:

```python
from eggserve import SecureRoot, StaticPolicy

root = SecureRoot("public", policy=StaticPolicy())
resource = root.resolve_path("/assets/app.css")
if resource.is_file:
    plan = resource.file.plan_response("GET")
    print(plan.status, plan.body_kind)  # 200 file_full
```

**Server primitives** — build HTTP servers with Rust-owned I/O:

```python
from eggserve import Server, ServerSecureRoot

root = ServerSecureRoot(".")
with Server(root=root) as server:
    print(f"Serving on {server.addr}")
```

**Handler callback** — intercept requests with a Python handler:

```python
from eggserve import Server, ServerSecureRoot, Request, Response

root = ServerSecureRoot(".")

def handler(request: Request) -> Response:
    if request.path == "/health":
        return Response.text(200, "ok")
    return Response.empty(404)

with Server(root=root, handler=handler) as server:
    print(f"Serving on {server.addr}")
```

**Subprocess lifecycle** — full HTTP serving via the Rust binary:

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

# Via Python wheel (CPython 3.14 on Linux, macOS, or Windows)
pip install eggserve

# Or run directly with pipx
pipx run eggserve
```

## Examples

See the [examples/](examples/) directory:

- `examples/python_basic.py` — minimal subprocess API usage
- `examples/python_dynamic_static.py` — dynamic health endpoint + static assets using primitives
- `examples/python_safe_download.py` — safe file download handler with user-provided names

Rust example: `crates/eggserve-core/examples/rust_primitives.rs` (run with `cargo run --example rust_primitives -p eggserve-core`)

## Local validation

Before pushing, run the full validation sequence:

```sh
cargo fmt --all -- --check                                 # format check
cargo clippy --workspace --all-targets -- -D warnings      # lint
cargo test --workspace                                     # tests
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                  # TLS tests
cargo test -p eggserve-core --features client              # client feature tests
cargo test -p eggserve-core --test http_wire_correctness   # raw wire tests
cargo test -p eggserve-core --test http_primitives_integration  # HTTP integration
cargo test -p eggserve-bin --test production_path          # production path tests
cargo test -p eggserve-core --test corpus_replay           # fuzz corpus replay
bash scripts/install-cargo-tools.sh                        # install and verify pinned tools
cargo audit                                                # vulnerability check
cargo deny check                                           # license/policy check
bash scripts/verify-cargo-packages.sh                      # crates.io/local-registry package gates
```

Or use the unified validation entry point:

```sh
./scripts/release-validate.sh fast                 # routine development
./scripts/release-validate.sh full                 # pre-release validation
./scripts/release-validate.sh gate http.raw-wire   # run a single gate
./scripts/release-validate.sh evidence --output ./ev  # copy evidence to output path
# Aggregate and validate evidence bundle:
python3 scripts/release_criteria.py aggregate --criteria release/criteria.toml --evidence <evidence-dir> --sha <commit-sha>
```

Python tests and packaging smoke:

```sh
# Native primitives tests (requires built wheel or maturin develop)
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_primitives -v

# Server primitives tests (requires built wheel)
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_server_primitives -v

# Python API unit tests (no wheel build needed)
PYTHONPATH=crates/eggserve-python/python \
  python -m unittest eggserve.test_server -v

# Python packaging smoke test (stage the CLI binary into the wheel first)
cargo build --release --locked -p eggserve-bin
mkdir -p crates/eggserve-python/python/eggserve/bin
cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
chmod +x crates/eggserve-python/python/eggserve/bin/eggserve
cd crates/eggserve-python
maturin build --release --interpreter python3.14 -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import ServeConfig, StaticPolicy, ServerProcess, serve_directory, Server, ServerSecureRoot
print(ServeConfig())
PY

# Installed-wheel validation (no source-tree imports)
cd crates/eggserve-python/packaging-tests
bash run_all.sh ../dist/*.whl python3.14
```

## Development

Development is plan-driven. All changes must be backed by a plan in the [plans/](plans/) directory. See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.
