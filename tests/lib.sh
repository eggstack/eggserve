#!/usr/bin/env bash
# lib.sh — Shared helpers for the integration test scripts under tests/.
#
# Sourced (not executed). Provides OS-assigned port selection and TCP
# readiness polling so scripts neither guess fixed ports (TOCTOU with other
# processes) nor rely on fixed sleeps for startup readiness.

# Pick a free TCP port on loopback by asking the kernel: bind
# 127.0.0.1:0, read the assigned port, and release it immediately.
# Prefers python3 or perl; falls back to a random fixed port when neither
# is available (residual race remains in that case only).
pick_free_port() {
    if command -v python3 >/dev/null 2>&1; then
        python3 -c 'import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()'
    elif command -v perl >/dev/null 2>&1; then
        perl -MSocket -e '
            socket(S, AF_INET, SOCK_STREAM, getprotobyname("tcp")) or exit 1;
            bind(S, sockaddr_in(0, inet_aton("127.0.0.1"))) or exit 1;
            my $port = (sockaddr_in(getsockname(S)))[0];
            print "$port\n";'
    else
        shuf -i 50000-60000 -n 1
    fi
}

# Wait until a TCP endpoint accepts connections. Polls with bash /dev/tcp
# until success or the deadline (seconds) expires. Returns non-zero on
# deadline without killing anything.
wait_for_tcp() {
    local host="$1" port="$2" timeout="${3:-15}"
    local deadline=$(( $(date +%s) + timeout ))
    while :; do
        if (exec 3<>"/dev/tcp/${host}/${port}") 2>/dev/null; then
            exec 3>&- 3<&- 2>/dev/null || true
            return 0
        fi
        if [[ "$(date +%s)" -ge "$deadline" ]]; then
            return 1
        fi
        sleep 0.05
    done
}

# Wait until a TCP endpoint stops accepting connections (e.g. after a
# graceful shutdown released its listener), bounded by the deadline.
wait_for_tcp_absent() {
    local host="$1" port="$2" timeout="${3:-15}"
    local deadline=$(( $(date +%s) + timeout ))
    while :; do
        if ! (exec 3<>"/dev/tcp/${host}/${port}") 2>/dev/null; then
            return 0
        fi
        exec 3>&- 3<&- 2>/dev/null || true
        if [[ "$(date +%s)" -ge "$deadline" ]]; then
            return 1
        fi
        sleep 0.05
    done
}
