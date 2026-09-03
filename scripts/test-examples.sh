#!/usr/bin/env bash
# Smoke-test the canonical Rust examples after they have been built.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON="${PYTHON:-python3}"

"$PYTHON" - "$REPO_ROOT" <<'PYEOF'
import http.client
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

repo = Path(sys.argv[1])


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def request(port, method, path):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
    try:
        connection.request(method, path, headers={"Connection": "close"})
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


def smoke(binary_name, args, expectations):
    port = free_port()
    binary = repo / "target" / "debug" / "examples" / binary_name
    process = subprocess.Popen(
        [str(binary), *args, f"127.0.0.1:{port}"],
        cwd=repo,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    actual = None
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            try:
                actual = [request(port, method, path) for method, path, _ in expectations]
                break
            except OSError:
                time.sleep(0.05)
        if actual is None:
            raise AssertionError(f"{binary_name} did not become ready")
        if process.poll() is not None:
            stdout, stderr = process.communicate()
            raise AssertionError(
                f"{binary_name} exited during startup: {stdout}\n{stderr}"
            )
        expected = [expected for _, _, expected in expectations]
        if actual != expected:
            raise AssertionError(f"{binary_name} responses: {actual!r}")
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            stdout, stderr = process.communicate(timeout=5)
            raise AssertionError(f"{binary_name} did not shut down: {stdout}\n{stderr}")
        if process.returncode != 0:
            raise AssertionError(
                f"{binary_name} exited with {process.returncode}: {stdout}\n{stderr}"
            )


smoke(
    "static_server",
    [str(repo / "examples" / "site")],
    [
        ("GET", "/", (200, (repo / "examples" / "site" / "index.html").read_bytes())),
        ("HEAD", "/assets/example.txt", (200, b"")),
        ("GET", "/.hidden-example", (403, b"403 Forbidden\n")),
    ],
)
smoke(
    "custom_service",
    [],
    [
        ("GET", "/health", (200, b"ok\n")),
        ("GET", "/missing", (404, b"not found\n")),
    ],
)
smoke(
    "streaming_service",
    [],
    [
        ("GET", "/known", (200, b"hello world")),
        ("GET", "/stream", (200, b"tick tock")),
    ],
)
print("Rust example smoke checks passed")
PYEOF
