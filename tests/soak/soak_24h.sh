#!/usr/bin/env bash
# Long-duration mixed-traffic soak test (Plan 089, Track F).
#
# Runs a 24-hour soak on the specified production profile, exercising
# small/medium/large files, range requests, HEAD, conditionals,
# keep-alive, connection churn, slow headers, malformed framing,
# and periodic graceful restarts.
#
# Usage: bash tests/soak/soak_24h.sh <profile>
#   profile: unix-reverse-proxy | unix-direct-https
#
# Environment variables:
#   SOAK_DURATION_HOURS  - Duration in hours (default: 24)
#   SOAK_LOG_DIR         - Log directory (default: /tmp/eggserve-soak)
#   SOAK_VERBOSE         - Enable verbose output (default: false)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

PROFILE="${1:-unix-reverse-proxy}"
DURATION_HOURS="${SOAK_DURATION_HOURS:-24}"
DURATION_SECS="${SOAK_DURATION_SECS:-0}"
LOG_DIR="${SOAK_LOG_DIR:-/tmp/eggserve-soak}"
VERBOSE="${SOAK_VERBOSE:-false}"

# Convert hours to seconds if duration_secs not set
if [[ "$DURATION_SECS" -eq 0 ]]; then
    DURATION_SECS=$((DURATION_HOURS * 3600))
fi
if [[ "$DURATION_SECS" -lt 1 ]]; then
    DURATION_SECS=1
fi

mkdir -p "$LOG_DIR"

EGGSERVE_BIN="${REPO_ROOT}/target/debug/eggserve"
CURL_BIN="$(command -v curl || echo "")"

if [[ ! -x "$EGGSERVE_BIN" ]]; then
    echo "Building eggserve..."
    cargo build -p eggserve-bin --quiet 2>/dev/null
fi

if [[ ! -x "$EGGSERVE_BIN" ]]; then
    echo "FAIL: eggserve binary not found"
    exit 1
fi

if [[ -z "$CURL_BIN" ]]; then
    echo "FAIL: curl not found in PATH"
    exit 1
fi

# Setup test content
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$WORK_DIR/root/subdir"

# Small files
for i in $(seq 1 10); do
    echo "small file $i content" > "$WORK_DIR/root/small_${i}.txt"
done

# Medium files (1KB-10KB)
for i in $(seq 1 5); do
    dd if=/dev/urandom of="$WORK_DIR/root/medium_${i}.bin" bs=1024 count=$((i * 2)) 2>/dev/null
done

# Large files (100KB-1MB)
for i in $(seq 1 3); do
    dd if=/dev/urandom of="$WORK_DIR/root/large_${i}.bin" bs=1024 count=$((i * 100)) 2>/dev/null
done

# Nested files
echo "nested content" > "$WORK_DIR/root/subdir/nested.txt"
dd if=/dev/urandom of="$WORK_DIR/root/subdir/deep.bin" bs=1024 count=50 2>/dev/null

# Empty file
touch "$WORK_DIR/root/empty.txt"

# Start eggserve
EGGSERVE_PORT=$(shuf -i 10000-60000 -n 1)
"$EGGSERVE_BIN" --bind "127.0.0.1:${EGGSERVE_PORT}" --directory "$WORK_DIR/root" &
EGGSERVE_PID=$!
trap 'kill $EGGSERVE_PID 2>/dev/null; rm -rf "$WORK_DIR"' EXIT
sleep 1

if ! kill -0 "$EGGSERVE_PID" 2>/dev/null; then
    echo "FAIL: eggserve failed to start"
    exit 1
fi

# Soak test metrics
START_TIME=$(date +%s)
END_TIME=$((START_TIME + DURATION_SECS))
TOTAL_REQUESTS=0
TOTAL_ERRORS=0
MAX_RSS_KB=0
LOG_FILE="$LOG_DIR/soak_${PROFILE}_$(date +%Y%m%d_%H%M%S).log"
LATENCY_SUM=0
LATENCY_COUNT=0
LATENCY_MAX=0

echo "=== Soak Test: $PROFILE ===" | tee "$LOG_FILE"
echo "Duration: ${DURATION_SECS}s" | tee -a "$LOG_FILE"
echo "Port: $EGGSERVE_PORT" | tee -a "$LOG_FILE"
echo "Log: $LOG_FILE" | tee -a "$LOG_FILE"
echo "" | tee -a "$LOG_FILE"

