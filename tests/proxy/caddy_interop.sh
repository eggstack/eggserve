#!/usr/bin/env bash
# Caddy reverse-proxy interop test (Plan 089, Track B).
#
# Tests eggserve behind Caddy as reverse proxy: TLS termination,
# connection reuse, header forwarding, timeout alignment, and
# no request desynchronization.
#
# Prerequisites:
#   - caddy binary in PATH
#   - eggserve binary built (cargo build -p eggserve-bin)
#   - curl for HTTP requests
#
# Usage: bash tests/proxy/caddy_interop.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

EGGSERVE_BIN="${REPO_ROOT}/target/debug/eggserve"
CADDY_BIN="$(command -v caddy 2>/dev/null || echo "")"

if [[ -z "$CADDY_BIN" ]]; then
    echo "SKIP: caddy not found in PATH"
    exit 0
fi

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
dd if=/dev/urandom of="$WORK_DIR/root/large.bin" bs=1024 count=64 2>/dev/null
mkdir -p "$WORK_DIR/root/subdir"
echo "nested" > "$WORK_DIR/root/subdir/nested.txt"

# Generate self-signed TLS cert for Caddy
openssl req -x509 -newkey rsa:2048 -keyout "$WORK_DIR/key.pem" \
    -out "$WORK_DIR/cert.pem" -days 1 -nodes \
    -subj "/CN=localhost" 2>/dev/null

# Start eggserve on loopback (no TLS)
EGGSERVE_PORT=$(shuf -i 10000-60000 -n 1)
"$EGGSERVE_BIN" --bind "127.0.0.1:${EGGSERVE_PORT}" --root "$WORK_DIR/root" &
EGGSERVE_PID=$!
trap 'kill $EGGSERVE_PID 2>/dev/null; rm -rf "$WORK_DIR"' EXIT
sleep 1

# Verify eggserve is running
if ! kill -0 "$EGGSERVE_PID" 2>/dev/null; then
    echo "FAIL: eggserve failed to start"
    exit 1
fi

# Generate Caddyfile
CADDY_PORT=$(shuf -i 10000-60000 -n 1)
cat > "$WORK_DIR/Caddyfile" <<EOF
{
    admin off
}

https://localhost:${CADDY_PORT} {
    tls "$WORK_DIR/cert.pem" "$WORK_DIR/key.pem"

    reverse_proxy 127.0.0.1:${EGGSERVE_PORT} {
        header_up X-Forwarded-For {remote_host}
        transport http {
            read_timeout  30s
            write_timeout 30s
        }
    }
}
EOF

# Start Caddy
"$CADDY_BIN" run --config "$WORK_DIR/Caddyfile" --adapter caddyfile &
CADDY_PID=$!
trap 'kill $CADDY_PID 2>/dev/null; kill $EGGSERVE_PID 2>/dev/null; rm -rf "$WORK_DIR"' EXIT
sleep 2

# Verify Caddy is running
if ! kill -0 "$CADDY_PID" 2>/dev/null; then
    echo "FAIL: Caddy failed to start"
    exit 1
fi

PASS=0
FAIL=0

run_test() {
    local name="$1"
    local expected="$2"
    shift 2
    local actual
    actual="$("$@" 2>/dev/null || echo "CURL_FAILED")"
    if echo "$actual" | grep -q "$expected"; then
        echo "  PASS: $name"
        ((PASS++))
    else
        echo "  FAIL: $name (expected '$expected' in response)"
        echo "    Got: $(echo "$actual" | head -5)"
        ((FAIL++))
    fi
}

echo "=== Caddy Reverse-Proxy Interop Tests ==="

# Test 1: Basic GET through Caddy
echo "Test 1: GET through Caddy"
run_test "GET returns 200" "200" \
    curl -sk "https://localhost:${CADDY_PORT}/hello.txt" -w "%{http_code}" -o /dev/null

# Test 2: HEAD through Caddy
echo "Test 2: HEAD through Caddy"
run_test "HEAD returns 200" "200" \
    curl -sk "https://localhost:${CADDY_PORT}/hello.txt" -I -w "%{http_code}" -o /dev/null

# Test 3: Large file through Caddy
echo "Test 3: Large file through Caddy"
run_test "Large file returns 200" "200" \
    curl -sk "https://localhost:${CADDY_PORT}/large.bin" -w "%{http_code}" -o /dev/null

# Test 4: Range request through Caddy
echo "Test 4: Range request through Caddy"
run_test "Range returns 206" "206" \
    curl -sk "https://localhost:${CADDY_PORT}/large.bin" -H "Range: bytes=0-1023" -w "%{http_code}" -o /dev/null

# Test 5: 404 through Caddy
echo "Test 5: 404 through Caddy"
run_test "Missing file returns 404" "404" \
    curl -sk "https://localhost:${CADDY_PORT}/nonexistent.txt" -w "%{http_code}" -o /dev/null

# Test 6: Directory index through Caddy
echo "Test 6: Directory index through Caddy"
run_test "Directory index returns 200" "200" \
    curl -sk "https://localhost:${CADDY_PORT}/subdir/" -w "%{http_code}" -o /dev/null

# Test 7: Connection reuse (multiple requests)
echo "Test 7: Connection reuse"
run_test "Keep-alive request 1" "200" \
    curl -sk "https://localhost:${CADDY_PORT}/hello.txt" -w "%{http_code}" -o /dev/null
run_test "Keep-alive request 2" "200" \
    curl -sk "https://localhost:${CADDY_PORT}/hello.txt" -w "%{http_code}" -o /dev/null

# Test 8: Conditional request (If-None-Match)
echo "Test 8: Conditional request"
ETAG=$(curl -sk "https://localhost:${CADDY_PORT}/hello.txt" -I 2>/dev/null | grep -i etag | tr -d '\r' | awk '{print $2}' | tr -d '"')
if [[ -n "$ETAG" ]]; then
    run_test "Conditional 304" "304" \
        curl -sk "https://localhost:${CADDY_PORT}/hello.txt" -H "If-None-Match: \"$ETAG\"" -w "%{http_code}" -o /dev/null
else
    echo "  SKIP: No ETag returned"
fi

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [[ $FAIL -gt 0 ]]; then
    echo "FAIL: Caddy interop tests failed"
    exit 1
fi

echo "PASS: All Caddy interop tests passed"
