#!/usr/bin/env bash
# test-python-wheel.sh — Single installed-wheel verification harness.
#
# Used by both routine Python CI and `scripts/verify.sh full`.
# Builds the release CLI, stages it, builds a wheel, installs into a
# fresh venv, runs smoke checks, and executes the test suite.

set -euo pipefail

export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON="${PYTHON:-python3.14}"

info()    { printf "\033[0;34m▸\033[0m %s\n" "$*"; }
success() { printf "\033[0;32m✓\033[0m %s\n" "$*"; }
fail()    { printf "\033[0;31m✗\033[0m %s\n" "$*"; }
die()     { printf "\033[0;31mFATAL:\033[0m %s\n" "$*" >&2; exit 1; }

VENV_DIR=""
DIST_DIR=""
STAGED=0

cleanup() {
    local rc=$?
    if [[ -n "$VENV_DIR" && -d "$VENV_DIR" ]]; then
        rm -rf "$VENV_DIR"
    fi
    if [[ -n "$DIST_DIR" && -d "$DIST_DIR" ]]; then
        rm -rf "$DIST_DIR"
    fi
    if [[ "$STAGED" -eq 1 ]]; then
        rm -rf "$REPO_ROOT/crates/eggserve-python/python/eggserve/bin"
    fi
    exit "$rc"
}
trap cleanup EXIT

# Prerequisites
command -v "$PYTHON" >/dev/null 2>&1 || die "$PYTHON not found."
"$PYTHON" -c "import sys; assert sys.version_info >= (3, 14)" || die "Python 3.14+ required."
command -v maturin >/dev/null 2>&1 || die "maturin not found. Install: pip install maturin==1.14.1"
command -v cargo >/dev/null 2>&1 || die "cargo not found."

# Build CLI binary
info "Building release CLI binary"
cargo build --release --locked -p eggserve-bin

# Stage CLI into package
info "Staging CLI binary into package"
mkdir -p "$REPO_ROOT/crates/eggserve-python/python/eggserve/bin"
if [[ "$(uname -s)" == *"MINGW"* || "$(uname -s)" == *"MSYS"* || "$(uname -s)" == *"CYGWIN"* ]]; then
    BINARY_NAME="eggserve.exe"
else
    BINARY_NAME="eggserve"
fi
cp "$REPO_ROOT/target/release/$BINARY_NAME" \
    "$REPO_ROOT/crates/eggserve-python/python/eggserve/bin/$BINARY_NAME"
chmod +x "$REPO_ROOT/crates/eggserve-python/python/eggserve/bin/$BINARY_NAME"
STAGED=1

# Build wheel
DIST_DIR="$(mktemp -d)"
info "Building wheel into $DIST_DIR"
(cd "$REPO_ROOT/crates/eggserve-python" && \
    maturin build --release --interpreter "$PYTHON" -o "$DIST_DIR")

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
"$VENV_PYTHON" -m eggserve --help >/dev/null 2>&1 || die "eggserve --help failed"
echo "  CLI --help OK"

"$VENV_PYTHON" -c "
from eggserve._bin import _find_binary
import os
b = _find_binary()
assert os.path.isfile(b), f'binary not found: {b}'
print(f'  bundled binary: {b}')
"

# Python test suite
info "Running Python test suite"
"$VENV_PYTHON" -m unittest discover \
    -s "$REPO_ROOT/crates/eggserve-python/tests" \
    -p 'test_*.py' \
    -v

success "All Python checks passed"