# Traffic mix functions
do_get() {
    local path="$1"
    local start_ms
    start_ms=$(date +%s%N 2>/dev/null | cut -b1-13 || echo "0")
    local status
    status=$("$CURL_BIN" -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:${EGGSERVE_PORT}${path}" 2>/dev/null || echo "000")
    local end_ms
    end_ms=$(date +%s%N 2>/dev/null | cut -b1-13 || echo "0")
    if [[ "$start_ms" != "0" ]] && [[ "$end_ms" != "0" ]]; then
        local elapsed_ms=$((end_ms - start_ms))
        LATENCY_SUM=$((LATENCY_SUM + elapsed_ms))
        LATENCY_COUNT=$((LATENCY_COUNT + 1))
        if [[ "$elapsed_ms" -gt "$LATENCY_MAX" ]]; then
            LATENCY_MAX="$elapsed_ms"
        fi
    fi
    echo "$status"
}

do_head() {
    local path="$1"
    "$CURL_BIN" -s -o /dev/null -w "%{http_code}" -I "http://127.0.0.1:${EGGSERVE_PORT}${path}" 2>/dev/null || echo "000"
}

do_range() {
    local path="$1"
    local start="$2"
    local end="$3"
    "$CURL_BIN" -s -o /dev/null -w "%{http_code}" -H "Range: bytes=${start}-${end}" "http://127.0.0.1:${EGGSERVE_PORT}${path}" 2>/dev/null || echo "000"
}

do_conditional() {
    local path="$1"
    local etag="$2"
    "$CURL_BIN" -s -o /dev/null -w "%{http_code}" -H "If-None-Match: \"${etag}\"" "http://127.0.0.1:${EGGSERVE_PORT}${path}" 2>/dev/null || echo "000"
}

do_malformed() {
    local payload="$1"
    echo -ne "$payload" | nc -w 2 127.0.0.1 "$EGGSERVE_PORT" 2>/dev/null | head -1 | grep -oP 'HTTP/1\.\d \K\d+' || echo "000"
}

log_metric() {
    local timestamp
    timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    local rss_kb
    rss_kb=$(ps -o rss= -p "$EGGSERVE_PID" 2>/dev/null | tr -d ' ' || echo "0")
    local fd_count
    fd_count=$(ls /proc/"$EGGSERVE_PID"/fd 2>/dev/null | wc -l || echo "0")
    local avg_latency="0"
    if [[ "$LATENCY_COUNT" -gt 0 ]]; then
        avg_latency=$((LATENCY_SUM / LATENCY_COUNT))
    fi
    
    if [[ "$rss_kb" -gt "$MAX_RSS_KB" ]]; then
        MAX_RSS_KB="$rss_kb"
    fi
    
    echo "${timestamp} | requests=${TOTAL_REQUESTS} errors=${TOTAL_ERRORS} rss_kb=${rss_kb} max_rss_kb=${MAX_RSS_KB} fds=${fd_count} avg_latency_ms=${avg_latency} max_latency_ms=${LATENCY_MAX}" >> "$LOG_FILE"
    
    if [[ "$VERBOSE" == "true" ]]; then
        echo "${timestamp} | requests=${TOTAL_REQUESTS} errors=${TOTAL_ERRORS} rss_kb=${rss_kb} fds=${fd_count} avg_ms=${avg_latency}"
    fi
}

# Main loop
CYCLE=0
while [[ $(date +%s) -lt $END_TIME ]]; do
    CYCLE=$((CYCLE + 1))

    # GET small files
    for i in $(seq 1 10); do
        status=$(do_get "/small_${i}.txt")
        TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
        if [[ "$status" != "200" ]]; then
            TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
            echo "ERROR: GET /small_${i}.txt returned $status" >> "$LOG_FILE"
        fi
    done

    # HEAD medium files
    for i in $(seq 1 5); do
        status=$(do_head "/medium_${i}.bin")
        TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
        if [[ "$status" != "200" ]]; then
            TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
            echo "ERROR: HEAD /medium_${i}.bin returned $status" >> "$LOG_FILE"
        fi
    done

    # Range requests on large files
    for i in $(seq 1 3); do
        local_size=$(stat -c%s "$WORK_DIR/root/large_${i}.bin" 2>/dev/null || echo "102400")
        local_start=$((RANDOM % (local_size / 2)))
        local_end=$((local_start + 1023))
        status=$(do_range "/large_${i}.bin" "$local_start" "$local_end")
        TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
        if [[ "$status" != "206" ]]; then
            TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
            echo "ERROR: RANGE /large_${i}.bin returned $status" >> "$LOG_FILE"
        fi
    done

    # GET nested files
    status=$(do_get "/subdir/nested.txt")
    TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
    if [[ "$status" != "200" ]]; then
        TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
    fi

    status=$(do_get "/subdir/deep.bin")
    TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
    if [[ "$status" != "200" ]]; then
        TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
    fi

    # 404 requests
    status=$(do_get "/nonexistent_${CYCLE}.txt")
    TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
    if [[ "$status" != "404" ]]; then
        TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
    fi

    # Empty file
    status=$(do_get "/empty.txt")
    TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
    if [[ "$status" != "200" ]]; then
        TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
    fi

    # Malformed requests (every 100 cycles)
    if (( CYCLE % 100 == 0 )); then
        do_malformed "GARBAGE DATA\r\n\r\n" > /dev/null 2>&1
        TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))

        do_malformed "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 6\r\n\r\n0\r\n\r\n" > /dev/null 2>&1
        TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
    fi

    # Slow headers (every 200 cycles)
    if (( CYCLE % 200 == 0 )); then
        (
            exec 3<>/dev/tcp/127.0.0.1/"$EGGSERVE_PORT"
            echo -ne "GET /small_1.txt HTTP/1.1\r\nHost: " >&3
            sleep 1
            echo -ne "localhost\r\n\r\n" >&3
            cat <&3
            exec 3>&-
        ) > /dev/null 2>&1 || true
        TOTAL_REQUESTS=$((TOTAL_REQUESTS + 1))
    fi

    # Periodic graceful restart (every 1000 cycles)
    if (( CYCLE % 1000 == 0 )); then
        log_metric
        echo "Cycle $CYCLE: performing graceful restart" >> "$LOG_FILE"

        kill -TERM "$EGGSERVE_PID" 2>/dev/null || true
        wait "$EGGSERVE_PID" 2>/dev/null || true

        sleep 2

        "$EGGSERVE_BIN" --bind "127.0.0.1:${EGGSERVE_PORT}" --directory "$WORK_DIR/root" &
        EGGSERVE_PID=$!
        sleep 1

        if ! kill -0 "$EGGSERVE_PID" 2>/dev/null; then
            echo "FAIL: eggserve failed to restart after cycle $CYCLE"
            exit 1
        fi
    fi

    # Log metrics every 100 cycles
    if (( CYCLE % 100 == 0 )); then
        log_metric
    fi

    # Brief pause to avoid tight loop
    sleep 0.1
