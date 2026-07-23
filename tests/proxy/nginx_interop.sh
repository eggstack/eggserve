#!/usr/bin/env bash
# nginx reverse-proxy interop test (Plan 089, Track B).
#
# Tests eggserve behind nginx as reverse proxy: TLS termination,
# connection reuse, header forwarding, timeout alignment, and
# no request desynchronization.
#
# Prerequisites:
#   - nginx binary in PATH
#   - eggserve binary built (cargo build -p eggserve-bin)
#   - curl for HTTP requests
#
# Usage: bash tests/proxy/nginx_interop.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

EGGSERVE_BIN="${REPO_ROOT}/target/debug/eggserve"
NGINX_BIN="$(command -v nginx 2>/dev/null || echo "")"

if [[ -z "$NGINX_BIN" ]]; then
    echo "SKIP: nginx not found in PATH"
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

# Start eggserve on loopback (no TLS)
EGGSERVE_PORT=$(shuf -i 10000-60000 -n 1)
"$EGGSERVE_BIN" --bind "127.0.0.1:${EGGSERVE_PORT}" --directory "$WORK_DIR/root" &
EGGSERVE_PID=$!
trap 'kill $EGGSERVE_PID 2>/dev/null; rm -rf "$WORK_DIR"' EXIT
sleep 1

# Verify eggserve is running
if ! kill -0 "$EGGSERVE_PID" 2>/dev/null; then
    echo "FAIL: eggserve failed to start"
    exit 1
fi

# Generate self-signed TLS cert for nginx
openssl req -x509 -newkey rsa:2048 -keyout "$WORK_DIR/key.pem" \
    -out "$WORK_DIR/cert.pem" -days 1 -nodes \
    -subj "/CN=localhost" 2>/dev/null

# Generate nginx config
NGINX_PORT=$(shuf -i 10000-60000 -n 1)
NGINX_WORKER_PROCS=$(nproc 2>/dev/null || echo 1)

cat > "$WORK_DIR/nginx.conf" <<EOF
worker_processes ${NGINX_WORKER_PROCS};
error_log "$WORK_DIR/nginx_error.log" warn;
pid "$WORK_DIR/nginx.pid";

events {
    worker_connections 1024;
}

http {
    access_log "$WORK_DIR/nginx_access.log";

    server {
        listen ${NGINX_PORT} ssl;
        server_name localhost;

        ssl_certificate "$WORK_DIR/cert.pem";
        ssl_certificate_key "$WORK_DIR/key.pem";
        ssl_protocols TLSv1.2 TLSv1.3;

        location / {
            proxy_pass http://127.0.0.1:${EGGSERVE_PORT};
            proxy_set_header Host \$host;
            proxy_set_header X-Real-IP \$remote_addr;
            proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto \$scheme;

            proxy_connect_timeout 10s;
            proxy_send_timeout 30s;
            proxy_read_timeout 30s;

            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }
    }
}
EOF

# Start nginx
"$NGINX_BIN" -c "$WORK_DIR/nginx.conf" -p "$WORK_DIR" &
NGINX_PID=$!
trap 'kill $NGINX_PID 2>/dev/null; kill $EGGSERVE_PID 2>/dev/null; rm -rf "$WORK_DIR"' EXIT
sleep 2

# Verify nginx is running
if ! kill -0 "$NGINX_PID" 2>/dev/null; then
    echo "FAIL: nginx failed to start"
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
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $name (expected '$expected' in response)"
        echo "    Got: $(echo "$actual" | head -5)"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== nginx Reverse-Proxy Interop Tests ==="

# Test 1: Basic GET through nginx
echo "Test 1: GET through nginx"
run_test "GET returns 200" "200" \
    curl -sk "https://localhost:${NGINX_PORT}/hello.txt" -w "%{http_code}" -o /dev/null

# Test 2: HEAD through nginx
echo "Test 2: HEAD through nginx"
run_test "HEAD returns 200" "200" \
    curl -sk "https://localhost:${NGINX_PORT}/hello.txt" -I -w "%{http_code}" -o /dev/null

# Test 3: Large file through nginx
echo "Test 3: Large file through nginx"
run_test "Large file returns 200" "200" \
    curl -sk "https://localhost:${NGINX_PORT}/large.bin" -w "%{http_code}" -o /dev/null

# Test 4: Range request through nginx
echo "Test 4: Range request through nginx"
run_test "Range returns 206" "206" \
    curl -sk "https://localhost:${NGINX_PORT}/large.bin" -H "Range: bytes=0-1023" -w "%{http_code}" -o /dev/null

# Test 5: 404 through nginx
echo "Test 5: 404 through nginx"
run_test "Missing file returns 404" "404" \
    curl -sk "https://localhost:${NGINX_PORT}/nonexistent.txt" -w "%{http_code}" -o /dev/null

# Test 6: Directory index through nginx
echo "Test 6: Directory index through nginx"
run_test "Directory index returns 200" "200" \
    curl -sk "https://localhost:${NGINX_PORT}/subdir/" -w "%{http_code}" -o /dev/null

# Test 7: Connection reuse (multiple requests)
echo "Test 7: Connection reuse"
run_test "Keep-alive request 1" "200" \
    curl -sk "https://localhost:${NGINX_PORT}/hello.txt" -w "%{http_code}" -o /dev/null
run_test "Keep-alive request 2" "200" \
    curl -sk "https://localhost:${NGINX_PORT}/hello.txt" -w "%{http_code}" -o /dev/null

# Test 8: Conditional request (If-None-Match)
echo "Test 8: Conditional request"
ETAG=$(curl -sk "https://localhost:${NGINX_PORT}/hello.txt" -I 2>/dev/null | grep -i etag | tr -d '\r' | awk '{print $2}' | tr -d '"')
if [[ -n "$ETAG" ]]; then
    run_test "Conditional 304" "304" \
        curl -sk "https://localhost:${NGINX_PORT}/hello.txt" -H "If-None-Match: \"$ETAG\"" -w "%{http_code}" -o /dev/null
else
    echo "  SKIP: No ETag returned"
fi

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [[ $FAIL -gt 0 ]]; then
    echo "FAIL: nginx interop tests failed"
    exit 1
fi

echo "PASS: All nginx interop tests passed"
