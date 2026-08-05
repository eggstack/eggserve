#!/usr/bin/env python3
"""Smoke-test a bundled eggserve binary against a controlled fixture."""

from __future__ import annotations

import http.client
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} BINARY", file=sys.stderr)
        return 2

    binary = Path(sys.argv[1]).resolve()
    if not binary.is_file():
        raise AssertionError(f"binary not found: {binary}")

    version = subprocess.run([str(binary), "--version"], check=False, capture_output=True, text=True)
    if version.returncode != 0:
        raise AssertionError(f"--version failed: {version.stderr}")
    print(f"  binary: {binary}")
    print(f"  version: {version.stdout.strip()}")

    with tempfile.TemporaryDirectory(prefix="eggserve-smoke-") as root:
        fixture = b"eggserve release smoke\n"
        (Path(root) / "smoke.txt").write_bytes(fixture)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]

        process = subprocess.Popen(
            [
                str(binary),
                "--directory",
                root,
                "--bind",
                f"127.0.0.1:{port}",
                "--log-format",
                "none",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            deadline = time.monotonic() + 5
            response = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    stderr = process.stderr.read() if process.stderr else ""
                    raise AssertionError(f"server exited during startup: {stderr}")
                try:
                    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=0.5)
                    connection.request("GET", "/smoke.txt")
                    response = connection.getresponse()
                    body = response.read()
                    connection.close()
                    break
                except OSError:
                    time.sleep(0.05)
            if response is None:
                raise AssertionError("server did not become ready")
            if response.status != 200 or body != fixture:
                raise AssertionError(f"unexpected smoke response: {response.status} {body!r}")
            print(f"  GET /smoke.txt => {response.status} (exact fixture body)")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
                raise AssertionError("server did not terminate cleanly")
            if process.returncode not in (0, -15, 143):
                stderr = process.stderr.read() if process.stderr else ""
                raise AssertionError(f"server exited unsuccessfully: {process.returncode}: {stderr}")
            print("  server stopped")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
