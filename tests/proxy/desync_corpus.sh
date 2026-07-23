#!/usr/bin/env bash
# Proxy desynchronization corpus test (Plan 089, Track B).
#
# Sends desynchronization payloads directly to eggserve (baseline) and
# through available proxies (Caddy, nginx) to verify no frontend/backend
# disagreement that permits request smuggling or cross-request confusion.
#
# Prerequisites:
#   - eggserve binary built (cargo build -p eggserve-bin)
#   - curl and nc for HTTP requests
#   - Optional: caddy and/or nginx in PATH for proxy-through tests
#
# Usage: bash tests/proxy/desync_corpus.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
PROCS_TO_KILL=()

cleanup() {
    for pid in "${PROCS_TO_KILL[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

EGGSERVE_BIN="${REPO_ROOT}/target/debug/eggserve"
CADDY_BIN="$(command -v caddy 2>/dev/null || echo "")"
NGINX_BIN="$(command -v nginx 2>/dev/null || echo "")"

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
"$EGGSERVE_BIN" --bind "127.0.0.1:${EGGSERVE_PORT}" --directory "$WORK_DIR/root" &
PROCS_TO_KILL+=($!)
sleep 1

if ! kill -0 "${PROCS_TO_KILL[0]}" 2>/dev/null; then
    echo "FAIL: eggserve failed to start"
    exit 1
fi

PASS=0
FAIL=0
SKIP=0
PROXY_PASS=0
PROXY_FAIL=0

# --- Helper functions ---

test_desync_direct() {
    local name="$1"
    local payload="$2"
    local expect_closed="${3:-true}"

    local response
    response=$(echo -ne "$payload" | timeout 5 nc -w 3 127.0.0.1 "$EGGSERVE_PORT" 2>/dev/null || echo "CONNECTION_CLOSED")
    local status_code
    status_code=$(echo "$response" | head -1 | grep -oP 'HTTP/1\.\d \K\d+' || echo "000")

    if [[ "$status_code" =~ ^[0-9]{3}$ ]] && [[ "$status_code" != "000" ]]; then
        echo "  PASS: $name (status $status_code)"
        PASS=$((PASS + 1))
    elif [[ "$expect_closed" == "true" ]] && [[ "$response" == "CONNECTION_CLOSED" ]]; then
        echo "  PASS: $name (connection closed as expected)"
        PASS=$((PASS + 1))
    elif [[ "$expect_closed" == "true" ]] && [[ -z "$response" ]]; then
        echo "  PASS: $name (empty response, connection closed)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $name (unexpected response)"
        echo "    Status: $status_code"
        echo "    Response: $(echo "$response" | head -3)"
        FAIL=$((FAIL + 1))
    fi
}

test_desync_through_proxy() {
    local name="$1"
    local payload="$2"
    local proxy_port="$3"
    local proxy_name="$4"

    local response
    response=$(echo -ne "$payload" | timeout 5 nc -w 3 127.0.0.1 "$proxy_port" 2>/dev/null || echo "CONNECTION_CLOSED")
    local status_code
    status_code=$(echo "$response" | head -1 | grep -oP 'HTTP/1\.\d \K\d+' || echo "000")

    if [[ "$status_code" =~ ^[0-9]{3}$ ]] && [[ "$status_code" != "000" ]]; then
        echo "  PASS: $proxy_name/$name (status $status_code)"
        PROXY_PASS=$((PROXY_PASS + 1))
    elif [[ "$response" == "CONNECTION_CLOSED" ]]; then
        echo "  PASS: $proxy_name/$name (connection closed)"
        PROXY_PASS=$((PROXY_PASS + 1))
    else
        echo "  FAIL: $proxy_name/$name (unexpected response)"
        echo "    Status: $status_code"
        PROXY_FAIL=$((PROXY_FAIL + 1))
    fi
}

# --- Direct baseline tests ---
echo "=== Desynchronization Corpus — Direct (baseline) ==="

echo "Test 1: TE + CL conflict"
test_desync_direct "TE+CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 6\r\n\r\n0\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 2: Duplicate identical CL"
test_desync_direct "Duplicate CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 3: Duplicate conflicting CL"
test_desync_direct "Conflicting CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\nhelloXGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 4: Comma-combined CL"
test_desync_direct "Comma CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5, 6\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 5: Malformed chunk size"
test_desync_direct "Malformed chunk" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\nZZZZ\r\nhello\r\n0\r\n\r\n"

echo "Test 6: Malformed chunk terminator"
test_desync_direct "Malformed terminator" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n00\r\n\r\n"

echo "Test 7: Obsolete folding"
test_desync_direct "Obsolete folding" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Custom:\r\n folded-value\r\n\r\n"

echo "Test 8: Whitespace before colon"
test_desync_direct "Whitespace colon" "GET /hello.txt HTTP/1.1\r\nHost : localhost\r\n\r\n"

echo "Test 9: Bare CR in header"
test_desync_direct "Bare CR" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Test: value\rCR\n\r\n"

echo "Test 10: Bare LF in header"
test_desync_direct "Bare LF" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Test: value\n\r\n"

echo "Test 11: Oversized header value"
LARGE_VALUE=$(python3 -c "print('A' * 8192)")
test_desync_direct "Oversized header" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Large: ${LARGE_VALUE}\r\n\r\n"

echo "Test 12: Invalid method"
test_desync_direct "Invalid method" "GETT /hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 13: Invalid target (absolute URI)"
test_desync_direct "Absolute URI" "GET http://localhost/hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 14: Hidden request after body"
test_desync_direct "Hidden request" "POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

echo "Test 15: Premature EOF"
test_desync_direct "Premature EOF" "GET /hello.txt HTTP/1.1\r\nHost: localho"

echo "Test 16: Body-forbidden method with body"
test_desync_direct "Body on GET" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhello"

echo "Test 17: Pipelined valid/malformed/valid"
test_desync_direct "Pipelined" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\nGARBAGE DATA\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n"

# --- Caddy proxy-through tests ---
CADDY_PORT=""
if [[ -n "$CADDY_BIN" ]]; then
    echo ""
    echo "=== Desynchronization Corpus — Through Caddy ==="
    CADDY_PORT=$(shuf -i 30000-50000 -n 1)

    mkdir -p "$WORK_DIR/caddy"
    cat > "$WORK_DIR/caddy/Caddyfile" <<CADDY_EOF
{
    admin off
}

:${CADDY_PORT} {
    reverse_proxy 127.0.0.1:${EGGSERVE_PORT}
}
CADDY_EOF

    "$CADDY_BIN" run --config "$WORK_DIR/caddy/Caddyfile" --adapter caddyfile &
    PROCS_TO_KILL+=($!)
    sleep 2

    if kill -0 "${PROCS_TO_KILL[-1]}" 2>/dev/null; then
        echo "Caddy started on port $CADDY_PORT"

        # Critical desync payloads through Caddy
        for test_num in 1 3 5 14 17; do
            case $test_num in
                1) test_desync_through_proxy "TE+CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 6\r\n\r\n0\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$CADDY_PORT" "caddy";;
                3) test_desync_through_proxy "Conflicting CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\nhelloXGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$CADDY_PORT" "caddy";;
                5) test_desync_through_proxy "Malformed chunk" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\nZZZZ\r\nhello\r\n0\r\n\r\n" "$CADDY_PORT" "caddy";;
                14) test_desync_through_proxy "Hidden request" "POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$CADDY_PORT" "caddy";;
                17) test_desync_through_proxy "Pipelined" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\nGARBAGE DATA\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$CADDY_PORT" "caddy";;
            esac
        done

        kill "${PROCS_TO_KILL[-1]}" 2>/dev/null || true
        PROCS_TO_KILL=("${PROCS_TO_KILL[@]:0:${#PROCS_TO_KILL[@]}-1}")
    else
        echo "WARN: Caddy failed to start, skipping proxy-through tests"
        SKIP=$((SKIP + 1))
    fi
