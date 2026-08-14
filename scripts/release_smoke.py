#!/usr/bin/env python3
"""Smoke-test the eggserve server against a controlled fixture.

Usage:
    python release_smoke.py                    # test installed entry point
    python release_smoke.py /path/to/eggserve  # test a specific binary
"""

from __future__ import annotations

import http.client
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def main() -> int:
    binary: str | None = None
    if len(sys.argv) == 2:
        candidate = Path(sys.argv[1]).resolve()
        if candidate.is_file():
            binary = str(candidate)
    elif len(sys.argv) > 2:
        print(f"usage: {sys.argv[0]} [BINARY]", file=sys.stderr)
        return 2

    if binary is not None:
        version = subprocess.run(
            [binary, "--version"], check=False, capture_output=True, text=True
        )
        if version.returncode != 0:
            raise AssertionError(f"--version failed: {version.stderr}")
        print(f"  binary: {binary}")
        print(f"  version: {version.stdout.strip()}")
    else:
        cmd = shutil.which("eggserve")
        if cmd is None:
            # Fall back to python -m eggserve
            cmd = sys.executable
            argv_base = [cmd, "-m", "eggserve"]
            print(f"  command: python -m eggserve")
        else:
            argv_base = [cmd]
            print(f"  command: {cmd}")

    with tempfile.TemporaryDirectory(prefix="eggserve-smoke-") as root:
        fixture = b"eggserve release smoke\n"
        (Path(root) / "smoke.txt").write_bytes(fixture)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]

        if binary is not None:
            argv = [
                binary,
                "--directory",
                root,
                "--bind",
                f"127.0.0.1:{port}",
                "--log-format",
                "none",
            ]
        else:
            argv = argv_base + [
                "--directory",
                root,
                "--bind",
                f"127.0.0.1:{port}",
                "--log-format",
                "none",
            ]

        process = subprocess.Popen(
            argv,
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
            expected_returncodes = {0, -15, 143}
            if os.name == "nt":
                # Popen.terminate() uses TerminateProcess on Windows, which
                # reports status 1 even though the requested termination
                # completed successfully.
                expected_returncodes.add(1)
            if process.returncode not in expected_returncodes:
                stderr = process.stderr.read() if process.stderr else ""
                raise AssertionError(f"server exited unsuccessfully: {process.returncode}: {stderr}")
            print("  server stopped")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
