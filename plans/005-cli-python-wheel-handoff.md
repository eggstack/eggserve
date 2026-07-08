# Plan 005: CLI parity and Python wheel launcher

## Goal

Package eggserve for the user-facing workflow that motivated the project: a Python-standard-library-shaped command that is easy to install and safe by default. This milestone should produce a Rust CLI binary and a Python wheel that supports `python -m eggserve` and exposes familiar `http.server`-like usage without pretending to be behaviorally identical to Python's unsafe development server.

The Python layer should be thin in this milestone. Do not expose a broad Python API yet. The goal is packaging and ergonomics, not embedding a dynamic server framework into Python.

## Scope

In scope:

```text
Rust CLI argument parser
safe default CLI behavior
explicit public-bind and unsafe-convenience flags
startup effective-policy display
Python package skeleton
python -m eggserve launcher
wheel build through maturin or equivalent
platform smoke tests
README install/usage documentation
package metadata
release checklist draft
```

Out of scope:

```text
full Python API
PyO3 request callbacks
ASGI/WSGI support
TLS unless already implemented as a feature
Range requests
CORS/authentication
config-file format unless trivial and already planned
```

## CLI design

The CLI should be familiar to `http.server` users while making safer defaults visible.

Target commands:

```bash
eggserve
eggserve 8000
eggserve --directory public
eggserve --directory public 8000
eggserve --bind 127.0.0.1 --port 8000
eggserve --addr 127.0.0.1:8000
eggserve --directory public --public
eggserve --directory public --directory-listing
eggserve --directory public --follow-symlinks
eggserve --directory public --allow-dotfiles
```

Recommended defaults:

```text
port: 8000
bind: 127.0.0.1
root: current working directory
public serving: disabled unless explicit
methods: GET, HEAD
directory listing: disabled
symlinks: denied
dotfiles: denied
```

If `--bind 0.0.0.0` or an equivalent public address is supplied without `--public`, either fail with a clear message or print a highly visible warning. Failing is safer. If compatibility pressure is high, allow it with a warning but require `--public` before 1.0.

## CLI flags

Initial stable-ish flags:

```text
--directory DIR        root directory to serve
--bind HOST           bind host, default 127.0.0.1
--port PORT           bind port, default 8000
--addr HOST:PORT      full socket address, overrides bind/port
--public              acknowledge public exposure intent
--directory-listing   enable generated directory listings
--follow-symlinks     allow symlink following according to policy
--allow-dotfiles      allow dotfile serving
--log-format text|json|none
--quiet               suppress startup banner except errors
--version
--help
```

Potential later flags, not required now:

```text
--config eggserve.toml
--index index.html
--max-connections N
--max-file-streams N
--header-timeout DURATION
--idle-timeout DURATION
--tls-cert PATH
--tls-key PATH
```

If limit flags are exposed now, they must be enforced. Do not expose knobs that do nothing.

## Argument parser choice

Prefer a small parser unless the UX demands `clap`.

Decision rule:

```text
If only the above flags are needed, use a small parser or pico-args.
If nested help, value validation, shell completion, and richer errors are required, consider clap.
Document the dependency tradeoff in docs/dependency-policy.md.
```

Given eggserve's auditability goal, avoid a large CLI dependency until necessary.

## Startup output

On startup, show effective policy unless `--quiet` is set:

```text
eggserve 0.x
Serving root: /absolute/path/public
Listening: http://127.0.0.1:8000
Methods: GET, HEAD
Directory listing: disabled
Symlinks: denied
Dotfiles: denied
Max connections: 1024
Max file streams: 128
```

If unsafe opt-ins are enabled, show them explicitly:

```text
WARNING: public bind enabled
WARNING: symlink following enabled
WARNING: dotfile serving enabled
```

Do not bury these in debug logs.

## Python package design

Package name should be `eggserve` if available. Python import namespace:

```python
import eggserve
```

Primary user command:

```bash
python -m eggserve --directory public 8000
```

Initial Python package structure:

```text
crates/eggserve-python/
  pyproject.toml
  README.md or package README reference
  python/eggserve/
    __init__.py
    __main__.py
    server.py
    _bin.py
```

The Python wrapper should locate and execute the packaged Rust binary. It should forward CLI arguments exactly. On Unix, prefer replacing the Python process with the binary when practical. On Windows, subprocess forwarding is acceptable if signal handling remains reasonable.

The Python wrapper should not reimplement policy, parse paths, or serve files. The Rust binary is the source of truth.

## Wheel build strategy

Use `maturin` or equivalent to produce wheels containing the Rust binary. Prefer the simplest packaging mode that works reliably across platforms. Avoid PyO3 if no extension module is needed; a binary-in-wheel approach is sufficient for the CLI milestone.

Initial target platforms:

```text
macOS arm64
macOS x86_64
Linux x86_64
Linux aarch64 if CI support is available
Windows x86_64
```

If not all targets are ready, document supported targets honestly. Do not ship untested Windows path behavior as production-ready.

## Python module behavior

`python -m eggserve` should behave like invoking `eggserve` directly.

`eggserve.__version__` should be available.

Optional helper:

```python
def main() -> int:
    ...
```

Do not add `serve_directory()` yet unless the core API and lifecycle are stable. That belongs to a later Python API milestone.

## Documentation updates

Update README with:

```text
pip install eggserve
python -m eggserve
python -m eggserve 8000
python -m eggserve --directory public
safe defaults table
compatibility notes versus http.server
public serving warning
unsupported features
```

Add `docs/python-packaging.md` covering:

```text
wheel build process
supported platforms
how the Python launcher finds the binary
why the Python API is intentionally narrow for now
```

Add `docs/cli.md` covering all flags and defaults.

## Tests

Rust CLI tests:

```text
--help exits successfully
--version exits successfully
default config binds loopback:8000
positional port is parsed
--directory is parsed
--addr overrides bind/port
unsafe flags update policy
invalid port fails
public bind without --public fails or warns according to chosen policy
```

Python packaging tests:

```text
python -m eggserve --help works from installed wheel
python -m eggserve --version works from installed wheel
python -m eggserve forwards arguments to binary
packaged binary exists in wheel
wheel smoke test can serve a temp directory
```

End-to-end smoke test:

```text
create temp directory with file.txt
start python -m eggserve --directory temp --port ephemeral
curl or Python stdlib client GET /file.txt
assert body matches
terminate process cleanly
```

## Release checklist draft

Add a non-final release checklist:

```text
cargo fmt/check/test pass
Python wheel smoke tests pass
README usage verified
safe defaults visible in startup banner
unsupported features documented
dependency tree reviewed
version synchronized across Cargo/Python package
artifacts built for supported platforms
```

## Acceptance criteria

This milestone is complete when:

```text
eggserve binary has a usable CLI with safe defaults.
python -m eggserve works from an installed wheel.
CLI output shows effective policy.
Unsafe public/dotfile/symlink/listing behavior requires explicit flags.
README and docs explain installation and scope.
Wheel smoke tests serve a real fixture directory.
No broad Python API or app-server behavior was introduced.
```

## Review checklist

Before merging, verify:

```text
The Python wrapper does not duplicate Rust serving logic.
No PyO3 callback/request-handler API was added.
No ASGI/WSGI language appears in public docs except as non-goals.
Public bind behavior is explicit.
Unsafe flags are visible in startup output.
All exposed CLI limit flags are actually enforced.
Wheel build does not accidentally include unnecessary large artifacts.
```

## Handoff notes

After this milestone, eggserve should be installable and usable by Python users as a safer `http.server`-shaped tool. The next work should focus on CI/fuzz/security validation and platform matrix hardening before adding convenience features.
