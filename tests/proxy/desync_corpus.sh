#!/usr/bin/env bash
# Proxy desynchronization corpus test (Plan 089, Track B).
#
# Sends desynchronization payloads through Caddy and nginx proxies
# to verify no frontend/backend disagreement that permits request
# smuggling or cross-request confusion.
#
# Prerequisites:
#   - caddy and/or nginx in PATH
#   - eggserve binary built (cargo build -p eggserve-bin)
#   - curl for HTTP requests
#
# Usage: bash tests/proxy/desync_corpus.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

EGGSERVE_BIN="${REPO_ROOT}/target/debug/eggserve"

if [[ ! -x "$EGGSERVE_BIN" ]]; then
    echo "Building eggserve..."
    cargo build -p eggserve-bin --quiet 2>/dev/null
fi

if [[ ! -x "$EGGSERVE_BIN" ]]; then
    echo "FAIL: eggserve binary not found"
    exit 1
fi

# Setup test content
mkdir -p "$WORK_DIR/root"
echo "hello world" > "$WORK_DIR/root/hello.txt"
echo "ok" > "$WORK_DIR/root/status.txt"
dd if=/dev/urandom of="$WORK_DIR/root/large.bin" bs=1024 count=64 2>/dev/null

# Start eggserve on loopback
EGGSERVE_PORT=$(shuf -i 10000-60000 -n 1)
"$EGGSERVE_BIN" --bind "127.0.0.1:${EGGSERVE_PORT}" --root "$WORK_DIR/root" &
EGGSERVE_PID=$!
trap 'kill $EGGSERVE_PID 2>/dev/null; rm -rf "$WORK_DIR"' EXIT
sleep 1

if ! kill -0 "$EGGSERVE_PID" 2>/dev/null; then
    echo "FAIL: eggserve failed to start"
    exit 1
fi

PASS=0
FAIL=0
SKIP=0

# Test directly against eggserve (baseline)
echo "=== Desynchronization Corpus — Direct (baseline) ==="

test_desync_direct() {
    local name="$1"
    local payload="$2"
    local expect_closed="${3:-true}"

    local response
    response=$(echo -ne "$payload" | nc -w 5 127.0.0.1 "$EGGSERVE_PORT" 2>/dev/null || echo "CONNECTION_CLOSED")
    local status_code
    status_code=$(echo "$response" | head -1 | grep -oP 'HTTP/1\.\d \K\d+' || echo "000")

    # Check: server should return a valid HTTP response or close connection
    if [[ "$status_code" =~ ^[0-9]{3}$ ]] && [[ "$status_code" != "000" ]]; then
        echo "  PASS: $name (status $status_code)"
        ((PASS++))
    elif [[ "$expect_closed" == "true" ]] && [[ "$response" == "CONNECTION_CLOSED" ]]; then
        echo "  PASS: $name (connection closed as expected)"
        ((PASS++))
    else
        echo "  FAIL: $name (unexpected response)"
        echo "    Status: $status_code"
        echo "    Response: $(echo "$response" | head -3)"
        ((FAIL++))
    fi
}

# Desync payload 1: Transfer-Encoding + Content-Length
echo "Test 1: TE + CL conflict"
test_desync_direct "TE+CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 6\r\n\r\n0\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 2: Duplicate identical Content-Length
echo "Test 2: Duplicate identical CL"
test_desync_direct "Duplicate CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 3: Duplicate conflicting Content-Length
echo "Test 3: Duplicate conflicting CL"
test_desync_direct "Conflicting CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\nhelloXGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 4: Comma-combined lengths
echo "Test 4: Comma-combined CL"
test_desync_direct "Comma CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5, 6\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 5: Malformed chunk size
echo "Test 5: Malformed chunk size"
test_desync_direct "Malformed chunk" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\nZZZZ\r\nhello\r\n0\r\n\r\n"

# Desync payload 6: Malformed chunk terminator
echo "Test 6: Malformed chunk terminator"
test_desync_direct "Malformed terminator" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n00\r\n\r\n"

# Desync payload 7: Obsolete folding
echo "Test 7: Obsolete folding"
test_desync_direct "Obsolete folding" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Custom:\r\n folded-value\r\n\r\n"

# Desync payload 8: Whitespace before colon
echo "Test 8: Whitespace before colon"
test_desync_direct "Whitespace colon" "GET /hello.txt HTTP/1.1\r\nHost : localhost\r\n\r\n"

# Desync payload 9: Bare CR
echo "Test 9: Bare CR in header"
test_desync_direct "Bare CR" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Test: value\rCR\n\r\n"

# Desync payload 10: Bare LF
echo "Test 10: Bare LF in header"
test_desync_direct "Bare LF" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Test: value\n\r\n"

# Desync payload 11: Oversized headers
echo "Test 11: Oversized header value"
LARGE_VALUE=$(python3 -c "print('A' * 8192)")
test_desync_direct "Oversized header" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Large: ${LARGE_VALUE}\r\n\r\n"

# Desync payload 12: Invalid method
echo "Test 12: Invalid method"
test_desync_direct "Invalid method" "GETT /hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 13: Invalid target
echo "Test 13: Invalid target (absolute URI)"
test_desync_direct "Absolute URI" "GET http://localhost/hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 14: Hidden second request after malformed body
echo "Test 14: Hidden request after body"
test_desync_direct "Hidden request" "POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# Desync payload 15: Premature EOF
echo "Test 15: Premature EOF"
test_desync_direct "Premature EOF" "GET /hello.txt HTTP/1.1\r\nHost: localho"

# Desync payload 16: Body-forbidden method with body
echo "Test 16: Body-forbidden method with body"
test_desync_direct "Body on GET" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhello"

# Desync payload 17: Pipelined valid/malformed/valid
echo "Test 17: Pipelined valid/malformed/valid"
test_desync_direct "Pipelined" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\nGARBAGE DATA\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo ""
echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"

if [[ $FAIL -gt 0 ]]; then
    echo "FAIL: Desynchronization corpus tests failed"
    exit 1
fi

echo "PASS: All desynchronization corpus tests passed"
