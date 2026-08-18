#!/usr/bin/env bash
# Installed-binary qualification smoke test.
#
# Tests the built binary in an isolated environment, simulating an
# installed artifact. Retained after Plan 091 as a manual deep check.
#
# Usage: bash tests/installed-binary-qual.sh [binary-path]
#
# If no binary path is given, builds the binary first.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap cleanup EXIT

cleanup() {
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    rm -rf "$WORK_DIR"
}

SERVER_PID=""

PASS=0
FAIL=0

run_test() {
    local name="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "  PASS: $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $name"
        FAIL=$((FAIL + 1))
    fi
}

# --- Locate or build the binary ---
if [[ -n "${1:-}" ]]; then
    EGGSERVE_BIN="$1"
    if [[ ! -x "$EGGSERVE_BIN" ]]; then
        echo "FAIL: binary not found at $EGGSERVE_BIN"
        exit 1
    fi
else
    echo "Building eggserve binary..."
    cargo build --release -p eggserve-bin --quiet 2>/dev/null
    EGGSERVE_BIN="${REPO_ROOT}/target/release/eggserve"
    if [[ ! -x "$EGGSERVE_BIN" ]]; then
        # Try .exe on Windows
        EGGSERVE_BIN="${REPO_ROOT}/target/release/eggserve.exe"
    fi
    if [[ ! -x "$EGGSERVE_BIN" ]]; then
        echo "FAIL: could not find built binary"
        exit 1
    fi
fi

echo "Binary: $EGGSERVE_BIN"
echo ""

# --- Create isolated test environment ---
ISOLATED_DIR="$WORK_DIR/isolated"
mkdir -p "$ISOLATED_DIR"
cp "$EGGSERVE_BIN" "$ISOLATED_DIR/eggserve"
chmod +x "$ISOLATED_DIR/eggserve"

# Create test content
mkdir -p "$ISOLATED_DIR/www/subdir"
echo "hello world" > "$ISOLATED_DIR/www/hello.txt"
echo "nested content" > "$ISOLATED_DIR/www/subdir/nested.txt"
dd if=/dev/urandom of="$ISOLATED_DIR/www/large.bin" bs=1024 count=64 2>/dev/null

echo "=== Installed Binary Qualification Tests ==="
echo ""

# --- Test 1: CLI help ---
echo "Test 1: CLI help"
run_test "eggserve --help exits 0" "$ISOLATED_DIR/eggserve" --help

# --- Test 2: CLI version ---
echo "Test 2: CLI version"
run_test "eggserve --version exits 0" "$ISOLATED_DIR/eggserve" --version

# --- Test 3: Serve a directory ---
echo "Test 3: Serve directory and fetch file"
PORT=$(shuf -i 50000-60000 -n 1)
"$ISOLATED_DIR/eggserve" --bind "127.0.0.1:${PORT}" --directory "$ISOLATED_DIR/www" &
SERVER_PID=$!
sleep 1

if kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "  PASS: server started"
    PASS=$((PASS + 1))
else
    echo "  FAIL: server failed to start"
    FAIL=$((FAIL + 1))
fi

# --- Test 4: GET request ---
echo "Test 4: GET returns 200"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:${PORT}/hello.txt" 2>/dev/null || echo "000")
if [[ "$STATUS" == "200" ]]; then
    echo "  PASS: GET returns 200"
    PASS=$((PASS + 1))
else
    echo "  FAIL: GET returned $STATUS"
    FAIL=$((FAIL + 1))
fi

# --- Test 5: HEAD request ---
echo "Test 5: HEAD returns 200"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" -I "http://127.0.0.1:${PORT}/hello.txt" 2>/dev/null || echo "000")
if [[ "$STATUS" == "200" ]]; then
    echo "  PASS: HEAD returns 200"
    PASS=$((PASS + 1))
else
    echo "  FAIL: HEAD returned $STATUS"
    FAIL=$((FAIL + 1))
fi

# --- Test 6: Range request ---
echo "Test 6: Range returns 206"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" -H "Range: bytes=0-4" "http://127.0.0.1:${PORT}/hello.txt" 2>/dev/null || echo "000")
if [[ "$STATUS" == "206" ]]; then
    echo "  PASS: Range returns 206"
    PASS=$((PASS + 1))
else
    echo "  FAIL: Range returned $STATUS"
    FAIL=$((FAIL + 1))
fi

# --- Test 7: 404 ---
echo "Test 7: Missing file returns 404"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:${PORT}/nonexistent.txt" 2>/dev/null || echo "000")
if [[ "$STATUS" == "404" ]]; then
    echo "  PASS: 404 returned"
    PASS=$((PASS + 1))
else
    echo "  FAIL: expected 404, got $STATUS"
    FAIL=$((FAIL + 1))
fi

# --- Test 8: Path confinement (traversal denied) ---
echo "Test 8: Path traversal denied"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:${PORT}/../etc/passwd" 2>/dev/null || echo "000")
if [[ "$STATUS" == "400" || "$STATUS" == "403" || "$STATUS" == "404" ]]; then
    echo "  PASS: traversal denied ($STATUS)"
    PASS=$((PASS + 1))
else
    echo "  FAIL: traversal returned $STATUS (should be 400/403/404)"
    FAIL=$((FAIL + 1))
fi

# --- Test 9: Safe defaults (no directory listing by default) ---
echo "Test 9: No directory listing by default"
BODY=$(curl -s "http://127.0.0.1:${PORT}/subdir/" 2>/dev/null || echo "")
if echo "$BODY" | grep -qi "index of\|directory listing\|<title>Index"; then
    echo "  FAIL: directory listing shown without --directory-listing"
    FAIL=$((FAIL + 1))
else
    echo "  PASS: no directory listing"
    PASS=$((PASS + 1))
fi

# --- Cleanup handled by EXIT trap ---

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [[ $FAIL -gt 0 ]]; then
    echo "FAIL: installed binary qualification tests failed"
    exit 1
fi

echo "PASS: All installed binary qualification tests passed"
exit 0