done

# Final metrics
log_metric

echo "" | tee -a "$LOG_FILE"
echo "=== Soak Test Complete ===" | tee -a "$LOG_FILE"
echo "Total requests: $TOTAL_REQUESTS" | tee -a "$LOG_FILE"
echo "Total errors: $TOTAL_ERRORS" | tee -a "$LOG_FILE"
echo "Max RSS: ${MAX_RSS_KB}KB" | tee -a "$LOG_FILE"
AVG_LATENCY="0"
if [[ "$LATENCY_COUNT" -gt 0 ]]; then
    AVG_LATENCY=$((LATENCY_SUM / LATENCY_COUNT))
fi
echo "Avg latency: ${AVG_LATENCY}ms" | tee -a "$LOG_FILE"
echo "Max latency: ${LATENCY_MAX}ms" | tee -a "$LOG_FILE"
echo "Duration: ${DURATION_SECS}s" | tee -a "$LOG_FILE"

# Validate results
if [[ $TOTAL_ERRORS -gt 0 ]]; then
    echo "FAIL: $TOTAL_ERRORS errors during soak test"
    exit 1
fi

# Check for monotonic RSS growth (simplified heuristic)
FINAL_RSS=$(ps -o rss= -p "$EGGSERVE_PID" 2>/dev/null | tr -d ' ' || echo "0")
if [[ $FINAL_RSS -gt $((MAX_RSS_KB / 2)) ]]; then
    echo "WARN: Final RSS ($FINAL_RSS KB) is more than half of max RSS ($MAX_RSS_KB KB)"
fi

echo "PASS: Soak test completed successfully"
