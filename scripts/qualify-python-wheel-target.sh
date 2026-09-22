#!/usr/bin/env bash
# qualify-python-wheel-target.sh — Portable real-device wheel qualification.
#
# Plan 266 Track D. Runs on Raspberry Pi OS / Debian / Ubuntu ARM64,
# Armbian/Debian on Le Potato or similar AArch64 boards, Raspberry Pi OS
# ARMv7, and Alpine ARM64/ARMv7 where available. Must not require root.
#
# Inputs (either a local wheel or a published package):
#   --wheel PATH               Local wheel file from a downloaded release artifact.
#   --package NAME --version V --index-url URL
#                              Post-publication qualification from an index.
#                              Implies binary-only resolution.
#
# Records: package version and wheel filename, Python version,
# platform.machine(), OS release, libc family/version where discoverable,
# import result, CLI/module result, real loopback server smoke result.

set -euo pipefail

WHEEL=""
PACKAGE="eggserve"
VERSION=""
INDEX_URL="https://pypi.org/simple/"

usage() {
    cat <<EOF
Usage: $0 (--wheel PATH | --package NAME --version V [--index-url URL])

Examples:
  $0 --wheel ./eggserve-0.2.0-cp311-abi3-manylinux_2_17_aarch64.whl
  $0 --package eggserve --version 0.2.0
  $0 --package eggserve --version 0.2.0 --index-url https://test.pypi.org/simple/
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --wheel) WHEEL="$2"; shift 2 ;;
        --package) PACKAGE="$2"; shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        --index-url) INDEX_URL="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
    esac
done

if [[ -n "$WHEEL" && -n "$VERSION" ]]; then
    echo "specify either --wheel or --package/--version, not both" >&2
    exit 2
fi
if [[ -z "$WHEEL" && -z "$VERSION" ]]; then
    echo "specify --wheel PATH or --package NAME --version V" >&2
    usage
    exit 2
fi
if [[ -n "$WHEEL" && ! -f "$WHEEL" ]]; then
    echo "wheel not found: $WHEEL" >&2
    exit 2
fi
command -v python3 >/dev/null 2>&1 || { echo "python3 not found" >&2; exit 2; }

# Rootless work area; never commit hostnames, credentials, or tokens.
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "=== device evidence ==="
python3 - <<'PYEOF'
import platform
print(f"python: {platform.python_version()} ({platform.python_implementation()})")
print(f"machine: {platform.machine()}")
print(f"system: {platform.system()} {platform.release()}")
try:
    import os
    for candidate in ("/etc/os-release",):
        if os.path.exists(candidate):
            with open(candidate) as f:
                for line in f:
                    if line.startswith(("PRETTY_NAME=", "VERSION_ID=")):
                        print(f"os-release: {line.strip()}")
except OSError as exc:
    print(f"os-release: unavailable ({exc})")
try:
    import ctypes
    libc = ctypes.CDLL(None)
    try:
        gnu_get_libc = libc.gnu_get_libc_version
        gnu_get_libc.restype = ctypes.c_char_p
        print(f"libc: glibc {gnu_get_libc().decode()}")
    except AttributeError:
        print("libc: non-glibc (musl or other; see os-release)")
except Exception as exc:  # noqa: BLE001 - evidence reporting must not fail
    print(f"libc: undiscoverable ({exc})")
PYEOF
uname -a

echo "=== install ==="
VENV="$WORK_DIR/venv"
python3 -m venv "$VENV" 2>/dev/null || python3 -m venv --without-pip "$VENV"
VENV_PY="$VENV/bin/python"
if [[ -n "$WHEEL" ]]; then
    "$VENV_PY" -m pip install --disable-pip-version-check -q --only-binary=:all: "$WHEEL" \
        || { echo "wheel install failed (pip missing? install pip in the venv first)" >&2; exit 1; }
    echo "wheel: $(basename "$WHEEL")"
else
    "$VENV_PY" -m pip install --disable-pip-version-check -q --only-binary=:all: \
        --index-url "$INDEX_URL" "$PACKAGE==$VERSION" \
        || { echo "index install failed" >&2; exit 1; }
    "$VENV_PY" -m pip show -f "$PACKAGE" | grep -E "^(Name|Version|Location)" || true
fi

echo "=== import ==="
"$VENV_PY" -c "import eggserve, eggserve._native; print(f'version={eggserve.__version__}')"

echo "=== CLI / module ==="
"$VENV_PY" -m eggserve --help >/dev/null && echo "python -m eggserve --help OK"
"$VENV/bin/eggserve" --help >/dev/null && echo "eggserve --help OK"

echo "=== loopback smoke ==="
PATH="$VENV/bin:$PATH" "$VENV_PY" "$(dirname "$0")/release_smoke.py"

echo "=== confinement smoke ==="
"$VENV_PY" - <<'PYEOF'
import http.client
import tempfile
from pathlib import Path

from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from functools import partial

with tempfile.TemporaryDirectory(prefix="eggserve-sbc-") as root:
    (Path(root) / "index.html").write_bytes(b"sbc qualification\n")
    (Path(root) / ".hidden").write_bytes(b"secret\n")
    handler = partial(SimpleHTTPRequestHandler, directory=root)
    with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        host, port = server.server_address

        def get(path):
            conn = http.client.HTTPConnection(host, port, timeout=5)
            try:
                conn.request("GET", path, headers={"Connection": "close"})
                resp = conn.getresponse()
                return resp.status, resp.read()
            finally:
                conn.close()

        assert get("/")[0] == 200, "root listing/file must serve"
        assert get("/.hidden")[0] == 403, "dotfiles must stay denied"
        print("  confinement smoke: 200 on public, 403 on dotfile")
PYEOF

echo "=== qualification complete ==="
