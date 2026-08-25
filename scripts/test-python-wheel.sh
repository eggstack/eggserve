#!/usr/bin/env bash
# test-python-wheel.sh — Single installed-wheel verification harness.
#
# Used by both routine Python CI and `scripts/verify.sh full`.
# Builds a wheel, installs into a fresh venv, runs smoke checks, and executes
# the test suite.

set -euo pipefail

export PYTHONNOUSERSITE=1
unset PYTHONPATH

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON="${PYTHON:-python3.14}"

info()    { printf "\033[0;34m▸\033[0m %s\n" "$*"; }
success() { printf "\033[0;32m✓\033[0m %s\n" "$*"; }
fail()    { printf "\033[0;31m✗\033[0m %s\n" "$*"; }
die()     { printf "\033[0;31mFATAL:\033[0m %s\n" "$*" >&2; exit 1; }

VENV_DIR=""
DIST_DIR=""

cleanup() {
    local rc=$?
    if [[ -n "$VENV_DIR" && -d "$VENV_DIR" ]]; then
        rm -rf "$VENV_DIR"
    fi
    if [[ -n "$DIST_DIR" && -d "$DIST_DIR" ]]; then
        rm -rf "$DIST_DIR"
    fi
    find "$REPO_ROOT/crates/eggserve-python" -type d -name '__pycache__' -exec rm -rf {} + 2>/dev/null || true
    find "$REPO_ROOT/crates/eggserve-python" -name '*.pyc' -delete 2>/dev/null || true
    exit "$rc"
}
trap cleanup EXIT

# Prerequisites
command -v "$PYTHON" >/dev/null 2>&1 || die "$PYTHON not found."
"$PYTHON" -c "
import sys
v = sys.version_info
assert v >= (3, 11), f'Python {v.major}.{v.minor} < 3.11'
" || die "Python >=3.11 required."
"$PYTHON" -m maturin --version >/dev/null 2>&1 || die "maturin not found. Install: pip install maturin==1.14.1"
command -v cargo >/dev/null 2>&1 || die "cargo not found."

# Build wheel
DIST_DIR="$(mktemp -d)"
info "Building wheel into $DIST_DIR"
(cd "$REPO_ROOT/crates/eggserve-python" && \
    "$PYTHON" -m maturin build --profile dist --interpreter "$PYTHON" -o "$DIST_DIR")
WHEEL_PATH="$(printf '%s\n' "$DIST_DIR"/*.whl)"
if WHEEL_SIZE="$(stat --printf='%s' "$WHEEL_PATH" 2>/dev/null)"; then
    :
else
    # macOS and other BSD systems use a different stat interface.
    WHEEL_SIZE="$(stat -f '%z' "$WHEEL_PATH")"
fi
info "Wheel size: $WHEEL_SIZE bytes"

# Create venv and install wheel
VENV_DIR="$(mktemp -d)"
info "Creating virtual environment in $VENV_DIR"
"$PYTHON" -m venv "$VENV_DIR"
VENV_PYTHON="$VENV_DIR/bin/python"

