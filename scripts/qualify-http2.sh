#!/usr/bin/env bash
# qualify-http2.sh — targeted HTTP/2 wire qualification for release work.
#
# This is intentionally outside routine CI. It requires an independent
# command-line client (curl with HTTP/2 support) and OpenSSL for a temporary
# loopback certificate. If nghttp is installed, it is used as a second client
# as well; otherwise the evidence record must say that it was unavailable.
#
# Promotion gates fail closed: curl and nghttp share the libnghttp2 stack and
# count as ONE implementation family. Set EGGSERVE_REQUIRE_TWO_H2_CLIENTS=1 to
# require two independent implementation families (exit 2 when absent),
# EGGSERVE_REQUIRE_BROWSER_EVIDENCE=1 to require caller-supplied browser
# evidence via EGGSERVE_H2_BROWSER_EVIDENCE (exit 2 when absent), and
# EGGSERVE_REQUIRE_PLATFORM_EVIDENCE=1 to require caller-supplied platform
# evidence via EGGSERVE_H2_PLATFORM_EVIDENCE (exit 2 when absent).
# "Tool not installed" is never reported as a passing promotion gate.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REQUIRE_TWO="${EGGSERVE_REQUIRE_TWO_H2_CLIENTS:-0}"
REQUIRE_BROWSER="${EGGSERVE_REQUIRE_BROWSER_EVIDENCE:-0}"
REQUIRE_PLATFORM="${EGGSERVE_REQUIRE_PLATFORM_EVIDENCE:-0}"

command -v cargo >/dev/null 2>&1 || { echo "cargo is required" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
command -v openssl >/dev/null 2>&1 || { echo "openssl is required" >&2; exit 1; }

curl --version | grep -q 'HTTP2' || {
    echo "curl was built without HTTP/2 support" >&2
    exit 1
}

QUAL_DIR="$(mktemp -d)"
SERVER_PID=""
cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$QUAL_DIR"
}
trap cleanup EXIT

free_port() {
    python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

wait_for_http() {
    local url="$1"
    local mode="$2"
    for _ in $(seq 1 100); do
        if curl -ksS "$mode" -o /dev/null "$url"; then
            return 0
        fi
        sleep 0.1
    done
    echo "server did not become ready: $url" >&2
    return 1
}

start_server() {
    local port="$1"
    shift
    "$REPO_ROOT/target/debug/eggserve" --bind 127.0.0.1 --port "$port" \
        --directory "$REPO_ROOT/examples/site" --log-format none "$@" \
        >"$QUAL_DIR/server.stdout" 2>"$QUAL_DIR/server.stderr" &
    SERVER_PID=$!
}

stop_server() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
        SERVER_PID=""
    fi
}

echo "Toolchain and client versions"
rustc --version
cargo --version
curl -V | sed -n '1,3p'
if command -v nghttp >/dev/null 2>&1; then
    nghttp --version 2>&1 | head -2 || true
else
    echo "nghttp: not installed"
fi
if python3 -c 'import h2; print("python-h2", h2.__version__)' 2>/dev/null; then
    true
else
    echo "python-h2: not installed (no second implementation family from Python)"
fi

echo "Building the H2/TLS CLI"
cargo build --locked -p eggserve-bin --features http2,tls

openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$QUAL_DIR/key.pem" -out "$QUAL_DIR/cert.pem" -days 1 \
    -subj /CN=localhost >/dev/null 2>&1

TLS_PORT="$(free_port)"
start_server "$TLS_PORT" --tls-cert "$QUAL_DIR/cert.pem" --tls-key "$QUAL_DIR/key.pem"
TLS_URL="https://localhost:$TLS_PORT/assets/example.txt"
wait_for_http "$TLS_URL" "--http2"

H2_VERSION="$(curl -ksS --http2 --resolve "localhost:$TLS_PORT:127.0.0.1" \
    -o /dev/null -w '%{http_version}' "$TLS_URL")"
H1_VERSION="$(curl -ksS --http1.1 --resolve "localhost:$TLS_PORT:127.0.0.1" \
    -o /dev/null -w '%{http_version}' "$TLS_URL")"
[[ "$H2_VERSION" == "2" ]] || { echo "expected TLS H2, got $H2_VERSION" >&2; exit 1; }
[[ "$H1_VERSION" == "1.1" ]] || { echo "expected TLS H1, got $H1_VERSION" >&2; exit 1; }
echo "TLS ALPN: HTTP/$H2_VERSION and HTTP/$H1_VERSION"

HEADERS="$QUAL_DIR/h2.headers"
BODY="$QUAL_DIR/h2.body"
curl -ksS --http2 --resolve "localhost:$TLS_PORT:127.0.0.1" \
    -D "$HEADERS" -o "$BODY" "$TLS_URL"
tr -d '\r' < "$HEADERS" > "$QUAL_DIR/h2.headers.normalized"
HEADERS="$QUAL_DIR/h2.headers.normalized"
grep -qi '^content-length: 52[[:space:]]*$' "$HEADERS"
! grep -qi '^connection:' "$HEADERS"
! grep -qi '^transfer-encoding:' "$HEADERS"
grep -q 'This is a small text fixture' "$BODY"