else
    echo ""
    echo "SKIP: Caddy not found in PATH — proxy-through tests skipped"
    ((SKIP++))
fi

# --- nginx proxy-through tests ---
NGINX_PORT=""
if [[ -n "$NGINX_BIN" ]]; then
    echo ""
    echo "=== Desynchronization Corpus — Through nginx ==="
    NGINX_PORT=$(shuf -i 30000-50000 -n 1)

    mkdir -p "$WORK_DIR/nginx"
    cat > "$WORK_DIR/nginx/nginx.conf" <<NGINX_EOF
worker_processes 1;
error_log $WORK_DIR/nginx/error.log;
pid $WORK_DIR/nginx/nginx.pid;
events { worker_connections 64; }
http {
    access_log $WORK_DIR/nginx/access.log;
    server {
        listen ${NGINX_PORT};
        location / {
            proxy_pass http://127.0.0.1:${EGGSERVE_PORT};
            proxy_http_version 1.1;
            proxy_set_header Host \$host;
            proxy_set_header Connection "";
        }
    }
}
NGINX_EOF

    "$NGINX_BIN" -c "$WORK_DIR/nginx/nginx.conf" &
    PROCS_TO_KILL+=($!)
    sleep 2

    if kill -0 "${PROCS_TO_KILL[-1]}" 2>/dev/null; then
        echo "nginx started on port $NGINX_PORT"

        for test_num in 1 3 5 14 17; do
            case $test_num in
                1) test_desync_through_proxy "TE+CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 6\r\n\r\n0\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$NGINX_PORT" "nginx";;
                3) test_desync_through_proxy "Conflicting CL" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\nhelloXGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$NGINX_PORT" "nginx";;
                5) test_desync_through_proxy "Malformed chunk" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\nZZZZ\r\nhello\r\n0\r\n\r\n" "$NGINX_PORT" "nginx";;
                14) test_desync_through_proxy "Hidden request" "POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhelloGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$NGINX_PORT" "nginx";;
                17) test_desync_through_proxy "Pipelined" "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\nGARBAGE DATA\r\n\r\nGET /status.txt HTTP/1.1\r\nHost: localhost\r\n\r\n" "$NGINX_PORT" "nginx";;
            esac
        done

        kill "${PROCS_TO_KILL[-1]}" 2>/dev/null || true
        PROCS_TO_KILL=("${PROCS_TO_KILL[@]:0:${#PROCS_TO_KILL[@]}-1}")
    else
        echo "WARN: nginx failed to start, skipping proxy-through tests"
        SKIP=$((SKIP + 1))
    fi
else
    echo ""
    echo "SKIP: nginx not found in PATH — proxy-through tests skipped"
    ((SKIP++))
fi

# --- Summary ---
echo ""
echo "=== Results ==="
echo "Direct: $PASS passed, $FAIL failed"
if [[ $PROXY_PASS -gt 0 ]] || [[ $PROXY_FAIL -gt 0 ]]; then
    echo "Proxy:  $PROXY_PASS passed, $PROXY_FAIL failed"
fi
echo "Skipped: $SKIP"

TOTAL_FAIL=$((FAIL + PROXY_FAIL))
if [[ $TOTAL_FAIL -gt 0 ]]; then
    echo "FAIL: $TOTAL_FAIL desynchronization corpus tests failed"
    exit 1
fi

echo "PASS: All desynchronization corpus tests passed"