info "Installing wheel"
"$VENV_PYTHON" -m pip install --disable-pip-version-check -q "$DIST_DIR"/*.whl

# Import boundary assertion
info "Verifying import boundary"
"$VENV_PYTHON" <<'PYEOF'
from pathlib import Path
import sys
import eggserve
import eggserve._native

ef = Path(eggserve.__file__).resolve()
nf = Path(eggserve._native.__file__).resolve()
maj = sys.version_info.major
minr = sys.version_info.minor
site_packages = Path(sys.prefix) / 'lib' / f'python{maj}.{minr}' / 'site-packages'

assert site_packages in ef.parents, f'eggserve.__file__ not in site-packages: {ef}'
assert site_packages in nf.parents, f'eggserve._native.__file__ not in site-packages: {nf}'
print(f'  eggserve: {ef}')
print(f'  _native:  {nf}')
PYEOF

# Smoke checks
info "Running smoke checks"
"$VENV_PYTHON" -c "import eggserve, eggserve._native; print('  imports OK')"
"$VENV_PYTHON" -m eggserve --help >/dev/null 2>&1 || die "python -m eggserve --help failed"
echo "  python -m eggserve --help OK"

# Verify installed console script exists and works
info "Verifying installed eggserve console script"
"$VENV_DIR/bin/eggserve" --help >/dev/null 2>&1 || die "eggserve --help failed"
echo "  eggserve --help OK"

# Verify the console script uses the native extension (not a binary from PATH)
"$VENV_PYTHON" -c "
import shutil, sys
eggserve_cmd = shutil.which('eggserve', path='$VENV_DIR/bin')
assert eggserve_cmd is not None, 'eggserve command not found in venv'
print(f'  installed command: {eggserve_cmd}')
"

# Canonical Python example smoke checks. The examples are loaded from the
# checkout but run entirely against the installed wheel in this venv.
info "Running canonical Python example smoke"
REPO_ROOT="$REPO_ROOT" "$VENV_PYTHON" - <<'PYEOF'
import http.client
import os
import runpy
from pathlib import Path

repo = Path(os.environ["REPO_ROOT"])


def request(server, method, path):
    connection = http.client.HTTPConnection(*server.server_address, timeout=5)
    try:
        connection.request(method, path, headers={"Connection": "close"})
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


static = runpy.run_path(str(repo / "examples" / "python_http_server_static.py"))
server = static["create_server"](("127.0.0.1", 0), repo / "examples" / "site")
try:
    assert server._native_fast_path, "stock static example must use the native fast path"
    assert request(server, "GET", "/")[0] == 200
    assert request(server, "HEAD", "/assets/example.txt") == (200, b"")
    assert request(server, "GET", "/.hidden-example")[0] == 403
finally:
    server.server_close()

custom = runpy.run_path(str(repo / "examples" / "python_custom_handler.py"))
server = custom["create_server"](("127.0.0.1", 0))
try:
    assert request(server, "GET", "/health") == (200, b"ok\n")
    assert request(server, "GET", "/missing") == (404, b"not found\n")
finally:
    server.server_close()

custom_headers = runpy.run_path(str(repo / "examples" / "python_custom_headers.py"))
server = custom_headers["create_server"](("127.0.0.1", 0), repo / "examples" / "site")
try:
    assert not server._native_fast_path, "metadata subclass must use the callback path"
    connection = http.client.HTTPConnection(*server.server_address, timeout=5)
    try:
        connection.request("GET", "/assets/example.txt", headers={"Connection": "close"})
        response = connection.getresponse()
        body = response.read()
        status = response.status
        served_by = response.getheader("X-Served-By")
        cache_control = response.getheader("Cache-Control")
    finally:
        connection.close()
    assert status == 200 and b"example" in body.lower(), (status, body[:80])
    assert served_by == "eggserve", f"missing X-Served-By: {served_by!r}"
    assert cache_control == "no-cache", f"missing Cache-Control: {cache_control!r}"
finally:
    server.server_close()

print("  canonical Python examples passed")
PYEOF

# Exercise the documented CLI example against its committed text fixture.
info "Running CLI example fixture smoke"
REPO_ROOT="$REPO_ROOT" EGG_SERVE_BIN="$VENV_DIR/bin/eggserve" "$VENV_PYTHON" - <<'PYEOF'
import http.client
import os
import signal
import socket
import subprocess
import time
from pathlib import Path

repo = Path(os.environ["REPO_ROOT"])
binary = os.environ["EGG_SERVE_BIN"]
with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]

process = subprocess.Popen(
    [binary, "--directory", str(repo / "examples" / "site"),
     "--bind", f"127.0.0.1:{port}", "--log-format", "none"],
)
try:
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise SystemExit(f"CLI example exited during startup: {process.returncode}")
        try:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            connection.request("GET", "/")
            response = connection.getresponse()
            body = response.read()
            connection.close()
            if response.status == 200 and b"EggServe example site" in body:
                break
        except OSError:
            time.sleep(0.05)
    else:
        raise SystemExit("CLI example did not become ready")

    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
    connection.request("GET", "/.hidden-example")
    response = connection.getresponse()
    response.read()
    connection.close()
    if response.status != 403:
        raise SystemExit(f"CLI example served hidden file: {response.status}")
finally:
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
        raise SystemExit("CLI example did not shut down")
    if process.returncode != 0:
        raise SystemExit(f"CLI example exited with {process.returncode}")
print("  CLI example fixture passed")
PYEOF

# Fixture serving via installed command
info "Running fixture serving via installed eggserve command"
"$VENV_PYTHON" <<'PYEOF'
import http.client
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

with tempfile.TemporaryDirectory(prefix="eggserve-smoke-") as root:
    fixture = b"eggserve release smoke\n"
    (Path(root) / "smoke.txt").write_bytes(fixture)
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]

    process = subprocess.Popen(
        [sys.executable, "-m", "eggserve",
         "--directory", root,
         "--bind", f"127.0.0.1:{port}",
         "--log-format", "none"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        deadline = time.monotonic() + 5
        response = None
        while time.monotonic() < deadline:
            if process.poll() is not None:
                stderr = process.stderr.read().decode() if process.stderr else ""
                raise SystemExit(f"server exited during startup: {stderr}")
            try:
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=0.5)
                connection.request("GET", "/smoke.txt")
                response = connection.getresponse()
                body = response.read()
                connection.close()
                break
            except OSError:
                time.sleep(0.05)
        if response is None:
            raise SystemExit("server did not become ready")
        if response.status != 200 or body != fixture:
            raise SystemExit(f"unexpected smoke response: {response.status} {body!r}")
        print(f"  GET /smoke.txt => {response.status} (exact fixture body)")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
PYEOF

# ServerProcess fixture via subprocess module
info "Running ServerProcess subprocess fixture"
"$VENV_PYTHON" <<'PYEOF'
import http.client
import socket
import sys
import tempfile
import time
from pathlib import Path

from eggserve.server import ServeConfig, ServerProcess, StaticPolicy

with tempfile.TemporaryDirectory(prefix="eggserve-proc-") as root:
    fixture = b"serverprocess smoke\n"
    (Path(root) / "proc.txt").write_bytes(fixture)
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]

    config = ServeConfig(
        directory=root,
        bind="127.0.0.1",
        port=port,
        log_format="none",
    )
    proc = ServerProcess(config)
    proc.start()
    try:
        deadline = time.monotonic() + 5
        response = None
        while time.monotonic() < deadline:
            if not proc.is_running:
                raise SystemExit("server process exited during startup")
            try:
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=0.5)
                connection.request("GET", "/proc.txt")
                response = connection.getresponse()
                body = response.read()
                connection.close()
                break
            except OSError:
                time.sleep(0.05)
        if response is None:
            raise SystemExit("server did not become ready")
        if response.status != 200 or body != fixture:
            raise SystemExit(f"unexpected response: {response.status} {body!r}")
        print(f"  ServerProcess GET /proc.txt => {response.status}")
        assert proc.pid is not None, "pid should be set"
        assert proc.is_running, "should be running"
    finally:
        proc.stop(timeout=5)
    print("  ServerProcess stopped cleanly")
PYEOF

# Python test suite
info "Running Python test suite"
"$VENV_PYTHON" -m unittest discover \
    -s "$REPO_ROOT/crates/eggserve-python/tests" \
    -p 'test_*.py' \
    -v

success "All Python checks passed"
