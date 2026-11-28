#!/usr/bin/env bash
# check-python-free-threaded-compile.sh — compile the Python extension against
# a free-threaded CPython (Plan 333, work package D).
#
# Requires a `-t` interpreter. Set PYTHON=/path/to/python3.14t to override;
# the default matches `uv python install 3.14t` and actions/setup-python
# `3.14t` on PATH.
#
# PYO3_PYTHON is derived from the resolved interpreter so the PyO3 build-script
# probe and the cargo build target the same free-threaded ABI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PYTHON="${PYTHON:-python3.14t}"

die() { printf 'FATAL: %s\n' "$*" >&2; exit 1; }

command -v "$PYTHON" >/dev/null 2>&1 || die \
    "free-threaded interpreter not found: $PYTHON (install with 'uv python install 3.14t' or actions/setup-python '3.14t', or set PYTHON=...)"

"$PYTHON" -c '
import sys
import sysconfig

if not sysconfig.get_config_var("Py_GIL_DISABLED"):
    raise SystemExit(f"interpreter is not free-threaded (Py_GIL_DISABLED unset): {sys.executable}")
' || die "$PYTHON is not a free-threaded (3.14t) build"

PYTHON_ABS="$("$PYTHON" -c 'import sys; print(sys.executable)')"
PYTHON_VERSION="$("$PYTHON" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"

echo "Free-threaded compile check"
echo "  interpreter: $PYTHON_ABS"
echo "  version:     $PYTHON_VERSION (Py_GIL_DISABLED)"

PYO3_PYTHON="$PYTHON_ABS" cargo check \
    --manifest-path "$REPO_ROOT/crates/eggserve-python/Cargo.toml" \
    --locked