RANGE_STATUS="$(curl -ksS --http2 --resolve "localhost:$TLS_PORT:127.0.0.1" \
    -r 0-3 -o "$QUAL_DIR/range.body" -w '%{http_code}' "$TLS_URL")"
[[ "$RANGE_STATUS" == "206" ]] || { echo "expected 206, got $RANGE_STATUS" >&2; exit 1; }
[[ "$(cat "$QUAL_DIR/range.body")" == "This" ]] || exit 1

ETAG="$(awk 'BEGIN { IGNORECASE=1 } /^etag:/ { sub(/^etag:[[:space:]]*/, ""); print; exit }' "$HEADERS")"
[[ -n "$ETAG" ]] || { echo "missing ETag in standard metadata profile" >&2; exit 1; }
CONDITIONAL_STATUS="$(curl -ksS --http2 --resolve "localhost:$TLS_PORT:127.0.0.1" \
    -H "If-None-Match: $ETAG" -o /dev/null -w '%{http_code}' "$TLS_URL")"
[[ "$CONDITIONAL_STATUS" == "304" ]] || {
    echo "expected conditional 304, got $CONDITIONAL_STATUS" >&2
    exit 1
}

curl -ksS --http2 --parallel --parallel-max 8 \
    --resolve "localhost:$TLS_PORT:127.0.0.1" -o /dev/null \
    "$TLS_URL" "$TLS_URL" "$TLS_URL" "$TLS_URL" >/dev/null
echo "TLS H2 static, range, conditional, and parallel requests passed"

if command -v nghttp >/dev/null 2>&1; then
    nghttp -nv -y "$TLS_URL" >/dev/null
    echo "nghttp: passed"
else
    echo "nghttp: unavailable (curl was the only command-line client in this environment)"
fi

# Count independent implementation families, not frontends. curl and nghttp
# share libnghttp2 and count as one family; python-h2 counts as a second
# family only when the module is importable.
H2_FAMILIES=()
if curl --version 2>/dev/null | grep -q 'nghttp2'; then
    H2_FAMILIES+=(libnghttp2)
elif curl --version 2>/dev/null | grep -q 'HTTP2'; then
    H2_FAMILIES+=(curl-h2)
fi
if python3 -c 'import h2' 2>/dev/null; then
    H2_FAMILIES+=(python-h2)
fi
# Deduplicate (curl+nghttp already collapsed to one entry above).
printf 'Detected H2 implementation families: %s\n' "${H2_FAMILIES[*]:-<none>}"
if [[ "$REQUIRE_TWO" == 1 && ${#H2_FAMILIES[@]} -lt 2 ]]; then
    echo "promotion gate: fewer than two independent H2 implementation families are available" >&2
    exit 2
fi
if [[ "$REQUIRE_BROWSER" == 1 ]]; then
    if [[ -z "${EGGSERVE_H2_BROWSER_EVIDENCE:-}" || ! -f "${EGGSERVE_H2_BROWSER_EVIDENCE:-}" ]]; then
        echo "promotion gate: browser evidence file is required (set EGGSERVE_H2_BROWSER_EVIDENCE to an existing file)" >&2
        exit 2
    fi
    echo "browser evidence supplied: $EGGSERVE_H2_BROWSER_EVIDENCE"
fi
if [[ "$REQUIRE_PLATFORM" == 1 ]]; then
    if [[ -z "${EGGSERVE_H2_PLATFORM_EVIDENCE:-}" || ! -f "${EGGSERVE_H2_PLATFORM_EVIDENCE:-}" ]]; then
        echo "promotion gate: platform evidence file is required (set EGGSERVE_H2_PLATFORM_EVIDENCE to an existing file)" >&2
        exit 2
    fi
    echo "platform evidence supplied: $EGGSERVE_H2_PLATFORM_EVIDENCE"
fi
stop_server

PLAIN_PORT="$(free_port)"
start_server "$PLAIN_PORT"
PLAIN_URL="http://127.0.0.1:$PLAIN_PORT/assets/example.txt"
wait_for_http "$PLAIN_URL" "--http2-prior-knowledge"
PLAIN_VERSION="$(curl -sS --http2-prior-knowledge -o /dev/null -w '%{http_version}' "$PLAIN_URL")"
[[ "$PLAIN_VERSION" == "2" ]] || { echo "expected cleartext H2, got $PLAIN_VERSION" >&2; exit 1; }
UPGRADE_VERSION="$(curl -sS --http1.1 -H 'Connection: Upgrade' -H 'Upgrade: h2c' \
    -o /dev/null -w '%{http_version}' "$PLAIN_URL")"
[[ "$UPGRADE_VERSION" == "1.1" ]] || {
    echo "HTTP/1 Upgrade unexpectedly changed protocol: $UPGRADE_VERSION" >&2
    exit 1
}
echo "Cleartext prior knowledge: HTTP/$PLAIN_VERSION; Upgrade remains HTTP/$UPGRADE_VERSION"

echo "HTTP/2 wire qualification passed"
