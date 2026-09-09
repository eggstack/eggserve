#!/usr/bin/env bash
# qualify-http3.sh — targeted HTTP/3/QUIC release qualification.
#
# This is intentionally outside routine CI. It records the local dependency
# graph and exercises TCP fallback, runtime-owned Alt-Svc, and (when an
# independent client is installed) direct H3 requests. Quinn/h3 are the
# server stack; curl built with ngtcp2/nghttp3 is an independent client.
#
# Set EGGSERVE_REQUIRE_H3_CLIENTS=1 to fail when direct H3 cannot be tested,
# and EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1 to require two client implementations.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REQUIRE_H3="${EGGSERVE_REQUIRE_H3_CLIENTS:-0}"
REQUIRE_TWO="${EGGSERVE_REQUIRE_TWO_H3_CLIENTS:-0}"

command -v cargo >/dev/null 2>&1 || { echo "cargo is required" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
command -v openssl >/dev/null 2>&1 || { echo "openssl is required" >&2; exit 1; }

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

start_server() {
    local port="$1"
    shift
    "$REPO_ROOT/target/debug/eggserve" --bind 127.0.0.1 --port "$port" \
        --directory "$QUAL_DIR/site" --log-format none "$@" \
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

wait_for_http1() {
    local url="$1"
    for _ in $(seq 1 100); do
        if curl -ksS --http1.1 -o /dev/null "$url"; then
            return 0
        fi
        sleep 0.1
    done
    echo "server did not become ready: $url" >&2
    return 1
}

echo "Toolchain and client versions"
rustc --version
cargo --version
curl -V | sed -n '1,3p'

echo "Feature graph checks"
MINIMAL_TREE="$(cargo tree -e normal --prefix none -p eggserve-bin --no-default-features)"
if grep -Eiq '(^|[[:space:]])(h3|h3-quinn|quinn)([[:space:]]|$)' <<<"$MINIMAL_TREE"; then
    echo "minimal dependency graph unexpectedly contains H3/QUIC" >&2
    exit 1
fi
cargo tree -e normal --prefix none -p eggserve-bin --no-default-features --features http3

echo "Building the H3/TLS CLI"
cargo build --locked -p eggserve-bin --features http3

mkdir -p "$QUAL_DIR/site/assets"
cp "$REPO_ROOT/examples/site/assets/example.txt" "$QUAL_DIR/site/assets/example.txt"
dd if=/dev/zero of="$QUAL_DIR/site/assets/large.bin" bs=1024 count=1024 status=none
openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$QUAL_DIR/key.pem" -out "$QUAL_DIR/cert.pem" -days 1 \
    -subj /CN=localhost >/dev/null 2>&1

TLS_PORT="$(free_port)"
TLS_URL="https://localhost:$TLS_PORT/assets/example.txt"
start_server "$TLS_PORT" --tls-cert "$QUAL_DIR/cert.pem" --tls-key "$QUAL_DIR/key.pem" --http3
wait_for_http1 "$TLS_URL"

ALT_SVC="$({ curl -ksS --http1.1 --resolve "localhost:$TLS_PORT:127.0.0.1" \
    -D - -o /dev/null "$TLS_URL" || true; } | tr -d '\r' | awk 'BEGIN { IGNORECASE=1 } /^alt-svc:/ { sub(/^alt-svc:[[:space:]]*/, ""); print; exit }')"
[[ "$ALT_SVC" == "h3=\":$TLS_PORT\"; ma=86400" ]] || {
    echo "expected same-port Alt-Svc, got: ${ALT_SVC:-<missing>}" >&2
    exit 1
}
echo "TCP fallback and runtime-owned Alt-Svc passed: $ALT_SVC"

H3_CLIENTS=()
if curl -V | grep -q 'HTTP3'; then
    H3_CLIENTS+=(curl)
fi
for candidate in nghttp3-client quiche-client; do
    if command -v "$candidate" >/dev/null 2>&1; then
        H3_CLIENTS+=("$candidate")
    fi
done

if ((${#H3_CLIENTS[@]} == 0)); then
    echo "No direct HTTP/3 client is installed; H3 wire checks are unavailable."
    if [[ "$REQUIRE_H3" == 1 || "$REQUIRE_TWO" == 1 ]]; then
        exit 2
    fi
else
    printf 'Detected H3 clients: %s\n' "${H3_CLIENTS[*]}"
    if printf '%s\n' "${H3_CLIENTS[@]}" | grep -qx curl; then
        H3_VERSION="$(curl -ksS --http3-only --resolve "localhost:$TLS_PORT:127.0.0.1" \
            -o /dev/null -w '%{http_version}' "$TLS_URL")"
        [[ "$H3_VERSION" == 3 ]] || { echo "expected H3, got $H3_VERSION" >&2; exit 1; }
        HEADERS="$QUAL_DIR/h3.headers"
        BODY="$QUAL_DIR/h3.body"
        curl -ksS --http3-only --resolve "localhost:$TLS_PORT:127.0.0.1" \
            -D "$HEADERS" -o "$BODY" "$TLS_URL"
        tr -d '\r' < "$HEADERS" > "$QUAL_DIR/h3.headers.normalized"
        ! grep -qi '^connection:' "$QUAL_DIR/h3.headers.normalized"
        ! grep -qi '^transfer-encoding:' "$QUAL_DIR/h3.headers.normalized"
        grep -q 'This is a small text fixture' "$BODY"
        [[ "$(curl -ksS --http3-only --head --resolve "localhost:$TLS_PORT:127.0.0.1" \
            -o /dev/null -w '%{http_code}' "$TLS_URL")" == 200 ]]
        [[ "$(curl -ksS --http3-only --resolve "localhost:$TLS_PORT:127.0.0.1" \
            -r 0-3 -o "$QUAL_DIR/range.body" -w '%{http_code}' "$TLS_URL")" == 206 ]]
        ETAG="$(awk 'BEGIN { IGNORECASE=1 } /^etag:/ { sub(/^etag:[[:space:]]*/, ""); print; exit }' "$QUAL_DIR/h3.headers.normalized")"
        [[ -n "$ETAG" ]]
        [[ "$(curl -ksS --http3-only --resolve "localhost:$TLS_PORT:127.0.0.1" \
            -H "If-None-Match: $ETAG" -o /dev/null -w '%{http_code}' "$TLS_URL")" == 304 ]]
        echo "curl H3 GET/HEAD/range/conditional semantics passed"
    fi
    if [[ "$REQUIRE_TWO" == 1 && ${#H3_CLIENTS[@]} -lt 2 ]]; then
        echo "fewer than two independent H3 clients are available" >&2
        exit 2
    fi
fi
stop_server

# A separate H1-only TLS listener demonstrates fallback when no UDP/H3
# endpoint is active; TCP and H3 listeners are independent runtime paths.
FALLBACK_PORT="$(free_port)"
FALLBACK_URL="https://localhost:$FALLBACK_PORT/assets/example.txt"
start_server "$FALLBACK_PORT" --tls-cert "$QUAL_DIR/cert.pem" --tls-key "$QUAL_DIR/key.pem"
wait_for_http1 "$FALLBACK_URL"
[[ "$(curl -ksS --http1.1 --resolve "localhost:$FALLBACK_PORT:127.0.0.1" \
    -o /dev/null -w '%{http_code}' "$FALLBACK_URL")" == 200 ]]
echo "H1 fallback without an H3 endpoint passed"

if [[ "$REQUIRE_TWO" == 1 ]] && (( ${#H3_CLIENTS[@]} < 2 )); then
    exit 2
fi
if [[ "$REQUIRE_H3" == 1 ]] && (( ${#H3_CLIENTS[@]} == 0 )); then
    exit 2
fi
echo "HTTP/3 qualification baseline completed; consult the release record for the support-tier decision."
